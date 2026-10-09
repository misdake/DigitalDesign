`timescale 1ns/1ps
module tb;
reg clk=0,pixel_clock=0,serial_clock=0,reset=1,video_locked=1;
always #9.259 clk=~clk; always #15.015 pixel_clock=~pixel_clock; always #1 serial_clock=~serial_clock;
reg memory_request_ready=1,memory_data_valid=0,memory_last=0,memory_error=0;
reg [63:0] memory_read_data=64'h1122334455667788;
reg [2:0] device_index=3;
reg [3:0] device_channel=3;
reg device_read_enable=1,device_write_enable=0;
reg [15:0] device_write_data=0;
wire memory_request_valid,memory_urgent,underflow,tmds_clk_p,tmds_clk_n;
wire [21:0] memory_address;
wire [2:0] tmds_data_p,tmds_data_n;
wire [15:0] device_read_data;
FramebufferHdmiFault dut(.*);
integer scenario,beat=0,requests=0;
reg auto_response=0;
always @(posedge clk) if(auto_response) begin
 memory_data_valid<=0; memory_last<=0;
 if(memory_request_valid && memory_request_ready) begin beat<=1; requests<=requests+1; end
 else if(beat!=0) begin
  memory_data_valid<=1; memory_last<=beat==4;
  if(beat==4) beat<=0; else beat<=beat+1;
 end
end
task reset_dut;
 begin
  auto_response=0;
  @(negedge clk); reset=1; memory_request_ready=0;
  memory_data_valid=0; memory_last=0; memory_error=0; beat=0; requests=0;
  repeat(12) @(negedge clk);
  reset=0; memory_request_ready=1;
 end
endtask
initial begin repeat(3*1056*525) @(posedge pixel_clock); $fatal(1,"fault watchdog scenario=%0d requests=%0d beat=%0d published=%b active=%b error=%b",scenario,requests,beat,dut.published,dut.burst_active,dut.memory_error_sticky); end
initial begin
 // Error at each beat and malformed LAST at each early position/final beat.
 for(scenario=0;scenario<8;scenario=scenario+1) begin
  reset_dut;
  wait(dut.burst_active);
  for(beat=0;beat<4;beat=beat+1) begin
   @(negedge clk); memory_data_valid=1;
   memory_last=beat==3; memory_error=0;
   if(beat==scenario%4) begin
    if(scenario<4) memory_error=1;
    else memory_last=!(beat==3);
   end
  end
  @(negedge clk); memory_data_valid=0; memory_error=0; memory_last=0;
  repeat(10) @(negedge clk);
  if(!device_read_data[4] || !underflow || memory_request_valid || dut.published!=0)
   $fatal(1,"failed segment was not stopped atomically, scenario %0d",scenario);
 end
 // A terminal error without DATA_VALID also stops future filling.
 reset_dut; wait(dut.burst_active);
 @(negedge clk); memory_error=1;
 @(negedge clk); memory_error=0;
 repeat(10) @(negedge clk);
 if(!device_read_data[4] || memory_request_valid || dut.published!=0) $fatal(1,"out-of-band error lost");
 // Fill exactly four rows, then hold SDRAM indefinitely. The consumer must
 // show black and set sticky underflow after exhausting those rows. No read
 // may release an unpublished group; black is observed across the failed row.
 reset_dut; auto_response=1;
 wait(dut.published==2'b11);
 @(negedge clk); auto_response=0; memory_request_ready=0; memory_data_valid=0; memory_last=0;
 wait(dut.underflow_sticky);
 repeat(100) @(posedge pixel_clock);
 if(dut.released!=2'b11 || !device_read_data[3]) $fatal(1,"underflow released an unpublished group");
 if(dut.framebuffer_pipe3 && dut.rgb_pipe!=0) $fatal(1,"unpublished row was visible");
 reset_dut;
 repeat(20) @(posedge pixel_clock);
 if(underflow || device_read_data[4:3]!=0) $fatal(1,"reset did not clear sticky faults");
 $display("DIGITAL_DESIGN_PASS"); $finish;
end
endmodule
