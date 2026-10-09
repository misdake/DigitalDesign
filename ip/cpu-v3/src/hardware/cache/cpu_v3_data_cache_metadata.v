module CpuV3DataCacheMetadata(input clk, input [5:0] read_set,
 input write_0, input [5:0] write_set_0, input [15:0] write_data_0,
 input write_1, input [5:0] write_set_1, input [15:0] write_data_1,
 output [15:0] q0, output [15:0] q1);
`ifdef __ICARUS__
`ifndef CPU_V3_CACHE_METADATA_VENDOR
`define CPU_V3_CACHE_METADATA_PORTABLE
`endif
`endif
`ifndef CPU_V3_CACHE_METADATA_PORTABLE
 DPB #(.READ_MODE0(1'b0),.READ_MODE1(1'b0),.WRITE_MODE0(2'b00),.WRITE_MODE1(2'b00),
 .BIT_WIDTH_0(16),.BIT_WIDTH_1(16),.BLK_SEL_0(3'b0),.BLK_SEL_1(3'b0),.RESET_MODE("SYNC")) ram(
 .DOA(q0),.DOB(q1),.DIA(write_data_0),.DIB(write_data_1),
 .ADA({3'b0,1'b0,(write_0?write_set_0:read_set),2'b0,2'b11}),
 .ADB({3'b0,1'b1,(write_1?write_set_1:read_set),2'b0,2'b11}),
 .BLKSELA(3'b0),.BLKSELB(3'b0),.WREA(write_0),.WREB(write_1),
 .CLKA(clk),.CLKB(clk),.CEA(1'b1),.CEB(1'b1),.OCEA(1'b0),.OCEB(1'b0),.RESETA(1'b0),.RESETB(1'b0));
`else
 reg [15:0] ram0[0:63],ram1[0:63]; reg [15:0] a=0,b=0;
 assign q0=a; assign q1=b;
 integer i; initial for(i=0;i<64;i=i+1) begin ram0[i]=0; ram1[i]=0; end
 always @(posedge clk) begin
  if(write_0) ram0[write_set_0]<=write_data_0; else a<=ram0[read_set];
  if(write_1) ram1[write_set_1]<=write_data_1; else b<=ram1[read_set];
 end
`undef CPU_V3_CACHE_METADATA_PORTABLE
`endif
endmodule
