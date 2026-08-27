`timescale 1ns/1ps
module tb;
reg clk=0,pixel_clock=0,serial_clock=0,reset=1,video_locked=0;
always #9 clk=~clk; always #7 pixel_clock=~pixel_clock; always #1 serial_clock=~serial_clock;
reg memory_request_ready=1,memory_data_valid=0,memory_last=0,memory_error=0;
reg [31:0] memory_read_data=0;
reg [2:0] device_index=3; reg [3:0] device_channel=0;
reg device_read_enable=0,device_write_enable=0; reg [15:0] device_write_data=0;
wire memory_request_valid,memory_urgent,underflow,tmds_clk_p,tmds_clk_n;
wire [21:0] memory_address; wire [2:0] tmds_data_p,tmds_data_n;
wire [15:0] device_read_data;
FramebufferHdmi dut(.*);
integer beat=0,requests=0,cycles=0,bursts=0;
integer col=0,bad=0,sampled=0;
integer old_frame=0;
reg [15:0] wa;
reg [15:0] base=0;
reg vis_d=0;
reg saw_second_base=0;
wire vis = dut.visible_pipe3;

task device_write;
 input [3:0] channel; input [15:0] value;
 begin
  @(negedge clk); device_channel=channel; device_write_data=value; device_write_enable=1;
  @(negedge clk); device_write_enable=0;
 end
endtask

task expect_status;
 input [15:0] mask; input [15:0] expected;
 begin
  @(negedge clk); device_channel=3; device_read_enable=1;
  #1 if ((device_read_data & mask) !== expected) $fatal(1,"status %h mask %h expected %h",device_read_data,mask,expected);
  device_read_enable=0;
 end
endtask
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
 if (!reset && memory_request_valid && memory_request_ready) begin
  if (memory_address==22'h212d00) saw_second_base<=1;
  if (memory_address<dut.active_base || memory_address>=dut.active_base+22'd76800)
   $fatal(1,"request %h outside active framebuffer %h",memory_address,dut.active_base);
 end
end
initial begin
 repeat(4) @(posedge clk); reset=0; video_locked=1;
 // Low and high halves are transferred without dropping the aligned low bits.
 device_write(1,16'h2d00);
 expect_status(16'h0003,16'h0002);
 device_write(2,16'h0021);
 expect_status(16'h0003,16'h0001);
 // A complete pending address is immutable until the frame boundary.
 device_write(1,16'h0100);
 wait(dut.active_base==22'h212d00);
 if(dut.next_base!==32'h00212d00) $fatal(1,"pending base was overwritten: %h",dut.next_base);
 expect_status(16'h0003,16'h0000);
 // An out-of-range complete pair is rejected and cannot replace the active base.
 device_write(1,16'h0100);
 device_write(2,16'h0040);
 expect_status(16'h0004,16'h0004);
 old_frame=dut.frame_index;
 wait(dut.frame_index!=old_frame);
 repeat(1000) @(posedge pixel_clock);
 if(dut.active_base!==22'h212d00) $fatal(1,"invalid framebuffer base became active");
 if(requests<4800) $fatal(1,"insufficient line fetches %0d",requests);
 if(!saw_second_base) $fatal(1,"second framebuffer was never fetched");
 if(dut.frame_index<2) $fatal(1,"frame index did not advance twice: %0d",dut.frame_index);
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
