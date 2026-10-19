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
module iz_params_async #(parameter WIDTH=54,parameter PC_CONSTANTS=0)(input clk,reset,ce,input we,input [3:0] wa,
 input [WIDTH-1:0] wd,input [3:0] ra,output [WIDTH-1:0] q);
`ifdef PORTABLE
 reg [WIDTH-1:0] memory[0:15];assign q=memory[ra];
 // Configuration-time constants share the data bank without runtime writes.
 initial if(PC_CONSTANTS)begin memory[4]=0;memory[6]=0;memory[7]=131071;end
 always @(posedge clk)if(ce&&!reset&&we)memory[wa]<=wd;
`else
 localparam LANES=(WIDTH+3)/4;
 wire [LANES*4-1:0] padded={{(LANES*4-WIDTH){1'b0}},wd};
 wire [LANES*4-1:0] lanes;
 genvar i;generate for(i=0;i<LANES;i=i+1)begin:word_lane
  RAM16SDP4 #(.INIT_0(PC_CONSTANTS&&i*4<17?16'h0080:16'h0000),
   .INIT_1(PC_CONSTANTS&&i*4+1<17?16'h0080:16'h0000),
   .INIT_2(PC_CONSTANTS&&i*4+2<17?16'h0080:16'h0000),
   .INIT_3(PC_CONSTANTS&&i*4+3<17?16'h0080:16'h0000)) ram(
   .DO(lanes[i*4+:4]),.DI(padded[i*4+:4]),.WAD(wa),.RAD(ra),
   .WRE(ce&&!reset&&we),.CLK(clk));
 end endgenerate
 assign q=lanes[WIDTH-1:0];
`endif
`ifndef SYNTHESIS
 always @(posedge clk)if(PC_CONSTANTS&&ce&&!reset&&we&&(wa==4||wa==6||wa==7))
  $fatal(1,"iz_params_async write to constant row");
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

// One physical 18-bit 1W1R table: mutable operands 0..15 and immutable factors.
// The synchronous Q replaces the former address register plus async RAM16 Q.
module iz_operand_table(input clk,reset,ce,input we,input[3:0]wa,input[16:0]wd,
 input re,input[6:0]ra,output[17:0]q);
 function[17:0]constant_value;
  input integer address;integer b;reg[17:0]step;
  begin
   constant_value=0;
   if(address>=32&&address<=49)constant_value=18'd131072>>(address-32);
   else if(address>=64&&address<80)begin
    b=address%8;if(b>4)b=0;step=18'd32<<b;
    if(address>=72)constant_value=18'd0-step;else constant_value=step;
   end
   else if(address==80)constant_value=30;
   else if(address==81)constant_value=18'h3ffff;
  end
 endfunction
`ifdef PORTABLE
 reg[17:0]memory[0:127];reg[17:0]rd;integer k;
 initial for(k=32;k<128;k=k+1)memory[k]=constant_value(k);
 assign q=rd;
 always @(posedge clk)begin
  if(reset)rd<=0;
  else if(ce)begin if(we)memory[{3'd0,wa}]<={1'b0,wd};if(re)rd<=memory[ra];end
 end
`else
 function[287:0]constant_block;
  input integer base;integer k;
  begin for(k=0;k<16;k=k+1)constant_block[k*18+:18]=constant_value(base+k);end
 endfunction
 wire[35:0]data;assign q=data[17:0];
 SDPX9B #(.BIT_WIDTH_0(18),.BIT_WIDTH_1(18),.READ_MODE(0),.RESET_MODE("SYNC"),
  .INIT_RAM_02(constant_block(32)),.INIT_RAM_03(constant_block(48)),
  .INIT_RAM_04(constant_block(64)),.INIT_RAM_05(constant_block(80))) ram(
  .DO(data),.DI({19'd0,wd}),.ADA({6'd0,wa,4'b1111}),.ADB({3'd0,ra,4'd0}),
  .CLKA(clk),.CLKB(clk),.CEA(ce&&we&&!reset),.CEB(ce&&re&&!reset),.OCE(ce),
  .RESETA(1'b0),.RESETB(reset),.BLKSELA(3'b0),.BLKSELB(3'b0));
`endif
`ifndef SYNTHESIS
 always @(posedge clk)if(ce&&!reset&&we&&re&&{3'd0,wa}==ra)
  $fatal(1,"iz_operand_table cross-port address collision");
`endif
endmodule
