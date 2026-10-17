// 新插值器原语边界：输入寄存＋输出寄存，共两个CE沿；CE=0全部冻结。
// 只提供固定算术，不带旧插值器的调度、上下文或存储结构。
module iz_pairmac(input clk,reset,ce,
 input [17:0] w0,w1,input signed [17:0] d0,d1,input signed [53:0] base,
 output signed [53:0] result);
`ifdef PORTABLE
 reg [17:0] aw0,aw1;reg signed [17:0] bd0,bd1;reg signed [53:0] bc,r;
 wire signed [18:0] sw0={1'b0,aw0},sw1={1'b0,aw1};
 always @(posedge clk)begin
  if(reset)begin aw0<=0;aw1<=0;bd0<=0;bd1<=0;bc<=0;r<=0;end
  else if(ce)begin aw0<=w0;aw1<=w1;bd0<=d0;bd1<=d1;bc<=base;r<=sw0*bd0+sw1*bd1+bc;end
 end
 assign result=r;
`else
 MULTADDALU18X18 #(.A0REG(1),.A1REG(1),.B0REG(1),.B1REG(1),.CREG(1),
  .PIPE0_REG(0),.PIPE1_REG(0),.OUT_REG(1),.ASIGN0_REG(0),.ASIGN1_REG(0),
  .BSIGN0_REG(0),.BSIGN1_REG(0),.ACCLOAD_REG0(0),.ACCLOAD_REG1(0),
  .B_ADD_SUB(0),.C_ADD_SUB(0),.MULTADDALU18X18_MODE(0),.MULT_RESET_MODE("SYNC")) m(
  .DOUT(result),.CASO(),.SOA(),.SOB(),.A0(w0),.A1(w1),.B0(d0),.B1(d1),.C(base),
  .SIA(18'd0),.SIB(18'd0),.CASI(55'd0),.ACCLOAD(1'b0),.ASEL(2'b00),.BSEL(2'b00),
  .ASIGN(2'b00),.BSIGN(2'b11),.CLK(clk),.CE(ce),.RESET(reset));
`endif
endmodule

// 同一固定输入口用于正深度K×R及G−uS；负S预存在参数中，DSP始终做加法。
module iz_widemac(input clk,reset,ce,input [17:0] a,input asign,input signed [35:0] b,
 input signed [53:0] c,output signed [53:0] result);
`ifdef PORTABLE
 reg [17:0] ar;reg sr;reg signed [35:0] br;reg signed [53:0] cr,r;
 wire signed [18:0] av={sr&&ar[17],ar};
 always @(posedge clk)begin
  if(reset)begin ar<=0;sr<=0;br<=0;cr<=0;r<=0;end
  else if(ce)begin ar<=a;sr<=asign;br<=b;cr<=c;r<=av*br+c;end
 end
 assign result=r;
`else
 MULTALU36X18 #(.AREG(1),.BREG(1),.CREG(0),.PIPE_REG(0),.OUT_REG(1),
  .ASIGN_REG(1),.BSIGN_REG(0),.ACCLOAD_REG0(0),.ACCLOAD_REG1(0),
  .MULTALU36X18_MODE(0),.C_ADD_SUB(0),.MULT_RESET_MODE("SYNC")) m(
  .DOUT(result),.CASO(),.A(a),.B(b),.C(c),.CASI(55'd0),.ACCLOAD(1'b0),
  .ASIGN(asign),.BSIGN(1'b1),.CLK(clk),.CE(ce),.RESET(reset));
`endif
endmodule

module iz_mul(input clk,reset,ce,input [17:0] a,b,output [35:0] result);
`ifdef PORTABLE
 reg [17:0] ar,br;reg [35:0] r;
 always @(posedge clk)begin
  if(reset)begin ar<=0;br<=0;r<=0;end
  else if(ce)begin ar<=a;br<=b;r<=ar*br;end
 end
 assign result=r;
`else
 MULT18X18 #(.AREG(1),.BREG(1),.PIPE_REG(0),.OUT_REG(1),
  .ASIGN_REG(0),.BSIGN_REG(0),.MULT_RESET_MODE("SYNC")) m(
  .DOUT(result),.SOA(),.SOB(),.A(a),.B(b),.SIA(18'd0),.SIB(18'd0),.ASEL(1'b0),.BSEL(1'b0),
  .ASIGN(1'b0),.BSIGN(1'b0),.CLK(clk),.CE(ce),.RESET(reset));
`endif
endmodule
