// 独立本地工作池/邮箱。同步一CE读、1W1R；AB=3也只用一组16深RAM16。
module rp_raster_rf #(parameter AB=4)(input clk,reset,ce,re,we,
 input[AB-1:0]ra,wa,input[29:0]wd,output reg[29:0]q);
`ifdef PORTABLE
 reg[29:0]mem[0:(1<<AB)-1];wire[29:0]raw=mem[ra];
 always @(posedge clk)if(ce&&we&&!reset)mem[wa]<=wd;
`else
 wire[31:0]raw32;wire[31:0]data={2'd0,wd};genvar i;
 generate for(i=0;i<8;i=i+1)begin:lanes
  RAM16SDP4 ram(.DO(raw32[i*4+:4]),.DI(data[i*4+:4]),
   .WAD({{(4-AB){1'b0}},wa}),.RAD({{(4-AB){1'b0}},ra}),.WRE(ce&&we&&!reset),.CLK(clk));
 end endgenerate
 wire[29:0]raw=raw32[29:0];
`endif
 always @(posedge clk)if(reset)q<=0;else if(ce&&re)q<=raw;
`ifndef SYNTHESIS
 always @(posedge clk)if(ce&&!reset&&we&&re&&wa==ra)$fatal(1,"独立RF/邮箱跨口同址冲突");
`endif
endmodule

// 不是镜像：两条18bit物理lane共同保存同一30bit行。
// B固定walker读；A准备期写，之后只scanner读。每端同步Q，无宽Q来源mux。
module rp_raster_steps(input clk,reset,ce,wre,we,input[7:0]wra,wa,input[29:0]wd,
 output[29:0]wq,input sre,input[7:0]sra,output[29:0]sq);
`ifdef PORTABLE
 reg[29:0]mem[0:255];reg[29:0]wheld,sheld;
 assign wq=wheld;assign sq=sheld;
 always @(posedge clk)begin
  if(ce&&we&&!reset)mem[wa]<=wd;
  if(reset)begin wheld<=0;sheld<=0;end
  else if(ce)begin if(wre)wheld<=mem[wra];if(sre)sheld<=mem[sra];end
 end
`else
 wire[35:0]aout,bout;wire[35:0]data={6'd0,wd};
 wire[9:0]aa={2'd0,(we?wa:sra)},ba={2'd0,wra};genvar j;
 generate for(j=0;j<2;j=j+1)begin:lanes
  DPX9B #(.BIT_WIDTH_0(18),.BIT_WIDTH_1(18),.READ_MODE0(0),.READ_MODE1(0),
   .WRITE_MODE0(2'b00),.WRITE_MODE1(2'b00),.RESET_MODE("SYNC")) ram(
   .DOA(aout[j*18+:18]),.DOB(bout[j*18+:18]),.DIA(data[j*18+:18]),.DIB(18'd0),
   .ADA({aa,2'b00,{2{we}}}),.ADB({ba,4'd0}),
   .CEA(ce&&!reset&&(we||sre)),.CEB(ce&&!reset&&wre),.WREA(we),.WREB(1'b0),
   .CLKA(clk),.CLKB(clk),.OCEA(ce),.OCEB(ce),.RESETA(reset),.RESETB(reset),.BLKSELA(3'd0),.BLKSELB(3'd0));
 end endgenerate
 assign wq=bout[29:0];assign sq=aout[29:0];
`endif
`ifndef SYNTHESIS
 always @(posedge clk)if(ce&&!reset)begin
  if(we&&sre)$fatal(1,"step准备写与scanner读重叠");
  if(we&&wre&&wa==wra)$fatal(1,"step跨口同址读写");
 end
`endif
endmodule
