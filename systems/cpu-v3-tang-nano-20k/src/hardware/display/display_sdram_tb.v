`timescale 1ns/1ps
module tb;
reg clk=0; always #5 clk=~clk;
reg reset=0, cpu_request_valid=0, cpu_write=0, cpu_line=0, cpu_response_ready=0;
reg [1:0] cpu_line_count_minus_1=0;
reg [21:0] cpu_address=0; reg [63:0] cpu_write_data=0;
reg [63:0] controller_read_data=0; reg controller_read_valid=0;
reg controller_init_done=0, controller_command_ack=0, controller_write_data_ready=1;
wire cpu_request_ready,cpu_write_data_ready,cpu_response_valid,cpu_response_last,cpu_error;
wire controller_command_valid,controller_precharge,controller_write_data_valid;
wire [63:0] cpu_read_data;
wire [2:0] controller_command; wire [20:0] controller_address;
wire [3:0] controller_write_mask; wire [63:0] controller_write_data; wire [7:0] controller_burst_length;
SharedSdramPort dut(.*);
localparam CMD_REFRESH=3'b001, CMD_ACTIVE=3'b011, CMD_WRITE=3'b100, CMD_READ=3'b101;
localparam ST_REFRESH_WAIT=9, ST_WRITE_STAGE=13;
integer cycles=0;
always @(posedge clk) begin
  cycles<=cycles+1;
  if(cycles>200000) $fatal(1,"testbench cycle limit exceeded: state=%0d refresh=%0d",
    dut.state,dut.refresh_count);
end
integer i;

// ---- unstallable line-write beat driver ----
reg [4:0] drive_beat=0, drive_total=0;
reg driving=0;
always @(posedge clk) begin
  if (cpu_request_valid && cpu_request_ready && cpu_line && cpu_write) begin
    driving <= 1;
    drive_beat <= 5'd1;
    drive_total <= {cpu_line_count_minus_1, 2'b00} + 5'd4;
  end else if (driving && (drive_total == 5'd4 || cpu_write_data_ready)) begin
    if (drive_beat == drive_total - 5'd1) driving <= 0;
    else drive_beat <= drive_beat + 5'd1;
  end
end
function [63:0] beat_value; input [4:0] n; begin
  beat_value = {32'hA0000000 + n, 32'hB0000000 + n};
end endfunction
// Single driver for the CPU write lane: either the word-write value or the
// streaming line-write beat counter.
reg word_write_active=0;
reg [63:0] word_write_value=0;
// Before a line write is accepted the counter is parked at beat zero; stale
// values from a previous transaction must never reach the accept edge.
always @(*) cpu_write_data = word_write_active ? word_write_value :
    beat_value(driving ? drive_beat : 5'd0);

// ---- capture and check the controller write stream ----
reg clear_capture=0;
reg check_capture=0;
reg [4:0] captured_count=0;
always @(posedge clk) begin
  if (clear_capture) captured_count <= 0;
  else if (controller_write_data_valid && controller_write_data_ready) begin
    if (check_capture && controller_write_data !== beat_value(captured_count))
      $fatal(1,"bad controller write beat %0d: %h",captured_count,controller_write_data);
    captured_count <= captured_count + 1;
  end
end

task ack; input [2:0] command; begin
  while (!(controller_command_valid && controller_command==command)) @(negedge clk);
  controller_command_ack=1; @(posedge clk); #1; controller_command_ack=0;
end endtask

task start_write; input [21:0] address; input [1:0] lc; begin
  @(negedge clk);
  clear_capture=1; @(posedge clk); #1; @(negedge clk); clear_capture=0; check_capture=1;
  cpu_address=address; cpu_line_count_minus_1=lc; cpu_write=1; cpu_line=1; cpu_request_valid=1;
  while(!cpu_request_ready) @(negedge clk);
  @(posedge clk); @(negedge clk); cpu_request_valid=0;
end endtask

task finish_write; input [1:0] lc; begin
  ack(CMD_ACTIVE);
  while (!(controller_command_valid && controller_command==CMD_WRITE)) @(negedge clk);
  if(controller_burst_length!==((lc+1)*8-1)) $fatal(1,"line write burst length %0d",controller_burst_length);
  if(controller_write_mask!=0) $fatal(1,"line write must enable all byte lanes");
  while(captured_count!==((lc+1)*4)) @(negedge clk);
  controller_command_ack=1; @(posedge clk); #1; controller_command_ack=0;
  if(!cpu_response_valid || !cpu_response_last) $fatal(1,"line write completion missing");
  @(negedge clk); cpu_response_ready=1; @(posedge clk); #1; cpu_response_ready=0;
  check_capture=0; cpu_write=0; cpu_line=0;
end endtask

task line_write; input [21:0] address; input [1:0] lc; begin
  start_write(address, lc);
  finish_write(lc);
  repeat(6) @(posedge clk);
end endtask

integer k;
integer beats;
task line_read; input [21:0] address; input [1:0] lc; begin
  beats = (lc+1)*4;
  @(negedge clk);
  cpu_address=address; cpu_line_count_minus_1=lc; cpu_line=1; cpu_write=0; cpu_request_valid=1;
  while(!cpu_request_ready) @(negedge clk);
  @(posedge clk); @(negedge clk); cpu_request_valid=0; cpu_line=0;
  ack(CMD_ACTIVE);
  while (!(controller_command_valid && controller_command==CMD_READ)) @(negedge clk);
  if(controller_burst_length!==((lc+1)*8-1)) $fatal(1,"line read burst length %0d",controller_burst_length);
  if(controller_address!==(address[21:1])) $fatal(1,"line read base is not the burst base");
  while(dut.state!=5) @(negedge clk);
  for(k=0;k<beats;k=k+1) begin
    #1; controller_read_data = {32'hC0000000+k, 32'hD0000000+k};
    controller_read_valid=1; @(posedge clk); #1;
    if(!cpu_response_valid || cpu_read_data!==controller_read_data)
      $fatal(1,"lost line read beat %0d",k);
    if(cpu_response_last !== (k==beats-1)) $fatal(1,"bad line read last at %0d",k);
    @(negedge clk);
  end
  controller_read_valid=0;
  @(posedge clk); #1;
  if(cpu_response_valid) $fatal(1,"line read response did not end");
  repeat(6) @(posedge clk);
end endtask

initial begin
  repeat(2) @(posedge clk); controller_init_done=1; @(posedge clk);

  // Dedicated word write from idle: the half-word must be staged through
  // ST_WRITE_STAGE (controller_write_data_valid) so the 108/54 gearbox can
  // capture it into write_buffer before ACTIVE/WRITE. A word write is a
  // fixed one-word request and must stay length-independent.
  cpu_address=22'h00000f; word_write_value=64'h1234; word_write_active=1; cpu_write=1; cpu_request_valid=1;
  while(!cpu_request_ready) @(posedge clk);
  @(posedge clk); cpu_request_valid=0; cpu_write=0;
  if(dut.state!=ST_WRITE_STAGE || !controller_write_data_valid) $fatal(1,"word write did not enter ST_WRITE_STAGE");
  if(controller_write_data!=32'h12340000) $fatal(1,"word write staged wrong data");
  ack(CMD_ACTIVE);
  ack(CMD_WRITE);
  if(controller_write_mask!=4'b0011 || controller_write_data!=32'h12340000) $fatal(1,"bad word lane");
  if(!cpu_response_valid || !cpu_response_last) $fatal(1,"word write completion must carry last");
  @(negedge clk); cpu_response_ready=1; @(posedge clk); #1; cpu_response_ready=0;
  word_write_active=0;
  repeat(6) @(posedge clk);

  // Every supported request length, read then write.
  for(i=0;i<4;i=i+1) begin
    line_read(22'h000200 + i*22'h000020, i[1:0]);
    line_write(22'h000300 + i*22'h000020, i[1:0]);
  end

  // A request offered exactly when refresh becomes due is accepted and
  // completed first; the overdue refresh follows the bounded transaction.
  @(negedge clk);
  // `line_read` begins at the next negedge; the intervening posedge advances
  // 599 to the due value while leaving the port in ST_IDLE.
  dut.refresh_count=10'd599;
  line_read(22'h000480, 0);
  if(dut.state!=ST_REFRESH_WAIT) $fatal(1,"overdue refresh did not follow accepted request");
  controller_command_ack=1; @(posedge clk); #1; controller_command_ack=0;
  repeat(2) @(posedge clk);

  // A long write accepted on the same refresh boundary must expose beat zero
  // to the gearbox on its request edge. Missing that beat shifts the circular
  // buffer and corrupts this transaction plus the following one.
  @(negedge clk);
  // `start_write` spends one extra cycle clearing its capture scoreboard, so
  // start at 598: the request edge itself then observes refresh_due.
  dut.refresh_count=10'd598;
  line_write(22'h000500, 3);
  if(captured_count!==16) $fatal(1,"refresh-boundary long write lost a beat");
  if(dut.state!=ST_REFRESH_WAIT) $fatal(1,"overdue refresh did not follow long write");
  controller_command_ack=1; @(posedge clk); #1; controller_command_ack=0;
  repeat(2) @(posedge clk);

  if(cpu_error) $fatal(1,"unexpected error");
  $display("DIGITAL_DESIGN_PASS"); $finish;
end
endmodule
