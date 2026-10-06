// Four-bank synchronous framebuffer cache controller.
//
// Storage is four explicit 1024x16 banks (`bank0`..`bank3`), each an inferred
// synchronous RAM with a registered read port and a synchronous write port. The
// portable bank mapping is `bank = x[1:0] ^ {y[0],1'b0}` and
// `addr = plane*512 + line*64 + y*4 + x[3:2]`, matching `ports::bank_address`.
// Each bank is driven by exactly one read/write process per edge with at most
// one read and one write; no same-address read/write dependence is assumed.
//
// Maintenance (the single-outstanding 128-byte refill/writeback engine) is a
// wall-clock owner: it is never gated by the compute `ce` and never abandoned
// once a request has been presented. A presented request and its beat remain
// combinational views of held state; the request valid is registered in a
// dedicated setup state and only withdrawn when the memory accepts it. A dirty
// writeback prefetches beat0 into the single bank-read register group in that
// setup state, then the read address for the next beat is issued on the same
// edge the current write beat is consumed, so admission has no
// M_WLOAD/M_WDATA bubble and continuous write-ready produces 16 continuous
// beats. A terminal ACK may coincide with the last read/write acceptance and is
// validated against the accumulated beat count. A fault cancels only
// unpresented work; accepted transport runs to its terminal response, and no
// partial refill tag or failed dirty clear is published.
//
// The ROP is a serial demand calendar with separate registered color/depth read
// and write phases. It reserves four lane results in the shared registered leaf,
// issuing one lane per enabled edge. The leaf inputs are combinational views of
// the active `RS_ISSUE` lane and the captured old/row state; the leaf is only
// enabled on `ce && !fault` and reset on fault. The row-stream payload is one
// registered 32-bit RAM read per edge using a combinational address
// `{head,rowc[2:0]}`; there is no parallel eight-row snapshot. ROP progression
// and ingress use `ce`; synchronous RAM returns are captured every edge.

module framebuffer_cache #(
    parameter [31:0] COLOR_BASE = 32'd0,
    parameter [31:0] DEPTH_BASE = 32'd16384,
    parameter [9:0]  WIDTH      = 10'd16,
    parameter [8:0]  HEIGHT     = 9'd16
)(
    input             clk,
    input             reset,
    input             ce,
    // Row-stream output ingress (final -> ROP). Publication is on row 7.
    input             row_valid,
    input      [8:0]  row_x,
    input      [7:0]  row_y,
    input      [3:0]  row_mask,
    input      [2:0]  row_index,
    input      [31:0] row_data,
    output            row_accept,
    // Immutable draw context, sampled at row 0.
    input      [2:0]  depth_func,
    input             depth_write,
    input             blend,
    // Status / flush / fault.
    output reg        commit_valid,
    output reg [8:0]  commit_x,
    output reg [7:0]  commit_y,
    output reg [3:0]  commit_mask,
    input             flush_request,
    output reg        flush_complete,
    output            fault,
    output            idle,
    // Single-outstanding 128-byte memory port.
    output            mem_req_valid,
    output            mem_req_write,
    output [31:0]     mem_req_addr,
    input             mem_req_ready,
    input      [63:0] mem_read_data,
    input             mem_read_valid,
    output            mem_write_valid,
    output [63:0]     mem_write_data,
    input             mem_write_ready,
    input             mem_complete_valid,
    input             mem_complete_ok
);
    // ---- Address helpers (portable inferred BSRAM layout) ----
    function [9:0] fbaddr;
        input        plane;
        input [2:0]  line;
        input [3:0]  x;
        input [3:0]  y;
        begin
            fbaddr = (plane ? 10'd512 : 10'd0) + ({7'd0, line} << 6)
                + {y, 2'b00} + {8'd0, x[3:2]};
        end
    endfunction

    function [1:0] fbbank;
        input [3:0] x;
        input [3:0] y;
        begin
            fbbank = x[1:0] ^ {y[0], 1'b0};
        end
    endfunction

    // ---- Four explicit synchronous 1024x16 banks ----
    // Data arrays are deliberately never reset; only the read registers and the
    // control state are reset.
    reg [15:0] bank0 [0:1023];
    reg [15:0] bank1 [0:1023];
    reg [15:0] bank2 [0:1023];
    reg [15:0] bank3 [0:1023];
    reg [15:0] rd_data [0:3];
    reg [9:0]  rda0, rda1, rda2, rda3;
    reg        rde0, rde1, rde2, rde3;
    reg [9:0]  wra0, wra1, wra2, wra3;
    reg        wre0, wre1, wre2, wre3;
    reg [15:0] wrd0, wrd1, wrd2, wrd3;

    // ---- Tag / line state ----
    reg [15:0] tag [0:7];
    reg        tvalid [0:7];
    reg        dirc [0:7];
    reg        dird [0:7];
    reg [2:0]  victim;

    // ---- Row-stream payload (one registered read per edge) ----
    reg [31:0] oram [0:15];
    reg [31:0] ordout;
    reg [8:0]  s_hx [0:1];
    reg [7:0]  s_hy [0:1];
    reg [3:0]  s_hm [0:1];
    reg [2:0]  s_func [0:1];
    reg        s_dw [0:1];
    reg        s_blend [0:1];
    reg        slot_ready [0:1];
    reg        head;
    reg        tail;
    reg [3:0]  fill_row;

    // ---- ROP sequencer state ----
    localparam RS_IDLE  = 4'd0;
    localparam RS_ROWS  = 4'd1;
    localparam RS_RDCOL = 4'd2;
    localparam RS_RDDEP = 4'd3;
    localparam RS_CAP   = 4'd4;
    localparam RS_ISSUE = 4'd5;
    localparam RS_WAIT  = 4'd6;
    localparam RS_WRCOL = 4'd7;
    localparam RS_WRDEP = 4'd8;
    localparam RS_DONE  = 4'd9;
    reg [3:0]  rst;
    reg        rop_active;
    reg [2:0]  rop_line;
    reg [8:0]  rop_x;
    reg [7:0]  rop_y;
    reg [3:0]  rop_mask;
    reg [2:0]  rop_func;
    reg        rop_dw;
    reg        rop_blend;
    reg [3:0]  rowc;
    reg [31:0] rowbuf [0:7];
    reg [15:0] oldc [0:3];
    reg [15:0] oldd [0:3];
    reg [1:0]  cbk [0:3];
    reg [1:0]  dbk [0:3];
    reg [1:0]  issue;
    reg [3:0]  got;
    reg [15:0] resc [0:3];
    reg [15:0] resd [0:3];
    reg        rcw [0:3];
    reg        rdw [0:3];

    // ---- Maintenance sequencer state ----
    localparam M_IDLE  = 4'd0;
    localparam M_RPRE  = 4'd1;
    localparam M_RREQ  = 4'd2;
    localparam M_RBEAT = 4'd3;
    localparam M_RACK  = 4'd4;
    localparam M_WPRE  = 4'd5;
    localparam M_WREQ  = 4'd6;
    localparam M_WBEAT = 4'd7;
    localparam M_WACK  = 4'd8;
    reg [3:0]  mst;
    reg        mt_active;
    reg [2:0]  mt_line;
    reg        mt_tvalid;
    reg [15:0] mt_target;
    reg        mt_write;
    reg        mt_plane;
    reg [1:0]  mt_sector;
    reg [4:0]  mt_beat;
    reg        mt_accepted;
    reg        mt_presented;
    reg        req_valid;
    reg        wr_beat_valid;

    reg        flushing;

    // Sticky fault is set either by the compute path (ingress/header violation)
    // or by the maintenance path (failed/early terminal). A single wire keeps the
    // two clocked owners from driving `fault` directly.
    reg        fault_c;
    reg        fault_m;
    assign fault = fault_c | fault_m;

    // ---- Shared registered leaf: combinational input views of active state ----
    // The active `RS_ISSUE` lane supplies valid/key/operands directly; there is no
    // redundant registered input holding that could lose a prepared lane on CE0.
    wire [6:0]  leaf_row_sel = {issue, 1'b0};
    wire        leaf_in_valid = (rst == RS_ISSUE);
    wire [1:0]  leaf_in_key = issue;
    wire [15:0] leaf_old_color = oldc[issue];
    wire [15:0] leaf_old_depth = oldd[issue];
    wire [7:0]  leaf_src_r = rowbuf[leaf_row_sel][7:0];
    wire [7:0]  leaf_src_g = rowbuf[leaf_row_sel][15:8];
    wire [7:0]  leaf_src_b = rowbuf[leaf_row_sel][23:16];
    wire [7:0]  leaf_src_a = rowbuf[leaf_row_sel][31:24];
    wire [15:0] leaf_src_depth = rowbuf[leaf_row_sel + 7'd1][15:0];
    wire        leaf_covered = rop_mask[issue];
    wire        leaf_in_ready;
    wire        leaf_out_valid;
    wire [1:0]  leaf_out_key;
    wire [15:0] leaf_new_color;
    wire [15:0] leaf_new_depth;
    wire        leaf_cw;
    wire        leaf_dw;
    rop_leaf leaf (
        .clk(clk),
        .reset(reset || fault),
        .ce(ce && !fault),
        .in_valid(leaf_in_valid),
        .in_key(leaf_in_key),
        .blend(rop_blend),
        .depth_func(rop_func),
        .depth_write(rop_dw),
        .covered(leaf_covered),
        .old_color(leaf_old_color),
        .old_depth(leaf_old_depth),
        .src_r(leaf_src_r),
        .src_g(leaf_src_g),
        .src_b(leaf_src_b),
        .src_a(leaf_src_a),
        .src_depth(leaf_src_depth),
        .in_ready(leaf_in_ready),
        .out_valid(leaf_out_valid),
        .out_key(leaf_out_key),
        .new_color(leaf_new_color),
        .new_depth(leaf_new_depth),
        .color_written(leaf_cw),
        .depth_written(leaf_dw)
    );

    // ---- Combinational tag lookup for the head slot ----
    wire [8:0] head_tile = ({1'b0, s_hy[head]} >> 4) * (WIDTH >> 4)
        + ({5'd0, s_hx[head]} >> 4);
    wire       full = slot_ready[tail];

    integer    k;
    reg        hit;
    reg [2:0]  hit_line;
    reg        free_found;
    reg [2:0]  free_line;
    always @(*) begin
        hit = 1'b0;
        hit_line = 3'd0;
        free_found = 1'b0;
        free_line = 3'd0;
        for (k = 0; k < 8; k = k + 1) begin
            if (tvalid[k] && tag[k] == head_tile) begin
                hit = 1'b1;
                hit_line = k[2:0];
            end
            if (!tvalid[k] && !free_found) begin
                free_found = 1'b1;
                free_line = k[2:0];
            end
        end
    end

    // Selected victim line and its dirty state, used to begin maintenance.
    wire [2:0]  sel_line = free_found ? free_line : victim;
    wire        sel_dirc = dirc[sel_line];
    wire        sel_dird = dird[sel_line];
    wire        sel_dirty = sel_dirc | sel_dird;
    wire        sel_plane = sel_dirc ? 1'b0 : 1'b1;

    // ---- Lane geometry for the active quad ----
    wire [3:0] rop_lx [0:3];
    wire [3:0] rop_ly [0:3];
    assign rop_lx[0] = rop_x[3:0];
    assign rop_lx[1] = rop_x[3:0] + 4'd1;
    assign rop_lx[2] = rop_x[3:0];
    assign rop_lx[3] = rop_x[3:0] + 4'd1;
    assign rop_ly[0] = rop_y[3:0];
    assign rop_ly[1] = rop_y[3:0];
    assign rop_ly[2] = rop_y[3:0] + 4'd1;
    assign rop_ly[3] = rop_y[3:0] + 4'd1;

    // ---- Maintenance beat coordinates, computed from held state ----
    wire [9:0] cur_word = {mt_sector, 6'b000000} + {mt_beat, 2'b00};
    wire [9:0] nxt_word = cur_word + 10'd4;
    reg [1:0]  cur_bank [0:3];
    reg [9:0]  cur_addr [0:3];
    reg [1:0]  nxt_bank [0:3];
    reg [9:0]  nxt_addr [0:3];
    integer    j;
    always @(*) begin
        for (j = 0; j < 4; j = j + 1) begin
            cur_bank[j] = fbbank((cur_word + j) & 4'hf, (cur_word + j) >> 4);
            cur_addr[j] = fbaddr(mt_plane, mt_line, (cur_word + j) & 4'hf,
                                 (cur_word + j) >> 4);
            nxt_bank[j] = fbbank((nxt_word + j) & 4'hf, (nxt_word + j) >> 4);
            nxt_addr[j] = fbaddr(mt_plane, mt_line, (nxt_word + j) & 4'hf,
                                 (nxt_word + j) >> 4);
        end
    end

    // Beat handshakes as combinational views of held old state. A write beat is
    // only consumed once its request has been accepted (or is accepted on this
    // same edge); a read beat is only consumed once accepted.
    wire req_accept = req_valid && mem_req_ready;
    wire wbeat      = mt_active && mt_write && wr_beat_valid && mem_write_ready
        && (mt_accepted || req_accept);
    wire rbeat      = mt_active && !mt_write && (mt_accepted || req_accept)
        && mt_beat < 5'd16 && mem_read_valid;
    wire refill_write = rbeat;
    wire use_nxt    = (mst == M_WREQ) || (mst == M_WBEAT);

    // Issue the next four-bank skid read when beat0 is loaded or a write beat is
    // consumed. This is the only place the writeback bank read is asserted.
    wire m_rd_issue = mt_active && mt_write &&
        ( (mst == M_WPRE) ||
          (mst == M_WREQ && wbeat) ||
          (mst == M_WBEAT && wbeat && mt_beat < 5'd15) );

    // Accumulated beats including the current edge, for terminal validation.
    wire [5:0] beats_total = {1'b0, mt_beat} + (rbeat ? 6'd1 : 6'd0)
        + (wbeat ? 6'd1 : 6'd0);

    // ---- Bank port control (single combinational owner) ----
    // Four explicit synchronous processes below consume these enables. With the
    // owners mutually exclusive there is at most one access per bank port/edge.
    // Maintenance is wall-clock owned and ungated; ROP accesses are issued only on
    // effective compute edges (`ce && !fault`), so their read returns hold across
    // CE0 and fault.
    integer    c;
    reg [1:0]  pb;
    reg [9:0]  pa;
    reg [15:0] mslice;
    always @(*) begin
        rde0 = 1'b0; rde1 = 1'b0; rde2 = 1'b0; rde3 = 1'b0;
        rda0 = 10'd0; rda1 = 10'd0; rda2 = 10'd0; rda3 = 10'd0;
        wre0 = 1'b0; wre1 = 1'b0; wre2 = 1'b0; wre3 = 1'b0;
        wra0 = 10'd0; wra1 = 10'd0; wra2 = 10'd0; wra3 = 10'd0;
        wrd0 = 16'd0; wrd1 = 16'd0; wrd2 = 16'd0; wrd3 = 16'd0;
        if (mt_active) begin
            if (m_rd_issue) begin
                for (c = 0; c < 4; c = c + 1) begin
                    pb = use_nxt ? nxt_bank[c] : cur_bank[c];
                    pa = use_nxt ? nxt_addr[c] : cur_addr[c];
                    case (pb)
                        2'd0: begin rde0 = 1'b1; rda0 = pa; end
                        2'd1: begin rde1 = 1'b1; rda1 = pa; end
                        2'd2: begin rde2 = 1'b1; rda2 = pa; end
                        default: begin rde3 = 1'b1; rda3 = pa; end
                    endcase
                end
            end
            if (refill_write) begin
                // Lane `c` of the incoming beat always holds pixel `cur_word+c`;
                // the destination bank rotates with the pixel geometry, so the
                // data slice follows the lane, never the bank index.
                for (c = 0; c < 4; c = c + 1) begin
                    pb = cur_bank[c];
                    pa = cur_addr[c];
                    mslice = mem_read_data[c * 16 +: 16];
                    case (pb)
                        2'd0: begin wre0 = 1'b1; wra0 = pa; wrd0 = mslice; end
                        2'd1: begin wre1 = 1'b1; wra1 = pa; wrd1 = mslice; end
                        2'd2: begin wre2 = 1'b1; wra2 = pa; wrd2 = mslice; end
                        default: begin wre3 = 1'b1; wra3 = pa; wrd3 = mslice; end
                    endcase
                end
            end
        end else if (rop_active && ce && !fault) begin
            if (rst == RS_RDCOL) begin
                for (c = 0; c < 4; c = c + 1) begin
                    pb = cbk[c];
                    pa = fbaddr(1'b0, rop_line, rop_lx[c], rop_ly[c]);
                    case (pb)
                        2'd0: begin rde0 = 1'b1; rda0 = pa; end
                        2'd1: begin rde1 = 1'b1; rda1 = pa; end
                        2'd2: begin rde2 = 1'b1; rda2 = pa; end
                        default: begin rde3 = 1'b1; rda3 = pa; end
                    endcase
                end
            end else if (rst == RS_RDDEP) begin
                for (c = 0; c < 4; c = c + 1) begin
                    pb = dbk[c];
                    pa = fbaddr(1'b1, rop_line, rop_lx[c], rop_ly[c]);
                    case (pb)
                        2'd0: begin rde0 = 1'b1; rda0 = pa; end
                        2'd1: begin rde1 = 1'b1; rda1 = pa; end
                        2'd2: begin rde2 = 1'b1; rda2 = pa; end
                        default: begin rde3 = 1'b1; rda3 = pa; end
                    endcase
                end
            end else if (rst == RS_WRCOL) begin
                for (c = 0; c < 4; c = c + 1) begin
                    if (rcw[c]) begin
                        pb = cbk[c];
                        pa = fbaddr(1'b0, rop_line, rop_lx[c], rop_ly[c]);
                        case (pb)
                            2'd0: begin wre0 = 1'b1; wra0 = pa; wrd0 = resc[c]; end
                            2'd1: begin wre1 = 1'b1; wra1 = pa; wrd1 = resc[c]; end
                            2'd2: begin wre2 = 1'b1; wra2 = pa; wrd2 = resc[c]; end
                            default: begin wre3 = 1'b1; wra3 = pa; wrd3 = resc[c]; end
                        endcase
                    end
                end
            end else if (rst == RS_WRDEP) begin
                for (c = 0; c < 4; c = c + 1) begin
                    if (rdw[c]) begin
                        pb = dbk[c];
                        pa = fbaddr(1'b1, rop_line, rop_lx[c], rop_ly[c]);
                        case (pb)
                            2'd0: begin wre0 = 1'b1; wra0 = pa; wrd0 = resd[c]; end
                            2'd1: begin wre1 = 1'b1; wra1 = pa; wrd1 = resd[c]; end
                            2'd2: begin wre2 = 1'b1; wra2 = pa; wrd2 = resd[c]; end
                            default: begin wre3 = 1'b1; wra3 = pa; wrd3 = resd[c]; end
                        endcase
                    end
                end
            end
        end
    end

    // ---- Four explicit synchronous bank read/write processes ----
    // A bank read register holds its previous value whenever no read is issued.
    always @(posedge clk) begin
        if (reset) rd_data[0] <= 16'd0;
        else if (rde0) rd_data[0] <= bank0[rda0];
        if (wre0) bank0[wra0] <= wrd0;
    end
    always @(posedge clk) begin
        if (reset) rd_data[1] <= 16'd0;
        else if (rde1) rd_data[1] <= bank1[rda1];
        if (wre1) bank1[wra1] <= wrd1;
    end
    always @(posedge clk) begin
        if (reset) rd_data[2] <= 16'd0;
        else if (rde2) rd_data[2] <= bank2[rda2];
        if (wre2) bank2[wra2] <= wrd2;
    end
    always @(posedge clk) begin
        if (reset) rd_data[3] <= 16'd0;
        else if (rde3) rd_data[3] <= bank3[rda3];
        if (wre3) bank3[wra3] <= wrd3;
    end

    // ---- Row payload registered read, combinational row address ----
    // The address is `{head,rowc[2:0]}` and the enable is limited to the active
    // row-strobe edge with `rowc<8`; `ordout` holds across CE0. RS_ROWS with
    // rowc=k (1..8) therefore captures the row read on the previous edge (k-1),
    // so row0 is captured once and row7 is not dropped.
    wire [3:0] row_raddr = {head, rowc[2:0]};
    wire       row_ren = ce && !fault && (rst == RS_ROWS) && (rowc < 4'd8);
    always @(posedge clk) begin
        if (reset) ordout <= 32'd0;
        else if (row_ren) ordout <= oram[row_raddr];
    end

    // ---- Memory request / write data are combinational views of held state ----
    wire [15:0] mt_tile = mt_write ? tag[mt_line] : mt_target;
    assign mem_req_valid  = req_valid;
    assign mem_req_write  = mt_write;
    assign mem_req_addr   = (mt_plane ? DEPTH_BASE : COLOR_BASE)
        + {mt_tile, 9'b0} + {18'd0, mt_sector, 7'b0};
    assign mem_write_valid = wr_beat_valid;
    assign mem_write_data  = {rd_data[cur_bank[3]], rd_data[cur_bank[2]],
                              rd_data[cur_bank[1]], rd_data[cur_bank[0]]};

    assign row_accept = ce && !flushing && !fault && !full && row_valid;
    assign idle = !rop_active && !mt_active && !slot_ready[0] && !slot_ready[1]
        && fill_row == 4'd0;

    wire ingress_error = ce && row_valid && !flushing && !full && (
        row_index != fill_row
        || (fill_row != 4'd0 && {row_x,row_y,row_mask} != {s_hx[tail],s_hy[tail],s_hm[tail]})
        || (fill_row != 4'd0 && {depth_func,depth_write,blend} != {s_func[tail],s_dw[tail],s_blend[tail]})
        || (row_index[0] && row_data[31:16] != 16'd0)
        || (fill_row == 4'd0 && (row_x[0] || row_y[0]
            || ({1'b0,row_x}+10'd1) >= WIDTH || ({1'b0,row_y}+9'd1) >= HEIGHT
            || row_mask == 4'd0)));
    wire extra_read = mt_active && mem_read_valid
        && (mt_write || !(mt_accepted || req_accept) || mt_beat >= 5'd16);
    // A new fault on the terminal edge must suppress tag publication/dirty clear
    // immediately; testing only the old sticky register would publish once.
    wire stop_publication = fault || ingress_error || extra_read;

    // ---- Single clocked owner: wall-clock maintenance plus ce-gated compute ----
    // All tag/dirty/valid/slot state is updated here so no register is driven by
    // two processes. Maintenance advances on every edge and is never gated by
    // `ce` or by `fault` once presented; ingress and the ROP advance only on
    // effective `ce` edges. A sticky fault stops compute and clears its queued
    // work without disturbing presented/accepted transport.
    integer f;
    reg     fstarted;
    always @(posedge clk) begin
        if (reset) begin
            mst <= M_IDLE;
            mt_active <= 1'b0;
            mt_accepted <= 1'b0;
            mt_presented <= 1'b0;
            req_valid <= 1'b0;
            wr_beat_valid <= 1'b0;
            fault_m <= 1'b0;
            mt_write <= 1'b0;
            mt_plane <= 1'b0;
            mt_sector <= 2'd0;
            mt_beat <= 5'd0;
            mt_line <= 3'd0;
            mt_tvalid <= 1'b0;
            mt_target <= 16'd0;
            rop_active <= 1'b0;
            rst <= RS_IDLE;
            flushing <= 1'b0;
            flush_complete <= 1'b0;
            fault_c <= 1'b0;
            commit_valid <= 1'b0;
            slot_ready[0] <= 1'b0;
            slot_ready[1] <= 1'b0;
            fill_row <= 4'd0;
            head <= 1'b0;
            tail <= 1'b0;
            victim <= 3'd0;
            issue <= 2'd0;
            got <= 4'd0;
            for (f = 0; f < 8; f = f + 1) begin
                tvalid[f] <= 1'b0;
                dirc[f] <= 1'b0;
                dird[f] <= 1'b0;
                tag[f] <= 16'd0;
            end
        end else begin
            if (extra_read) fault_m <= 1'b1;
            // A fault cancels only work that has never been presented. A
            // presented request stays stable until accepted; accepted transport
            // runs to its terminal response.
            if (mt_active && fault && !mt_accepted && !mt_presented) begin
                mt_active <= 1'b0;
                mst <= M_IDLE;
                req_valid <= 1'b0;
                wr_beat_valid <= 1'b0;
            end else if (mt_active) begin
                if ((mt_accepted || req_accept) && mem_complete_valid) begin
                    // Terminal response: validate the accumulated beat count.
                    if (stop_publication) begin
                        // Already faulted: terminate without publishing.
                        mt_active <= 1'b0; mst <= M_IDLE;
                        req_valid <= 1'b0; wr_beat_valid <= 1'b0;
                    end else if (!mem_complete_ok) begin
                        fault_m <= 1'b1;
                        mt_active <= 1'b0; mst <= M_IDLE;
                        req_valid <= 1'b0; wr_beat_valid <= 1'b0;
                    end else if (beats_total != 6'd16) begin
                        // A success terminal before 16 beats is a protocol fault.
                        fault_m <= 1'b1;
                        mt_active <= 1'b0; mst <= M_IDLE;
                        req_valid <= 1'b0; wr_beat_valid <= 1'b0;
                    end else if (mt_sector != 2'd3) begin
                        mt_sector <= mt_sector + 2'd1;
                        mt_beat <= 5'd0;
                        mt_accepted <= 1'b0;
                        mt_presented <= 1'b0;
                        req_valid <= 1'b0;
                        wr_beat_valid <= 1'b0;
                        mst <= mt_write ? M_WPRE : M_RPRE;
                    end else if (mt_write) begin
                        if (mt_plane == 1'b0) dirc[mt_line] <= 1'b0;
                        else dird[mt_line] <= 1'b0;
                        if (mt_plane == 1'b0 && dird[mt_line]) begin
                            mt_plane <= 1'b1;
                            mt_sector <= 2'd0; mt_beat <= 5'd0;
                            mt_accepted <= 1'b0; mt_presented <= 1'b0;
                            req_valid <= 1'b0; wr_beat_valid <= 1'b0;
                            mst <= M_WPRE;
                        end else if (mt_plane == 1'b1 && dirc[mt_line]) begin
                            mt_plane <= 1'b0;
                            mt_sector <= 2'd0; mt_beat <= 5'd0;
                            mt_accepted <= 1'b0; mt_presented <= 1'b0;
                            req_valid <= 1'b0; wr_beat_valid <= 1'b0;
                            mst <= M_WPRE;
                        end else if (mt_tvalid) begin
                            mt_write <= 1'b0;
                            mt_plane <= 1'b0;
                            mt_sector <= 2'd0; mt_beat <= 5'd0;
                            mt_accepted <= 1'b0; mt_presented <= 1'b0;
                            req_valid <= 1'b0; wr_beat_valid <= 1'b0;
                            tvalid[mt_line] <= 1'b0;
                            mst <= M_RPRE;
                        end else begin
                            mt_active <= 1'b0; mst <= M_IDLE;
                        end
                    end else if (mt_plane == 1'b0) begin
                        mt_plane <= 1'b1;
                        mt_sector <= 2'd0; mt_beat <= 5'd0;
                        mt_accepted <= 1'b0; mt_presented <= 1'b0;
                        req_valid <= 1'b0; wr_beat_valid <= 1'b0;
                        mst <= M_RPRE;
                    end else begin
                        tag[mt_line] <= mt_target;
                        tvalid[mt_line] <= 1'b1;
                        mt_active <= 1'b0; mst <= M_IDLE;
                    end
                end else begin
                    case (mst)
                        M_RPRE: begin
                            req_valid <= 1'b1;
                            mt_presented <= 1'b1;
                            mst <= M_RREQ;
                        end
                        M_RREQ: begin
                            if (req_accept) begin
                                req_valid <= 1'b0;
                                mt_accepted <= 1'b1;
                                if (rbeat) mt_beat <= 5'd1;
                                mst <= M_RBEAT;
                            end
                        end
                        M_RBEAT: begin
                            if (rbeat && mt_beat == 5'd15) begin
                                mt_beat <= 5'd16;
                                mst <= M_RACK;
                            end else if (rbeat) begin
                                mt_beat <= mt_beat + 5'd1;
                            end
                        end
                        M_RACK: begin
                        end
                        M_WPRE: begin
                            req_valid <= 1'b1;
                            wr_beat_valid <= 1'b1;
                            mt_presented <= 1'b1;
                            mst <= M_WREQ;
                        end
                        M_WREQ: begin
                            if (req_accept) begin
                                req_valid <= 1'b0;
                                mt_accepted <= 1'b1;
                                if (wbeat) mt_beat <= 5'd1;
                                mst <= M_WBEAT;
                            end
                        end
                        M_WBEAT: begin
                            if (wbeat) begin
                                if (mt_beat == 5'd15) begin
                                    mt_beat <= 5'd16;
                                    wr_beat_valid <= 1'b0;
                                    mst <= M_WACK;
                                end else begin
                                    mt_beat <= mt_beat + 5'd1;
                                end
                            end
                        end
                        M_WACK: begin
                        end
                        default: mst <= M_IDLE;
                    endcase
                end
            end

            // Sticky-fault cleanup for compute-owned work: cancel queued slots,
            // partial row fills and the active quad/leaf. Presented or accepted
            // maintenance is owned by the wall-clock FSM above and is untouched,
            // and no tag publication or dirty clear happens on the fault edge.
            if (fault) begin
                rop_active <= 1'b0;
                rst <= RS_IDLE;
                slot_ready[0] <= 1'b0;
                slot_ready[1] <= 1'b0;
                fill_row <= 4'd0;
            end

            // ---- Compute boundary: ingress and ROP advance only on ce ----
            commit_valid <= 1'b0;

            if (flush_request && !flushing && !flush_complete && !fault) begin
                flushing <= 1'b1;
                flush_complete <= 1'b0;
            end

            if (ce && !fault) begin
                // ---- Row ingress with explicit header/context/reserved checks ----
                if (row_valid && !flushing && !full) begin
                    if (row_index != fill_row
                        || (fill_row != 4'd0
                            && {row_x, row_y, row_mask}
                               != {s_hx[tail], s_hy[tail], s_hm[tail]})
                        || (fill_row != 4'd0
                            && {depth_func, depth_write, blend}
                               != {s_func[tail], s_dw[tail], s_blend[tail]})
                        || (row_index[0] && row_data[31:16] != 16'd0)
                        || (fill_row == 4'd0
                            && (row_x[0] || row_y[0]
                                || ({1'b0, row_x} + 10'd1) >= WIDTH
                                || ({1'b0, row_y} + 9'd1) >= HEIGHT
                                || row_mask == 4'd0))) begin
                        fault_c <= 1'b1;
                    end else begin
                        oram[{tail, row_index}] <= row_data;
                        if (row_index == 3'd0) begin
                            s_hx[tail] <= row_x;
                            s_hy[tail] <= row_y;
                            s_hm[tail] <= row_mask;
                            s_func[tail] <= depth_func;
                            s_dw[tail] <= depth_write;
                            s_blend[tail] <= blend;
                        end
                        if (row_index == 3'd7) begin
                            slot_ready[tail] <= 1'b1;
                            tail <= ~tail;
                            fill_row <= 4'd0;
                        end else begin
                            fill_row <= {1'b0, row_index} + 4'd1;
                        end
                    end
                end

                // ---- Start a quad or a demand refill ----
                if (!mt_active && !rop_active) begin
                    if (slot_ready[head]) begin
                        if (hit) begin
                            rop_active <= 1'b1;
                            rop_line <= hit_line;
                            rst <= RS_ROWS;
                            rowc <= 4'd0;
                            rop_x <= s_hx[head];
                            rop_y <= s_hy[head];
                            rop_mask <= s_hm[head];
                            rop_func <= s_func[head];
                            rop_dw <= s_dw[head];
                            rop_blend <= s_blend[head];
                        end else begin
                            mt_active <= 1'b1;
                            mt_line <= sel_line;
                            victim <= (free_found ? free_line : victim) + 3'd1;
                            mt_tvalid <= 1'b1;
                            mt_target <= head_tile;
                            mt_write <= sel_dirty;
                            // A refill always starts on the color plane; a
                            // writeback starts on the first dirty plane.
                            mt_plane <= sel_dirty ? sel_plane : 1'b0;
                            mt_sector <= 2'd0;
                            mt_beat <= 5'd0;
                            mt_accepted <= 1'b0;
                            mt_presented <= 1'b0;
                            req_valid <= 1'b0;
                            wr_beat_valid <= 1'b0;
                            if (!sel_dirty) begin
                                tvalid[sel_line] <= 1'b0;
                                mst <= M_RPRE;
                            end else begin
                                mst <= M_WPRE;
                            end
                        end
                    end else if (flushing) begin
                        fstarted = 1'b0;
                        for (f = 0; f < 8; f = f + 1) begin
                            if (!fstarted && (dirc[f] || dird[f])) begin
                                fstarted = 1'b1;
                                mt_active <= 1'b1;
                                mt_line <= f[2:0];
                                mt_tvalid <= 1'b0;
                                mt_write <= 1'b1;
                                mt_plane <= dirc[f] ? 1'b0 : 1'b1;
                                mt_sector <= 2'd0;
                                mt_beat <= 5'd0;
                                mt_accepted <= 1'b0;
                                mt_presented <= 1'b0;
                                req_valid <= 1'b0;
                                wr_beat_valid <= 1'b0;
                                mst <= M_WPRE;
                            end
                        end
                        if (!fstarted) begin
                            flushing <= 1'b0;
                            flush_complete <= 1'b1;
                        end
                    end
                end

                // ---- ROP sequencer ----
                if (rop_active) begin
                    case (rst)
                        RS_ROWS: begin
                            if (rowc != 4'd0) rowbuf[rowc - 4'd1] <= ordout;
                            if (rowc == 4'd8) begin
                                cbk[0] <= fbbank(rop_x[3:0], rop_y[3:0]);
                                cbk[1] <= fbbank(rop_x[3:0] + 4'd1, rop_y[3:0]);
                                cbk[2] <= fbbank(rop_x[3:0], rop_y[3:0] + 4'd1);
                                cbk[3] <= fbbank(rop_x[3:0] + 4'd1, rop_y[3:0] + 4'd1);
                                dbk[0] <= fbbank(rop_x[3:0], rop_y[3:0]);
                                dbk[1] <= fbbank(rop_x[3:0] + 4'd1, rop_y[3:0]);
                                dbk[2] <= fbbank(rop_x[3:0], rop_y[3:0] + 4'd1);
                                dbk[3] <= fbbank(rop_x[3:0] + 4'd1, rop_y[3:0] + 4'd1);
                                rst <= RS_RDCOL;
                            end else begin
                                rowc <= rowc + 4'd1;
                            end
                        end
                        RS_RDCOL: rst <= RS_RDDEP;
                        RS_RDDEP: begin
                            oldc[0] <= rd_data[cbk[0]];
                            oldc[1] <= rd_data[cbk[1]];
                            oldc[2] <= rd_data[cbk[2]];
                            oldc[3] <= rd_data[cbk[3]];
                            rst <= RS_CAP;
                        end
                        RS_CAP: begin
                            oldd[0] <= rd_data[dbk[0]];
                            oldd[1] <= rd_data[dbk[1]];
                            oldd[2] <= rd_data[dbk[2]];
                            oldd[3] <= rd_data[dbk[3]];
                            issue <= 2'd0;
                            got <= 4'd0;
                            rst <= RS_ISSUE;
                        end
                        RS_ISSUE: begin
                            // The leaf input is a combinational view of this lane;
                            // advance only when the leaf actually accepts.
                            if (leaf_in_ready) begin
                                if (issue == 2'd3) rst <= RS_WAIT;
                                else issue <= issue + 2'd1;
                            end
                        end
                        RS_WAIT: begin
                            // Each returned key is reserved at most once.
                            if (leaf_out_valid && !got[leaf_out_key]) begin
                                resc[leaf_out_key] <= leaf_new_color;
                                resd[leaf_out_key] <= leaf_new_depth;
                                rcw[leaf_out_key] <= leaf_cw;
                                rdw[leaf_out_key] <= leaf_dw;
                                got <= got | (4'b0001 << leaf_out_key);
                                if ((got | (4'b0001 << leaf_out_key)) == 4'b1111)
                                    rst <= RS_WRCOL;
                            end
                        end
                        RS_WRCOL: rst <= RS_WRDEP;
                        RS_WRDEP: begin
                            if (rcw[0] || rcw[1] || rcw[2] || rcw[3])
                                dirc[rop_line] <= 1'b1;
                            if (rdw[0] || rdw[1] || rdw[2] || rdw[3])
                                dird[rop_line] <= 1'b1;
                            rst <= RS_DONE;
                        end
                        RS_DONE: begin
                            commit_valid <= 1'b1;
                            commit_x <= rop_x;
                            commit_y <= rop_y;
                            commit_mask <= rop_mask;
                            slot_ready[head] <= 1'b0;
                            head <= ~head;
                            rop_active <= 1'b0;
                            rst <= RS_IDLE;
                        end
                        default: rst <= RS_ROWS;
                    endcase
                end
            end
        end
    end
endmodule
