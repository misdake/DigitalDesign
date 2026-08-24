module DspBoardSelfTest(
    input wire clk,
    input wire [1:0] buttons,
    output wire [5:0] leds,
    output wire uart_tx
);

reg [3:0] phase = 0;
reg error_sticky = 0;

wire signed [35:0] product;
wire signed [53:0] madd_result;
wire signed [53:0] accumulator;
wire signed [53:0] mul_sum_result;
wire signed [53:0] mul_difference_result;
wire signed [35:0] pre_add_product;
wire signed [35:0] pre_sub_product;
wire mac_reset_n = phase >= 2 && phase < 5;

DspMulS18 u_mul
    /* synthesis syn_hier = "macro" */ /* synthesis syn_noprune = 1 */ (
    .clk(clk), .a(-18'sd123), .b(18'sd77), .product(product));

DspMulAddS18 u_mul_add
    /* synthesis syn_hier = "macro" */ /* synthesis syn_noprune = 1 */ (
    .clk(clk), .a(-18'sd31), .b(18'sd19), .addend(36'sd1000),
    .result(madd_result));

DspMacS18 u_mac
    /* synthesis syn_hier = "macro" */ /* synthesis syn_noprune = 1 */ (
    .clk(clk), .reset_n(mac_reset_n), .a(-18'sd7), .b(18'sd13),
    .accumulator(accumulator));

DspMulSumS18 u_mul_sum
    /* synthesis syn_hier = "macro" */ /* synthesis syn_noprune = 1 */ (
    .clk(clk), .a(-18'sd3), .b(18'sd7), .c(18'sd5), .d(-18'sd9),
    .result(mul_sum_result));

DspMulDifferenceS18 u_mul_difference
    /* synthesis syn_hier = "macro" */ /* synthesis syn_noprune = 1 */ (
    .clk(clk), .a(-18'sd3), .b(18'sd7), .c(18'sd5), .d(-18'sd9),
    .result(mul_difference_result));

DspPreAddMulS18 u_pre_add_mul
    /* synthesis syn_hier = "macro" */ /* synthesis syn_noprune = 1 */ (
    .clk(clk), .a(18'sd10), .b(-18'sd3), .c(-18'sd8),
    .product(pre_add_product));

DspPreSubMulS18 u_pre_sub_mul
    /* synthesis syn_hier = "macro" */ /* synthesis syn_noprune = 1 */ (
    .clk(clk), .a(18'sd10), .b(-18'sd3), .c(-18'sd8),
    .product(pre_sub_product));

always @(posedge clk) begin
    if (|buttons) begin
        phase <= 0;
        error_sticky <= 0;
    end else begin
        if (phase == 4 &&
            (product != -36'sd9471 ||
             madd_result != 54'sd411 ||
             accumulator != -54'sd91 ||
             mul_sum_result != -54'sd66 ||
             mul_difference_result != 54'sd24 ||
             pre_add_product != -36'sd56 ||
             pre_sub_product != -36'sd104))
            error_sticky <= 1;
        if (phase < 8)
            phase <= phase + 1'b1;
    end
end

wire done = phase == 8;
assign leds = error_sticky ? 6'b100000 : (done ? 6'b000001 : 6'b000000);
assign uart_tx = 1'b1;

endmodule
