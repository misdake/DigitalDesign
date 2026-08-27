`timescale 1ns/1ps
module tb;
reg clk=0,pixel_clock=0,serial_clock=0,reset=1,video_locked=0;
always #9 clk=~clk; always #7 pixel_clock=~pixel_clock; always #1 serial_clock=~serial_clock;
reg memory_request_ready=1,memory_data_valid=0,memory_last=0,memory_error=0;
reg [31:0] memory_read_data=0;
wire memory_request_valid,memory_urgent,underflow,tmds_clk_p,tmds_clk_n;
wire [21:0] memory_address; wire [2:0] tmds_data_p,tmds_data_n;
FramebufferHdmi dut(.*);
integer beat=0,requests=0,cycles=0,bursts=0;
integer col=0,bad=0,sampled=0;
reg [15:0] wa;
reg [15:0] base=0;
reg vis_d=0;
wire vis = dut.visible_pipe3;
always @(posedge clk) begin
 memory_data_valid<=0; memory_last<=0;
 // Ignore requests while the DUT is in reset: its fill pointers do not
 // advance there, so accepting would desynchronize the burst count.
 if (!reset && memory_request_valid&&memory_request_ready) begin beat<=1; requests<=requests+1; bursts<=bursts+1; end
 else if(beat!=0) begin
  // Position-dependent fill: each 32-bit beat carries its own two 16-bit
  // word offsets within the row (low half = even pixel, high half = odd),
  // so a displayed pixel value must equal its source x coordinate.
  wa = (((bursts-1) % 20) * 16) + (beat-1)*2;
  memory_data_valid<=1; memory_read_data<={wa+16'd1, wa}; memory_last<=beat==8;
  if(beat==8) beat<=0; else beat<=beat+1;
 end
end
initial begin
 repeat(4) @(posedge clk); reset=0; video_locked=1;
 for(cycles=0;cycles<2600000;cycles=cycles+1) @(posedge pixel_clock);
 if(requests<4800) $fatal(1,"insufficient line fetches %0d",requests);
 if(underflow) $fatal(1,"unexpected underflow");
 if(tmds_clk_n!==~tmds_clk_p || tmds_data_n!==~tmds_data_p) $fatal(1,"bad differential outputs");
 if(bad>0) $fatal(1,"pixel mismatches: %0d of %0d sampled",bad,sampled);
 if(sampled<100000) $fatal(1,"too few visible pixels sampled: %0d",sampled);
 $display("DIGITAL_DESIGN_PASS"); $finish;
end
// Pixel-accuracy monitor: every visible pixel carries its source x
// coordinate as its value, so within one visible run column c must show
// exactly base + c/3, where base is the run's first value. The run can start
// mid-row because a slot may be published after the line began (the demo is
// deliberately unsynchronized), so only the relative alignment is checked.
always @(posedge pixel_clock) begin
 vis_d <= vis;
 if (vis && !vis_d) begin col=0; base=dut.pixel565_pipe; end
 if (vis) begin
  sampled=sampled+1;
  if (dut.pixel565_pipe !== base + col/3) begin
   if (bad<10) $display("pixel mismatch at column %0d: got %h want %h", col, dut.pixel565_pipe, base + col/3);
   bad=bad+1;
  end
  col=col+1;
 end
end
endmodule
