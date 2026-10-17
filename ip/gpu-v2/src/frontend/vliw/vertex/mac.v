// Exact low 54 bits of signed32*signed32 + signed54.
// Both multipliers have one pipeline edge; both ALUs and C inputs bypass registers.
module shared_mac(input clk,reset,en,input [31:0] a,input [35:0] b,input [53:0] c,output [53:0] y);
 wire [53:0] lo,hi;
 wire [35:0] b36=b;
 wire [17:0] ah={{4{a[31]}},a[31:18]};
 wire [53:0] carry={{18{lo[53]}},lo[53:18]};
`ifdef PORTABLE
 reg [53:0] pl,ph;
 wire signed [18:0] al_signed={1'b0,a[17:0]};
 wire signed [17:0] ah_signed=ah;
 wire signed [35:0] b_signed=b36;
 always @(posedge clk) begin
   if(reset) begin pl<=0; ph<=0; end
   else if(en) begin pl<=al_signed*b_signed; ph<=ah_signed*b_signed; end
 end
 assign lo=pl+c;
 assign hi=ph+carry;
`else
 MULTALU36X18 #(.AREG(0),.BREG(0),.CREG(0),.OUT_REG(0),.PIPE_REG(1),
   .ASIGN_REG(0),.BSIGN_REG(0),.MULTALU36X18_MODE(0),.C_ADD_SUB(0)) low_mac(
   .DOUT(lo),.CASO(),.A(a[17:0]),.B(b36),.C(c),.CASI(55'd0),.ACCLOAD(1'b0),
   .ASIGN(1'b0),.BSIGN(1'b1),.CLK(clk),.CE(en),.RESET(reset));
 MULTALU36X18 #(.AREG(0),.BREG(0),.CREG(0),.OUT_REG(0),.PIPE_REG(1),
   .ASIGN_REG(0),.BSIGN_REG(0),.MULTALU36X18_MODE(0),.C_ADD_SUB(0)) high_mac(
   .DOUT(hi),.CASO(),.A(ah),.B(b36),.C(carry),.CASI(55'd0),.ACCLOAD(1'b0),
   .ASIGN(1'b1),.BSIGN(1'b1),.CLK(clk),.CE(en),.RESET(reset));
`endif
 assign y={hi[35:0],lo[17:0]};
endmodule
