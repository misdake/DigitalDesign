module {{ module_name }}(
    input wire clk,
    input wire signed [17:0] a,
    input wire signed [17:0] b,
    input wire signed [17:0] c,
    output wire signed [35:0] product
);

`ifdef __ICARUS__
reg signed [17:0] a_r = 18'sd0;
reg signed [17:0] b_r = 18'sd0;
reg signed [17:0] c_r = 18'sd0;
wire signed [17:0] pre_add = a_r + b_r;
reg signed [35:0] product_r = 36'sd0;
always @(posedge clk) begin
    a_r <= a;
    b_r <= b;
    c_r <= c;
    product_r <= pre_add * c_r;
end
assign product = product_r;
`else
wire [17:0] pre_add;
wire [17:0] unused_padd_so;
wire [17:0] unused_padd_sbo;
wire [17:0] unused_mult_soa;
wire [17:0] unused_mult_sob;
PADD18 #(
    .AREG(1'b0), .BREG(1'b0), .ADD_SUB(1'b0),
    .PADD_RESET_MODE("SYNC"), .BSEL_MODE(1'b1), .SOREG(1'b0)
) pre_adder (
    .DOUT(pre_add), .SO(unused_padd_so), .SBO(unused_padd_sbo),
    .A(a), .B(b), .SI(18'd0), .SBI(18'd0), .ASEL(1'b0),
    .CLK(clk), .CE(1'b1), .RESET(1'b0)
);
MULT18X18 #(
    .AREG(1'b1), .BREG(1'b1), .OUT_REG(1'b1), .PIPE_REG(1'b0),
    .ASIGN_REG(1'b0), .BSIGN_REG(1'b0), .SOA_REG(1'b0),
    .MULT_RESET_MODE("SYNC")
) multiplier (
    .DOUT(product), .SOA(unused_mult_soa), .SOB(unused_mult_sob),
    .A(pre_add), .B(c), .SIA(18'd0), .SIB(18'd0),
    .ASEL(1'b0), .BSEL(1'b0), .ASIGN(1'b1), .BSIGN(1'b1),
    .CLK(clk), .CE(1'b1), .RESET(1'b0)
);
`endif

endmodule
