`timescale 1ns/1ps
module tb;
reg clk=0; always #5 clk=~clk;
reg reset=0, cpu_request_valid=0, cpu_write=0, cpu_response_ready=0;
reg [21:0] cpu_address=0; reg [15:0] cpu_write_data=0;
reg display_request_valid=0, display_urgent=0; reg [21:0] display_address=0;
reg [31:0] controller_read_data=0; reg controller_read_valid=0;
reg controller_init_done=0, controller_command_ack=0;
wire cpu_request_ready,cpu_response_valid,cpu_error,display_request_ready;
wire display_data_valid,display_last,display_error,controller_command_valid,controller_precharge;
wire [15:0] cpu_read_data; wire [31:0] display_read_data;
wire [2:0] controller_command; wire [20:0] controller_address;
wire [3:0] controller_write_mask; wire [31:0] controller_write_data; wire [7:0] controller_burst_length;
DisplaySdramPort dut(.*);
integer i;
task ack; input [2:0] command; begin
  while (!(controller_command_valid && controller_command==command)) @(posedge clk);
  controller_command_ack<=1; @(posedge clk); controller_command_ack<=0;
end endtask
initial begin
  repeat(2) @(posedge clk); controller_init_done=1; @(posedge clk);
  display_address=22'h200100; display_request_valid=1; display_urgent=1;
  while(!display_request_ready) @(posedge clk); @(posedge clk); display_request_valid=0;
  ack(3'b011);
  while (!(controller_command_valid && controller_command==3'b101)) @(posedge clk);
  if(controller_burst_length!=7) $fatal(1,"display did not request 8 beats");
  controller_command_ack<=1;
  for(i=0;i<8;i=i+1) begin
    controller_read_data=32'h10000000+i; controller_read_valid=1; @(posedge clk); #1;
    if(!display_data_valid || display_read_data!==32'h10000000+i) $fatal(1,"lost display beat %0d",i);
    if(display_last !== (i==7)) $fatal(1,"bad last at %0d",i);
    controller_command_ack<=0;
  end
  controller_read_valid<=0;
  repeat(6) @(posedge clk);
  cpu_address=22'h000007; cpu_write_data=16'habcd; cpu_write=1; cpu_request_valid=1;
  while(!cpu_request_ready) @(posedge clk); @(posedge clk); cpu_request_valid=0;
  ack(3'b011); ack(3'b100);
  if(controller_write_mask!=4'b0011 || controller_write_data!=32'habcd0000) $fatal(1,"bad word lane");
  while(!cpu_response_valid) @(posedge clk); cpu_response_ready=1; @(posedge clk);
  if(display_error || cpu_error) $fatal(1,"unexpected error");
  $display("DIGITAL_DESIGN_PASS"); $finish;
end
endmodule
