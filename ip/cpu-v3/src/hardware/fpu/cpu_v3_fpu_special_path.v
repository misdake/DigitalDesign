// FPU v2 special-function execution-path controller (opcode 0xD subops
// 0x0C RCP, 0x0D RSQRT and 0x0E SINCOS).
//
// The special path is blocking (fpu-design-v2 section 4.2): while active it
// monopolizes both register-file BSRAM read ports. RCP reads port A (mirror 0,
// RCP @128..255), RSQRT reads port B (mirror 1, even table @128..255, odd table
// @256..383) and SINCOS reads port A (mirror 0, packed intervals @256..511). It
// owns one inferred 18x18 multiplier below: RCP/RSQRT and SINCOS time-share it
// for their interpolation product. The multiply inputs are muxed, so synthesis
// still maps exactly one MULT18X18 lane (the resource request in `mod.rs`).
//
// SINCOS range reduction is no longer done here. It reuses the shared 36x36
// multiply pipeline (CpuV3FpuMulPipe) that the multiply and dot paths own in
// the unit top: the architectural signed Q16.16 operand times the hardware
// constant K = round((2/pi)*2^32) = 2734261102 = 0xA2F9836E. Because K's bit 31
// is set, both shared-pipe operand buses are signed 36 bits and K is fed as the
// positive 36'h0_A2F9836E; the ordinary MUL/DOT owners keep exact signed-32
// behavior by explicit sign extension through the same widened buses. The
// returned product supplies phase = (signed(Fa) * K) >>> 32; only phase[17:0]
// is consumed as the quadrant q = phase[17:16] and fraction f = phase[15:0].
// This is algebraically identical to the former two-term C0/C1 reducer, since
// C0*2^16 + C1 == K, so sin/cos stay bit-exact for every i32 input:
//   a*K = h0*2^32 + (h1 + p0)*2^16 + p1,
//   (a*K) >>> 32 = h0 + ((((p0 + h1) << 16) + p1) >> 32).
// The core serializes instructions, so the special path is the only owner
// active while it drives the pipe; its single range product is the only entry
// in flight and abort voids it through the pipe's own abort.
//
// Word0 carries Fa in bits [11:6]; for 0xD subops only Fa matters (the
// operand), and the front-end parks that address on RF read port A one beat
// before T0, so the operand is on rf_read_a_data during T0. Word1 carries Fd in
// bits [15:10], the 6-bit subop in bits [9:4] and the mode field in bits
// [1:0]. T0 is the cycle where instr_complete is high and instr_opcode == 0xD.
//
// RCP/RSQRT keep the frozen T0..T3 profile. Each hidden BSRAM word packs a
// 17-bit sample and its signed 10-bit delta to the next sample, so linear
// interpolation needs only one table read:
//   T0         : capture the operand and instruction controls. This register
//                boundary keeps the RF output out of the CLZ/LUT-address path.
//   T1         : normalize the captured operand and drive the aligned packed
//                table address; the RF latches `{delta,current}`.
//   T2         : interpolate the packed interval and capture its 17-bit result.
//   T3         : scale by the power of two (barrel shift; RCP saturating) and
//                apply the sign, presenting rf_write_enable with Fd.
//   after T3   : busy returns low.
//
// SINCOS (design section 9.4) is a shorter fixed microsequence on the same
// blocking path. It writes two registers (Fd = sin, Fd+1 = cos) on consecutive
// beats. The range product is issued into the shared pipe at T0 and returns at
// T3 with a fixed 3-cycle latency:
//   T0         : capture the instruction controls and issue the range product
//                (signed Fa, K) into the shared pipe.
//   T1, T2     : the shared pipe advances; the local 18x18 multiplier is idle.
//   T3         : the product returns; register q = phase[17:16],
//                f = phase[15:0] from (product >>> 32).
//   T4         : drive the first LUT address (sine, or cosine for mode 10)
//                from the registered q/f.
//   T5         : interpolate the first result into the result register; for
//                dual output drive the second (cosine) LUT address.
//   T6         : write Fd = first result while interpolating the second into
//                the same result register (dual), or clear (single).
//   T7         : write Fd+1 = cos; clear the context.
//   after T7   : busy returns low (8 busy beats dual, 7 single).
// The interpolation result register keeps the BSRAM -> local 18x18 -> add -> RF
// write path registered, exactly as before.
//
// Special values (section 9.4): RCP(0) -> 0x7FFF_FFFF with the input's sign;
// RCP of a negative value is the negative of the magnitude's reciprocal;
// RSQRT of x <= 0 is zero. For SINCOS, u == 1.0 in a quarter uses the exact
// sine endpoint instead of an interval read (the 257th sample is folded into
// interval 255's delta; the `u[16]` flag selects the exact 1.0).
//
// abort combinationally clears the context, gates the write port and voids the
// in-flight shared-pipe product; no register update is committed on the abort
// edge.
module CpuV3FpuSpecialPath (
    input wire clk,
    input wire abort,
    input wire instr_complete,
    input wire [3:0] instr_opcode,
    input wire [15:0] word1_raw,
    input wire [31:0] rf_read_a_data,
    input wire [31:0] rf_read_b_data,
    output wire [8:0] rf_read_a_address,
    output wire [8:0] rf_read_b_address,
    output wire rf_write_enable,
    output wire [8:0] rf_write_address,
    output wire [31:0] rf_write_data,
    output wire busy,
    // Section-16 resource countdowns (a single blocking path: R was satisfied
    // before T0; W/X span the fixed busy window).
    output wire [3:0] r_wait,
    output wire [3:0] w_wait,
    output wire [3:0] x_wait,
    // Shared 36x36 multiply pipeline (in the unit top). SINCOS issues its one
    // range-reduction product here at T0 and reads it back at T3; RCP/RSQRT
    // leave the interface idle. The widened signed-36 operands keep exact
    // signed-32 behavior for the ordinary MUL/DOT owners (explicit sign
    // extension at the multiplexer in the unit top).
    output wire mul_in_valid,
    output wire signed [35:0] mul_in_a,
    output wire signed [35:0] mul_in_b,
    output wire [8:0] mul_in_tag,
    input wire mul_out_valid,
    input wire signed [63:0] mul_out_product,
    input wire [8:0] mul_out_tag
);

// Word1 fields.
wire [5:0] subop = word1_raw[9:4];
wire [5:0] fd = word1_raw[15:10];
wire [1:0] mode = word1_raw[1:0];
wire mode_single = mode[0] ^ mode[1];

wire is_scalar = (instr_opcode == 4'hD);
wire is_rcp = is_scalar && (subop == 6'h0C);
wire is_rsqrt = is_scalar && (subop == 6'h0D);
wire is_sincos = is_scalar && (subop == 6'h0E);
wire is_special = is_rcp || is_rsqrt || is_sincos;

// T0 of this cycle.
wire load_now = instr_complete && is_special && !abort;

// ---------------------------------------------------------------------------
// T0 operand capture and T1 combinational normalization (RCP/RSQRT).
// ---------------------------------------------------------------------------
wire [31:0] x0 = rf_read_a_data;
wire x0_negative = x0[31];
wire [31:0] x0_magnitude = x0_negative ? (~x0 + 32'd1) : x0;

// Pre-normalization register: the timing boundary between the synchronous RF
// output and the CLZ/barrel-shift/LUT-address logic.
reg p0_valid = 1'b0;
reg p0_rcp = 1'b0;
reg p0_negative = 1'b0;
reg [31:0] p0_magnitude = 32'd0;
reg [5:0] p0_fd = 6'd0;

wire p0_zero = (p0_magnitude == 32'd0);

// Leading-zero count of the T0 magnitude; 32 for zero.
wire [5:0] clz = clz32(p0_magnitude);

// Both functions use the same normalized index/residue bits. Only these 16
// bits are consumed downstream; synthesis can prune the other shift outputs.
wire [31:0] normalized = p0_magnitude << clz[4:0];
wire [6:0] norm_index = normalized[30:24];
wire [8:0] norm_residue = normalized[23:15];

// RCP rescales by 2^(clz-15). The useful magnitude is 0..16, so five bits are
// sufficient once the explicit zero case is carried alongside it.
wire rcp_left = (clz >= 6'd15);
wire [4:0] rcp_shift = rcp_left
    ? (clz[4:0] - 5'd15)
    : (5'd15 - clz[4:0]);

// RSQRT table parity is parity(31-clz), i.e. !clz[0]. Its scale is
// floor((15-clz)/2): right by floor(distance/2) below clz=16, otherwise left
// by ceil(distance/2). The largest useful magnitude is eight.
wire rsqrt_odd = !clz[0];
wire rsqrt_left = (clz > 6'd15);
wire [5:0] rsqrt_distance = rsqrt_left
    ? (clz - 6'd15)
    : (6'd15 - clz);
wire [4:0] rsqrt_shift = rsqrt_left
    ? ((rsqrt_distance + 6'd1) >> 1)
    : (rsqrt_distance >> 1);

// Aligned hidden-table bases turn address formation into wiring rather than
// addition: 128..255 for RCP/even RSQRT, 256..383 for odd RSQRT.
wire [8:0] table_address = p0_rcp
    ? {2'b01, norm_index}
    : (rsqrt_odd ? {2'b10, norm_index} : {2'b01, norm_index});

// One blocking operation owns a single context. Control fields are written
// once after normalization and remain stable while only the valid token and
// useful data advance.
reg s1_valid = 1'b0;
reg [8:0] s1_residue = 9'd0;
reg [4:0] s1_shift = 5'd0;
reg s1_left = 1'b0;
reg s1_zero = 1'b0;

reg s2_valid = 1'b0;
reg [16:0] s2_interpolated = 17'd0;

// ---------------------------------------------------------------------------
// SINCOS context (T0..T7). The reducer is the shared 36x36 pipe in the unit
// top; this context only registers the quadrant/fraction it returns and the
// interpolation of the two results.
// ---------------------------------------------------------------------------
// round((2/pi) * 2^32), the positive hardware constant. Kept as a 36-bit
// positive value because bit 31 is set.
localparam [35:0] SINCOS_K = 36'h0_A2F9836E;

reg sc_valid = 1'b0;
reg [3:0] sc_stage = 4'd0;
reg [5:0] sc_fd = 6'd0;
reg [1:0] sc_mode = 2'b00;
reg [1:0] sc_q = 2'd0;
reg [15:0] sc_f = 16'd0;
reg signed [17:0] sc_result_reg = 18'sd0;

// Reduced-argument reflection. sin uses (q, f) and cos uses (q+1, f); the
// quarter-wave symmetries give sin's reflection from q[0] and cos's from
// !q[0]. `u == 0x10000` (bit 16) selects the exact quarter endpoint.
wire [16:0] sc_u_sin = sc_q[0]
    ? (17'h10000 - {1'b0, sc_f})
    : {1'b0, sc_f};
wire [16:0] sc_u_cos = sc_q[0]
    ? {1'b0, sc_f}
    : (17'h10000 - {1'b0, sc_f});
// mode 00 writes sin+cos, 01 writes sin only, 10 writes cos only. Reserved 11
// currently follows the compatible dual-output path but remains unavailable to
// software. Single-output modes use the first interpolation slot and finish at
// T6; dual output uses T5 for the first result and T6 for the second.
wire sc_single = sc_mode[0] ^ sc_mode[1];
wire sc_first_is_cos = sc_mode[1] && !sc_mode[0];
wire sc_interp_is_cos = sc_first_is_cos || (!sc_single && (sc_stage == 4'd6));
wire [16:0] sc_interp_u = sc_interp_is_cos ? sc_u_cos : sc_u_sin;
wire signed [17:0] sc_residue_ext = $signed({10'b0, sc_interp_u[7:0]});

// Packed interval read for the interpolation stages.
wire [16:0] sc_current = rf_read_a_data[16:0];
wire signed [9:0] sc_delta_raw = $signed(rf_read_a_data[26:17]);
wire signed [17:0] sc_delta_ext = {{8{sc_delta_raw[9]}}, sc_delta_raw};

// ---------------------------------------------------------------------------
// SINCOS range product through the shared 36x36 multiply pipe. Issued at T0;
// the pipe returns it at T0+3. phase = (signed(Fa) * K) >>> 32 and only the
// low 18 bits are the quadrant/fraction the lookup consumes. The explicit
// shift assignment keeps `>>>` out of any ternary.
// ---------------------------------------------------------------------------
assign mul_in_valid = load_now && is_sincos;
assign mul_in_a = {{4{x0[31]}}, x0};
assign mul_in_b = SINCOS_K;
assign mul_in_tag = 9'd0;

wire signed [63:0] sc_phase_shifted = mul_out_product >>> 32;
wire [17:0] sc_phase18 = sc_phase_shifted[17:0];

// ---------------------------------------------------------------------------
// Shared 18x18 multiplier. RCP/RSQRT feed `interp_delta * interp_residue` and
// consume the product as `>>> 9`; SINCOS feeds the packed interval delta and
// the reflected fraction and consumes it as `>>> 8`. Exactly one `*` remains,
// hence one MULT18X18.
// ---------------------------------------------------------------------------
wire [31:0] packed_interval = p0_rcp ? rf_read_a_data : rf_read_b_data;
wire [16:0] interval_current = packed_interval[16:0];
wire signed [9:0] interval_delta = $signed(packed_interval[26:17]);
wire signed [17:0] interp_delta = {{8{interval_delta[9]}}, interval_delta};
wire signed [17:0] interp_residue = $signed({9'b0, s1_residue});

wire signed [17:0] mul_x = sc_valid ? sc_delta_ext : interp_delta;
wire signed [17:0] mul_y = sc_valid ? sc_residue_ext : interp_residue;
wire signed [35:0] mul_product = mul_x * mul_y;

// T4 drives the selected first result; dual-output mode drives cosine at T5
// while interpolating sine.
wire [16:0] sc_addr_u = ((sc_stage == 4'd5) || sc_first_is_cos)
    ? sc_u_cos
    : sc_u_sin;
wire [8:0] sc_lut_address = 9'd256 + {1'b0, sc_addr_u[15:8]};

wire signed [35:0] interp_shifted = mul_product >>> 9;
wire signed [9:0] interp_correction = interp_shifted[9:0];
wire signed [17:0] interp_sum =
    $signed({1'b0, interval_current}) + {{8{interp_correction[9]}}, interp_correction};

// ---------------------------------------------------------------------------
// SINCOS interpolation and result sign (T5 first result, T6 second).
// ---------------------------------------------------------------------------
wire signed [35:0] sc_interp_shifted = mul_product >>> 8;
// The quarter-wave sample is unsigned Q16.16 (17 bits) and the interpolated
// correction is far below one unit, so their exact sum fits signed 18 bits.
// Keeping this narrow avoids a pointless 34-bit add/negate chain.
wire signed [17:0] sc_interp_correction = sc_interp_shifted[17:0];
wire signed [17:0] sc_interp_sum =
    $signed({1'b0, sc_current}) + sc_interp_correction;
wire [16:0] sc_interp_mag = sc_interp_u[16] ? 17'h10000 : sc_interp_sum[16:0];
wire sc_result_sign = sc_interp_is_cos ? (sc_q[1] ^ sc_q[0]) : sc_q[1];
wire signed [17:0] sc_positive_result = $signed({1'b0, sc_interp_mag});
wire signed [17:0] sc_signed_result = sc_result_sign
    ? -sc_positive_result
    : sc_positive_result;

// Scaling starts from a 17-bit positive magnitude. RCP needs at most a 33-bit
// temporary to detect signed-32 overflow; RSQRT's left shift is at most eight
// and never needs saturation. This replaces the old 64-bit shift/compare.
wire [31:0] right_scaled = {15'b0, s2_interpolated} >> s1_shift;
wire [32:0] rcp_left_wide = {16'b0, s2_interpolated} << s1_shift;
wire rcp_left_overflow = |rcp_left_wide[32:31];
wire [31:0] rcp_left_scaled = rcp_left_overflow
    ? 32'h7FFFFFFF
    : {1'b0, rcp_left_wide[30:0]};
wire [31:0] rsqrt_left_scaled = {15'b0, s2_interpolated} << s1_shift;
wire [31:0] scaled = s1_left
    ? (p0_rcp ? rcp_left_scaled : rsqrt_left_scaled)
    : right_scaled;
wire [31:0] rcp_value = p0_negative ? (~scaled + 32'd1) : scaled;
wire [31:0] out_rcp = s1_zero
    ? (p0_negative ? 32'h80000001 : 32'h7FFFFFFF)
    : rcp_value;
wire [31:0] out_rsqrt = s1_zero ? 32'h00000000 : scaled;
wire [31:0] result = p0_rcp ? out_rcp : out_rsqrt;

// The multiplier result is registered before the RF port: T6 writes the first
// result captured in T5, while T7 writes the second captured in T6.
wire sc_write = sc_valid
    && (sc_stage == 4'd6 || (!sc_single && (sc_stage == 4'd7)))
    && !abort;
wire [5:0] sc_write_index = sc_fd + ((sc_stage == 4'd7) ? 6'd1 : 6'd0);

assign rf_read_a_address = (p0_valid && p0_rcp) ? table_address
                         : (sc_valid && (sc_stage == 4'd4
                             || (!sc_single && (sc_stage == 4'd5))))
                             ? sc_lut_address
                             : 9'd0;
assign rf_read_b_address = (p0_valid && !p0_rcp) ? table_address : 9'd0;

// ---------------------------------------------------------------------------
// Registers.
// ---------------------------------------------------------------------------
always @(posedge clk) begin
    if (abort) begin
        p0_valid <= 1'b0;
        s1_valid <= 1'b0;
        s2_valid <= 1'b0;
        sc_valid <= 1'b0;
        sc_stage <= 4'd0;
    end else if (load_now) begin
        if (is_sincos) begin
            sc_valid <= 1'b1;
            sc_stage <= 4'd1;
            sc_fd <= fd;
            sc_mode <= mode;
            p0_valid <= 1'b0;
            s1_valid <= 1'b0;
            s2_valid <= 1'b0;
        end else begin
            sc_valid <= 1'b0;
            p0_valid <= 1'b1;
            p0_rcp <= is_rcp;
            p0_negative <= x0_negative;
            p0_magnitude <= x0_magnitude;
            p0_fd <= fd;
            s1_valid <= 1'b0;
            s2_valid <= 1'b0;
        end
    end else if (sc_valid) begin
        case (sc_stage)
            // T3: the shared pipe returns the range product; register q/f
            // from phase[17:0].
            4'd3: begin
                sc_q <= sc_phase18[17:16];
                sc_f <= sc_phase18[15:0];
            end
            // Register each interpolation before it reaches the RF write mux.
            4'd5: sc_result_reg <= sc_signed_result;
            // Single-output mode has written its only result during T6.
            // Dual-output mode captures the second result for the T7 write.
            4'd6: begin
                if (sc_single) begin
                    sc_valid <= 1'b0;
                    sc_stage <= 4'd0;
                end else begin
                    sc_result_reg <= sc_signed_result;
                end
            end
            // T7: both writes have been presented; clear the context.
            4'd7: begin
                sc_valid <= 1'b0;
                sc_stage <= 4'd0;
            end
            default: ;
        endcase
        if ((sc_stage != 4'd7) && !((sc_stage == 4'd6) && sc_single))
            sc_stage <= sc_stage + 4'd1;
    end else if (p0_valid) begin
        p0_valid <= 1'b0;
        s1_valid <= 1'b1;
        s1_residue <= norm_residue;
        s1_shift <= p0_rcp ? rcp_shift : rsqrt_shift;
        s1_left <= p0_rcp ? rcp_left : rsqrt_left;
        // RCP's zero flag selects the clamp; RSQRT's also covers x < 0 (its
        // result is zero for every non-positive input).
        s1_zero <= p0_rcp ? p0_zero : (p0_negative || p0_zero);
        s2_valid <= 1'b0;
    end else if (s1_valid) begin
        // T2 edge: capture the interpolation from the packed interval.
        s2_valid <= 1'b1;
        s2_interpolated <= interp_sum[16:0];
        s1_valid <= 1'b0;
    end else begin
        s2_valid <= 1'b0;
    end
end

assign rf_write_enable = (s2_valid || sc_write) && !abort;
assign rf_write_address = sc_write ? {3'b000, sc_write_index} : {3'b000, p0_fd};
assign rf_write_data = sc_write
    ? {{14{sc_result_reg[17]}}, sc_result_reg}
    : result;

// Busy is combinational over the T0..T3 (RCP/RSQRT) or T0..T7 (SINCOS)
// window; abort clears it immediately.
assign busy = (load_now || p0_valid || s1_valid || s2_valid || sc_valid) && !abort;

// Section-16 countdowns: R was satisfied before T0; W/X span the fixed busy
// window. The presented values read the window length during the load beat.
assign r_wait = 4'd0;
assign w_wait = load_now ? (is_sincos ? (mode_single ? 4'd7 : 4'd8)
    : 4'd4) : 4'd0;
assign x_wait = load_now ? (is_sincos ? (mode_single ? 4'd7 : 4'd8)
    : 4'd4) : 4'd0;

// 32-bit leading-zero count; 32 for an all-zero input. The count is the
// position of the highest set bit. The decision tree is balanced (five halving
// stages) rather than a per-bit priority loop: the loop form synthesizes to a
// 32-deep LUT chain, which blew the cpu_clk setup budget on the operand ->
// LUT-address path. The result is identical for every input. `clz_ref` in the
// leaf testbench stays an independent implementation.
function [5:0] clz32;
    input [31:0] value;
    reg [5:0] count;
    reg [15:0] v16;
    reg [7:0] v8;
    reg [3:0] v4;
    reg [1:0] v2;
    begin
        if (value == 32'd0) begin
            clz32 = 6'd32;
        end else begin
            count = 6'd0;
            if (value[31:16] == 16'd0) begin
                count = count + 6'd16;
                v16 = value[15:0];
            end else begin
                v16 = value[31:16];
            end
            if (v16[15:8] == 8'd0) begin
                count = count + 6'd8;
                v8 = v16[7:0];
            end else begin
                v8 = v16[15:8];
            end
            if (v8[7:4] == 4'd0) begin
                count = count + 6'd4;
                v4 = v8[3:0];
            end else begin
                v4 = v8[7:4];
            end
            if (v4[3:2] == 2'd0) begin
                count = count + 6'd2;
                v2 = v4[1:0];
            end else begin
                v2 = v4[3:2];
            end
            if (v2[1] == 1'b0)
                count = count + 6'd1;
            clz32 = count;
        end
    end
endfunction

endmodule
