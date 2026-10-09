`timescale 1ns/1ps
// Independent bounded framebuffer-cache protocol testbench.
//
// The external memory model enforces the single-outstanding 128-byte contract
// and actively checks the properties the controller must satisfy:
//   * request valid/ready is evaluated on the old edge; a presented request and
//     its address/type stay stable across backpressure and are only consumed on
//     valid&&ready;
//   * after a write request is admitted the write data/valid stream is
//     continuous on every following edge (no M_WLOAD/M_WDATA bubble) and stable
//     while a beat is blocked;
//   * a read may present its terminal success on the same edge as its last beat
//     and the controller must count that beat;
//   * a write terminal ACK is delayed after the last beat and still required;
//   * four synchronous banks, no hierarchical force.
//
// The per-row stimulus carries the captured draw context, so the testbench can
// change `depth_func`/`depth_write`/`blend` between quads while an older quad is
// still computing. One stimulus bit can corrupt an odd row's reserved half to
// inject a compute header fault after transport has been accepted. Memory
// terminal failures are selected by request type.
//
// The DUT is driven only by the stimulus rows and the initial memory image; no
// DUT answer is replayed. Committed quads are dumped for an independent host
// reference comparison.

module tb;
    localparam integer STIM       = __STIM__;
    localparam integer BYTES      = __BYTES__;
    localparam integer MAXC       = __MAXC__;
    localparam integer FAIL_READ  = __FAIL_READ__;
    localparam integer FAIL_WRITE = __FAIL_WRITE__;

    reg clk = 0;
    always #5 clk = ~clk;

    reg reset = 1;
    integer cyc = 0;
    always @(posedge clk) cyc <= cyc + 1;

    reg [63:0] stim [0:STIM-1];
    reg [7:0]  mem  [0:BYTES-1];
    reg [31:0] commits [0:MAXC-1];

    integer si;
    integer ci;
    reg [63:0] cur;
    reg flush_request = 0;
    reg done = 0;
    reg fault_reported = 0;

    // Stimulus fields (see tests/framebuffer_cache_actual_rtl.rs).
    wire [31:0] stim_data  = cur[31:0];
    wire [2:0]  row_index  = cur[34:32];
    wire [3:0]  row_mask   = cur[38:35];
    wire [7:0]  row_y      = cur[46:39];
    wire [8:0]  row_x      = cur[55:47];
    wire [2:0]  ctx_func   = cur[58:56];
    wire        ctx_blend  = cur[59];
    wire        ctx_dw     = cur[60];
    wire        inject_bad = cur[61];
    // A bad odd row sets a reserved high bit, which the controller must reject.
    wire [31:0] row_data   = stim_data
        | ((inject_bad && row_index[0]) ? 32'h0001_0000 : 32'd0);
    wire        row_valid  = (si < STIM);
    wire        row_accept;

    wire        commit_valid;
    wire [8:0]  commit_x;
    wire [7:0]  commit_y;
    wire [3:0]  commit_mask;
    wire        flush_complete;
    wire        fault;
    wire        idle;

    wire        mem_req_valid;
    wire        mem_req_write;
    wire [31:0] mem_req_addr;
    wire        mem_req_ready;
    reg  [63:0] mem_read_data;
    wire        mem_read_valid;
    wire [63:0] mem_write_data;
    wire        mem_write_valid;
    wire        mem_write_ready;
    wire        mem_complete_valid;
    wire        mem_complete_ok;

    framebuffer_cache #(
        .COLOR_BASE(__COLOR__),
        .DEPTH_BASE(__DEPTH__),
        .WIDTH(__WIDTH__),
        .HEIGHT(__HEIGHT__)
    ) dut (
        .clk(clk),
        .reset(reset),
        .ce(__CE__ == 0 ? 1'b1 : (cyc % __CE__ != 0)),
        .row_valid(row_valid),
        .row_x(row_x),
        .row_y(row_y),
        .row_mask(row_mask),
        .row_index(row_index),
        .row_data(row_data),
        .row_accept(row_accept),
        .depth_func(ctx_func),
        .depth_write(ctx_dw),
        .blend(ctx_blend),
        .commit_valid(commit_valid),
        .commit_x(commit_x),
        .commit_y(commit_y),
        .commit_mask(commit_mask),
        .flush_request(flush_request),
        .flush_complete(flush_complete),
        .fault(fault),
        .idle(idle),
        .mem_req_valid(mem_req_valid),
        .mem_req_write(mem_req_write),
        .mem_req_addr(mem_req_addr),
        .mem_req_ready(mem_req_ready),
        .mem_read_data(mem_read_data),
        .mem_read_valid(mem_read_valid),
        .mem_write_data(mem_write_data),
        .mem_write_valid(mem_write_valid),
        .mem_write_ready(mem_write_ready),
        .mem_complete_valid(mem_complete_valid),
        .mem_complete_ok(mem_complete_ok)
    );

    // ---- External memory model with protocol checks ----
    localparam MIDLE = 2'd0, MREAD = 2'd1, MWRITE = 2'd2, MACK = 2'd3;
    reg [1:0]  mstate = MIDLE;
    reg [31:0] ma;
    reg        mw;
    reg [4:0]  mi;
    reg [31:0] reqs;
    reg [2:0]  ackwait;
    reg [1:0]  stallreq;
    reg [2:0]  stallbeat;
    reg        resp_fail;
    reg        did_fail;

    // Request stability watch.
    reg        pend;
    reg [31:0] pend_addr;
    reg        pend_write;
    // Beat stability watch.
    reg [63:0] held_beat;
    reg        held_valid;

    assign mem_req_ready = (mstate == MIDLE) && (stallreq == 2'd0);
    assign mem_write_ready = (mstate == MWRITE && stallbeat == 3'd0)
        || (mstate == MIDLE && mem_req_valid && mem_req_write && stallreq == 2'd0);
    wire admission_read = __READ_ON_ACCEPT__ && mstate == MIDLE
        && mem_req_valid && mem_req_ready && !mem_req_write;
    assign mem_read_valid = (mstate == MREAD) || admission_read;
    // Read terminal success coincides with the last read beat; the write ACK is
    // delayed in MACK.
    assign mem_complete_valid = (mstate == MREAD && mi == 5'd15)
        || (mstate == MACK && mw && ackwait == 3'd0);
    assign mem_complete_ok = !resp_fail;

    always @(*) begin
        mem_read_data = {
            mem[ma + (mi << 3) + 7], mem[ma + (mi << 3) + 6],
            mem[ma + (mi << 3) + 5], mem[ma + (mi << 3) + 4],
            mem[ma + (mi << 3) + 3], mem[ma + (mi << 3) + 2],
            mem[ma + (mi << 3) + 1], mem[ma + (mi << 3) + 0]
        };
        if (admission_read) mem_read_data = {
            mem[mem_req_addr+7], mem[mem_req_addr+6],
            mem[mem_req_addr+5], mem[mem_req_addr+4],
            mem[mem_req_addr+3], mem[mem_req_addr+2],
            mem[mem_req_addr+1], mem[mem_req_addr+0]
        };
    end

    always @(posedge clk) begin
        if (reset) begin
            mstate <= MIDLE;
            mi <= 5'd0;
            reqs <= 32'd0;
            ackwait <= 3'd0;
            stallreq <= 2'd0;
            stallbeat <= 3'd0;
            resp_fail <= 1'b0;
            did_fail <= 1'b0;
            pend <= 1'b0;
            held_valid <= 1'b0;
            ci <= 0;
        end else begin
            // ---- Request stability / old-edge handshake ----
            if (mstate == MIDLE && mem_req_valid && !mem_req_ready) begin
                if (pend && (mem_req_addr !== pend_addr || mem_req_write !== pend_write)) begin
                    $display("FAIL request changed under backpressure");
                    $finish;
                end
                pend <= 1'b1;
                pend_addr <= mem_req_addr;
                pend_write <= mem_req_write;
            end
            if (__STALL__ && mstate == MIDLE && !mem_req_valid) stallreq <= 2'd2;
            else if (stallreq != 2'd0) stallreq <= stallreq - 2'd1;

            if (mstate == MWRITE) begin
                // Continuous supply: no valid bubble while beats remain.
                if (mi < 5'd16 && !mem_write_valid) begin
                    $display("FAIL write valid bubble at beat %0d", mi);
                    $finish;
                end
                // Blocked beat stability.
                if (mem_write_valid && !mem_write_ready) begin
                    if (held_valid && mem_write_data !== held_beat) begin
                        $display("FAIL write beat changed while blocked at beat %0d", mi);
                        $finish;
                    end
                    held_valid <= 1'b1;
                    held_beat <= mem_write_data;
                end else begin
                    held_valid <= 1'b0;
                end
                if (__STALL__ && stallbeat == 3'd0) stallbeat <= 3'd2;
                else if (stallbeat != 3'd0) stallbeat <= stallbeat - 3'd1;
            end

            // ---- State transitions ----
            if (mstate == MIDLE && mem_req_valid && mem_req_ready) begin
                ma <= mem_req_addr;
                mw <= mem_req_write;
                reqs <= reqs + 32'd1;
                pend <= 1'b0;
                mi <= 5'd0;
                if (!did_fail && ((FAIL_READ && !mem_req_write)
                                  || (FAIL_WRITE && mem_req_write))) begin
                    did_fail <= 1'b1;
                    resp_fail <= 1'b1;
                end else begin
                    resp_fail <= 1'b0;
                end
                if (mem_req_write) begin
                    // Same-edge admission: consume beat0 if already offered.
                    // `ma` is only registered after this edge, so the accepted
                    // request's own address must be used for this write.
                    mstate <= MWRITE;
                    if (mem_write_valid && mem_write_ready) begin
                        mem[mem_req_addr + 0] <= mem_write_data[7:0];
                        mem[mem_req_addr + 1] <= mem_write_data[15:8];
                        mem[mem_req_addr + 2] <= mem_write_data[23:16];
                        mem[mem_req_addr + 3] <= mem_write_data[31:24];
                        mem[mem_req_addr + 4] <= mem_write_data[39:32];
                        mem[mem_req_addr + 5] <= mem_write_data[47:40];
                        mem[mem_req_addr + 6] <= mem_write_data[55:48];
                        mem[mem_req_addr + 7] <= mem_write_data[63:56];
                        mi <= 5'd1;
                    end
                end else begin
                    mstate <= MREAD;
                    if (admission_read) mi <= 5'd1;
                end
            end else if (mstate == MREAD) begin
                if (mi == 5'd15) begin
                    mstate <= MACK;
                    ackwait <= 3'd3;
                end else begin
                    mi <= mi + 5'd1;
                end
            end else if (mstate == MWRITE) begin
                if (mem_write_valid && mem_write_ready) begin
                    mem[ma + (mi << 3) + 0] <= mem_write_data[7:0];
                    mem[ma + (mi << 3) + 1] <= mem_write_data[15:8];
                    mem[ma + (mi << 3) + 2] <= mem_write_data[23:16];
                    mem[ma + (mi << 3) + 3] <= mem_write_data[31:24];
                    mem[ma + (mi << 3) + 4] <= mem_write_data[39:32];
                    mem[ma + (mi << 3) + 5] <= mem_write_data[47:40];
                    mem[ma + (mi << 3) + 6] <= mem_write_data[55:48];
                    mem[ma + (mi << 3) + 7] <= mem_write_data[63:56];
                    if (mi == 5'd15) begin
                        mstate <= MACK;
                        ackwait <= 3'd3;
                    end else begin
                        mi <= mi + 5'd1;
                    end
                end
            end else if (mstate == MACK) begin
                if (ackwait == 3'd0) mstate <= MIDLE;
                else ackwait <= ackwait - 3'd1;
            end
        end
    end

    // ---- Commit capture ----
    always @(posedge clk) begin
        if (!reset && commit_valid && ci < MAXC) begin
            commits[ci] <= {3'b0, commit_x, commit_y, commit_mask};
            ci <= ci + 1;
        end
    end

    // ---- Stimulus / flush / finish ----
    always @(posedge clk) begin
        if (reset) begin
            si <= 0;
            flush_request <= 1'b0;
            done <= 1'b0;
            fault_reported <= 1'b0;
        end else if (fault) begin
            // Drain accepted traffic; the model keeps clocking. Record and stop.
            if (!fault_reported) begin
                fault_reported <= 1'b1;
                $display("FAULT reqs=%0d mstate=%0d", reqs, mstate);
            end
            if (idle || done) begin
                done <= 1'b1;
                $writememh("cache_out.hex", mem);
                $writememh("commits.hex", commits);
                $display("PASS cache fault=1 commits=%0d", ci);
                $finish;
            end
        end else if (si < STIM) begin
            if (row_accept) begin
                si <= si + 1;
                if (si + 1 < STIM) cur <= stim[si + 1];
            end
        end else if (!done) begin
            flush_request <= 1'b1;
            if (flush_complete) begin
                done <= 1'b1;
                flush_request <= 1'b0;
                $writememh("cache_out.hex", mem);
                $writememh("commits.hex", commits);
                $display("PASS cache fault=%b commits=%0d", fault, ci);
                $finish;
            end
        end
    end

    initial begin
        $readmemh("rows.hex", stim);
        $readmemh("mem.hex", mem);
        for (ci = 0; ci < MAXC; ci = ci + 1) commits[ci] = 32'd0;
        cur = stim[0];
        repeat (4) @(posedge clk);
        reset = 0;
    end

    // Finite watchdog: no unbounded simulation.
    initial begin
        repeat (__WATCH__) @(posedge clk);
        $writememh("cache_out.hex", mem);
        $writememh("commits.hex", commits);
        $display("FAIL watchdog fault=%b flush=%b", fault, flush_complete);
        $finish;
    end
endmodule
