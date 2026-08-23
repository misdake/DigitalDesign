module {{ module_name }}(
    input wire clk,
    input wire reset_n,
    input wire signed [17:0] a,
    input wire signed [17:0] b,
    output reg signed [53:0] accumulator
);

reg signed [17:0] a_r = 18'sd0;
reg signed [17:0] b_r = 18'sd0;
initial accumulator = 54'sd0;

always @(posedge clk or negedge reset_n) begin
    if (!reset_n) begin
        a_r <= 18'sd0;
        b_r <= 18'sd0;
        accumulator <= 54'sd0;
    end else begin
        a_r <= a;
        b_r <= b;
        accumulator <= accumulator + (a_r * b_r);
    end
end

endmodule
