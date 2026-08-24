module {{ module_name }}(
    input wire clk,
    input wire signed [17:0] a,
    input wire signed [17:0] b,
    output reg signed [35:0] product
);

reg signed [17:0] a_r = 18'sd0;
reg signed [17:0] b_r = 18'sd0;
initial product = 36'sd0;

always @(posedge clk) begin
    a_r <= a;
    b_r <= b;
    product <= a_r * b_r;
end

endmodule
