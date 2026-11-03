`timescale 1ns/1ps
// Only the producer Timing boundary is controlled. Source, address expressions,
// capture body and holding registers below are projected from production RTL.
module PixelLeaf(
 input wire pixel_clock,pixel_reset,scan_enabled,scan_line_ready,scan_valid,scan_linear,
 input wire display_slot,display_second,
 input wire [10:0] h_count,
 input wire [9:0] v_count,
 input wire [31:0] line_read_data_a,line_read_data_b,
 output reg [8:0] line_read_address_a,line_read_address_b
);
__CONFIG__
localparam integer PRECODE_BITS=9;
wire [8:0] display_slot_base=(display_slot?9'd200:9'd0)+(display_second?9'd100:9'd0);
__PIXEL_DECLARATIONS__
__DECLARATIONS__
always @(posedge pixel_clock) begin
 if (pixel_reset) begin
__RESET__
 end else begin
__PIXEL_BODY__
  if (h_count==0) begin
__H0__
__BODY__
 end
end
endmodule

module tb;
reg write_clock=0,pixel_clock=0,write_enable=0,pixel_reset=0;
reg scan_enabled=0,scan_line_ready=0,scan_valid=0,scan_linear=0,display_slot=0,display_second=0;
reg [10:0] h_count=0;
reg [9:0] v_count=0;
reg [8:0] write_address=0;
reg [63:0] write_data=0;
wire [8:0] line_read_address_a,line_read_address_b;
wire [31:0] line_read_data_a,line_read_data_b;
PixelLeaf dut(.*);
DisplayLineBuffer ram(.write_clock(write_clock),.write_enable(write_enable),.write_address(write_address),.write_data(write_data),
 .read_clock(pixel_clock),.read_address_a(line_read_address_a),.read_address_b(line_read_address_b),
 .read_data_a(line_read_data_a),.read_data_b(line_read_data_b));
reg [2047:0] vectors;
integer fd,result,index,le,pe,we,rst,en,ready,valid,lin,slot,second,count=0;
initial begin
 if(!$value$plusargs("vectors=%s",vectors))$fatal(1,"missing vectors");
 fd=$fopen(vectors,"r");if(!fd)$fatal(1,"cannot open vectors");
 while(!$feof(fd))begin
  result=$fscanf(fd,"%d %d %d %d %h %h %d %d %d %d %d %d %d %d %d\n",
   index,le,pe,we,write_address,write_data,rst,h_count,v_count,en,ready,valid,lin,slot,second);
  if(result!=15)$fatal(1,"malformed row %0d fields=%0d",count,result);
  write_enable=we;pixel_reset=rst;scan_enabled=en;scan_line_ready=ready;scan_valid=valid;scan_linear=lin;display_slot=slot;display_second=second;
  #1;
  $write("R %0d %h %h %h %h %h %h %h ",index,line_read_address_a,line_read_address_b,line_read_data_a,line_read_data_b,
   dut.current_precode[26:18],dut.current_precode[17:9],dut.current_precode[8:0]);
  $write("%h %h %h %h %h %h %h ",dut.precode_pipe[26:18],dut.precode_pipe[17:9],dut.precode_pipe[8:0],
   dut.visible_pipe3,dut.framebuffer_pipe3,dut.hsync_pipe3,dut.vsync_pipe3);
  write_clock=le;pixel_clock=pe;
  #1;
  $write("%h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h ",dut.read_phase,dut.fetch_pair,dut.pair_bank,dut.raw_pair,
   dut.r0,dut.r1,dut.g0,dut.g1,
   dut.output_pair[26:18],dut.output_pair[17:9],dut.output_pair[8:0],dut.output_pair[53:45],dut.output_pair[44:36],dut.output_pair[35:27],
   line_read_data_a,line_read_data_b);
  $display("%h %h %h %h %h %h %h %h %h %h",dut.precode_pipe[26:18],dut.precode_pipe[17:9],dut.precode_pipe[8:0],
   dut.rgb_pipe[23:16],dut.rgb_pipe[15:8],dut.rgb_pipe[7:0],dut.visible_pipe3,dut.framebuffer_pipe3,dut.hsync_pipe3,dut.vsync_pipe3);
  write_clock=0;pixel_clock=0;#1;
  count=count+1;if(count>32768)$fatal(1,"read edge cap");
 end
 $fclose(fd);$display("PASS READ %0d",count);$finish;
end
initial begin #200000;$fatal(1,"read time cap");end
endmodule
