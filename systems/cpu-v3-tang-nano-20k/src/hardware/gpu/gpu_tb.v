// Bounded self-checking Icarus co-simulation of the handwritten GPU command
// FSM and tile framebuffer cache.
//
// The testbench replaces the real arbiter/SharedSdramPort with a synchronous
// direct-memory model that serves all three masters: `gpu_ro` streams four
// 64-bit beats for a 32-byte line, `gpu_fb_r` streams sixteen beats for a
// 128-byte refill, and `gpu_fb_w` captures sixteen beats for a 128-byte clean
// before committing them to memory. The model presents request-ready in the
// same cycle the DUT asserts a request, exactly like the host model, and is
// the only driver of the DUT's memory handshake inputs.
//
// Scenarios are reset-separated and cover partial-row LOAD preservation, CLEAR
// initialization, duplicate/unordered tile indices, dirty victim eviction
// between tiles 0 and 8, END drain before retirement, invalid command fields,
// a command crossing a 32-byte line, and the framebuffer guard. Every wait
// loop is bounded, and a global clock limit fails the run if the DUT hangs.
`timescale 1ns/1ps
module tb;
reg clk = 1'b0;
always #5 clk = ~clk;

integer cycles = 0;
always @(posedge clk) begin
    cycles = cycles + 1;
    if (cycles > 4000000) $fatal(1, "GPU testbench cycle limit exceeded");
end

// Command buffer at word address 0x100; 32-byte list slots at 0x400.
localparam [21:0] CMD_BASE = 22'h000100;
localparam [21:0] LIST_BASE = 22'h000400;
localparam [21:0] LIST_STRIDE = 22'h000010;
localparam [21:0] FB_A = 22'h200000;
localparam [21:0] FB_B = 22'h218000;
// 400x240 live pixels; the guard word sits just past the last tile.
localparam [21:0] FB_GUARD = FB_A + 22'd96000;
localparam MEM_WORDS = 22'h240000;

reg [15:0] mem [0:MEM_WORDS];

reg [2:0] device_index = 0;
reg [3:0] device_channel = 0;
reg device_read_enable = 0, device_write_enable = 0;
reg [15:0] device_write_data = 0;
wire [15:0] device_read_data;

reg gpu_ro_request_ready = 0, gpu_ro_write_data_ready = 0;
reg gpu_ro_response_valid = 0, gpu_ro_response_last = 0, gpu_ro_error = 0;
reg [63:0] gpu_ro_read_data = 0;
reg gpu_fb_w_request_ready = 0, gpu_fb_w_write_data_ready = 0;
reg gpu_fb_w_response_valid = 0, gpu_fb_w_response_last = 0, gpu_fb_w_error = 0;
reg gpu_fb_r_request_ready = 0, gpu_fb_r_write_data_ready = 0;
reg gpu_fb_r_response_valid = 0, gpu_fb_r_response_last = 0, gpu_fb_r_error = 0;
reg [63:0] gpu_fb_r_read_data = 0;

wire gpu_ro_request_valid, gpu_ro_write;
wire [21:0] gpu_ro_address;
wire [1:0] gpu_ro_line_count_minus_1;
wire [63:0] gpu_ro_write_data;
wire gpu_fb_w_request_valid, gpu_fb_w_write;
wire [21:0] gpu_fb_w_address;
wire [1:0] gpu_fb_w_line_count_minus_1;
wire [63:0] gpu_fb_w_write_data;
wire gpu_fb_r_request_valid, gpu_fb_r_write;
wire [21:0] gpu_fb_r_address;
wire [1:0] gpu_fb_r_line_count_minus_1;
wire [63:0] gpu_fb_r_write_data;

reg reset = 1;
CpuV3Gpu dut(
    .clk(clk), .reset(reset),
    .device_index(device_index), .device_channel(device_channel),
    .device_read_enable(device_read_enable), .device_write_enable(device_write_enable),
    .device_write_data(device_write_data), .device_read_data(device_read_data),
    .gpu_ro_request_ready(gpu_ro_request_ready), .gpu_ro_write_data_ready(gpu_ro_write_data_ready),
    .gpu_ro_response_valid(gpu_ro_response_valid), .gpu_ro_read_data(gpu_ro_read_data),
    .gpu_ro_response_last(gpu_ro_response_last), .gpu_ro_error(gpu_ro_error),
    .gpu_fb_w_request_ready(gpu_fb_w_request_ready), .gpu_fb_w_write_data_ready(gpu_fb_w_write_data_ready),
    .gpu_fb_w_response_valid(gpu_fb_w_response_valid), .gpu_fb_w_response_last(gpu_fb_w_response_last),
    .gpu_fb_w_error(gpu_fb_w_error),
    .gpu_fb_r_request_ready(gpu_fb_r_request_ready), .gpu_fb_r_write_data_ready(gpu_fb_r_write_data_ready),
    .gpu_fb_r_response_valid(gpu_fb_r_response_valid), .gpu_fb_r_read_data(gpu_fb_r_read_data),
    .gpu_fb_r_response_last(gpu_fb_r_response_last), .gpu_fb_r_error(gpu_fb_r_error),
    .gpu_ro_request_valid(gpu_ro_request_valid), .gpu_ro_write(gpu_ro_write),
    .gpu_ro_address(gpu_ro_address), .gpu_ro_line_count_minus_1(gpu_ro_line_count_minus_1),
    .gpu_ro_write_data(gpu_ro_write_data),
    .gpu_fb_w_request_valid(gpu_fb_w_request_valid), .gpu_fb_w_write(gpu_fb_w_write),
    .gpu_fb_w_address(gpu_fb_w_address), .gpu_fb_w_line_count_minus_1(gpu_fb_w_line_count_minus_1),
    .gpu_fb_w_write_data(gpu_fb_w_write_data),
    .gpu_fb_r_request_valid(gpu_fb_r_request_valid), .gpu_fb_r_write(gpu_fb_r_write),
    .gpu_fb_r_address(gpu_fb_r_address), .gpu_fb_r_line_count_minus_1(gpu_fb_r_line_count_minus_1),
    .gpu_fb_r_write_data(gpu_fb_r_write_data));

// ---- direct-memory responder for all three masters ----
// State is updated on the clock edge; response outputs are combinational from
// that state, so the DUT samples a request/response in the same cycle.
localparam MS_IDLE = 3'd0, MS_RD = 3'd1, MS_WR = 3'd2, MS_WRESP = 3'd3, MS_REC = 3'd4;
reg [2:0] mstate = MS_IDLE;
reg [21:0] maddr = 0;
reg [4:0] mbeats = 0;
reg [4:0] mbeat = 0;
reg mport = 1'b0;
reg [63:0] wbuf [0:15];

function [63:0] beat_of;
    input [21:0] a;
    input [4:0] b;
    reg [21:0] base;
    begin
        base = a + {b, 2'b0};
        beat_of = {mem[base + 3], mem[base + 2], mem[base + 1], mem[base]};
    end
endfunction

always @(*) begin
    gpu_ro_request_ready = 1'b0;
    gpu_fb_r_request_ready = 1'b0;
    gpu_fb_w_request_ready = 1'b0;
    gpu_ro_response_valid = 1'b0;
    gpu_ro_response_last = 1'b0;
    gpu_ro_read_data = 64'h0;
    gpu_ro_error = 1'b0;
    gpu_fb_r_response_valid = 1'b0;
    gpu_fb_r_response_last = 1'b0;
    gpu_fb_r_read_data = 64'h0;
    gpu_fb_r_error = 1'b0;
    gpu_fb_w_write_data_ready = 1'b0;
    gpu_fb_w_response_valid = 1'b0;
    gpu_fb_w_response_last = 1'b0;
    gpu_fb_w_error = 1'b0;
    case (mstate)
        MS_IDLE: begin
            gpu_ro_request_ready = 1'b1;
            gpu_fb_r_request_ready = 1'b1;
            gpu_fb_w_request_ready = 1'b1;
        end
        MS_RD: begin
            if (mport == 1'b0) begin
                gpu_ro_response_valid = 1'b1;
                gpu_ro_read_data = beat_of(maddr, mbeat);
                gpu_ro_response_last = (mbeat + 5'd1 == mbeats);
            end else begin
                gpu_fb_r_response_valid = 1'b1;
                gpu_fb_r_read_data = beat_of(maddr, mbeat);
                gpu_fb_r_response_last = (mbeat + 5'd1 == mbeats);
            end
        end
        // Deterministic backpressure: hold every fourth write-data cycle so
        // the DUT must retain the unaccepted cache beat and read address.
        MS_WR: gpu_fb_w_write_data_ready = (cycles[1:0] != 2'b00);
        MS_WRESP: begin
            gpu_fb_w_response_valid = 1'b1;
            gpu_fb_w_response_last = 1'b1;
        end
        default: ;
    endcase
end

integer k;
reg [21:0] cbase;
// Transaction trace sequence number; every GPU line bumps it. The comparison
// in `tests/gpu_trace_cosim.rs` ignores the number and compares event bodies.
integer tseq = 0;
reg [63:0] tbeat = 64'h0;
always @(posedge clk) begin
    if (reset) begin
        mstate <= MS_IDLE;
        maddr <= 0;
        mbeats <= 0;
        mbeat <= 0;
        mport <= 1'b0;
        for (k = 0; k < 16; k = k + 1) wbuf[k] <= 64'h0;
    end else begin
        case (mstate)
            MS_IDLE: begin
                if (gpu_ro_request_valid) begin
                    $display("GPU %0d REQ ro R %06h %0d", tseq, gpu_ro_address,
                             gpu_ro_line_count_minus_1 + 1);
                    tseq = tseq + 1;
                    maddr <= gpu_ro_address;
                    mbeats <= 5'd4;
                    mbeat <= 5'd0;
                    mport <= 1'b0;
                    mstate <= MS_RD;
                end else if (gpu_fb_r_request_valid) begin
                    $display("GPU %0d REQ fb_r R %06h %0d", tseq, gpu_fb_r_address,
                             gpu_fb_r_line_count_minus_1 + 1);
                    tseq = tseq + 1;
                    maddr <= gpu_fb_r_address;
                    mbeats <= 5'd16;
                    mbeat <= 5'd0;
                    mport <= 1'b1;
                    mstate <= MS_RD;
                end else if (gpu_fb_w_request_valid) begin
                    $display("GPU %0d REQ fb_w W %06h %0d", tseq, gpu_fb_w_address,
                             gpu_fb_w_line_count_minus_1 + 1);
                    tseq = tseq + 1;
                    // Beat zero is captured on the accepting edge.
                    $display("GPU %0d WDAT fb_w 0 %04h %04h %04h %04h", tseq,
                             gpu_fb_w_write_data[15:0], gpu_fb_w_write_data[31:16],
                             gpu_fb_w_write_data[47:32], gpu_fb_w_write_data[63:48]);
                    tseq = tseq + 1;
                    maddr <= gpu_fb_w_address;
                    mbeats <= 5'd16;
                    mbeat <= 5'd1;
                    wbuf[0] = gpu_fb_w_write_data;
                    mstate <= MS_WR;
                end
            end
            MS_RD: begin
                tbeat = beat_of(maddr, mbeat);
                if (mport == 1'b0) begin
                    $display("GPU %0d RDAT ro %0d %04h %04h %04h %04h", tseq, mbeat,
                             tbeat[15:0], tbeat[31:16], tbeat[47:32], tbeat[63:48]);
                    tseq = tseq + 1;
                    if (mbeat + 5'd1 == mbeats) begin
                        $display("GPU %0d RESP ro 0", tseq);
                        tseq = tseq + 1;
                    end
                end else begin
                    $display("GPU %0d RDAT fb_r %0d %04h %04h %04h %04h", tseq, mbeat,
                             tbeat[15:0], tbeat[31:16], tbeat[47:32], tbeat[63:48]);
                    tseq = tseq + 1;
                    if (mbeat + 5'd1 == mbeats) begin
                        $display("GPU %0d RESP fb_r 0", tseq);
                        tseq = tseq + 1;
                    end
                end
                if (mbeat + 5'd1 == mbeats) mstate <= MS_REC;
                else mbeat <= mbeat + 5'd1;
            end
            MS_WR: begin
                if (gpu_fb_w_write_data_ready) begin
                    $display("GPU %0d WDAT fb_w %0d %04h %04h %04h %04h", tseq, mbeat,
                             gpu_fb_w_write_data[15:0], gpu_fb_w_write_data[31:16],
                             gpu_fb_w_write_data[47:32], gpu_fb_w_write_data[63:48]);
                    tseq = tseq + 1;
                    wbuf[mbeat[3:0]] = gpu_fb_w_write_data;
                    if (mbeat + 5'd1 == mbeats) begin
                        for (k = 0; k < 16; k = k + 1) begin
                            cbase = maddr + {k[4:0], 2'b0};
                            mem[cbase + 0] = wbuf[k][15:0];
                            mem[cbase + 1] = wbuf[k][31:16];
                            mem[cbase + 2] = wbuf[k][47:32];
                            mem[cbase + 3] = wbuf[k][63:48];
                        end
                        mstate <= MS_WRESP;
                    end else begin
                        mbeat <= mbeat + 5'd1;
                    end
                end
            end
            MS_WRESP: begin
                $display("GPU %0d RESP fb_w 0", tseq);
                tseq = tseq + 1;
                mstate <= MS_REC;
            end
            default: mstate <= MS_IDLE;
        endcase
    end
end

// ---- completion-point trace ----
// DONE submission: executed_count change. DONE draw: a nonempty draw handing
// control back from PH_TILE_STEP (6) to PH_DECODE (3).
reg [15:0] prev_executed_count = 16'h0;
reg [4:0] prev_dut_phase = 5'd0;
always @(posedge clk) begin
    if (!reset) begin
        if (dut.executed_count !== prev_executed_count) begin
            $display("GPU %0d DONE submission %0d", tseq, dut.executed_count);
            tseq = tseq + 1;
        end
        if ((prev_dut_phase == 5'd6) && (dut.phase == 5'd3)) begin
            $display("GPU %0d DONE draw %0d", tseq, dut.draw_tile_count);
            tseq = tseq + 1;
        end
    end
    prev_executed_count <= dut.executed_count;
    prev_dut_phase <= dut.phase;
end

// ---- device register helpers ----
reg [15:0] read_result = 16'h0;
reg [15:0] expected_exec = 16'h0;
integer i;
integer j;
integer raster_tile;
integer raster_image;
reg [15:0] raster_expected;

task device_write;
    input [3:0] channel;
    input [15:0] value;
    begin
        @(negedge clk);
        device_index = 3'd4;
        device_channel = channel;
        device_write_enable = 1'b1;
        device_write_data = value;
        @(posedge clk);
        @(negedge clk);
        device_write_enable = 1'b0;
        device_write_data = 16'h0;
    end
endtask

task device_read;
    input [3:0] channel;
    begin
        @(negedge clk);
        device_index = 3'd4;
        device_channel = channel;
        device_read_enable = 1'b1;
        #1;
        read_result = device_read_data;
        device_read_enable = 1'b0;
    end
endtask

task do_submit;
    input [21:0] base;
    input [15:0] words;
    begin
        device_write(4'd0, base[15:0]);
        device_write(4'd1, {10'b0, base[21:16]});
        device_write(4'd2, words);
        device_write(4'd3, 16'h0);
        device_write(4'd4, 16'h0);
    end
endtask

task wait_executed;
    input [15:0] expected;
    input integer limit;
    integer waited;
    begin
        waited = 0;
        device_read(4'd1);
        while (read_result !== expected) begin
            waited = waited + 1;
            if (waited > limit) $fatal(1, "executed_count wait exceeded limit");
            device_read(4'd1);
        end
    end
endtask

task run_ok_case;
    input [15:0] words;
    begin
        do_submit(CMD_BASE, words);
        wait_executed(expected_exec, 200000);
        device_read(4'd2);
        if ((read_result & 16'h0008) != 0) $fatal(1, "unexpected command error");
        expected_exec = expected_exec + 16'd1;
    end
endtask

task run_error_case;
    input [15:0] words;
    begin
        do_submit(CMD_BASE, words);
        wait_executed(expected_exec, 200000);
        device_read(4'd2);
        if ((read_result & 16'h0008) == 0) $fatal(1, "expected command error");
        device_write(4'd5, 16'h0002);
        expected_exec = expected_exec + 16'd1;
    end
endtask

// Store one 64-bit qword as four little-endian 16-bit words.
task put_qword;
    input [21:0] addr;
    input [63:0] value;
    begin
        mem[addr + 0] = value[15:0];
        mem[addr + 1] = value[31:16];
        mem[addr + 2] = value[47:32];
        mem[addr + 3] = value[63:48];
    end
endtask

task put_set_target;
    input [21:0] addr;
    input [21:0] base;
    begin
        put_qword(addr, {10'b0, base, 16'h0000, 8'h01, 8'he0});
    end
endtask

task put_fake_draw_flags;
    input [21:0] addr;
    input [21:0] list;
    input [15:0] tile_count;
    input [1:0] load_op;
    input [15:0] clear_color;
    input [15:0] draw_color;
    input [15:0] row_mask;
    input [15:0] flags;
    reg [31:0] arg0;
    begin
        arg0 = {14'b0, load_op, tile_count};
        put_qword(addr, {arg0, 16'h0000, 8'd3, 8'he1});
        put_qword(addr + 4, {32'h0, 10'b0, list});
        put_qword(addr + 8, {flags, row_mask, draw_color, clear_color});
    end
endtask

task put_fake_draw;
    input [21:0] addr;
    input [21:0] list;
    input [15:0] tile_count;
    input [1:0] load_op;
    input [15:0] clear_color;
    input [15:0] draw_color;
    input [15:0] row_mask;
    begin
        put_fake_draw_flags(addr, list, tile_count, load_op, clear_color,
                            draw_color, row_mask, 16'h0000);
    end
endtask

task put_end;
    input [21:0] addr;
    begin
        put_qword(addr, {32'h0, 16'h0000, 8'h01, 8'hff});
    end
endtask

task do_reset;
    begin
        reset = 1'b1;
        repeat (4) @(posedge clk);
        reset = 1'b0;
        repeat (2) @(posedge clk);
    end
endtask

// Address of pixel (row, col) of `tile` within `base`.
function [21:0] tile_word;
    input [21:0] base;
    input [15:0] tile;
    input integer row;
    input integer col;
    begin
        tile_word = base + {tile[8:0], 8'b0} + row * 16 + col;
    end
endfunction

function [15:0] gradient_pixel;
    input [15:0] base;
    input integer x;
    input integer y;
    reg [3:0] x4, y4, sum4;
    reg [3:0] phase4;
    begin
        x4 = x[3:0];
        y4 = y[3:0];
        phase4 = base[3:0];
        sum4 = x4 + y4 + phase4;
        gradient_pixel = {base[15], x4 + phase4,
                          base[10:9], y4 + phase4, base[4], sum4};
    end
endfunction

task check_uniform_tile;
    input [15:0] tile;
    input [21:0] base;
    input [15:0] value;
    integer r, c;
    begin
        for (r = 0; r < 16; r = r + 1) begin
            for (c = 0; c < 16; c = c + 1) begin
                if (mem[tile_word(base, tile, r, c)] !== value)
                    $fatal(1, "tile %0d row %0d col %0d mismatch", tile, r, c);
            end
        end
    end
endtask

initial begin
    for (i = 0; i < MEM_WORDS; i = i + 1) mem[i] = 16'h0;

`ifdef GPU_RASTER_TEST
    // One CPU-style command buffer: SET_TARGET, inline viewport triangle, END.
    // The triangle spans four tiles, preserving every uncovered pixel on LOAD.
    $display("GPU %0d SCENE RASTER", tseq); tseq = tseq + 1;
    do_reset();
    for (i = 0; i < 32; i = i + 1)
        for (j = 0; j < 32; j = j + 1) begin
            raster_tile = (i / 16) * 25 + (j / 16);
            mem[tile_word(FB_A, raster_tile, i % 16, j % 16)] = 16'h5a5a;
        end
    mem[FB_GUARD] = 16'hbeef;
    put_set_target(CMD_BASE, FB_A);
    put_qword(CMD_BASE + 4, {32'd0, 16'd0, 8'd4, 8'he2});
    put_qword(CMD_BASE + 8,  64'h0);                 // (0, 0)
    put_qword(CMD_BASE + 12, 64'h0000000000000200); // (32, 0)
    put_qword(CMD_BASE + 16, 64'h0000000002000000); // (0, 32)
    put_end(CMD_BASE + 20);
    expected_exec = 16'd1;
    run_ok_case(16'd24);
    // Export the complete 32x32 tile-linear framebuffer crop before local
    // assertions, so an image mismatch still leaves diagnostic artifacts.
    raster_image = $fopen("raster-frame.hex", "w");
    if (raster_image == 0) $fatal(1, "cannot open raster-frame.hex");
    for (i = 0; i < 32; i = i + 1)
        for (j = 0; j < 32; j = j + 1) begin
            raster_tile = (i / 16) * 25 + (j / 16);
            $fdisplay(raster_image, "%04x",
                mem[tile_word(FB_A, raster_tile, i % 16, j % 16)]);
        end
    $fclose(raster_image);
    if (mem[tile_word(FB_A, 0, 1, 1)] !== 16'h0000)
        $fatal(1, "covered origin pixel not committed");
    if (mem[tile_word(FB_A, 0, 4, 8)] !== 16'h0820)
        $fatal(1, "covered tile-0 pixel has wrong gradient");
    if (mem[tile_word(FB_A, 1, 4, 8)] !== 16'h1821)
        $fatal(1, "covered tile-1 pixel has wrong gradient");
    if (mem[tile_word(FB_A, 1, 15, 15)] !== 16'h5a5a)
        $fatal(1, "uncovered pixel changed");
    // Keep direct RTL checks for strictly interior and exterior pixels,
    // including the next tile row. The image differential also checks the
    // diagonal's exact top-left tie against an independent oracle.
    for (i = 0; i < 32; i = i + 1)
        for (j = 0; j < 32; j = j + 1) begin
            raster_tile = (i / 16) * 25 + (j / 16);
            if (i + j != 31) begin
                raster_expected = (i + j < 31)
                    ? (((j >> 3) << 11) | ((i >> 2) << 5) | (j >> 4))
                    : 16'h5a5a;
                if (mem[tile_word(FB_A, raster_tile, i % 16, j % 16)] !== raster_expected)
                    $fatal(1, "raster pixel (%0d,%0d) tile %0d: got %04x expected %04x",
                        j, i, raster_tile,
                        mem[tile_word(FB_A, raster_tile, i % 16, j % 16)], raster_expected);
            end
        end
    if (mem[FB_GUARD] !== 16'hbeef)
        $fatal(1, "raster crossed framebuffer guard");
    $display("DIGITAL_DESIGN_RASTER_PASS");
    $finish;
`else

    // ------------------------------------------------------------------
    // Scenario A: partial-row LOAD preservation and CLEAR initialization.
    // ------------------------------------------------------------------
    $display("GPU %0d SCENE A", tseq); tseq = tseq + 1;
    do_reset();
    for (i = 0; i < 256; i = i + 1) mem[FB_A + 512 + i] = 16'h1000 + i;
    mem[FB_GUARD] = 16'hbeef;
    put_set_target(CMD_BASE, FB_A);
    put_fake_draw(CMD_BASE + 4, LIST_BASE, 16'd1, 2'd0, 16'hffff, 16'h1234, 16'h000f);
    put_fake_draw(CMD_BASE + 16, LIST_BASE + LIST_STRIDE, 16'd1, 2'd1, 16'h0abc, 16'h5678, 16'h00f0);
    put_end(CMD_BASE + 28);
    mem[LIST_BASE] = 16'd2;
    mem[LIST_BASE + LIST_STRIDE] = 16'd5;
    expected_exec = 16'd1;
    run_ok_case(16'd32);

    for (i = 0; i < 16; i = i + 1) begin
        for (j = 0; j < 16; j = j + 1) begin
            if (mem[tile_word(FB_A, 16'd2, i, j)]
                !== ((i < 4) ? 16'h1234 : 16'h1000 + i * 16 + j))
                $fatal(1, "LOAD tile 2 row %0d col %0d mismatch", i, j);
            if (mem[tile_word(FB_A, 16'd5, i, j)]
                !== (((i >= 4) && (i < 8)) ? 16'h5678 : 16'h0abc))
                $fatal(1, "CLEAR tile 5 row %0d col %0d mismatch", i, j);
        end
    end
    if (mem[FB_GUARD] !== 16'hbeef) $fatal(1, "framebuffer guard overwritten");

    // ------------------------------------------------------------------
    // Scenario B: duplicate/unordered indices, then dirty victim eviction.
    // ------------------------------------------------------------------
    $display("GPU %0d SCENE B_UNORDERED", tseq); tseq = tseq + 1;
    do_reset();
    put_set_target(CMD_BASE, FB_A);
    put_fake_draw(CMD_BASE + 4, LIST_BASE, 16'd4, 2'd1, 16'h0, 16'h1111, 16'hffff);
    put_fake_draw(CMD_BASE + 16, LIST_BASE + LIST_STRIDE, 16'd2, 2'd0, 16'h0, 16'h2222, 16'h0001);
    put_end(CMD_BASE + 28);
    mem[LIST_BASE + 0] = 16'd5;
    mem[LIST_BASE + 1] = 16'd2;
    mem[LIST_BASE + 2] = 16'd5;
    mem[LIST_BASE + 3] = 16'd2;
    mem[LIST_BASE + LIST_STRIDE + 0] = 16'd2;
    mem[LIST_BASE + LIST_STRIDE + 1] = 16'd5;
    expected_exec = 16'd1;
    run_ok_case(16'd32);
    for (i = 0; i < 16; i = i + 1) begin
        if (mem[tile_word(FB_A, 16'd2, i, 0)] !== ((i == 0) ? 16'h2222 : 16'h1111))
            $fatal(1, "unordered LOAD tile 2 mismatch");
        if (mem[tile_word(FB_A, 16'd5, i, 0)] !== ((i == 0) ? 16'h2222 : 16'h1111))
            $fatal(1, "unordered LOAD tile 5 mismatch");
    end

    $display("GPU %0d SCENE B_EVICT", tseq); tseq = tseq + 1;
    do_reset();
    for (i = 0; i < 256; i = i + 1) mem[FB_A + 2048 + i] = 16'h2000 + i;
    put_set_target(CMD_BASE, FB_A);
    put_fake_draw(CMD_BASE + 4, LIST_BASE, 16'd1, 2'd0, 16'h0, 16'haaaa, 16'hffff);
    put_fake_draw(CMD_BASE + 16, LIST_BASE + LIST_STRIDE, 16'd1, 2'd0, 16'h0, 16'hbbbb, 16'hffff);
    put_fake_draw(CMD_BASE + 28, LIST_BASE + 2 * LIST_STRIDE, 16'd1, 2'd0, 16'h0, 16'hcccc, 16'h0001);
    put_end(CMD_BASE + 40);
    mem[LIST_BASE] = 16'd0;
    mem[LIST_BASE + LIST_STRIDE] = 16'd8;
    mem[LIST_BASE + 2 * LIST_STRIDE] = 16'd0;
    expected_exec = 16'd1;
    run_ok_case(16'd44);
    check_uniform_tile(16'd8, FB_A, 16'hbbbb);
    for (i = 0; i < 16; i = i + 1) begin
        if (mem[tile_word(FB_A, 16'd0, i, 0)] !== ((i == 0) ? 16'hcccc : 16'haaaa))
            $fatal(1, "eviction-cleaned tile 0 mismatch");
    end

    // ------------------------------------------------------------------
    // Scenario C: END retires only after every dirty entry is clean.
    // ------------------------------------------------------------------
    $display("GPU %0d SCENE C_DRAIN", tseq); tseq = tseq + 1;
    do_reset();
    put_set_target(CMD_BASE, FB_A);
    put_fake_draw(CMD_BASE + 4, LIST_BASE, 16'd3, 2'd1, 16'h0, 16'h0f0f, 16'hffff);
    put_end(CMD_BASE + 16);
    mem[LIST_BASE + 0] = 16'd0;
    mem[LIST_BASE + 1] = 16'd1;
    mem[LIST_BASE + 2] = 16'd2;
    expected_exec = 16'd1;
    run_ok_case(16'd20);
    // Retirement implies the final clean write already committed to memory.
    check_uniform_tile(16'd0, FB_A, 16'h0f0f);
    check_uniform_tile(16'd1, FB_A, 16'h0f0f);
    check_uniform_tile(16'd2, FB_A, 16'h0f0f);

    // A new submission owns a fresh cache namespace and may switch targets
    // without a device reset after the preceding END drained all dirty data.
    $display("GPU %0d SCENE C_GRADIENT", tseq); tseq = tseq + 1;
    put_set_target(CMD_BASE, FB_B);
    put_fake_draw_flags(CMD_BASE + 4, LIST_BASE, 16'd1, 2'd1,
                        16'h0, 16'h8215, 16'hffff, 16'h0001);
    put_end(CMD_BASE + 16);
    mem[LIST_BASE] = 16'd7;
    run_ok_case(16'd20);
    for (i = 0; i < 16; i = i + 1) begin
        for (j = 0; j < 16; j = j + 1) begin
            if (mem[tile_word(FB_B, 16'd7, i, j)] !== gradient_pixel(16'h8215, j, i))
                $fatal(1, "target-switch gradient row %0d col %0d mismatch", i, j);
        end
    end

    // ------------------------------------------------------------------
    // Scenario D: fake draw can generate a tile-local XY gradient.
    // ------------------------------------------------------------------
    $display("GPU %0d SCENE D", tseq); tseq = tseq + 1;
    do_reset();
    put_set_target(CMD_BASE, FB_A);
    put_fake_draw_flags(CMD_BASE + 4, LIST_BASE, 16'd1, 2'd1,
                        16'h0, 16'h8215, 16'hffff, 16'h0001);
    put_end(CMD_BASE + 16);
    mem[LIST_BASE] = 16'd7;
    expected_exec = 16'd1;
    run_ok_case(16'd20);
    for (i = 0; i < 16; i = i + 1) begin
        for (j = 0; j < 16; j = j + 1) begin
            if (mem[tile_word(FB_A, 16'd7, i, j)] !== gradient_pixel(16'h8215, j, i))
                $fatal(1, "gradient tile row %0d col %0d mismatch", i, j);
        end
    end

    // ------------------------------------------------------------------
    // Scenario E: every invalid command class retires with command error.
    // ------------------------------------------------------------------
    $display("GPU %0d SCENE E1", tseq); tseq = tseq + 1;
    do_reset();
    expected_exec = 16'd1;

    // Wrong qword count for FAKE_DRAW (two instead of three).
    put_set_target(CMD_BASE, FB_A);
    put_qword(CMD_BASE + 4, {32'h0, 16'h0000, 8'd2, 8'he1});
    put_qword(CMD_BASE + 8, 64'h0);
    run_error_case(16'd12);

    $display("GPU %0d SCENE E2", tseq); tseq = tseq + 1;
    // Reserved load op 2.
    put_set_target(CMD_BASE, FB_A);
    put_fake_draw(CMD_BASE + 4, LIST_BASE, 16'd0, 2'd2, 16'h0, 16'h0, 16'h0);
    run_error_case(16'd16);

    $display("GPU %0d SCENE E3", tseq); tseq = tseq + 1;
    // Nonzero reserved arg0 bit above the load op.
    put_set_target(CMD_BASE, FB_A);
    put_qword(CMD_BASE + 4, {32'h00040000, 16'h0000, 8'd3, 8'he1});
    put_qword(CMD_BASE + 8, 64'h0);
    put_qword(CMD_BASE + 12, 64'h0);
    run_error_case(16'd16);

    $display("GPU %0d SCENE E4", tseq); tseq = tseq + 1;
    // Nonzero payload-0 high half.
    put_set_target(CMD_BASE, FB_A);
    put_fake_draw(CMD_BASE + 4, LIST_BASE, 16'd0, 2'd0, 16'h0, 16'h0, 16'h0);
    put_qword(CMD_BASE + 8, {32'h00000001, 32'h0});
    run_error_case(16'd16);

    $display("GPU %0d SCENE E5", tseq); tseq = tseq + 1;
    // Nonzero payload-1 reserved bits.
    put_set_target(CMD_BASE, FB_A);
    put_fake_draw(CMD_BASE + 4, LIST_BASE, 16'd0, 2'd0, 16'h0, 16'h0, 16'h0);
    put_qword(CMD_BASE + 12, {16'h0001, 48'h0});
    run_error_case(16'd16);

    $display("GPU %0d SCENE E6", tseq); tseq = tseq + 1;
    // Unaligned tile list.
    put_set_target(CMD_BASE, FB_A);
    put_fake_draw(CMD_BASE + 4, LIST_BASE + 1, 16'd1, 2'd1, 16'h0, 16'h0, 16'h0);
    mem[LIST_BASE + 1] = 16'd0;
    run_error_case(16'd16);

    $display("GPU %0d SCENE E7", tseq); tseq = tseq + 1;
    // Tile list that leaves 22-bit word memory.
    put_set_target(CMD_BASE, FB_A);
    put_fake_draw(CMD_BASE + 4, 22'h3ffff0, 16'd32, 2'd1, 16'h0, 16'h0, 16'h0);
    run_error_case(16'd16);

    $display("GPU %0d SCENE E8", tseq); tseq = tseq + 1;
    // Tile index at the tile limit.
    put_set_target(CMD_BASE, FB_A);
    put_fake_draw(CMD_BASE + 4, LIST_BASE, 16'd1, 2'd1, 16'h0, 16'h0, 16'h0);
    mem[LIST_BASE] = 16'd375;
    run_error_case(16'd16);

    $display("GPU %0d SCENE E9", tseq); tseq = tseq + 1;
    // FAKE_DRAW before any SET_TARGET.
    put_fake_draw(CMD_BASE, LIST_BASE, 16'd0, 2'd1, 16'h0, 16'h0, 16'h0);
    run_error_case(16'd12);

    $display("GPU %0d SCENE E10", tseq); tseq = tseq + 1;
    // A legal empty list performs no read and no error.
    put_set_target(CMD_BASE, FB_A);
    put_fake_draw(CMD_BASE + 4, LIST_BASE, 16'd0, 2'd0, 16'h0, 16'h0, 16'h0);
    put_end(CMD_BASE + 16);
    run_ok_case(16'd20);

    // ------------------------------------------------------------------
    // Scenario E: a command that crosses a 32-byte line boundary.
    // ------------------------------------------------------------------
    $display("GPU %0d SCENE X", tseq); tseq = tseq + 1;
    do_reset();
    put_set_target(CMD_BASE, FB_A);
    put_set_target(CMD_BASE + 4, FB_B);
    put_set_target(CMD_BASE + 8, FB_A);
    put_fake_draw(CMD_BASE + 12, LIST_BASE, 16'd1, 2'd1, 16'h0, 16'h2222, 16'hffff);
    put_end(CMD_BASE + 24);
    mem[LIST_BASE] = 16'd0;
    expected_exec = 16'd1;
    run_ok_case(16'd28);
    if (mem[FB_A] !== 16'h2222) $fatal(1, "cross-line command landed at the wrong target");

    // ------------------------------------------------------------------
    // Scenario F: one active plus two queued submissions, then a rejection.
    // ------------------------------------------------------------------
    $display("GPU %0d SCENE F", tseq); tseq = tseq + 1;
    do_reset();
    put_set_target(CMD_BASE, FB_A);
    put_fake_draw(CMD_BASE + 4, LIST_BASE, 16'd3, 2'd1, 16'h0, 16'h0f0f, 16'hffff);
    put_end(CMD_BASE + 16);
    mem[LIST_BASE + 0] = 16'd0;
    mem[LIST_BASE + 1] = 16'd1;
    mem[LIST_BASE + 2] = 16'd2;
    do_submit(CMD_BASE, 16'd20);
    do_submit(CMD_BASE, 16'd20);
    do_submit(CMD_BASE, 16'd20);
    do_submit(CMD_BASE, 16'd20); // full two-deep FIFO: rejected
    device_read(4'd0);
    if (read_result !== 16'd3) $fatal(1, "expected three accepted submissions");
    device_read(4'd2);
    if ((read_result & 16'h0002) == 0) $fatal(1, "expected fifo-full status");
    if ((read_result & 16'h0004) == 0) $fatal(1, "expected submit rejection");
    device_read(4'd3);
    if (read_result !== 16'd2) $fatal(1, "expected two queued submissions");
    wait_executed(16'd3, 200000);
    device_read(4'd3);
    if (read_result !== 16'd0) $fatal(1, "FIFO not drained after retirement");
    check_uniform_tile(16'd0, FB_A, 16'h0f0f);
    check_uniform_tile(16'd1, FB_A, 16'h0f0f);
    check_uniform_tile(16'd2, FB_A, 16'h0f0f);

    // ------------------------------------------------------------------
    // Scenario G: a target change with resident cache entries is illegal,
    // while re-selecting the same target is legal.
    // ------------------------------------------------------------------
    $display("GPU %0d SCENE G1", tseq); tseq = tseq + 1;
    do_reset();
    put_set_target(CMD_BASE, FB_A);
    put_fake_draw(CMD_BASE + 4, LIST_BASE, 16'd1, 2'd1, 16'h0, 16'h1234, 16'hffff);
    put_set_target(CMD_BASE + 16, FB_B);
    put_end(CMD_BASE + 20);
    mem[LIST_BASE] = 16'd0;
    expected_exec = 16'd1;
    run_error_case(16'd24);

    $display("GPU %0d SCENE G2", tseq); tseq = tseq + 1;
    do_reset();
    put_set_target(CMD_BASE, FB_A);
    put_fake_draw(CMD_BASE + 4, LIST_BASE, 16'd1, 2'd1, 16'h0, 16'h1234, 16'hffff);
    put_set_target(CMD_BASE + 16, FB_A);
    put_end(CMD_BASE + 20);
    mem[LIST_BASE] = 16'd0;
    expected_exec = 16'd1;
    run_ok_case(16'd24);
    check_uniform_tile(16'd0, FB_A, 16'h1234);

    $display("DIGITAL_DESIGN_PASS");
    $finish;
`endif
end
endmodule
