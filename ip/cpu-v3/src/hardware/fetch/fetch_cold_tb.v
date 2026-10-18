`timescale 1ns/1ps
module tb;
reg clk=0;always #5 clk=!clk;
wire reset=1'b0;reg flush=0,core_request_valid=0;
reg [31:0] core_address=32'h70000;
reg core_response_ready=1;
wire core_request_ready,core_response_valid,core_error;
wire [15:0]core_read_data;
wire memory_request_valid,memory_response_ready;
wire [31:0]memory_address;
reg memory_response_valid=0,memory_error=0;
reg [15:0]memory_read_data=0;
wire memory_request_ready=!memory_response_valid || memory_response_ready;
__FETCH_QUEUE__ dut(.*);
function [15:0] pattern;input [31:0]address;begin pattern=16'h6a95 ^ address[15:0] ^ {address[23:16],address[31:24]};end endfunction
integer cycles=0,installs=0,replays=0,accepted=0;
always @(posedge clk)begin
 cycles=cycles+1;if(cycles>3000)$fatal(1,"bounded cold frontend");
 if(memory_request_ready)begin
  memory_response_valid<=memory_request_valid;
  if(memory_request_valid)memory_read_data<=pattern(memory_address);
 end
 if(dut.install)installs=installs+1;
 if(dut.btc_response && dut.core_pop)replays=replays+1;
 if(core_request_ready && core_request_valid)accepted=accepted+1;
end
task consume;input [31:0]address;begin
 @(negedge clk);core_address=address;core_request_valid=1;
 #1;while(!core_request_ready)begin @(negedge clk);#1;end
 if(!core_response_valid || core_error || core_read_data!==pattern(address))$fatal(1,"cold fetch %h data%h expected%h",address,core_read_data,pattern(address));
 @(posedge clk);#1;core_request_valid=0;
end endtask
integer pass,target;
initial begin
 repeat(3)@(negedge clk);
 // No reset pulse occurs at any point. Repeated complete two-word targets
 // exchange the spare with distinct victims, then replay installed entries.
 for(pass=0;pass<3;pass=pass+1)begin
  for(target=0;target<6;target=target+1)begin
   consume(32'h70000+target*32'h100);
   consume(32'h70001+target*32'h100);
   consume(32'h70000+target*32'h100);
   consume(32'h70001+target*32'h100);
  end
 end
 if(accepted!=72)$fatal(1,"cold acceptance count");
 if(dut.BTC_ENTRIES!=0 && (installs<6 || replays<6))$fatal(1,"cold install/replay not exercised %0d/%0d",installs,replays);
 $display("PASS cold frontend accepted%0d installs%0d replays%0d BTC%0d",accepted,installs,replays,dut.BTC_ENTRIES);
 $finish;
end
initial begin #40000;$fatal(1,"cold frontend wall bound");end
endmodule
