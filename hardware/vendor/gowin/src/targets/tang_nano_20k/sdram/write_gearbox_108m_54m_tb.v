`timescale 1ns/1ps
module tb;
reg logic_clk=0, controller_clk=0, reset=1;
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
task capture_one; input integer transaction; input integer beat; begin
  @(negedge logic_clk); capture_data=value(transaction,beat); capture_valid=1;
  @(posedge logic_clk); #1; capture_valid=0;
end endtask

task run_burst; input integer transaction; input [4:0] length; begin
  total=(length+1)/2;
  for(count=0;count<4;count=count+1) capture_one(transaction,count);
  burst_length=length;
  @(negedge controller_clk); write_start=1; #1;
  if(controller_data!==expected_word(transaction,0))
    $fatal(1,"tx %0d physical 0 got %h",transaction,controller_data);
  @(posedge controller_clk); #1; write_start=0;
  fork
    begin
      for(count=4;count<total;count=count+1) capture_one(transaction,count);
    end
    begin
      for(physical=1;physical<=length;physical=physical+1) begin
        @(negedge controller_clk); #1;
        if(controller_data!==expected_word(transaction,physical))
          $fatal(1,"tx %0d physical %0d got %h",transaction,physical,controller_data);
        @(posedge controller_clk);
      end
    end
  join
  repeat(2) @(posedge controller_clk);
end endtask

initial begin
  repeat(3) @(posedge controller_clk); reset=0;
  // The two one-line writes deliberately start at ring indices 0 then 4.
  run_burst(0,5'd7);
  run_burst(1,5'd7);
  run_burst(2,5'd15);
  run_burst(3,5'd23);
  run_burst(4,5'd31);
  $display("DIGITAL_DESIGN_PASS"); $finish;
end
endmodule
