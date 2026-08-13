module DspBoardSelfTest(
    input wire clk,
    input wire [1:0] buttons,
    output wire [5:0] leds,
    output wire uart_tx
);

reg [3:0] phase = 0;
reg error_sticky = 0;

wire signed [17:0] mul_a = -18'sd123;
wire signed [17:0] mul_b = 18'sd77;
wire signed [35:0] product;

wire signed [17:0] madd_a = -18'sd31;
wire signed [17:0] madd_b = 18'sd19;
wire signed [35:0] madd_c = 36'sd1000;
wire signed [53:0] madd_result;

wire mac_reset_n = phase >= 2 && phase < 5;
wire signed [17:0] mac_a = -18'sd7;
wire signed [17:0] mac_b = 18'sd13;
wire signed [53:0] accumulator;

DspMulS18 u_mul
    /* synthesis syn_hier = "macro" */ /* synthesis syn_noprune = 1 */ (
    .clk(clk), .a(mul_a), .b(mul_b), .product(product));

DspMulAddS18 u_mul_add
    /* synthesis syn_hier = "macro" */ /* synthesis syn_noprune = 1 */ (
    .clk(clk), .a(madd_a), .b(madd_b), .addend(madd_c),
    .result(madd_result));

DspMacS18 u_mac
    /* synthesis syn_hier = "macro" */ /* synthesis syn_noprune = 1 */ (
    .clk(clk), .reset_n(mac_reset_n), .a(mac_a), .b(mac_b),
    .accumulator(accumulator));

always @(posedge clk) begin
    if (|buttons) begin
        phase <= 0;
        error_sticky <= 0;
    end else begin
        if (phase == 4 &&
            (product != -36'sd9471 ||
             madd_result != 54'sd411 ||
             accumulator != -54'sd91))
            error_sticky <= 1;
        if (phase < 8)
            phase <= phase + 1'b1;
    end
end

wire done = phase == 8;
assign leds = error_sticky ? 6'b100000 : (done ? 6'b000001 : 6'b000000);
assign uart_tx = 1'b1;

endmodule
