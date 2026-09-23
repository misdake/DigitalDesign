`timescale 1ns/1ps
module tb;
// The fitted PLL aligns every other controller rising edge with a logic rising
// edge.  Start controller high so this testbench preserves that relationship.
reg logic_clk=0, controller_clk=1, reset=1;
always #10 logic_clk=~logic_clk;
always #5 controller_clk=~controller_clk;
reg capture_valid=0, write_start=0;
reg [63:0] capture_data=0;
reg [4:0] burst_length=0;
wire [31:0] controller_data;
TangNano20KSdramWriteGearbox108M54M dut(.*);

integer cycles=0;
always @(posedge controller_clk) begin
  cycles=cycles+1;
  if(cycles>2000) $fatal(1,"gearbox timeout");
end

function [63:0] value; input integer transaction; input integer beat; begin
  value={8'hc0,transaction[7:0],8'h80,beat[7:0],8'h40,transaction[7:0],8'h00,beat[7:0]};
end endfunction
function [31:0] expected_word; input integer transaction; input integer physical; integer beat; begin
  beat=physical/2;
  expected_word=physical[0] ?
    {8'hc0,transaction[7:0],8'h80,beat[7:0]} :
    {8'h40,transaction[7:0],8'h00,beat[7:0]};
end endfunction

integer tx, count, total, physical;
integer source_transaction=0, source_beat=0, source_total=0;
reg source_active=0;
always @(posedge logic_clk) begin
  if(source_active) begin
    if(source_beat==source_total-1) begin
      source_active<=0;
      capture_valid<=0;
    end
    else begin
      source_beat<=source_beat+1;
      capture_data<=value(source_transaction,source_beat+1);
    end
  end
end

task run_burst; input integer transaction; input [4:0] length; begin
  total=(length+1)/2;
  source_transaction=transaction; source_beat=0; source_total=total;
  source_active=1; capture_data=value(transaction,0); capture_valid=1;
  burst_length=length;
  // Fill the one holding register; the remainder stays at the source.
  @(posedge logic_clk); #1;
  // Start so M0 lands on the logic-clock falling edge. M1 then lands on the
  // rising edge that refills the register for M2.
  @(negedge controller_clk);
  while(logic_clk!==1'b1) @(negedge controller_clk);
  write_start=1; #1;
  if(controller_data!==expected_word(transaction,0))
    $fatal(1,"tx %0d physical 0 got %h",transaction,controller_data);
  @(posedge controller_clk); #1; write_start=0;
  for(physical=1;physical<=length;physical=physical+1) begin
    @(negedge controller_clk); #1;
    if(controller_data!==expected_word(transaction,physical))
      $fatal(1,"tx %0d physical %0d got %h",transaction,physical,controller_data);
    @(posedge controller_clk);
  end
  @(posedge logic_clk); #1;
  if(source_active) $fatal(1,"tx %0d source did not retire",transaction);
  capture_valid=0;
  repeat(2) @(posedge controller_clk);
end endtask

initial begin
  repeat(3) @(posedge controller_clk); reset=0;
  // Consecutive bursts exercise restart as well as every supported length.
  run_burst(0,5'd7);
  run_burst(1,5'd7);
  run_burst(2,5'd15);
  run_burst(3,5'd23);
  run_burst(4,5'd31);
  $display("DIGITAL_DESIGN_PASS"); $finish;
end
endmodule
