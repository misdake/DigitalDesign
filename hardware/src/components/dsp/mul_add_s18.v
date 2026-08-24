module {{ module_name }}(
    input wire clk,
    input wire signed [17:0] a,
    input wire signed [17:0] b,
    input wire signed [35:0] addend,
    output reg signed [53:0] result
);

reg signed [17:0] a_r = 18'sd0;
reg signed [17:0] b_r = 18'sd0;
reg signed [35:0] addend_r = 36'sd0;
initial result = 54'sd0;

always @(posedge clk) begin
    a_r <= a;
    b_r <= b;
    addend_r <= addend;
    result <= (a_r * b_r) + addend_r;
end

endmodule
