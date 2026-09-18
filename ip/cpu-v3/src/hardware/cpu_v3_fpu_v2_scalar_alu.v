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
module CpuV3FpuV2ScalarAlu (
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

// Arithmetic primitives. Verilog keeps these at 32 bits, so the results wrap.
wire [31:0] sum = a + b;
wire [31:0] diff = a - b;
wire [31:0] neg_a = 32'h00000000 - a;

// |a|: negation also wraps, so for 0x80000000 (the most negative value) the
// absolute value is itself, exactly as specified.
wire [31:0] abs_a = a_signed[31] ? neg_a : a;

wire signed_lt = (a_signed < b_signed);
wire signed_eq = (a_signed == b_signed);
wire signed_gt = (a_signed > b_signed);
wire [31:0] min_ab = signed_lt ? a : b;
wire [31:0] max_ab = signed_gt ? a : b;

wire frac_nonzero = |a[15:0];

// FLOOR: clearing the 16 fractional bits rounds toward negative infinity for
// positive and negative two's-complement values alike.
wire [31:0] floor_a = {a[31:16], 16'h0000};

// CEIL: the floor, plus one unit in the last place when a fractional part
// remains. An exact integer input (fraction zero) is returned unchanged.
wire [31:0] ceil_a =
    floor_a + (frac_nonzero ? 32'h00010000 : 32'h00000000);

// ROUND: round-half-up, frozen rule. Adding 0x00008000 (one half unit) and then
// flooring makes an exact half always round upward.
wire [31:0] round_shifted = a + 32'h00008000;
wire [31:0] round_a = {round_shifted[31:16], 16'h0000};

// TRUNC: toward zero. Non-negative inputs use the floor; negative inputs use
// the ceil, which equals the floor for exact integers and floor + 1 otherwise.
wire [31:0] trunc_a = a_signed[31] ? ceil_a : floor_a;

assign flag_lt = signed_lt;
assign flag_eq = signed_eq;
assign flag_gt = signed_gt;

// Unlisted op codes fall into the default arm and produce a defined zero, so an
// unknown op can never propagate X into result.
always @(*) begin
    case (op)
        4'h0: result = sum;
        4'h1: result = diff;
        4'h3: result = min_ab;
        4'h4: result = max_ab;
        4'h5: result = abs_a;
        4'h6: result = neg_a;
        4'h7: result = floor_a;
        4'h8: result = ceil_a;
        4'h9: result = round_a;
        4'hA: result = trunc_a;
        4'hB: result = 32'h00000000;
        4'hF: result = a;
        default: result = 32'h00000000;
    endcase
end

endmodule
