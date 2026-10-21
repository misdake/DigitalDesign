// Scalar ALU for the FPU v2 fixed-point datapath.
//
// Purely combinational leaf: no clock, no state. The enclosing controller owns
// every register and decides when to capture result, so this module only maps
// (a, b, op) to result and to the comparison flags.
//
// Numbers are Q16.16: value = signed(a) / 65536, bit 31 is the sign bit and
// bits [15:0] are the fractional part. The overflow policy for every operation
// is wrap (two's-complement modulo 2^32); there is no saturation, no rounding
// flag and no exception output.
//
// flag_lt / flag_eq / flag_gt are only specified while op == CMP. For every
// other op their value is don't-care; they are still driven combinationally
// from a and b here so no undefined value ever leaks onto them.
//
// Gowin synthesis silently absorbs wide additions into a DSP block. All
// add/sub on this leaf must stay in fabric LUTs, hence the attribute below.
/* synthesis syn_dspstyle = "logic" */
module CpuV3FpuScalarAlu (
    input wire [31:0] a,
    input wire [31:0] b,
    input wire [3:0] op,
    output reg [31:0] result,
    output wire flag_lt,
    output wire flag_eq,
    output wire flag_gt
);

wire signed [31:0] a_signed;
wire signed [31:0] b_signed;
assign a_signed = a;
assign b_signed = b;

// Only one ALU result is consumed per lane. ADD, SUB, MIN/MAX, CMP and
// NEG/ABS share one wrapping adder; comparison uses the difference sign only
// when operand signs agree, so signed overflow cannot corrupt ordering.
wire negate = op == 4'h5 || op == 4'h6;
wire subtract = op != 4'h0;
wire [31:0] arithmetic_left = negate ? 32'b0 : a;
wire [31:0] arithmetic_right = negate ? a : b;
wire [31:0] arithmetic_rhs = arithmetic_right ^ {32{subtract}};
wire [31:0] arithmetic_result = arithmetic_left + arithmetic_rhs + subtract;

// |a|: negation also wraps, so for 0x80000000 (the most negative value) the
// absolute value is itself, exactly as specified.
wire [31:0] abs_a = a_signed[31] ? arithmetic_result : a;

wire signed_lt = a[31] != b[31] ? a[31] : arithmetic_result[31];
// CMP and MIN/MAX use subtraction; their equality result is already present
// in the shared difference. Flags for other operations remain unspecified.
wire signed_eq = ~|arithmetic_result;
wire signed_gt = !signed_lt && !signed_eq;
// MIN/MAX publish one selected operand. Equality is needed only by CMP;
// tie operands have identical bits, so one ordering/direction control serves
// the common wide result mux without waiting for equality reduction.
wire select_a = signed_lt ^ (op == 4'h4);
wire [31:0] minmax_result = select_a ? a : b;

wire frac_nonzero = |a[15:0];

// FLOOR: clearing the 16 fractional bits rounds toward negative infinity for
// positive and negative two's-complement values alike.
wire [31:0] floor_a = {a[31:16], 16'h0000};

// These consumers discard the fractional half. A single 16-bit increment
// produces CEIL, half-up ROUND and toward-zero TRUNC directly, including wrap
// at the signed integer boundary; no full-width rounded temporary is needed.
wire integer_increment = op == 4'h9 ? a[15] :
    (frac_nonzero && (op == 4'h8 || a[31]));
wire [15:0] rounded_integer = a[31:16] + integer_increment;
wire [31:0] rounded_a = {rounded_integer, 16'h0000};

assign flag_lt = signed_lt;
assign flag_eq = signed_eq;
assign flag_gt = signed_gt;

// Unlisted op codes fall into the default arm and produce a defined zero, so an
// unknown op can never propagate X into result.
always @(*) begin
    case (op)
        4'h0: result = arithmetic_result;
        4'h1: result = arithmetic_result;
        4'h3, 4'h4: result = minmax_result;
        4'h5: result = abs_a;
        4'h6: result = arithmetic_result;
        4'h7: result = floor_a;
        4'h8: result = rounded_a;
        4'h9: result = rounded_a;
        4'hA: result = rounded_a;
        4'hB: result = 32'h00000000;
        4'hF: result = a;
        default: result = 32'h00000000;
    endcase
end

endmodule
