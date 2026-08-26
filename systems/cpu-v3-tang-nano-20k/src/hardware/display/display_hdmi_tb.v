`timescale 1ns/1ps
module tb;
reg clk=0,pixel_clock=0,serial_clock=0,reset=1,video_locked=0;
always #9 clk=~clk; always #7 pixel_clock=~pixel_clock; always #1 serial_clock=~serial_clock;
reg memory_request_ready=1,memory_data_valid=0,memory_last=0,memory_error=0;
reg [31:0] memory_read_data=0;
wire memory_request_valid,memory_urgent,underflow,tmds_clk_p,tmds_clk_n;
wire [21:0] memory_address; wire [2:0] tmds_data_p,tmds_data_n;
FramebufferHdmi dut(.*);
integer beat=0,requests=0,cycles=0;
always @(posedge clk) begin
 memory_data_valid<=0; memory_last<=0;
 if(memory_request_valid&&memory_request_ready) begin beat<=1; requests<=requests+1; end
 else if(beat!=0) begin
  memory_data_valid<=1; memory_read_data<={16'hf800,16'h07e0}; memory_last<=beat==8;
  if(beat==8) beat<=0; else beat<=beat+1;
 end
end
initial begin
 repeat(4) @(posedge clk); reset=0; video_locked=1;
 for(cycles=0;cycles<2600000;cycles=cycles+1) @(posedge pixel_clock);
 if(requests<4800) $fatal(1,"insufficient line fetches %0d",requests);
 if(underflow) $fatal(1,"unexpected underflow");
 if(tmds_clk_n!==~tmds_clk_p || tmds_data_n!==~tmds_data_p) $fatal(1,"bad differential outputs");
 $display("DIGITAL_DESIGN_PASS"); $finish;
end
endmodule
