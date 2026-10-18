// 512×36同步1写1读。地址提前一CE沿发射；禁止同地址跨口读写。
module iz_ram36(input clk,reset,ce,input we,input [8:0] wa,input [35:0] wd,
 input re,input [8:0] ra,output [35:0] q);
`ifdef PORTABLE
 reg [35:0] memory[0:511];reg [35:0] rd;
 assign q=rd;
 always @(posedge clk)begin
  if(reset)rd<=0;
  else if(ce)begin if(we)memory[wa]<=wd;if(re)rd<=memory[ra];end
 end
`else
 SDPX9B #(.BIT_WIDTH_0(36),.BIT_WIDTH_1(36),.READ_MODE(0),.RESET_MODE("SYNC")) ram(
  .DO(q),.DI(wd),.ADA({wa,1'b0,4'b1111}),.ADB({ra,5'b0}),
  .CLKA(clk),.CLKB(clk),.CEA(ce&&we&&!reset),.CEB(ce&&re&&!reset),.OCE(ce),
  .RESETA(1'b0),.RESETB(reset),.BLKSELA(3'b0),.BLKSELB(3'b0));
`endif
`ifndef SYNTHESIS
 always @(posedge clk)if(ce&&!reset&&we&&re&&wa==ra)$fatal(1,"iz_ram36跨口同址冲突");
`endif
endmodule

// 参数只在流水全排空时写。运行期由提前一CE的地址寄存驱动异步RAM16，
// 下一个CE沿直接进入DSP的A/B/C输入寄存，不再经过重复的宽Q寄存。
// 本模块没有隐含同步读延迟；地址寄存和真实采样沿由调用方负责。
module iz_params_async #(parameter WIDTH=54)(input clk,reset,ce,input we,input [3:0] wa,
 input [WIDTH-1:0] wd,input [3:0] ra,output [WIDTH-1:0] q);
`ifdef PORTABLE
 reg [WIDTH-1:0] memory[0:15];assign q=memory[ra];
 always @(posedge clk)if(ce&&!reset&&we)memory[wa]<=wd;
`else
 localparam LANES=(WIDTH+3)/4;
 wire [LANES*4-1:0] padded={{(LANES*4-WIDTH){1'b0}},wd};
 wire [LANES*4-1:0] lanes;
 genvar i;generate for(i=0;i<LANES;i=i+1)begin:word_lane
  RAM16SDP4 ram(.DO(lanes[i*4+:4]),.DI(padded[i*4+:4]),.WAD(wa),.RAD(ra),
   .WRE(ce&&!reset&&we),.CLK(clk));
 end endgenerate
 assign q=lanes[WIDTH-1:0];
`endif
endmodule

// 少量需并行取出的参数用SSRAM；输出显式寄存，仍按同步一沿契约排程。
module iz_params #(parameter WIDTH=54)(input clk,reset,ce,input we,input [3:0] wa,
 input [WIDTH-1:0] wd,input re,input [3:0] ra,output reg [WIDTH-1:0] q);
 wire [WIDTH-1:0] raw;
`ifdef PORTABLE
 reg [WIDTH-1:0] memory[0:15];assign raw=memory[ra];
 always @(posedge clk)if(ce&&!reset&&we)memory[wa]<=wd;
`else
 localparam LANES=(WIDTH+3)/4;
 wire [LANES*4-1:0] padded={{(LANES*4-WIDTH){1'b0}},wd};
 wire [LANES*4-1:0] lanes;
 genvar i;generate for(i=0;i<LANES;i=i+1)begin:word_lane
  RAM16SDP4 ram(.DO(lanes[i*4+:4]),.DI(padded[i*4+:4]),.WAD(wa),.RAD(ra),
   .WRE(ce&&!reset&&we),.CLK(clk));
 end endgenerate
 assign raw=lanes[WIDTH-1:0];
`endif
 always @(posedge clk)if(reset)q<=0;else if(ce&&re)q<=raw;
`ifndef SYNTHESIS
 always @(posedge clk)if(ce&&!reset&&we&&re&&wa==ra)$fatal(1,"iz_params同址读写未定义");
`endif
endmodule

// Independent coefficient-high bank; one synchronous read with the low bank.
module iz_ram18(input clk,reset,ce,input we,input [8:0] wa,input [17:0] wd,
 input re,input [8:0] ra,output [17:0] q);
`ifdef PORTABLE
 reg [17:0] memory[0:511];reg [17:0] rd;
 assign q=rd;
 always @(posedge clk)begin
  if(reset)rd<=0;
  else if(ce)begin if(we)memory[wa]<=wd;if(re)rd<=memory[ra];end
 end
`else
 wire [35:0] data;
 assign q=data[17:0];
 SDPX9B #(.BIT_WIDTH_0(18),.BIT_WIDTH_1(18),.READ_MODE(0),.RESET_MODE("SYNC")) ram(
  .DO(data),.DI({18'd0,wd}),.ADA({1'b0,wa,4'b1111}),.ADB({1'b0,ra,4'b0}),
  .CLKA(clk),.CLKB(clk),.CEA(ce&&we&&!reset),.CEB(ce&&re&&!reset),.OCE(ce),
  .RESETA(1'b0),.RESETB(reset),.BLKSELA(3'b0),.BLKSELB(3'b0));
`endif
`ifndef SYNTHESIS
 always @(posedge clk)if(ce&&!reset&&we&&re&&wa==ra)$fatal(1,"iz_ram18 cross-port address collision");
`endif
endmodule
