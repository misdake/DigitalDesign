module {{ module_name }}(
    input wire clk,
    input wire signed [17:0] a,
    input wire signed [17:0] b,
    input wire signed [17:0] c,
    input wire signed [17:0] d,
    output wire signed [53:0] result
);

`ifdef __ICARUS__
reg signed [17:0] a_r = 18'sd0;
reg signed [17:0] b_r = 18'sd0;
reg signed [17:0] c_r = 18'sd0;
reg signed [17:0] d_r = 18'sd0;
reg signed [53:0] result_r = 54'sd0;
always @(posedge clk) begin
    a_r <= a;
    b_r <= b;
    c_r <= c;
    d_r <= d;
    result_r <= (a_r * b_r) - (c_r * d_r);
end
assign result = result_r;
`else
wire [54:0] unused_cascade;
wire [17:0] unused_soa;
wire [17:0] unused_sob;
MULTADDALU18X18 #(
    .A0REG(1'b1), .B0REG(1'b1), .A1REG(1'b1), .B1REG(1'b1),
    .CREG(1'b0), .PIPE0_REG(1'b0), .PIPE1_REG(1'b0), .OUT_REG(1'b1),
    .ASIGN0_REG(1'b0), .ASIGN1_REG(1'b0),
    .BSIGN0_REG(1'b0), .BSIGN1_REG(1'b0),
    .ACCLOAD_REG0(1'b0), .ACCLOAD_REG1(1'b0), .SOA_REG(1'b0),
    .B_ADD_SUB(1'b1), .C_ADD_SUB(1'b0),
    .MULTADDALU18X18_MODE(0), .MULT_RESET_MODE("SYNC")
) dsp (
    .DOUT(result), .CASO(unused_cascade), .SOA(unused_soa), .SOB(unused_sob),
    .A0(a), .B0(b), .A1(c), .B1(d), .C(54'd0),
    .SIA(18'd0), .SIB(18'd0), .CASI(55'd0), .ACCLOAD(1'b0),
    .ASEL(2'b00), .BSEL(2'b00), .ASIGN(2'b11), .BSIGN(2'b11),
    .CLK(clk), .CE(1'b1), .RESET(1'b0)
);
`endif

endmodule
