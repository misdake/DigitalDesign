`timescale 1ns/1ps
module tb;
reg clk=0,pixel_clock=0,serial_clock=0,video_locked=1; always #9 clk=~clk; always #7 pixel_clock=~pixel_clock; always #1 serial_clock=~serial_clock;
reg [1:0] buttons=0; reg [31:0] sdram_read_data=0; reg sdram_read_valid=0,sdram_init_done=0,sdram_command_ack=0;
wire [5:0] leds; wire uart_tx,sdram_command_valid,sdram_precharge,tmds_clk_p,tmds_clk_n;
wire [2:0] sdram_command,tmds_data_p,tmds_data_n; wire [20:0] sdram_address;
wire [3:0] sdram_write_mask; wire [31:0] sdram_write_data; wire [7:0] sdram_burst_length;
CpuV3Display dut(.*);
integer phase=0,reads=0,writes=0,cycles;
always @(posedge clk) begin
 sdram_command_ack<=0; sdram_read_valid<=0;
 if(sdram_command_valid&&sdram_command==3'b011) sdram_command_ack<=1;
 if(sdram_command_valid&&sdram_command==3'b100) begin sdram_command_ack<=1; writes<=writes+1; end
 if(sdram_command_valid&&sdram_command==3'b101) begin phase<=1; reads<=reads+1; end
 else if(phase!=0) begin
  sdram_read_valid<=1; sdram_read_data<={16'hf800,16'h07e0};
  if(phase==7) sdram_command_ack<=1;
  if(phase==8) phase<=0; else phase<=phase+1;
 end
 if(sdram_command_valid&&sdram_command==3'b001) sdram_command_ack<=1;
end
initial begin
 repeat(5) @(posedge clk); sdram_init_done=1;
 for(cycles=0;cycles<100000;cycles=cycles+1) @(posedge clk);
 if(reads==0 || writes==0) $fatal(1,"missing concurrent traffic reads=%0d writes=%0d",reads,writes);
 if(leds[5]) $fatal(1,"CPU faulted");
 $display("DIGITAL_DESIGN_PASS"); $finish;
end
endmodule
