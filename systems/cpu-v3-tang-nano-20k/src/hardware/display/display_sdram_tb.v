`timescale 1ns/1ps
module tb;
reg clk=0; always #5 clk=~clk;
reg reset=0, cpu_request_valid=0, cpu_write=0, cpu_line=0, cpu_response_ready=0;
reg [21:0] cpu_address=0; reg [63:0] cpu_write_data=0;
reg [63:0] controller_read_data=0; reg controller_read_valid=0;
reg controller_init_done=0, controller_command_ack=0, controller_write_data_ready=1;
wire cpu_request_ready,cpu_response_valid,cpu_response_last,cpu_error;
wire controller_command_valid,controller_precharge,controller_write_data_valid;
wire [63:0] cpu_read_data;
wire [2:0] controller_command; wire [20:0] controller_address;
wire [3:0] controller_write_mask; wire [63:0] controller_write_data; wire [7:0] controller_burst_length;
SharedSdramPort dut(.*);
localparam CMD_REFRESH=3'b001, CMD_ACTIVE=3'b011, CMD_WRITE=3'b100, CMD_READ=3'b101;
localparam ST_IDLE=1, ST_ACTIVE_WAIT=3, ST_OP_WAIT=5, ST_WRITE_STAGE=13;
integer cycles=0;
always @(posedge clk) begin
  cycles<=cycles+1;
  if(cycles>20000) $fatal(1,"testbench cycle limit exceeded");
end
integer i;
task ack; input [2:0] command; begin
  while (!(controller_command_valid && controller_command==command)) @(negedge clk);
  controller_command_ack=1; @(posedge clk); #1; controller_command_ack=0;
end endtask
initial begin
  repeat(2) @(posedge clk); controller_init_done=1; @(posedge clk);

  // Dedicated word write from idle: the half-word must be staged through
  // ST_WRITE_STAGE (controller_write_data_valid) so the 108/54 gearbox can
  // capture it into write_buffer before ACTIVE/WRITE.
  cpu_address=22'h00000f; cpu_write_data=16'h1234; cpu_write=1; cpu_request_valid=1;
  while(!cpu_request_ready) @(posedge clk);
  @(posedge clk); cpu_request_valid=0; cpu_write=0;
  if(dut.state!=ST_WRITE_STAGE || !controller_write_data_valid) $fatal(1,"word write did not enter ST_WRITE_STAGE");
  if(controller_write_data!=32'h12340000) $fatal(1,"word write staged wrong data");
  ack(CMD_ACTIVE);
  ack(CMD_WRITE);
  if(controller_write_mask!=4'b0011 || controller_write_data!=32'h12340000) $fatal(1,"bad word lane");
  if(!cpu_response_valid || !cpu_response_last) $fatal(1,"word write completion must carry last");
  @(negedge clk); cpu_response_ready=1; @(posedge clk); #1; cpu_response_ready=0;

  // CPU line read: one burst command, four ordered 64-bit beats.
  repeat(6) @(posedge clk);
  @(negedge clk);
  cpu_address=22'h000200; cpu_line=1; cpu_request_valid=1;
  while(!cpu_request_ready) @(negedge clk);
  @(posedge clk); @(negedge clk); cpu_request_valid=0; cpu_line=0;
  ack(CMD_ACTIVE);
  while (!(controller_command_valid && controller_command==CMD_READ)) @(negedge clk);
  if(controller_burst_length!=7) $fatal(1,"cpu line did not request 8 beats");
  if(controller_address!==21'h000100) $fatal(1,"cpu line base is not the burst base");
  controller_command_ack<=1; @(posedge clk); #1; controller_command_ack<=0;
  while(dut.state!=ST_OP_WAIT) @(negedge clk);
  for(i=0;i<4;i=i+1) begin
    controller_read_data[31:0]=32'h20000000+2*i;
    controller_read_data[63:32]=32'h20000001+2*i;
    controller_read_valid=1; @(posedge clk); #1;
    if(!cpu_response_valid || cpu_read_data[31:0]!==32'h20000000+2*i ||
       cpu_read_data[63:32]!==32'h20000001+2*i) $fatal(1,"lost cpu beat %0d",i);
    if(cpu_response_last !== (i==3)) $fatal(1,"bad cpu last at %0d",i);
    @(negedge clk);
  end
  controller_read_valid<=0;
  @(posedge clk); #1;
  if(cpu_response_valid) $fatal(1,"cpu line response did not end after the fourth beat");

  // CPU line write: capture four incoming 64-bit beats first, then present beat
  // zero with the command acknowledgement and advance once per controller clock.
  repeat(6) @(posedge clk);
  @(negedge clk);
  cpu_address=22'h000300; cpu_write=1; cpu_line=1;
  cpu_write_data=64'h3000000130000000; cpu_request_valid=1;
  #1;
  if(!cpu_request_ready) $fatal(1,"line write request was not accepted from idle");
  @(posedge clk);
  for(i=1;i<4;i=i+1) begin
    @(negedge clk);
    cpu_request_valid=0;
    cpu_write_data[31:0]=32'h30000000+2*i;
    cpu_write_data[63:32]=32'h30000001+2*i;
    @(posedge clk);
  end
  @(negedge clk);
  cpu_write=0; cpu_line=0;
  for(i=0;i<4;i=i+1) begin
    while(!controller_write_data_valid) @(posedge clk);
    #1;
    if(controller_write_data[31:0]!==32'h30000000+2*i ||
       controller_write_data[63:32]!==32'h30000001+2*i)
      $fatal(1,"bad staged line write beat %0d: %h",i,controller_write_data);
    @(posedge clk);
  end
  ack(CMD_ACTIVE);
  while (!(controller_command_valid && controller_command==CMD_WRITE)) @(negedge clk);
  if(controller_burst_length!=7) $fatal(1,"cpu line write did not request 8 beats");
  if(controller_write_mask!=0) $fatal(1,"cpu line write must enable all byte lanes");
  controller_command_ack=1;
  @(posedge clk); #1; controller_command_ack=0;
  if(!cpu_response_valid || !cpu_response_last)
    $fatal(1,"cpu line write completion missing");
  @(negedge clk); cpu_response_ready=1; @(posedge clk); #1; cpu_response_ready=0;
  if(cpu_error) $fatal(1,"unexpected error");
  $display("DIGITAL_DESIGN_PASS"); $finish;
end
endmodule
