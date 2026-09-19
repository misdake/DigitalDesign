// FPU v2 special-function execution-path controller (opcode 0xD subops
// 0x0C RCP and 0x0D RSQRT).
//
// The special path is blocking (fpu-design-v2 section 4.2): while active it
// monopolizes both register-file BSRAM read ports. Each function addresses its
// own asymmetric mirror -- RCP reads port A (mirror 0, RCP @64..191), RSQRT
// reads port B (mirror 1, even table @64..191, odd table @192..319). It is
// fully self-contained: no shared multiply pipe and no Newton step (revised
// Stage 7 freeze, section 9.4). The one inferred 18x18 multiplier below is the
// interpolation product `delta * residue`.
//
// Word0 carries Fa in bits [11:6]; for 0xD subops only Fa matters (the
// operand), and the front-end parks that address on RF read port A one beat
// before T0, so the operand is on rf_read_a_data during T0. Word1 carries Fd in
// bits [15:10], the 6-bit subop in bits [9:4] and the unused mode field in
// bits [3:0]. T0 is the cycle where instr_complete is high and
// instr_opcode == 0xD with subop 0x0C or 0x0D.
//
// Linear interpolation needs two consecutive table entries. They are read one
// per beat on the function's own port:
//   T0         : normalize rf_read_a_data (CLZ, index/residue/exponent) and
//                drive the table address `base + index`; the T0 edge registers
//                the operand and the read, so the RF latches `table[index]`.
//   T1         : drive the address `base + index + 1` (the RF presents
//                `table[index]` as `current`); the T1 edge captures `current`
//                and the RF latches `table[index + 1]`.
//   T2         : the RF presents `next`; interpolate combinationally with the
//                residue (one inferred 18x18 signed product plus fabric adds);
//                the T2 edge captures the interpolated magnitude.
//   T3         : scale by the power of two (barrel shift; RCP saturating) and
//                apply the sign, presenting rf_write_enable with Fd.
//   after T3   : busy returns low.
//
// The final table entry's `next` is the exact endpoint at index 127: 0.5 for
// RCP and for the odd RSQRT table, round(2^16/sqrt(2)) for the even RSQRT
// table. The second read address is masked and the constant swapped in, so the
// interpolation never reads past the table.
//
// Special values (section 9.4): RCP(0) -> 0x7FFF_FFFF with the input's sign;
// RCP of a negative value is the negative of the magnitude's reciprocal;
// RSQRT of x <= 0 is zero. These cases are handled explicitly, not left to the
// barrel shift, because CLZ(0) has no meaningful exponent.
//
// abort combinationally clears busy and gates the write port, cancelling any
// in-flight work; no register update is committed on the abort edge.
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
    // before T0; W/X span the fixed T0..T3 window).
    output wire [3:0] r_wait,
    output wire [3:0] w_wait,
    output wire [3:0] x_wait
);

// Word1 fields.
wire [5:0] subop = word1_raw[9:4];
wire [5:0] fd = word1_raw[15:10];

wire is_scalar = (instr_opcode == 4'hD);
wire is_rcp = is_scalar && (subop == 6'h0C);
wire is_rsqrt = is_scalar && (subop == 6'h0D);
wire is_special = is_rcp || is_rsqrt;

// T0 of this cycle.
wire load_now = instr_complete && is_special && !abort;

// ---------------------------------------------------------------------------
// T0 combinational normalization.
// ---------------------------------------------------------------------------
wire [31:0] x0 = rf_read_a_data;
wire x0_negative = x0[31];
wire [31:0] x0_magnitude = x0_negative ? (~x0 + 32'd1) : x0;
wire x0_zero = (x0_magnitude == 32'd0);

// Leading-zero count of the T0 magnitude; 32 for zero.
wire [5:0] clz = clz32(x0_magnitude);

// RCP: m = |x| << clz is in [2^31, 2^32). Bits [30:24] are the top 7 fraction
// bits (index) and bits [23:15] the 9-bit residue. The real exponent of |x| is
// e = clz - 16, so 1/|x| rescales by 2^(15 - clz): a left shift by clz-15 when
// clz >= 15, a right shift by 15-clz otherwise.
wire [31:0] rcp_norm = x0_magnitude << clz[4:0];
wire [6:0] rcp_index = rcp_norm[30:24];
wire [8:0] rcp_residue = rcp_norm[23:15];
wire [5:0] rcp_shift = (clz >= 6'd15) ? (clz - 6'd15) : (6'd15 - clz);
wire rcp_left = (clz >= 6'd15);

// RSQRT: normalize |x| = m * 2^e with m in [1,2), the same shape as RCP. The
// top 7 fraction bits of m index a 128-entry table and the next 9 bits are the
// residue -- plain bit slices, no division. The table is selected by the parity
// of the binary exponent: even uses T_even @64 (1/sqrt(m)), odd uses T_odd @192
// (1/sqrt(2m)). e = binary_exponent - 16, and the result rescales by
// 2^(-floor(e/2)): a right shift when e >= 0, a left shift otherwise.
wire [5:0] rsqrt_binary_exponent = 6'd31 - clz;
wire rsqrt_odd = rsqrt_binary_exponent[0];
wire [31:0] rsqrt_norm = x0_magnitude << clz[4:0];
wire [6:0] rsqrt_index = rsqrt_norm[30:24];
wire [8:0] rsqrt_residue = rsqrt_norm[23:15];
wire [8:0] rsqrt_base = rsqrt_odd ? 9'd192 : 9'd64;
wire signed [5:0] rsqrt_e =
    $signed({1'b0, rsqrt_binary_exponent}) - 6'sd16;
wire signed [5:0] rsqrt_shift = rsqrt_e >>> 1;
wire [4:0] rsqrt_shift_mag =
    rsqrt_shift[5] ? (~rsqrt_shift[4:0] + 5'd1) : rsqrt_shift[4:0];

// Entry index (7 bits, zero-extended) and base of this instruction's table.
wire [7:0] t0_index = is_rcp ? {1'b0, rcp_index} : {1'b0, rsqrt_index};
wire t0_final = (t0_index == 8'd127);
wire [8:0] t0_base = is_rcp ? 9'd64 : rsqrt_base;

// T0 address `base + index`; T1 reuses the registered base/index to address
// `base + index + 1` (masked for the final entry, which uses the endpoint).
wire [8:0] t0_address = t0_base + {1'b0, t0_index};

// ---------------------------------------------------------------------------
// Pipeline registers.
// ---------------------------------------------------------------------------
// Stage 1: normalization registered at the T0 edge; its base/index/final drive
// the T1 read address.
reg s1_valid = 1'b0;
reg s1_rcp = 1'b0;
reg s1_negative = 1'b0;
reg [8:0] s1_base = 9'd0;
reg [7:0] s1_index = 8'd0;
reg s1_final = 1'b0;
reg s1_odd = 1'b0;
reg [8:0] s1_residue = 9'd0;
reg [5:0] s1_shift = 6'd0;
reg s1_left = 1'b0;
reg s1_zero = 1'b0;

// Stage 2: `current` (= table[index], presented during T1) registered at the
// T1 edge. `next` is read combinationally during T2 from the live port.
reg s2_valid = 1'b0;
reg s2_rcp = 1'b0;
reg s2_negative = 1'b0;
reg s2_final = 1'b0;
reg s2_odd = 1'b0;
reg [8:0] s2_residue = 9'd0;
reg [5:0] s2_shift = 6'd0;
reg s2_left = 1'b0;
reg s2_zero = 1'b0;
reg signed [31:0] s2_current = 32'sd0;

// Stage 3: the interpolated magnitude registered at the T2 edge.
reg s3_valid = 1'b0;
reg s3_rcp = 1'b0;
reg s3_negative = 1'b0;
reg signed [31:0] s3_interpolated = 32'sd0;
reg [5:0] s3_shift = 6'd0;
reg s3_left = 1'b0;
reg s3_zero = 1'b0;
reg [5:0] s3_fd = 6'd0;

// T1 read address, from the registered base and index.
wire [8:0] t1_address = s1_base + {1'b0, s1_index} + (s1_final ? 9'd0 : 9'd1);
// During T0 the path drives t0_address; during T1 (s1_valid one-shot) it
// drives t1_address.
wire driving = load_now || s1_valid;

assign rf_read_a_address =
    is_rcp ? (driving ? (load_now ? t0_address : t1_address) : 9'd0) : 9'd0;
assign rf_read_b_address =
    is_rsqrt ? (driving ? (load_now ? t0_address : t1_address) : 9'd0) : 9'd0;

// T1/T2: the function's own port presents table[index] during T1 and
// table[index+1] during T2. The final entry substitutes the exact endpoint:
// 0x8000 for RCP and the odd RSQRT table, 0xB505 for the even RSQRT table.
wire signed [31:0] port_current =
    is_rcp ? $signed(rf_read_a_data) : $signed(rf_read_b_data);
wire signed [31:0] s2_next = s2_final
    ? ((s2_rcp || s2_odd) ? 32'sh00008000 : 32'sh0000B505)
    : port_current;

// T2 combination: linear interpolation. `delta` is small and signed, `residue`
// unsigned, so the product fits one inferred 18x18 multiplier; every table is
// 128-entry with a 9-bit residue, so the arithmetic shift is always 9.
wire signed [31:0] s2_delta = s2_next - s2_current;
wire signed [31:0] s2_product = s2_delta * $signed({23'b0, s2_residue});
wire signed [31:0] s2_interpolated = s2_current + (s2_product >>> 9);

// T3 combination: scale and sign. Both functions rescale by a power of two --
// a right shift for a non-negative shift, a saturating left shift otherwise --
// then RCP applies the input sign; its zero case is the clamp 0x7FFF_FFFF with
// the input sign. RSQRT is always positive and its non-positive case is zero.
wire signed [31:0] s3_scaled = s3_left
    ? saturate_shift(s3_interpolated, s3_shift)
    : (s3_interpolated >>> s3_shift);
wire signed [31:0] s3_rcp_value = s3_negative ? (~s3_scaled + 32'd1) : s3_scaled;
wire [31:0] s3_out_rcp = s3_zero
    ? (s3_negative ? 32'h80000001 : 32'h7FFFFFFF)
    : s3_rcp_value;
wire [31:0] s3_out_rsqrt = s3_zero ? 32'h00000000 : s3_scaled;
wire [31:0] s3_signed = s3_rcp ? s3_out_rcp : s3_out_rsqrt;

// Left-shift a signed value by `shift` bits, saturating to 0x7FFF_FFFF. The
// 64-bit intermediate is wide enough for a 32-bit magnitude shifted by 16.
function signed [31:0] saturate_shift;
    input signed [31:0] value;
    input [5:0] shift;
    reg [63:0] wide;
    begin
        wide = {32'b0, value[31:0]} << shift;
        if (wide > 64'h000000007FFFFFFF)
            saturate_shift = 32'sh7FFFFFFF;
        else
            saturate_shift = $signed(wide[31:0]);
    end
endfunction

// ---------------------------------------------------------------------------
// Registers.
// ---------------------------------------------------------------------------
always @(posedge clk) begin
    if (abort) begin
        s1_valid <= 1'b0;
        s2_valid <= 1'b0;
        s3_valid <= 1'b0;
    end else if (load_now) begin
        s2_valid <= 1'b0;
        s3_valid <= 1'b0;
        s1_valid <= 1'b1;
        s1_rcp <= is_rcp;
        s1_negative <= x0_negative;
        s1_base <= t0_base;
        s1_index <= t0_index;
        s1_final <= t0_final;
        s1_odd <= is_rcp ? 1'b0 : rsqrt_odd;
        s1_residue <= is_rcp ? rcp_residue : rsqrt_residue;
        s1_shift <= is_rcp ? rcp_shift : {1'b0, rsqrt_shift_mag};
        s1_left <= is_rcp ? rcp_left : rsqrt_shift[5];
        // RCP's zero flag selects the clamp; RSQRT's also covers x < 0 (its
        // result is zero for every non-positive input).
        s1_zero <= is_rcp ? x0_zero : (x0_negative || x0_zero);
    end else if (s1_valid) begin
        // T1 edge: capture `current` (table[index], on the port now) and the
        // control bits; `next` is read live during T2.
        s2_valid <= 1'b1;
        s2_rcp <= s1_rcp;
        s2_negative <= s1_negative;
        s2_final <= s1_final;
        s2_odd <= s1_odd;
        s2_residue <= s1_residue;
        s2_shift <= s1_shift;
        s2_left <= s1_left;
        s2_zero <= s1_zero;
        s2_current <= port_current;
        s1_valid <= 1'b0;
        s3_valid <= 1'b0;
    end else if (s2_valid) begin
        // T2 edge: capture the interpolated magnitude.
        s3_valid <= 1'b1;
        s3_rcp <= s2_rcp;
        s3_negative <= s2_negative;
        s3_interpolated <= s2_interpolated;
        s3_shift <= s2_shift;
        s3_left <= s2_left;
        s3_zero <= s2_zero;
        s3_fd <= fd;
        s2_valid <= 1'b0;
    end else begin
        s3_valid <= 1'b0;
    end
end

assign rf_write_enable = s3_valid && !abort;
assign rf_write_address = {3'b000, s3_fd};
assign rf_write_data = s3_signed;

// Busy is combinational over the T0..T3 window; abort clears it immediately.
assign busy = (load_now || s1_valid || s2_valid || s3_valid) && !abort;

// Section-16 countdowns: R was satisfied before T0; W/X span the fixed T0..T3
// latency. The presented values read 4 during the load beat and count down.
assign r_wait = 4'd0;
assign w_wait = load_now ? 4'd4 : 4'd0;
assign x_wait = load_now ? 4'd4 : 4'd0;

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
