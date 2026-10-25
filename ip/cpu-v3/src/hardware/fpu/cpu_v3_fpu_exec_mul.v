// Single product register; selected top ties tags to zero.
module CpuV3FpuExecMul (
    input wire clk,
    input wire abort,
    input wire in_valid,
    input wire signed [35:0] in_a,
    input wire signed [35:0] in_b,
    input wire [8:0] in_tag,
    output wire out_valid,
    // The low 64 bits of the product: Q16.16 pairs never exceed 2^62, the
    // SINCOS range product (|Fa| < 2^31, K < 2^32) stays below 2^63, and the
    // dot accumulator wraps mod 2^64 identically either way. (The harness IO
    // layer is u64-wide.)
    output wire signed [63:0] out_product,
    output wire [8:0] out_tag,
    // Include the visible output slot until its consumer's write edge.
    // Ordinary destinations use 0..66; special bit7 and integer bit8 exclude
    // their entries. This derives ownership from the single product slot.
    output wire ordinary_pending
);

// One product/output slot. The enclosing synchronous RF already holds the
// FPU operands; integer operands remain valid throughout their Execute edge.
reg valid_r = 0;
reg signed [71:0] product_r = 0;
reg [8:0] tag_r = 0;
wire signed [71:0] product_next = $signed(in_a) * $signed(in_b);
always @(posedge clk) begin
    if (abort) valid_r <= 0;
    else begin
        valid_r <= in_valid;
        if (in_valid) begin
            product_r <= product_next;
            tag_r <= in_tag;
        end
    end
end
assign out_valid = valid_r && !abort;
assign out_product = product_r[63:0];
assign out_tag = tag_r;
assign ordinary_pending = !abort && valid_r && tag_r[8:7] == 2'b00;
endmodule
