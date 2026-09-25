`timescale 1ns/1ps
// GPU rasterizer leaf. The default stage-5 build consumes snapped 2D
// viewport vertices and performs setup -> tile -> quad. VIEWPORT_ONLY=0
// enables the unfitted clip/rcp/viewport geometry probe for differential
// testing; that path belongs to the later frontend stage.
//
// The stage decomposition, FIFO depths, prefetch limit (K = 2), merger
// pin/tile-end rules and every fixed-point arithmetic leaf match the Rust
// emu; the emitted TRI/PREFETCH/QUAD/TILE_END/RETIRE_MARKER/DONE values are
// bit-exact against the functional reference. Geometry-probe device
// arbitration is collapsed: its grants only change cycle timing. The
// co-simulation compares per-kind values, not cycle timing.
//
// Input is a serial vertex beat stream with `input_last` on the last beat
// of a scene. Output is an ordered stream (covered quads, tile-end, and
// per-input-triangle retire markers) drained by `sink_ready`,
// plus a prefetch-acquire observation port for the later cache integration.
//
// Trace (`$display`, simulation-only): one `RAST <seq> <body>` line per
// event, emitted where the event happens; the quad stream itself flows
// through the real output FIFO, so the trace is not a bypass.
//
// Signed-arithmetic discipline (house rule): no signed shift and no signed
// comparison is ever nested inside a `?:`; signed results are computed in
// their own assignments or selected between in statement `if`/`case` form.

module CpuV3GpuRaster #(
    parameter VIEWPORT_ONLY = 1
) (
    input wire clk,
    input wire reset,

    // Stage-5 viewport mode: three {y:s12.4, x:s12.4} beats per triangle.
    // Geometry probe mode: 12 x/y/z/w Q16.16 beats. `input_last` marks
    // the final beat of a scene in either mode.
    input wire input_valid,
    input wire input_last,
    input wire [31:0] input_data,
    output wire input_ready,

    // Quad stream sink readiness.
    input wire sink_ready,

    // Ordered stream (head of the 24-entry output FIFO).
    output wire quad_valid,
    output wire quad_is_tile_end,
    output wire quad_is_retire_marker,
    output wire retire_marker_draw,
    output wire [31:0] quad_tri,
    output wire [15:0] quad_tile,
    output wire [15:0] quad_x,
    output wire [15:0] quad_y,
    output wire [3:0] quad_mask,

    // Prefetch acquire observation port (one pulse per accepted tile).
    output reg prefetch_acquire_valid = 1'b0,
    output reg [15:0] prefetch_acquire_tile = 16'd0,

    // One-cycle pulse when a scene's DONE event is emitted.
    output reg scene_done = 1'b0
);

// ---------------------------------------------------------------------------
// Constants (framebuffer geometry and device parameters).

localparam [15:0] SCREEN_W = 16'd400;
localparam [15:0] SCREEN_H = 16'd240;
localparam [4:0] TILE_COLUMNS = 5'd25;
localparam [3:0] TILE_ROWS = 4'd15;
localparam [3:0] TILE_SHIFT = 4'd4; // log2(16 px tile)

localparam [9:0] CLIP_MASK = 10'b11_1100_0011; // near, far, four guard planes

// FIFO depths (match the emu).
localparam IN_DEPTH = 4;
localparam CLIP_OUT_DEPTH = 4;
localparam TRI_DEPTH = 4;
localparam TILE_DEPTH = 8;
localparam QUAD_DEPTH = 24;
localparam [3:0] K_PREFETCH = 4'd2;

// Setup record bus layout ([356:0]).
localparam SR_ID = 0;      // [31:0]
localparam SR_X = 32;      // 3 x s12.4 (16b)  [79:32]
localparam SR_Y = 80;      // 3 x s12.4        [127:80]
localparam SR_DEPTH = 128; // 3 x U0.18        [181:128]
localparam SR_CX = 182;    // 3 x s13.4 (18b)  [235:182]
localparam SR_CY = 236;    // 3 x s13.4        [289:236]
localparam SR_TL = 290;    // 3 x top-left     [292:290]
localparam SR_AABB = 293;  // 4 x s16          [356:293]

// Tile job bus: tile index and clamped rect, plus the setup record only in
// geometry probe mode (a clipped fan can overlap multiple setup triangles).
// Stage-5 viewport mode holds one setup record across the serialized triangle.
localparam TJ_INDEX = 0;  // [15:0]
localparam TJ_RECT = 16;  // 4 x s16 [79:16]
localparam TJ_REC = 80;   // [436:80], eliminated in viewport mode

// Output FIFO entry ([47:0]). The lower 32 bits hold every stage-5 field;
// the upper triangle-id field is constant zero in viewport mode and is
// removed by synthesis. Geometry probe mode keeps that field because a
// clipped fan can have multiple setup ids in flight. For a marker, MASK[0]
// carries retire_draw.
localparam QI_MASK = 0;  // [3:0]
localparam QI_Y = 4;     // [11:4]
localparam QI_X = 12;    // [20:12]
localparam QI_TILE = 21; // [29:21]
localparam QI_KIND = 30; // [31:30]: quad, tile-end, retire marker
localparam QI_TRI = 32;  // [47:32], geometry probe only

// ---------------------------------------------------------------------------
// Reciprocal LUT (256 segments over [0.5, 1), U1.18), computed with the same
// integer formula as `rastersim::fixed::rcp_lerp_base`.

// The first endpoint is exactly 2^18, so table storage needs 19 bits even
// though the clamped reciprocal result fits 18 bits.
reg [18:0] rcp_base [0:256]; // gowin-lint: allow EX3780 (geometry-probe ROM)
integer rcp_init_i;
initial begin
    for (rcp_init_i = 0; rcp_init_i <= 256; rcp_init_i = rcp_init_i + 1)
        rcp_base[rcp_init_i] = ((1 << 18) * 256 + (256 + rcp_init_i) / 2) / (256 + rcp_init_i);
end

// ---------------------------------------------------------------------------
// Arithmetic leaf functions (identical to the rastersim devices).

// Homogeneous clip-plane distance (40-bit signed Q16.16); inside = d >= 0.
function signed [39:0] clip_dist;
    input [3:0] plane;
    input [31:0] vx;
    input [31:0] vy;
    input [31:0] vz;
    input [31:0] vw;
    reg signed [39:0] sx;
    reg signed [39:0] sy;
    reg signed [39:0] sz;
    reg signed [39:0] sw;
    reg signed [39:0] w64;
    reg signed [39:0] x25;
    reg signed [39:0] y25;
    begin
        sx = {{8{vx[31]}}, vx};
        sy = {{8{vy[31]}}, vy};
        sz = {{8{vz[31]}}, vz};
        sw = {{8{vw[31]}}, vw};
        w64 = sw <<< 6;
        x25 = (sx <<< 4) + (sx <<< 3) + sx;
        y25 = (sy <<< 4) + (sy <<< 3) + sy;
        case (plane)
            4'd0: clip_dist = sz;          // near: z >= 0
            4'd1: clip_dist = sw - sz;     // far: z <= w
            4'd2: clip_dist = sx + sw;     // viewport left
            4'd3: clip_dist = sw - sx;     // viewport right
            4'd4: clip_dist = sy + sw;     // viewport bottom
            4'd5: clip_dist = sw - sy;     // viewport top
            4'd6: clip_dist = w64 + x25;   // guard left
            4'd7: clip_dist = w64 - x25;   // guard right
            4'd8: clip_dist = w64 + y25;   // guard bottom
            default: clip_dist = w64 - y25; // guard top
        endcase
    end
endfunction

// The six planes that truly clip, in fixed order.
function [3:0] clip_plane;
    input [2:0] pos;
    begin
        case (pos)
            3'd0: clip_plane = 4'd0;
            3'd1: clip_plane = 4'd1;
            3'd2: clip_plane = 4'd6;
            3'd3: clip_plane = 4'd7;
            3'd4: clip_plane = 4'd8;
            default: clip_plane = 4'd9;
        endcase
    end
endfunction

// 10-bit outcode over all planes.
function [9:0] outcode10;
    input [31:0] vx;
    input [31:0] vy;
    input [31:0] vz;
    input [31:0] vw;
    integer j;
    reg signed [39:0] d;
    begin
        outcode10 = 10'd0;
        for (j = 0; j < 10; j = j + 1) begin
            d = clip_dist(j[3:0], vx, vy, vz, vw);
            if (d < 0)
                outcode10[j] = 1'b1;
        end
    end
endfunction

// Leading-zero count of a nonzero 32-bit word.
function [5:0] clz32;
    input [31:0] v;
    integer k;
    reg found;
    begin
        clz32 = 6'd32;
        found = 1'b0;
        for (k = 31; k >= 0; k = k - 1) begin
            if (!found && v[k]) begin
                clz32 = 31 - k[5:0];
                found = 1'b1;
            end
        end
    end
endfunction

// Reciprocal of a positive Q16.16 (raw u32): {shift[5:0], mag[17:0]} such
// that 1/w ~ mag * 2^(16-shift); LUT + slope lerp, one small multiply.
function [23:0] rcp_q16;
    input [31:0] w;
    reg [31:0] wm;
    reg [31:0] normalized;
    reg [31:0] frac;
    reg [5:0] lz;
    reg [8:0] index;
    reg signed [13:0] slope;
    reg signed [13:0] off12;
    reg signed [27:0] lerp_prod;
    reg signed [20:0] y;
    reg [5:0] shift;
    begin
        wm = (w < 32'd8192) ? 32'd8192 : w;
        lz = clz32(wm);
        normalized = wm << lz;
        frac = normalized - 32'h8000_0000;
        index = frac >> 23;
        off12 = {2'b0, (frac >> 11) & 12'hfff};
        slope = $signed({1'b0, rcp_base[index + 1]}) - $signed({1'b0, rcp_base[index]});
        lerp_prod = slope * off12 + 28'sd2048;
        y = $signed({2'b0, rcp_base[index]}) + (lerp_prod >>> 12);
        if (y < 21'sd131072)
            y = 21'sd131072;
        if (y > 21'sd262143)
            y = 21'sd262143;
        shift = 6'd49 - lz;
        rcp_q16 = {shift, y[17:0]};
    end
endfunction

// NDC x/w as S2.29: x_raw * mag * 2^-shift (36x18 multiplier site).
function signed [31:0] ndc_rcp;
    input [31:0] x;
    input [17:0] mag;
    input [5:0] shift;
    reg signed [54:0] prod;
    reg signed [54:0] scaled;
    begin
        prod = $signed(x) * $signed({1'b0, mag});
        if (shift >= 6'd29)
            scaled = prod >>> (shift - 6'd29);
        else
            scaled = prod <<< (6'd29 - shift);
        ndc_rcp = scaled[31:0];
    end
endfunction

// Depth z/w as U0.18, saturated to the format range.
function [17:0] depth_rcp;
    input [31:0] z;
    input [17:0] mag;
    input [5:0] shift;
    reg [31:0] zc;
    reg [49:0] prod;
    reg [49:0] scaled;
    begin
        zc = (z[31] == 1'b1) ? 32'd0 : z;
        prod = {18'b0, zc} * {32'b0, mag};
        if (shift >= 6'd18)
            scaled = prod >> (shift - 6'd18);
        else
            scaled = prod << (6'd18 - shift);
        if (scaled > 50'h3ffff)
            depth_rcp = 18'h3ffff;
        else
            depth_rcp = scaled[17:0];
    end
endfunction

// Viewport snap: floor(v*16 + 0.5) saturated to the s12.4 format range.
function signed [15:0] snap_s12_4;
    input signed [31:0] v_q16;
    reg signed [33:0] raw;
    begin
        raw = (v_q16 + 34'sd2048) >>> 12;
        if (raw > 34'sd32767)
            snap_s12_4 = 16'sd32767;
        else if (raw < -34'sd32767)
            snap_s12_4 = -16'sd32767;
        else
            snap_s12_4 = raw[15:0];
    end
endfunction

// Clip intersection lerp: out + t*(in - out), 36x36 multiplier site, one
// rounding at the end.
function signed [31:0] lerp_q16;
    input [31:0] o;
    input [31:0] i;
    input [31:0] t;
    reg signed [40:0] diff;
    reg signed [72:0] product;
    reg signed [72:0] rounding;
    reg signed [41:0] step;
    reg signed [41:0] sum;
    begin
        diff = $signed({{9{i[31]}}, i}) - $signed({{9{o[31]}}, o});
        product = diff * $signed({1'b0, t});
        rounding = 73'sd2147483648;
        step = (product + rounding) >>> 32;
        sum = $signed({{10{o[31]}}, o}) + $signed({step[41], step[40:0]});
        lerp_q16 = sum[31:0];
    end
endfunction

// Edge function at one pixel (index, not coordinate): cx*(center_x - x_i) +
// cy*(center_y - y_i) with the 16-bit wrapping delta adders and the 40-bit
// wrapping accumulator, exactly like the sim devices.
function signed [39:0] edge_eval_raw;
    input signed [17:0] cx;
    input signed [17:0] cy;
    input signed [15:0] xi;
    input signed [15:0] yi;
    input [15:0] px;
    input [15:0] py;
    reg signed [15:0] cx16;
    reg signed [15:0] cy16;
    reg signed [15:0] dxw;
    reg signed [15:0] dyw;
    reg signed [35:0] term_x;
    reg signed [35:0] term_y;
    begin
        cx16 = $signed({px[11:0], 4'b1000});
        cy16 = $signed({py[11:0], 4'b1000});
        dxw = cx16 - xi;
        dyw = cy16 - yi;
        term_x = cx * $signed({{2{dxw[15]}}, dxw});
        term_y = cy * $signed({{2{dyw[15]}}, dyw});
        edge_eval_raw = $signed({{4{term_x[35]}}, term_x}) + $signed({{4{term_y[35]}}, term_y});
    end
endfunction

// ---------------------------------------------------------------------------
// State.

// Trace sequence number (simulation-only).
integer trace_seq = 0;

// Input beat assembly and the input triangle FIFO (384b = 3 x {x,y,z,w}).
reg [31:0] asm_data [0:10];
reg [3:0] asm_pos = 4'd0;
reg [383:0] in_fifo [0:IN_DEPTH-1];
reg [1:0] in_head = 2'd0;
reg [1:0] in_tail = 2'd0;
reg [2:0] in_count = 3'd0;

// Clip output FIFO (clipped triangles).
reg [383:0] co_fifo [0:CLIP_OUT_DEPTH-1];
reg [1:0] co_head = 2'd0;
reg [1:0] co_tail = 2'd0;
reg [2:0] co_count = 3'd0;

// Triangle FIFO (setup records).
(* syn_ramstyle = "registers" *) reg [356:0] tri_fifo [0:TRI_DEPTH-1];
reg [1:0] tri_head = 2'd0;
reg [1:0] tri_tail = 2'd0;
reg [2:0] tri_count = 3'd0;

// Tile FIFO (tile jobs).
(* syn_ramstyle = "registers" *) reg [436:0] tj_fifo [0:TILE_DEPTH-1];
reg [2:0] tj_head = 3'd0;
reg [2:0] tj_tail = 3'd0;
reg [3:0] tj_count = 4'd0;

// Output FIFO. The logical depth gate is QUAD_DEPTH; the physical array has
// 32 entries for natural pointer wrap. Only one entry is written per cycle.
reg [47:0] qd_fifo [0:31];
reg [4:0] qd_head = 5'd0;
reg [4:0] qd_tail = 5'd0;
reg [5:0] qd_count = 6'd0;

// Clip stage.
localparam CL_IDLE = 3'd0;
localparam CL_CLASSIFY = 3'd1;
localparam CL_PLANE = 3'd2;
localparam CL_STEP = 3'd3;
localparam CL_DIV = 3'd4;
localparam CL_LERP = 3'd5;
localparam CL_FAN = 3'd6;
reg [2:0] clip_state = CL_IDLE;
reg [383:0] clip_tri = 384'd0;
reg [31:0] poly_x [0:15];
reg [31:0] poly_y [0:15];
reg [31:0] poly_z [0:15];
reg [31:0] poly_w [0:15];
reg [31:0] next_x [0:15];
reg [31:0] next_y [0:15];
reg [31:0] next_z [0:15];
reg [31:0] next_w [0:15];
reg [3:0] poly_len = 4'd0;
reg [3:0] next_len = 4'd0;
reg [2:0] plane_pos = 3'd0;
reg [3:0] cl_index = 4'd0;
reg [31:0] prev_x = 32'd0;
reg [31:0] prev_y = 32'd0;
reg [31:0] prev_z = 32'd0;
reg [31:0] prev_w = 32'd0;
reg signed [39:0] d_prev = 40'sd0;
// Pending plane-step arguments (applied in CL_STEP).
reg [3:0] step_index = 4'd0;
reg [31:0] step_x = 32'd0;
reg [31:0] step_y = 32'd0;
reg [31:0] step_z = 32'd0;
reg [31:0] step_w = 32'd0;
reg signed [39:0] step_d = 40'sd0;
// Intersection sub-FSM.
reg [31:0] out_x = 32'd0;
reg [31:0] out_y = 32'd0;
reg [31:0] out_z = 32'd0;
reg [31:0] out_w = 32'd0;
reg [31:0] in_x = 32'd0;
reg [31:0] in_y = 32'd0;
reg [31:0] in_z = 32'd0;
reg [31:0] in_w = 32'd0;
reg signed [41:0] rem = 42'sd0;
reg signed [41:0] den = 42'sd0;
reg [31:0] quot = 32'd0;
reg [5:0] iter = 6'd0;
reg [1:0] lerp_i = 2'd0;
reg [31:0] res_x = 32'd0;
reg [31:0] res_y = 32'd0;
reg [31:0] res_z = 32'd0;
reg push_cur = 1'b0;
reg [3:0] fan_index = 4'd0;

// Setup stage.
localparam S_IDLE = 4'd0;
localparam S_VALIDATE = 4'd1;
localparam S_RCP = 4'd2;
localparam S_NDCX = 4'd3;
localparam S_NDCY = 4'd4;
localparam S_VIEW = 4'd5;
localparam S_SNAP = 4'd6;
localparam S_DEPTH = 4'd7;
localparam S_AREA = 4'd8;
localparam S_COEFF = 4'd9;
localparam S_AABB = 4'd10;
localparam S_EMIT = 4'd11;
reg [3:0] setup_state = S_IDLE;
reg [383:0] s_tri = 384'd0;
reg [1:0] s_i = 2'd0;
reg [17:0] rcp_mag = 18'd0;
reg [5:0] rcp_shift = 6'd0;
reg signed [31:0] ndc_x = 32'sd0;
reg signed [31:0] ndc_y = 32'sd0;
reg signed [31:0] prod_x = 32'sd0;
reg signed [31:0] prod_y = 32'sd0;
reg signed [15:0] sx [0:2];
reg signed [15:0] sy [0:2];
reg [17:0] sdepth [0:2];
reg signed [39:0] sarea = 40'sd0;
reg signed [17:0] scx [0:2];
reg signed [17:0] scy [0:2];
reg stl [0:2];
reg signed [15:0] saabb [0:3];
reg saabb_valid = 1'b0;
reg [31:0] next_tri_id = 32'd0;

// Tile stage.
reg t_valid = 1'b0;
reg [356:0] t_rec = 357'd0;
reg signed [15:0] tx0 = 16'sd0;
reg signed [15:0] tx1 = 16'sd0;
reg signed [15:0] ty0 = 16'sd0;
reg signed [15:0] ty1 = 16'sd0;
reg signed [15:0] tx = 16'sd0;
reg signed [15:0] ty = 16'sd0;
reg [1:0] corner_edge = 2'd0;
reg corner_pending = 1'b0;
reg signed [39:0] corner_value = 40'sd0;
reg corner_top_left = 1'b0;

// Quad stage.
localparam Q_IDLE = 2'd0;
localparam Q_INIT = 2'd1;
localparam Q_RUN = 2'd2;
localparam Q_END = 2'd3;
reg [1:0] quad_state = Q_IDLE;
reg [356:0] q_rec = 357'd0;
reg [15:0] q_tile = 16'd0;
reg [63:0] q_rect = 64'd0;
reg signed [15:0] qx = 16'sd0;
reg signed [15:0] qy = 16'sd0;
reg [1:0] q_init = 2'd0;
reg signed [39:0] e_base [0:2];
reg signed [39:0] e_row [0:2];

// Merger pin and prefetch accounting.
reg pin_valid = 1'b0;
reg [15:0] pin_tile = 16'd0;
reg [3:0] prefetch_outstanding = 4'd0;
reg [31:0] walked = 32'd0;

// Scene bookkeeping.
reg scene_active = 1'b0;
reg input_ended = 1'b0;
reg triangle_inflight = 1'b0;
reg marker_queued = 1'b0;
reg [31:0] active_input_id = 32'd0;
reg [31:0] next_input_id = 32'd0;

// FIFO push/pop pulse flags (blocking temporaries, combined at the end of
// the clocked block so every count has exactly one nonblocking update).
reg in_push;
reg in_pop;
reg co_push;
reg co_pop;
reg tri_push;
reg tri_pop;
reg tj_push;
reg tj_pop;
reg [2:0] qd_push;
reg qd_pop;
reg po_push;
reg po_pop;

// Advances the clip plane walk past vertex `index`; at the end of the
// polygon, moves to the next clip plane or to fan triangulation. Called only
// from CL_STEP, after the polygon-write state of the previous cycle settled.
task clip_step_plane;
    input [3:0] index;
    input [31:0] cur_x;
    input [31:0] cur_y;
    input [31:0] cur_z;
    input [31:0] cur_w;
    input signed [39:0] cur_d;
    integer k;
    reg [3:0] ni;
    begin
        ni = index + 4'd1;
        if (ni < poly_len) begin
            cl_index <= ni;
            prev_x <= cur_x;
            prev_y <= cur_y;
            prev_z <= cur_z;
            prev_w <= cur_w;
            d_prev <= cur_d;
            clip_state <= CL_PLANE;
        end else begin
            for (k = 0; k < 16; k = k + 1) begin
                poly_x[k] <= next_x[k];
                poly_y[k] <= next_y[k];
                poly_z[k] <= next_z[k];
                poly_w[k] <= next_w[k];
            end
            poly_len <= next_len;
            next_len <= 4'd0;
            plane_pos <= plane_pos + 3'd1;
            if (next_len < 4'd3 || (plane_pos + 3'd1) >= 3'd6) begin
                if (next_len < 4'd3) begin
                    clip_state <= CL_IDLE;
                end else begin
                    fan_index <= 4'd0;
                    clip_state <= CL_FAN;
                end
            end else begin
                cl_index <= 4'd0;
                prev_x <= next_x[next_len - 4'd1];
                prev_y <= next_y[next_len - 4'd1];
                prev_z <= next_z[next_len - 4'd1];
                prev_w <= next_w[next_len - 4'd1];
                d_prev <= clip_dist(clip_plane(plane_pos + 3'd1),
                                    next_x[next_len - 4'd1], next_y[next_len - 4'd1],
                                    next_z[next_len - 4'd1], next_w[next_len - 4'd1]);
                clip_state <= CL_PLANE;
            end
        end
    end
endtask

// Advances the tile walk to the next tile (row-major over the tile AABB).
task tile_next;
    begin
        corner_edge <= 2'd0;
        if ((tx + 16'sd1) <= tx1) begin
            tx <= tx + 16'sd1;
        end else begin
            tx <= tx0;
            if ((ty + 16'sd1) <= ty1) begin
                ty <= ty + 16'sd1;
            end else begin
                t_valid <= 1'b0;
                walked <= walked + 32'd1;
            end
        end
    end
endtask

// ---------------------------------------------------------------------------
// Combinational outputs.

assign input_ready = VIEWPORT_ONLY ? (!triangle_inflight && !input_ended && !reset)
                                   : ((in_count < 3'd4) && !reset);
assign quad_valid = (qd_count != 6'd0);
assign quad_is_tile_end = qd_fifo[qd_head][QI_KIND +: 2] == 2'd1;
assign quad_is_retire_marker = qd_fifo[qd_head][QI_KIND +: 2] == 2'd2;
assign retire_marker_draw = quad_is_retire_marker && qd_fifo[qd_head][QI_MASK];
assign quad_tri = VIEWPORT_ONLY
                ? (quad_is_retire_marker ? {16'd0, active_input_id[15:0]}
                                         : {16'd0, t_rec[15:0]})
                : {16'd0, qd_fifo[qd_head][QI_TRI +: 16]};
assign quad_tile = {7'd0, qd_fifo[qd_head][QI_TILE +: 9]};
assign quad_x = {7'd0, qd_fifo[qd_head][QI_X +: 9]};
assign quad_y = {8'd0, qd_fifo[qd_head][QI_Y +: 8]};
assign quad_mask = qd_fifo[qd_head][QI_MASK +: 4];

// ---------------------------------------------------------------------------
// One clocked process holding every stage, mirroring `RasterCore::advance`.

always @(posedge clk) begin : pipeline
    // Stage temporaries.
    reg [31:0] t_x;
    reg [31:0] t_y;
    reg [31:0] t_z;
    reg [31:0] t_w;
    reg [9:0] code0;
    reg [9:0] code1;
    reg [9:0] code2;
    reg [3:0] plane;
    reg signed [39:0] d_cur;
    reg prev_in;
    reg cur_in;
    reg [31:0] o_x;
    reg [31:0] o_y;
    reg [31:0] o_z;
    reg [31:0] o_w;
    reg [31:0] i_x;
    reg [31:0] i_y;
    reg [31:0] i_z;
    reg [31:0] i_w;
    reg signed [39:0] d_out;
    reg signed [39:0] d_in;
    reg signed [41:0] rem2;
    reg [31:0] quot2;
    reg [31:0] lerp_val;
    reg [31:0] lerp_o;
    reg [31:0] lerp_i_v;
    reg [23:0] rcp_out;
    reg signed [17:0] hi_x;
    reg signed [17:0] hi_y;
    reg signed [31:0] v_q16;
    reg signed [15:0] df_0;
    reg signed [15:0] df_1;
    reg signed [15:0] df_2;
    reg signed [15:0] df_3;
    reg signed [35:0] mul_a;
    reg signed [35:0] mul_b;
    reg signed [39:0] area_v;
    reg signed [15:0] mn;
    reg signed [15:0] mx;
    reg signed [15:0] px0;
    reg signed [15:0] px1;
    reg signed [15:0] py0;
    reg signed [15:0] py1;
    reg signed [15:0] ta;
    reg signed [15:0] tb;
    reg signed [17:0] cxe;
    reg signed [17:0] cye;
    reg signed [15:0] xie;
    reg signed [15:0] yie;
    reg [15:0] pxi;
    reg [15:0] pyi;
    reg signed [39:0] ev;
    reg cov;
    reg [3:0] mask;
    reg signed [39:0] stepx;
    reg signed [39:0] stepy;
    reg [47:0] qd_item;
    reg [436:0] tj_item;
    reg [356:0] rec;
    integer ci;
    integer cj;
    integer dx;
    integer dy;
    reg [3:0] nl;
    reg signed [15:0] r0;
    reg signed [15:0] r1;
    reg signed [15:0] r2;
    reg signed [15:0] r3;
    reg [15:0] tindex;
    reg valid_v;
    reg signed [31:0] vw;
    reg signed [31:0] vz;

    if (reset) begin
        asm_pos <= 4'd0;
        in_head <= 2'd0;
        in_tail <= 2'd0;
        in_count <= 3'd0;
        co_head <= 2'd0;
        co_tail <= 2'd0;
        co_count <= 3'd0;
        tri_head <= 2'd0;
        tri_tail <= 2'd0;
        tri_count <= 3'd0;
        tj_head <= 3'd0;
        tj_tail <= 3'd0;
        tj_count <= 4'd0;
        qd_head <= 5'd0;
        qd_tail <= 5'd0;
        qd_count <= 6'd0;
        clip_state <= CL_IDLE;
        poly_len <= 4'd0;
        next_len <= 4'd0;
        plane_pos <= 3'd0;
        d_prev <= 40'sd0;
        setup_state <= S_IDLE;
        next_tri_id <= 32'd0;
        t_valid <= 1'b0;
        corner_edge <= 2'd0;
        corner_pending <= 1'b0;
        quad_state <= Q_IDLE;
        pin_valid <= 1'b0;
        prefetch_outstanding <= 4'd0;
        walked <= 32'd0;
        scene_active <= 1'b0;
        input_ended <= 1'b0;
        triangle_inflight <= 1'b0;
        marker_queued <= 1'b0;
        active_input_id <= 32'd0;
        next_input_id <= 32'd0;
        prefetch_acquire_valid <= 1'b0;
        scene_done <= 1'b0;
        trace_seq = 0;
    end else begin
        in_push = 1'b0;
        in_pop = 1'b0;
        co_push = 1'b0;
        co_pop = 1'b0;
        tri_push = 1'b0;
        tri_pop = 1'b0;
        tj_push = 1'b0;
        tj_pop = 1'b0;
        qd_push = 1'b0;
        qd_pop = 1'b0;
        po_push = 1'b0;
        po_pop = 1'b0;
        prefetch_acquire_valid <= 1'b0;
        scene_done <= 1'b0;

        // ---- input beat assembly --------------------------------------
        if (VIEWPORT_ONLY) begin
            if (input_valid && input_ready) begin
                if (!scene_active) begin
                    scene_active <= 1'b1;
                    walked <= 32'd0;
                    next_tri_id <= 32'd0;
                    next_input_id <= 32'd0;
                end
                if (input_last)
                    input_ended <= 1'b1;
                if (asm_pos == 4'd2) begin
                    sx[0] <= asm_data[0][15:0];
                    sy[0] <= asm_data[0][31:16];
                    sx[1] <= asm_data[1][15:0];
                    sy[1] <= asm_data[1][31:16];
                    sx[2] <= input_data[15:0];
                    sy[2] <= input_data[31:16];
                    sdepth[0] <= 18'd0;
                    sdepth[1] <= 18'd0;
                    sdepth[2] <= 18'd0;
                    active_input_id <= next_input_id;
                    next_input_id <= next_input_id + 32'd1;
                    triangle_inflight <= 1'b1;
                    setup_state <= S_AREA;
                    asm_pos <= 4'd0;
                end else begin
                    asm_data[asm_pos] <= input_data;
                    asm_pos <= asm_pos + 4'd1;
                end
            end
        end else begin
        if (input_valid && input_ready) begin
            if (!scene_active) begin
                scene_active <= 1'b1;
                walked <= 32'd0;
                next_tri_id <= 32'd0;
                next_input_id <= 32'd0;
            end
            if (input_last)
                input_ended <= 1'b1;
            if (asm_pos == 4'd11) begin
                in_fifo[in_tail] <= {input_data,
                                     asm_data[10], asm_data[9], asm_data[8],
                                     asm_data[7], asm_data[6], asm_data[5],
                                     asm_data[4], asm_data[3], asm_data[2],
                                     asm_data[1], asm_data[0]};
                in_tail <= in_tail + 2'd1;
                in_push = 1'b1;
                asm_pos <= 4'd0;
            end else begin
                asm_data[asm_pos] <= input_data;
                asm_pos <= asm_pos + 4'd1;
            end
        end

        // ---- clip stage ------------------------------------------------
        case (clip_state)
            CL_IDLE: begin
                if (in_count != 3'd0 && !triangle_inflight) begin
                    clip_tri <= in_fifo[in_head];
                    in_head <= in_head + 2'd1;
                    in_pop = 1'b1;
                    active_input_id <= next_input_id;
                    next_input_id <= next_input_id + 32'd1;
                    triangle_inflight <= 1'b1;
                    clip_state <= CL_CLASSIFY;
                end
            end
            CL_CLASSIFY: begin
                code0 = outcode10(clip_tri[31:0], clip_tri[63:32],
                                  clip_tri[95:64], clip_tri[127:96]);
                code1 = outcode10(clip_tri[159:128], clip_tri[191:160],
                                  clip_tri[223:192], clip_tri[255:224]);
                code2 = outcode10(clip_tri[287:256], clip_tri[319:288],
                                  clip_tri[351:320], clip_tri[383:352]);
                if (((code0 | code1 | code2) & CLIP_MASK) == 10'd0) begin
                    // Trivial accept: bit-exact.
                    if (co_count < 3'd4) begin
                        co_fifo[co_tail] <= clip_tri;
                        co_tail <= co_tail + 2'd1;
                        co_push = 1'b1;
                        clip_state <= CL_IDLE;
                    end
                end else if ((code0 & code1 & code2) != 10'd0) begin
                    clip_state <= CL_IDLE; // trivial reject
                end else begin
                    poly_x[0] <= clip_tri[31:0];
                    poly_y[0] <= clip_tri[63:32];
                    poly_z[0] <= clip_tri[95:64];
                    poly_w[0] <= clip_tri[127:96];
                    poly_x[1] <= clip_tri[159:128];
                    poly_y[1] <= clip_tri[191:160];
                    poly_z[1] <= clip_tri[223:192];
                    poly_w[1] <= clip_tri[255:224];
                    poly_x[2] <= clip_tri[287:256];
                    poly_y[2] <= clip_tri[319:288];
                    poly_z[2] <= clip_tri[351:320];
                    poly_w[2] <= clip_tri[383:352];
                    poly_len <= 4'd3;
                    next_len <= 4'd0;
                    plane_pos <= 3'd0;
                    cl_index <= 4'd0;
                    prev_x <= clip_tri[287:256];
                    prev_y <= clip_tri[319:288];
                    prev_z <= clip_tri[351:320];
                    prev_w <= clip_tri[383:352];
                    d_prev <= clip_dist(4'd0, clip_tri[287:256], clip_tri[319:288],
                                        clip_tri[351:320], clip_tri[383:352]);
                    clip_state <= CL_PLANE;
                end
            end
            CL_PLANE: begin
                plane = clip_plane(plane_pos);
                d_cur = clip_dist(plane, poly_x[cl_index], poly_y[cl_index],
                                  poly_z[cl_index], poly_w[cl_index]);
                prev_in = (d_prev >= 40'sd0);
                cur_in = (d_cur >= 40'sd0);
                // Latch the plane-step arguments; the step itself runs in
                // CL_STEP so polygon writes from this cycle have settled.
                step_index <= cl_index;
                step_x <= poly_x[cl_index];
                step_y <= poly_y[cl_index];
                step_z <= poly_z[cl_index];
                step_w <= poly_w[cl_index];
                step_d <= d_cur;
                clip_state <= CL_STEP;
                if (prev_in && cur_in) begin
                    next_x[next_len] <= poly_x[cl_index];
                    next_y[next_len] <= poly_y[cl_index];
                    next_z[next_len] <= poly_z[cl_index];
                    next_w[next_len] <= poly_w[cl_index];
                    next_len <= next_len + 4'd1;
                end else if (!prev_in && !cur_in) begin
                    // Both outside: nothing pushed.
                end else begin
                    // Crossing edge; the intersection is always computed from
                    // the outside endpoint (AB == BA).
                    if (prev_in) begin
                        o_x = poly_x[cl_index]; o_y = poly_y[cl_index];
                        o_z = poly_z[cl_index]; o_w = poly_w[cl_index];
                        d_out = d_cur;
                        i_x = prev_x; i_y = prev_y; i_z = prev_z; i_w = prev_w;
                        d_in = d_prev;
                    end else begin
                        o_x = prev_x; o_y = prev_y; o_z = prev_z; o_w = prev_w;
                        d_out = d_prev;
                        i_x = poly_x[cl_index]; i_y = poly_y[cl_index];
                        i_z = poly_z[cl_index]; i_w = poly_w[cl_index];
                        d_in = d_cur;
                    end
                    if (d_in == 40'sd0) begin
                        // The inside endpoint lies exactly on the plane.
                        next_x[next_len] <= i_x;
                        next_y[next_len] <= i_y;
                        next_z[next_len] <= i_z;
                        next_w[next_len] <= i_w;
                        if (cur_in) begin
                            next_x[next_len + 4'd1] <= poly_x[cl_index];
                            next_y[next_len + 4'd1] <= poly_y[cl_index];
                            next_z[next_len + 4'd1] <= poly_z[cl_index];
                            next_w[next_len + 4'd1] <= poly_w[cl_index];
                            next_len <= next_len + 4'd2;
                        end else begin
                            next_len <= next_len + 4'd1;
                        end
                    end else begin
                        out_x <= o_x;
                        out_y <= o_y;
                        out_z <= o_z;
                        out_w <= o_w;
                        in_x <= i_x;
                        in_y <= i_y;
                        in_z <= i_z;
                        in_w <= i_w;
                        rem <= -$signed({d_out[39], d_out});
                        den <= $signed({d_in[39], d_in}) - $signed({d_out[39], d_out});
                        quot <= 32'd0;
                        iter <= 6'd0;
                        push_cur <= cur_in;
                        clip_state <= CL_DIV;
                    end
                end
            end
            CL_STEP: begin
                clip_step_plane(step_index, step_x, step_y, step_z, step_w, step_d);
            end
            CL_DIV: begin
                // One shift-subtract iteration of the 32-cycle divider.
                rem2 = rem <<< 1;
                quot2 = {quot[30:0], 1'b0};
                if (rem2 >= den) begin
                    rem2 = rem2 - den;
                    quot2 = quot2 | 32'd1;
                end
                rem <= rem2;
                quot <= quot2;
                iter <= iter + 6'd1;
                if (iter == 6'd31) begin
                    lerp_i <= 2'd0;
                    clip_state <= CL_LERP;
                end
            end
            CL_LERP: begin
                case (lerp_i)
                    2'd0: begin lerp_o = out_x; lerp_i_v = in_x; end
                    2'd1: begin lerp_o = out_y; lerp_i_v = in_y; end
                    2'd2: begin lerp_o = out_z; lerp_i_v = in_z; end
                    default: begin lerp_o = out_w; lerp_i_v = in_w; end
                endcase
                lerp_val = lerp_q16(lerp_o, lerp_i_v, quot);
                if (lerp_i == 2'd3) begin
                    // Done: push the intersection (res_x..res_z plus this
                    // component), then the inside vertex when required.
                    next_x[next_len] <= res_x;
                    next_y[next_len] <= res_y;
                    next_z[next_len] <= res_z;
                    next_w[next_len] <= lerp_val;
                    nl = next_len + 4'd1;
                    if (push_cur) begin
                        next_x[nl] <= step_x;
                        next_y[nl] <= step_y;
                        next_z[nl] <= step_z;
                        next_w[nl] <= step_w;
                        nl = nl + 4'd1;
                    end
                    next_len <= nl;
                    clip_state <= CL_STEP;
                end else begin
                    if (lerp_i == 2'd0)
                        res_x <= lerp_val;
                    if (lerp_i == 2'd1)
                        res_y <= lerp_val;
                    if (lerp_i == 2'd2)
                        res_z <= lerp_val;
                    lerp_i <= lerp_i + 2'd1;
                end
            end
            CL_FAN: begin
                if (poly_len < 4'd3 || (fan_index + 4'd2) >= poly_len) begin
                    clip_state <= CL_IDLE;
                end else if (co_count < 3'd4) begin
                    co_fifo[co_tail] <= {poly_w[fan_index + 4'd2], poly_z[fan_index + 4'd2],
                                         poly_y[fan_index + 4'd2], poly_x[fan_index + 4'd2],
                                         poly_w[fan_index + 4'd1], poly_z[fan_index + 4'd1],
                                         poly_y[fan_index + 4'd1], poly_x[fan_index + 4'd1],
                                         poly_w[0], poly_z[0], poly_y[0], poly_x[0]};
                    co_tail <= co_tail + 2'd1;
                    co_push = 1'b1;
                    fan_index <= fan_index + 4'd1;
                end
            end
            default: clip_state <= CL_IDLE;
        endcase
        end

        // ---- setup stage -----------------------------------------------
        case (setup_state)
            S_IDLE: begin
                if (!VIEWPORT_ONLY && co_count != 3'd0) begin
                    s_tri <= co_fifo[co_head];
                    co_head <= co_head + 2'd1;
                    co_pop = 1'b1;
                    setup_state <= S_VALIDATE;
                end
            end
            S_VALIDATE: begin
                if (!VIEWPORT_ONLY) begin
                // Legal-projection validation: reject (never clamp)
                // triangles violating w >= 1/8 or 0 <= z <= w (z carries a
                // small epsilon for intersection rounding).
                valid_v = 1'b1;
                for (ci = 0; ci < 3; ci = ci + 1) begin
                    vw = $signed(s_tri[ci*128 + 96 +: 32]);
                    vz = $signed(s_tri[ci*128 + 64 +: 32]);
                    if (vw < 32'sd8192 || vz < -32'sd1024 || (vz - vw) > 32'sd1024)
                        valid_v = 1'b0;
                end
                if (valid_v) begin
                    s_i <= 2'd0;
                    setup_state <= S_RCP;
                end else begin
                    setup_state <= S_IDLE;
                end
                end
            end
            S_RCP: begin
                if (!VIEWPORT_ONLY) begin
                    rcp_out = rcp_q16(s_tri[s_i*128 + 96 +: 32]);
                    rcp_shift <= rcp_out[23:18];
                    rcp_mag <= rcp_out[17:0];
                    setup_state <= S_NDCX;
                end
            end
            S_NDCX: begin
                if (!VIEWPORT_ONLY) begin
                    ndc_x <= ndc_rcp(s_tri[s_i*128 +: 32], rcp_mag, rcp_shift);
                    setup_state <= S_NDCY;
                end
            end
            S_NDCY: begin
                if (!VIEWPORT_ONLY) begin
                    ndc_y <= ndc_rcp(s_tri[s_i*128 + 32 +: 32], rcp_mag, rcp_shift);
                    setup_state <= S_VIEW;
                end
            end
            S_VIEW: begin
                if (!VIEWPORT_ONLY) begin
                // high17 = ndc >> 14; hi carries 15 fractional bits, so
                // *half-extent and rescale to Q16.16 is a << 1.
                hi_x = ndc_x >>> 14;
                hi_y = ndc_y >>> 14;
                prod_x <= (hi_x * 18'sd200) <<< 1;
                prod_y <= (hi_y * 18'sd120) <<< 1;
                setup_state <= S_SNAP;
                end
            end
            S_SNAP: begin
                if (!VIEWPORT_ONLY) begin
                v_q16 = prod_x + 32'sd13107200; // + 200 px in Q16.16
                sx[s_i] <= snap_s12_4(v_q16);
                v_q16 = 32'sd7864320 - prod_y;  // 120 px in Q16.16
                sy[s_i] <= snap_s12_4(v_q16);
                setup_state <= S_DEPTH;
                end
            end
            S_DEPTH: begin
                if (!VIEWPORT_ONLY) begin
                sdepth[s_i] <= depth_rcp(s_tri[s_i*128 + 64 +: 32], rcp_mag, rcp_shift);
                if (s_i == 2'd2)
                    setup_state <= S_AREA;
                else begin
                    s_i <= s_i + 2'd1;
                    setup_state <= S_RCP;
                end
                end
            end
            S_AREA: begin
                // Twice the signed area: two 18x18 products, 40-bit subtract.
                df_0 = sx[1] - sx[0];
                df_1 = sy[1] - sy[0];
                df_2 = sx[2] - sx[0];
                df_3 = sy[2] - sy[0];
                mul_a = df_0 * $signed({{2{df_3[15]}}, df_3});
                mul_b = df_2 * $signed({{2{df_1[15]}}, df_1});
                area_v = $signed({{4{mul_a[35]}}, mul_a}) - $signed({{4{mul_b[35]}}, mul_b});
                sarea <= area_v;
                if (area_v <= 40'sd0) begin
                    // Backface or degenerate: nothing is emitted.
                    setup_state <= S_IDLE;
                end else begin
                    setup_state <= S_COEFF;
                end
            end
            S_COEFF: begin
                // Edge i runs v_i -> v_{i+1}: cx = -dy, cy = dx; top-left
                // when the edge goes up, or is horizontal going right.
                for (ci = 0; ci < 3; ci = ci + 1) begin
                    cj = (ci == 2) ? 0 : ci + 1;
                    df_0 = sx[cj] - sx[ci];
                    df_1 = sy[cj] - sy[ci];
                    scx[ci] <= -$signed({{2{df_1[15]}}, df_1});
                    scy[ci] <= $signed({{2{df_0[15]}}, df_0});
                    if (df_1 < 16'sd0)
                        stl[ci] <= 1'b1;
                    else if (df_1 == 16'sd0 && df_0 > 16'sd0)
                        stl[ci] <= 1'b1;
                    else
                        stl[ci] <= 1'b0;
                end
                setup_state <= S_AABB;
            end
            S_AABB: begin
                mn = sx[0];
                if (sx[1] < mn) mn = sx[1];
                if (sx[2] < mn) mn = sx[2];
                mx = sx[0];
                if (sx[1] > mx) mx = sx[1];
                if (sx[2] > mx) mx = sx[2];
                px0 = mn >>> 4;
                if (px0 < 16'sd0) px0 = 16'sd0;
                px1 = mx >>> 4;
                if (px1 > $signed(SCREEN_W - 16'sd1)) px1 = $signed(SCREEN_W - 16'sd1);
                mn = sy[0];
                if (sy[1] < mn) mn = sy[1];
                if (sy[2] < mn) mn = sy[2];
                mx = sy[0];
                if (sy[1] > mx) mx = sy[1];
                if (sy[2] > mx) mx = sy[2];
                py0 = mn >>> 4;
                if (py0 < 16'sd0) py0 = 16'sd0;
                py1 = mx >>> 4;
                if (py1 > $signed(SCREEN_H - 16'sd1)) py1 = $signed(SCREEN_H - 16'sd1);
                if (px0 > px1 || py0 > py1) begin
                    saabb_valid <= 1'b0;
                end else begin
                    saabb_valid <= 1'b1;
                    saabb[0] <= {px0[15:1], 1'b0};
                    saabb[1] <= {py0[15:1], 1'b0};
                    saabb[2] <= px1 | 16'sd1;
                    saabb[3] <= py1 | 16'sd1;
                end
                setup_state <= S_EMIT;
            end
            S_EMIT: begin
                if (tri_count < 3'd4) begin
                    if (saabb_valid) begin
                        rec = {saabb[3], saabb[2], saabb[1], saabb[0],
                               stl[2], stl[1], stl[0],
                               scy[2], scy[1], scy[0],
                               scx[2], scx[1], scx[0],
                               sdepth[2], sdepth[1], sdepth[0],
                               sy[2], sy[1], sy[0],
                               sx[2], sx[1], sx[0],
                               next_tri_id};
                        tri_fifo[tri_tail] <= rec;
                        tri_tail <= tri_tail + 2'd1;
                        tri_push = 1'b1;
                        // synthesis translate_off
                        $display("RAST %0d TRI %0d X %0d %0d %0d Y %0d %0d %0d Z %0d %0d %0d AABB %0d %0d %0d %0d",
                                 trace_seq, next_tri_id,
                                 sx[0], sx[1], sx[2],
                                 sy[0], sy[1], sy[2],
                                 sdepth[0], sdepth[1], sdepth[2],
                                 $signed(saabb[0]), $signed(saabb[1]),
                                 $signed(saabb[2]), $signed(saabb[3]));
                        trace_seq = trace_seq + 1;
                        // synthesis translate_on
                        next_tri_id <= next_tri_id + 32'd1;
                    end
                    setup_state <= S_IDLE;
                end
            end
            default: setup_state <= S_IDLE;
        endcase

        // ---- tile stage --------------------------------------------------
        if (!t_valid) begin
            if (tri_count != 3'd0) begin
                rec = tri_fifo[tri_head];
                tri_head <= tri_head + 2'd1;
                tri_pop = 1'b1;
                t_rec <= rec;
                ta = $signed(rec[SR_AABB +: 16]) >>> 4;
                if (ta < 16'sd0) ta = 16'sd0;
                if (ta > $signed({11'd0, TILE_COLUMNS} - 16'sd1)) ta = $signed({11'd0, TILE_COLUMNS} - 16'sd1);
                tx0 <= ta;
                tx <= ta;
                ta = $signed(rec[SR_AABB + 32 +: 16]) >>> 4;
                if (ta < 16'sd0) ta = 16'sd0;
                if (ta > $signed({11'd0, TILE_COLUMNS} - 16'sd1)) ta = $signed({11'd0, TILE_COLUMNS} - 16'sd1);
                tx1 <= ta;
                ta = $signed(rec[SR_AABB + 16 +: 16]) >>> 4;
                if (ta < 16'sd0) ta = 16'sd0;
                if (ta > $signed({12'd0, TILE_ROWS} - 16'sd1)) ta = $signed({12'd0, TILE_ROWS} - 16'sd1);
                ty0 <= ta;
                ty <= ta;
                ta = $signed(rec[SR_AABB + 48 +: 16]) >>> 4;
                if (ta < 16'sd0) ta = 16'sd0;
                if (ta > $signed({12'd0, TILE_ROWS} - 16'sd1)) ta = $signed({12'd0, TILE_ROWS} - 16'sd1);
                ty1 <= ta;
                corner_edge <= 2'd0;
                corner_pending <= 1'b0;
                t_valid <= 1'b1;
            end
        end else if (corner_pending) begin
            // Keep the DSP edge result off the tile-walk control path.
            corner_pending <= 1'b0;
            if (corner_top_left)
                cov = (corner_value >= 40'sd0);
            else
                cov = (corner_value > 40'sd0);
            if (!cov)
                tile_next();
            else
                corner_edge <= corner_edge + 2'd1;
        end else if (corner_edge < 2'd3) begin
            // Worst-corner test, one edge per two cycles. The product and
            // comparison are separated by a register for the 54 MHz path.
            cxe = $signed(t_rec[SR_CX + corner_edge*18 +: 18]);
            cye = $signed(t_rec[SR_CY + corner_edge*18 +: 18]);
            xie = $signed(t_rec[SR_X + corner_edge*16 +: 16]);
            yie = $signed(t_rec[SR_Y + corner_edge*16 +: 16]);
            // E grows with x when cx > 0 and with y when cy > 0.
            if (cxe > 18'sd0)
                pxi = {tx[11:0], 4'b0000} + 16'd15;
            else
                pxi = {tx[11:0], 4'b0000};
            if (cye > 18'sd0)
                pyi = {ty[11:0], 4'b0000} + 16'd15;
            else
                pyi = {ty[11:0], 4'b0000};
            corner_value <= edge_eval_raw(cxe, cye, xie, yie, pxi, pyi);
            corner_top_left <= t_rec[SR_TL + corner_edge];
            corner_pending <= 1'b1;
        end else begin
            // Accepted tile: backpressure from the tile FIFO and the
            // prefetch outstanding limit.
            if (tj_count < 4'd8 && prefetch_outstanding < K_PREFETCH) begin
                r0 = $signed(t_rec[SR_AABB +: 16]);
                ta = {tx[11:0], 4'b0000};
                if (ta > r0) r0 = ta;
                r1 = $signed(t_rec[SR_AABB + 16 +: 16]);
                tb = {ty[11:0], 4'b0000};
                if (tb > r1) r1 = tb;
                r2 = $signed(t_rec[SR_AABB + 32 +: 16]);
                ta = {tx[11:0], 4'b0000} + 16'd15;
                if (ta < r2) r2 = ta;
                r3 = $signed(t_rec[SR_AABB + 48 +: 16]);
                tb = {ty[11:0], 4'b0000} + 16'd15;
                if (tb < r3) r3 = tb;
                tindex = {11'd0, ty[4:0]} * 16'd25 + {11'd0, tx[4:0]};
                if (VIEWPORT_ONLY)
                    tj_item = {357'd0, r3, r2, r1, r0, tindex};
                else
                    tj_item = {t_rec, r3, r2, r1, r0, tindex};
                tj_fifo[tj_tail] <= tj_item;
                tj_tail <= tj_tail + 3'd1;
                tj_push = 1'b1;
                po_push = 1'b1;
                prefetch_acquire_valid <= 1'b1;
                prefetch_acquire_tile <= tindex;
                // synthesis translate_off
                $display("RAST %0d PREFETCH %0d", trace_seq, tindex);
                trace_seq = trace_seq + 1;
                // synthesis translate_on
                tile_next();
            end
        end

        // ---- quad stage --------------------------------------------------
        case (quad_state)
            Q_IDLE: begin
                if (tj_count != 4'd0) begin
                    tj_item = tj_fifo[tj_head];
                    tj_head <= tj_head + 3'd1;
                    tj_pop = 1'b1;
                    if (VIEWPORT_ONLY)
                        q_rec <= t_rec;
                    else
                        q_rec <= tj_item[TJ_REC +: 357];
                    q_tile <= tj_item[TJ_INDEX +: 16];
                    q_rect <= tj_item[TJ_RECT +: 64];
                    qx <= $signed(tj_item[TJ_RECT +: 16]);
                    qy <= $signed(tj_item[TJ_RECT + 16 +: 16]);
                    q_init <= 2'd0;
                    quad_state <= Q_INIT;
                end
            end
            Q_INIT: begin
                // Per-tile initialization: the three edge values at the
                // first quad origin, one edge per cycle.
                cxe = $signed(q_rec[SR_CX + q_init*18 +: 18]);
                cye = $signed(q_rec[SR_CY + q_init*18 +: 18]);
                xie = $signed(q_rec[SR_X + q_init*16 +: 16]);
                yie = $signed(q_rec[SR_Y + q_init*16 +: 16]);
                ev = edge_eval_raw(cxe, cye, xie, yie, qx[15:0], qy[15:0]);
                e_base[q_init] <= ev;
                e_row[q_init] <= ev;
                if (q_init == 2'd2)
                    quad_state <= Q_RUN;
                q_init <= q_init + 2'd1;
            end
            Q_RUN: begin
                // Depth gate matches the emu (checked once per quad).
                if (qd_count < 6'd24) begin
                    // One quad per cycle: the four pixel edge values derive
                    // from the quad origin by 40-bit adds (one pixel step =
                    // 16 raw units).
                    mask = 4'd0;
                    for (dy = 0; dy < 2; dy = dy + 1) begin
                        for (dx = 0; dx < 2; dx = dx + 1) begin
                            pxi = qx + dx[15:0];
                            pyi = qy + dy[15:0];
                            if ($signed(pxi) <= $signed(q_rect[32 +: 16])
                                    && $signed(pyi) <= $signed(q_rect[48 +: 16])) begin
                                cov = 1'b1;
                                for (ci = 0; ci < 3; ci = ci + 1) begin
                                    stepx = $signed(q_rec[SR_CX + ci*18 +: 18]) <<< 4;
                                    stepy = $signed(q_rec[SR_CY + ci*18 +: 18]) <<< 4;
                                    ev = e_base[ci];
                                    if (dx == 1) ev = ev + stepx;
                                    if (dy == 1) ev = ev + stepy;
                                    if (q_rec[SR_TL + ci]) begin
                                        if (!(ev >= 40'sd0)) cov = 1'b0;
                                    end else begin
                                        if (!(ev > 40'sd0)) cov = 1'b0;
                                    end
                                end
                                if (cov)
                                    mask[dy*2 + dx] = 1'b1;
                            end
                        end
                    end
                    if (mask != 4'd0) begin
                        if (VIEWPORT_ONLY)
                            qd_item = {16'd0, 2'd0, q_tile[8:0], qx[8:0], qy[7:0], mask};
                        else
                            qd_item = {q_rec[15:0], 2'd0, q_tile[8:0], qx[8:0], qy[7:0], mask};
                        qd_fifo[qd_tail] <= qd_item;
                        qd_push = 2'd1;
                        // synthesis translate_off
                        $display("RAST %0d QUAD %0d %0d %0d %0x",
                                 trace_seq, q_rec[31:0], qx, qy, mask);
                        trace_seq = trace_seq + 1;
                        // synthesis translate_on
                    end
                    // Advance the quad cursor with incremental edge updates
                    // (one quad step = 2 pixels = 32 raw units).
                    if ((qx + 16'sd2) <= $signed(q_rect[32 +: 16])) begin
                        for (ci = 0; ci < 3; ci = ci + 1) begin
                            stepx = $signed(q_rec[SR_CX + ci*18 +: 18]) <<< 5;
                            e_base[ci] <= e_base[ci] + stepx;
                        end
                        qx <= qx + 16'sd2;
                        qd_tail <= qd_tail + {4'd0, qd_push[0]};
                    end else if ((qy + 16'sd2) <= $signed(q_rect[48 +: 16])) begin
                        for (ci = 0; ci < 3; ci = ci + 1) begin
                            stepy = $signed(q_rec[SR_CY + ci*18 +: 18]) <<< 5;
                            e_row[ci] <= e_row[ci] + stepy;
                            e_base[ci] <= e_row[ci] + stepy;
                        end
                        qx <= $signed(q_rect[0 +: 16]);
                        qy <= qy + 16'sd2;
                        qd_tail <= qd_tail + {4'd0, qd_push[0]};
                    end else begin
                        // The tile-end uses the next write cycle so this
                        // FIFO has a single write port.
                        qd_tail <= qd_tail + {4'd0, qd_push[0]};
                        quad_state <= Q_END;
                    end
                end
            end
            Q_END: begin
                if (qd_count < 6'd24) begin
                    qd_item = {16'd0, 2'd1, q_tile[8:0], 9'd0, 8'd0, 4'd0};
                    qd_fifo[qd_tail] <= qd_item;
                    qd_tail <= qd_tail + 5'd1;
                    qd_push[1] = 1'b1;
                    // synthesis translate_off
                    $display("RAST %0d TILE_END %0d", trace_seq, q_tile);
                    trace_seq = trace_seq + 1;
                    // synthesis translate_on
                    quad_state <= Q_IDLE;
                end
            end
            default: quad_state <= Q_IDLE;
        endcase

        // A marker enters the same ready/valid FIFO only after this input
        // triangle's clipped fan, tiles, and output writes have drained.
        // Serializing input triangles here keeps culled and zero-coverage
        // triangles ordered without needing a per-stage source-id queue.
        if (triangle_inflight && !marker_queued
                && clip_state == CL_IDLE && setup_state == S_IDLE
                && !t_valid && quad_state == Q_IDLE
                && co_count == 3'd0 && tri_count == 3'd0
                && tj_count == 4'd0 && qd_count == 6'd0) begin
            // synthesis translate_off
            if (prefetch_outstanding != 4'd0 || pin_valid)
                $fatal(1, "raster marker overtook a pending tile");
            // synthesis translate_on
            if (VIEWPORT_ONLY)
                qd_item = {16'd0, 2'd2, 9'd0, 9'd0, 8'd0,
                           3'd0, (input_ended && in_count == 3'd0)};
            else
                qd_item = {active_input_id[15:0], 2'd2, 9'd0, 9'd0, 8'd0,
                           3'd0, (input_ended && in_count == 3'd0)};
            qd_fifo[qd_tail] <= qd_item;
            qd_tail <= qd_tail + 5'd1;
            qd_push[2] = 1'b1;
            marker_queued <= 1'b1;
            // synthesis translate_off
            $display("RAST %0d RETIRE_MARKER %0d DRAW %0d", trace_seq,
                     active_input_id, input_ended && in_count == 3'd0);
            trace_seq = trace_seq + 1;
            // synthesis translate_on
        end

        // ---- merger/sink: pop one quad FIFO entry when the sink is ready.
        if (sink_ready && qd_count != 6'd0) begin
            qd_item = qd_fifo[qd_head];
            if (qd_item[QI_KIND +: 2] == 2'd0) begin
                if (!pin_valid) begin
                    pin_valid <= 1'b1;
                    pin_tile <= {7'd0, qd_item[QI_TILE +: 9]};
                end else if (pin_tile != {7'd0, qd_item[QI_TILE +: 9]}) begin
                    // synthesis translate_off
                    $fatal(1, "raster merger pin conflict across tiles");
                    // synthesis translate_on
                end
            end else if (qd_item[QI_KIND +: 2] == 2'd1) begin
                // A tile with zero covered pixels acquires no pin; its
                // tile-end only retires the prefetch.
                if (pin_valid && pin_tile == {7'd0, qd_item[QI_TILE +: 9]})
                    pin_valid <= 1'b0;
                po_pop = 1'b1;
            end else if (qd_item[QI_KIND +: 2] == 2'd2) begin
                marker_queued <= 1'b0;
                triangle_inflight <= 1'b0;
            end else begin
                // synthesis translate_off
                $fatal(1, "invalid raster output kind");
                // synthesis translate_on
            end
            qd_head <= qd_head + 5'd1;
            qd_pop = 1'b1;
        end

        // ---- FIFO counts and prefetch accounting -------------------------
        in_count <= in_count + {2'b0, in_push} - {2'b0, in_pop};
        co_count <= co_count + {2'b0, co_push} - {2'b0, co_pop};
        tri_count <= tri_count + {2'b0, tri_push} - {2'b0, tri_pop};
        tj_count <= tj_count + {3'b0, tj_push} - {3'b0, tj_pop};
        qd_count <= qd_count + {5'b0, qd_push[0]} + {5'b0, qd_push[1]}
                              + {5'b0, qd_push[2]}
                              - {5'b0, qd_pop};
        prefetch_outstanding <= prefetch_outstanding + {3'b0, po_push} - {3'b0, po_pop};

        // ---- scene DONE ---------------------------------------------------
        if (scene_active && input_ended && !triangle_inflight && !marker_queued
                && clip_state == CL_IDLE && setup_state == S_IDLE
                && !t_valid && quad_state == Q_IDLE
                && in_count == 3'd0 && co_count == 3'd0 && tri_count == 3'd0
                && tj_count == 4'd0 && qd_count == 6'd0) begin
            // synthesis translate_off
            if (prefetch_outstanding != 4'd0 || pin_valid)
                $fatal(1, "raster scene retired with a pending tile");
            $display("RAST %0d DONE draw %0d", trace_seq, walked);
            trace_seq = trace_seq + 1;
            // synthesis translate_on
            scene_done <= 1'b1;
            scene_active <= 1'b0;
            input_ended <= 1'b0;
        end
    end
end

endmodule
