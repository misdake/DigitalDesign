`timescale 1ns/1ps
module tb;
reg clk=0,reset=1,invalidate_all=0;
always #5 clk=~clk;
reg [31:0] cpu_address=0;
reg cpu_request_valid=0,cpu_response_ready=0,memory_request_ready=1;
reg memory_response_valid=0,memory_error=0;
reg [63:0] memory_read_data=0;
wire cpu_request_ready,cpu_response_valid,cpu_error,memory_request_valid,memory_response_ready;
wire [15:0] cpu_read_data;wire [21:0] memory_address;
CpuV3InstructionCache dut(.*);
integer cycles=0,dram_requests=0,beat=0;
reg active=0,fail_enabled=0;
reg [21:0] line=0,fail_line=0;
function [15:0] pattern;
 input [21:0] address;
 begin pattern=16'h5371 ^ address[15:0] ^ {address[6:0],9'b0};end
endfunction
always @(posedge clk) begin
 cycles<=cycles+1;
 if(cycles>3000)$fatal(1,"direct held-error watchdog");
 memory_response_valid<=0;memory_error<=0;
 if(reset)begin active<=0;beat<=0;end
 else if(memory_request_valid && memory_request_ready)begin
  if(active)$fatal(1,"overlapping line transactions");
  active<=1;beat<=0;line<=memory_address;dram_requests<=dram_requests+1;
 end else if(active)begin
  memory_response_valid<=1;
  memory_read_data<={pattern(line+beat*4+3),pattern(line+beat*4+2),pattern(line+beat*4+1),pattern(line+beat*4)};
  memory_error<=fail_enabled && line==fail_line && beat==2;
  if(beat==3 || (fail_enabled && line==fail_line && beat==2))active<=0;
  else beat<=beat+1;
 end
end
task issue;
 input [31:0] address;
 begin
  @(negedge clk);cpu_address=address;cpu_request_valid=1;
  while(!cpu_request_ready)@(negedge clk);
  @(posedge clk);@(negedge clk);cpu_request_valid=0;
 end
endtask
task wait_success;
 input [21:0] address;
 begin
  while(!cpu_response_valid)@(negedge clk);
  if(cpu_error || cpu_read_data!==pattern(address))$fatal(1,"successor payload/error wrong");
  @(posedge clk);@(negedge clk);
 end
endtask
integer scenario,requests_before,passed=0;
reg [31:0] bad_address,next_address;
reg next_hit;
initial begin
`ifdef HELD_ERROR_REFILL_ONLY
 scenario=2;
`else
 scenario=0;
`endif
 for(scenario=scenario;scenario<4;scenario=scenario+1)begin
  @(negedge clk);reset=1;cpu_request_valid=0;cpu_response_ready=0;fail_enabled=0;
  repeat(3)@(negedge clk);reset=0;
  next_hit=scenario%2;
  next_address=next_hit ? 32'h1000+scenario*16 : 32'h3000+scenario*16;
  if(next_hit)begin cpu_response_ready=1;issue(next_address);wait_success(next_address[21:0]);end
  cpu_response_ready=0;
  bad_address=scenario<2 ? 32'h400000 : 32'h2000+scenario*16;
  fail_enabled=scenario>=2;fail_line=bad_address[21:0];
  issue(bad_address);
  while(!cpu_response_valid)@(negedge clk);
  if(!cpu_error || cpu_read_data!==0)$fatal(1,"error was not initially published");
  requests_before=dram_requests;
  cpu_address=next_address;cpu_request_valid=1;
  if(!cpu_request_ready)$fatal(1,"successor lookup slot not available");
  @(posedge clk);#1;
  if(!cpu_response_valid || !cpu_error || cpu_read_data!==0)
   $fatal(1,"HELD_ERROR_CLOBBERED_AFTER_SUCCESSOR scenario=%0d",scenario);
  @(negedge clk);cpu_request_valid=0;
  repeat(4)begin
   if(!cpu_response_valid || !cpu_error || cpu_read_data!==0)$fatal(1,"held error changed");
   @(negedge clk);
  end
  cpu_response_ready=1;
  @(posedge clk);@(negedge clk);
  wait_success(next_address[21:0]);
  if(dram_requests-requests_before!==(next_hit?0:1))$fatal(1,"successor hit/miss scenario was not exercised");
  passed=passed+1;
  $display("DIRECT_HELD_ERROR_CASE scenario=%0d refill_error=%0d next_hit=%0d PASS",scenario,scenario>=2,next_hit);
 end
`ifdef HELD_ERROR_REFILL_ONLY
 if(passed!=2)$fatal(1,"direct coverage incomplete");
`else
 if(passed!=4)$fatal(1,"direct coverage incomplete");
`endif
 $display("HELD_ERROR_DIRECT_PASS");$finish;
end
endmodule
