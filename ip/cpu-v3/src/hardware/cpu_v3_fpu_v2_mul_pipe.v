// Shared 36x36 multiply pipeline for the FPU v2 multiply and dot paths.
//
// The core serializes instructions, so the two execution paths are never
// active at once and share this one inferred multiplier (one MULT36X36 =
// four 18x18 DSP lanes). The pipe is a FIFO: a tag (the RF write address)
// travels with the operands and comes back with the product, so the owner
// never recomputes a lane's destination.
//
// Timing (II = 1, latency 3):
//   T0   : in_valid with operands + tag on the input ports.
//   T0+1 : stage 1 registers captured the operands.
//   T0+2 : stage 2 holds the product.
//   T0+3 : stage 3 presents out_valid with the full-width product and tag.
// abort voids every in-flight entry combinationally and clears the valids.
module CpuV3FpuV2MulPipe (
    input wire clk,
    input wire abort,
    input wire in_valid,
    input wire [31:0] in_a,
    input wire [31:0] in_b,
    input wire [8:0] in_tag,
    output wire out_valid,
    // The low 64 bits of the product: Q16.16 pairs never exceed 2^62, so the
    // dropped bits are pure sign extension, and the dot accumulator wraps mod
    // 2^64 identically either way. (The harness IO layer is u64-wide.)
    output wire signed [63:0] out_product,
    output wire [8:0] out_tag
);

// Stage 1: latched operands (sign-extended to 36 bits) plus the tag.
reg s1_valid_r = 1'b0;
reg signed [35:0] s1_a_r = 36'sd0;
reg signed [35:0] s1_b_r = 36'sd0;
reg [8:0] s1_tag_r = 9'd0;

// Stage 2: first product register.
reg s2_valid_r = 1'b0;
reg signed [71:0] s2_prod_r = 72'sd0;
reg [8:0] s2_tag_r = 9'd0;

// Stage 3: second product register; the ports present it combinationally.
reg s3_valid_r = 1'b0;
reg signed [71:0] s3_prod_r = 72'sd0;
reg [8:0] s3_tag_r = 9'd0;

// Inferred 36x36 -> 72-bit signed multiplier: the stage-1 registers feed this
// combinational product and stage 2 registers it (repository mul_s18 style).
wire signed [71:0] product_next = $signed(s1_a_r) * $signed(s1_b_r);

always @(posedge clk) begin
    if (abort) begin
        s1_valid_r <= 1'b0;
        s2_valid_r <= 1'b0;
        s3_valid_r <= 1'b0;
    end else begin
        s3_valid_r <= s2_valid_r;
        if (s2_valid_r) begin
            s3_prod_r <= s2_prod_r;
            s3_tag_r <= s2_tag_r;
        end
        s2_valid_r <= s1_valid_r;
        if (s1_valid_r) begin
            s2_prod_r <= product_next;
            s2_tag_r <= s1_tag_r;
        end
        s1_valid_r <= in_valid;
        if (in_valid) begin
            s1_a_r <= $signed(in_a);
            s1_b_r <= $signed(in_b);
            s1_tag_r <= in_tag;
        end
    end
end

assign out_valid = s3_valid_r && !abort;
assign out_product = s3_prod_r[63:0];
assign out_tag = s3_tag_r;

endmodule
