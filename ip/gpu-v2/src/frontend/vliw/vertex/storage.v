// 四块1024x18；DMA32字先整字符号扩展，R工作写口只接emit36。
module shared_scratchpad #(parameter COMPACT=0)(input clk,reset,dma_we,input [9:0] dma_addr,
 input [63:0] dma_data,input le,re,work_we,input [9:0] la,ra,work_wa,input [35:0] work_data,
 output [35:0] lq,rq);
 wire [35:0] dl={{4{dma_data[31]}},dma_data[31:0]};
 wire [35:0] dr={{4{dma_data[63]}},dma_data[63:32]};
generate if(COMPACT)begin:compact
`ifdef PORTABLE
 reg [35:0] lm[0:511],rm[0:511];reg [35:0] lo,ro;
 assign lq=lo;assign rq=ro;
 always @(posedge clk)begin
 if(dma_we)begin lm[dma_addr[8:0]]<=dl;rm[dma_addr[8:0]]<=dr;end
 if(work_we)rm[work_wa[8:0]]<=work_data;
 if(reset)begin lo<=0;ro<=0;end else begin if(le)lo<=lm[la[8:0]];if(re)ro<=rm[ra[8:0]];end
 end
`else
 genvar b;for(b=0;b<2;b=b+1)begin:bank
 wire [35:0] q;wire writing_work=(b==1)&&work_we;
 wire [35:0] wd=writing_work?work_data:(b==0?dl:dr);
 wire [8:0] wa=writing_work?work_wa[8:0]:dma_addr[8:0];
 SDPX9B #(.BIT_WIDTH_0(36),.BIT_WIDTH_1(36),.READ_MODE(0),.RESET_MODE("SYNC")) ram(
 .DO(q),.DI(wd),.BLKSELA(3'd0),.BLKSELB(3'd0),.ADA({wa,5'b01111}),.ADB({(b==0?la[8:0]:ra[8:0]),5'd0}),
 .CLKA(clk),.CLKB(clk),.CEA(dma_we||writing_work),.CEB(b==0?le:re),.OCE(1'b1),.RESETA(1'b0),.RESETB(reset));
 if(b==0)assign lq=q;else assign rq=q;
 end
`endif
end else begin:baseline
`ifdef PORTABLE
 reg [35:0] lm[0:1023],rm[0:1023];reg [35:0] lout,rout;
 assign lq=lout;assign rq=rout;
 always @(posedge clk) begin
  if(dma_we)begin lm[dma_addr]<=dl;rm[dma_addr]<=dr;end
  if(work_we)rm[ra]<=work_data;
  if(reset)begin lout<=0;rout<=0;end
  else begin if(le)lout<=lm[la];if(re)rout<=rm[ra];end
 end
`else
 genvar b;for(b=0;b<4;b=b+1)begin:bank
 wire [17:0] q;wire [35:0] d=b<2?dl:dr;
 DPX9B #(.BIT_WIDTH_0(18),.BIT_WIDTH_1(18),.READ_MODE0(0),.READ_MODE1(0),
 .WRITE_MODE0(2'b00),.WRITE_MODE1(2'b00),.RESET_MODE("SYNC")) ram(
 .DOA(),.DOB(q),.DIA(d[18*(b%2) +: 18]),.DIB(work_data[18*(b%2) +: 18]),
 .ADA({dma_addr,4'b1111}),.ADB({(b<2?la:ra),4'b1111}),
 .BLKSELA(3'd0),.BLKSELB(3'd0),.CLKA(clk),.CLKB(clk),
 .CEA(dma_we),.CEB(b<2?le:(re||work_we)),.OCEA(1'b1),.OCEB(1'b1),
 .WREA(1'b1),.WREB(b<2?1'b0:work_we),.RESETA(reset),.RESETB(reset));
 if(b<2)assign lq[18*b +: 18]=q;else assign rq[18*(b-2) +: 18]=q;
 end
`endif
end endgenerate
endmodule

// P/A独立写使能；同一读地址，消费者固定拼54，不做宽通道选择。
module shared_results #(parameter COMPACT=0)(input clk,reset,pwe,awe,input [8:0] wa,input [35:0] wd,
 input re,input [8:0] ra,output [35:0] pq,aq);
generate if(COMPACT)begin:compact
`ifdef PORTABLE
 reg [35:0] mem[0:511];reg [35:0] q;
 assign pq=q;assign aq=q;
 always @(posedge clk)begin
 if(pwe||awe)mem[wa]<=wd;
 if(reset)q<=0;else if(re)q<=mem[ra];
 end
`else
 wire [35:0] q;assign pq=q;assign aq=q;
 SDPX9B #(.BIT_WIDTH_0(36),.BIT_WIDTH_1(36),.READ_MODE(0),.RESET_MODE("SYNC")) ram(
 .DO(q),.DI(wd),.BLKSELA(3'd0),.BLKSELB(3'd0),.ADA({wa,5'b01111}),.ADB({ra,5'd0}),
 .CLKA(clk),.CLKB(clk),.CEA(pwe||awe),.CEB(re),.OCE(1'b1),.RESETA(1'b0),.RESETB(reset));
`endif
end else begin:baseline
`ifdef PORTABLE
 reg [35:0] pm[0:511],am[0:511];reg [35:0] po,ao;
 assign pq=po;assign aq=ao;
 always @(posedge clk)begin
  if(pwe)pm[wa]<=wd;if(awe)am[wa]<=wd;
  if(reset)begin po<=0;ao<=0;end else if(re)begin po<=pm[ra];ao<=am[ra];end
 end
`else
 genvar b;for(b=0;b<2;b=b+1)begin:bank
 wire [35:0] q;
 SDPX9B #(.BIT_WIDTH_0(36),.BIT_WIDTH_1(36),.READ_MODE(0),.RESET_MODE("SYNC")) ram(
 .DO(q),.DI(wd),.BLKSELA(3'd0),.BLKSELB(3'd0),
 .ADA({wa,5'b01111}),.ADB({ra,5'd0}),.CLKA(clk),.CLKB(clk),
 .CEA(b==0?pwe:awe),.CEB(re),.OCE(1'b1),.RESETA(1'b0),.RESETB(reset));
 if(b==0)assign pq=q;else assign aq=q;
 end
`endif
end endgenerate
endmodule
