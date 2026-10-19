// 独立本地工作池/邮箱。同步一CE读、1W1R；AB=3也只用一组16深RAM16。
module rp_raster_rf #(parameter AB=4,parameter WIDTH=30)(input clk,reset,ce,re,we,
 input[AB-1:0]ra,wa,input[WIDTH-1:0]wd,output reg[WIDTH-1:0]q);
`ifdef PORTABLE
 reg[WIDTH-1:0]mem[0:(1<<AB)-1];wire[WIDTH-1:0]raw=mem[ra];
 always @(posedge clk)if(ce&&we&&!reset)mem[wa]<=wd;
`else
 wire[31:0]raw32;wire[31:0]data={{(32-WIDTH){1'b0}},wd};genvar i;
 generate for(i=0;i<(WIDTH+3)/4;i=i+1)begin:lanes
  RAM16SDP4 ram(.DO(raw32[i*4+:4]),.DI(data[i*4+:4]),
   .WAD({{(4-AB){1'b0}},wa}),.RAD({{(4-AB){1'b0}},ra}),.WRE(ce&&we&&!reset),.CLK(clk));
 end endgenerate
 wire[WIDTH-1:0]raw=raw32[WIDTH-1:0];
`endif
 always @(posedge clk)if(reset)q<=0;else if(ce&&re)q<=raw;
`ifndef SYNTHESIS
 always @(posedge clk)if(ce&&!reset&&we&&re&&wa==ra)$fatal(1,"独立RF/邮箱跨口同址冲突");
`endif
endmodule

// Ordinary rows use low30; row {edge,62} packs signed18 Nx and Ny.
// A writes import halves or preparation rows, then serves only scanner reads.
// B is the independent walker read port; its complete 36-bit Q feeds origin MAC.
module rp_raster_steps(input clk,reset,ce,wre,we,input[7:0]wra,wa,input[29:0]wd,
 input n_we,n_axis,input[1:0]n_edge,input[17:0]n_data,
 output[35:0]wq,input sre,input[7:0]sra,output[29:0]sq);
`ifdef PORTABLE
 reg[35:0]mem[0:255];reg[35:0]wheld;reg[29:0]sheld;
 assign wq=wheld;assign sq=sheld;
 always @(posedge clk)begin
  if(ce&&we&&!reset)mem[wa]<={6'd0,wd};
  if(ce&&n_we&&!reset)mem[{n_edge,6'd62}][n_axis*18+:18]<=n_data;
  if(reset)begin wheld<=0;sheld<=0;end
  else if(ce)begin if(wre)wheld<=mem[wra];if(sre)sheld<=mem[sra];end
 end
`else
 wire[35:0]aout,bout;wire[35:0]data={6'd0,wd};
 wire[9:0]aa={2'd0,(n_we?{n_edge,6'd62}:we?wa:sra)},ba={2'd0,wra};genvar j;
 generate for(j=0;j<2;j=j+1)begin:lanes
  wire lane_write=we||(n_we&&n_axis==j);
  wire[17:0]lane_data=n_we?n_data:data[j*18+:18];
  DPX9B #(.BIT_WIDTH_0(18),.BIT_WIDTH_1(18),.READ_MODE0(0),.READ_MODE1(0),
   .WRITE_MODE0(2'b00),.WRITE_MODE1(2'b00),.RESET_MODE("SYNC")) ram(
   .DOA(aout[j*18+:18]),.DOB(bout[j*18+:18]),.DIA(lane_data),.DIB(18'd0),
   .ADA({aa,2'b00,{2{lane_write}}}),.ADB({ba,4'd0}),
   .CEA(ce&&!reset&&(we||n_we||sre)),.CEB(ce&&!reset&&wre),.WREA(lane_write),.WREB(1'b0),
   .CLKA(clk),.CLKB(clk),.OCEA(ce),.OCEB(ce),.RESETA(reset),.RESETB(reset),.BLKSELA(3'd0),.BLKSELB(3'd0));
 end endgenerate
 assign wq=bout;assign sq=aout[29:0];
`endif
`ifndef SYNTHESIS
 always @(posedge clk)if(ce&&!reset)begin
  if((we||n_we)&&sre)$fatal(1,"step准备写与scanner读重叠");
  if(we&&n_we)$fatal(1,"normal and packed step writers overlap");
  if(we&&wre&&wa==wra)$fatal(1,"step跨口同址读写");
 end
`endif
endmodule
