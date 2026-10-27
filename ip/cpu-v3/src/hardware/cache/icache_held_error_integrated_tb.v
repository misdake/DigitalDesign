`timescale 1ns/1ps
module tb;
reg clk=0,reset=1,warm=1,invalidate_all=0;
always #5 clk=~clk;
reg core_request_valid=0,core_response_ready=0;
reg [31:0] core_address=0;
wire core_request_ready,core_response_valid,core_error;
wire [15:0] core_read_data;
wire fetch_request_valid,fetch_response_ready;
wire [31:0] fetch_address;
wire cache_request_ready,cache_response_valid,cache_error;
wire [15:0] cache_read_data;
reg warm_valid=0,warm_ready=0;reg [31:0] warm_address=0;
wire memory_request_valid,memory_response_ready;
wire [21:0] memory_address;
reg memory_request_ready=1,memory_response_valid=0,memory_error=0;
reg [63:0] memory_read_data=0;
CpuV3InstructionFetchQueue fetch(
 .clk(clk),.reset(reset||warm),.flush(1'b0),
 .core_request_valid(core_request_valid),.core_address(core_address),.core_response_ready(core_response_ready),
 .memory_request_ready(cache_request_ready && !warm),.memory_response_valid(cache_response_valid && !warm),
 .memory_read_data(cache_read_data),.memory_error(cache_error),
 .core_request_ready(core_request_ready),.core_response_valid(core_response_valid),.core_read_data(core_read_data),.core_error(core_error),
 .memory_request_valid(fetch_request_valid),.memory_address(fetch_address),.memory_response_ready(fetch_response_ready));
CpuV3InstructionCache cache(
 .clk(clk),.reset(reset),.invalidate_all(invalidate_all),
 .cpu_request_valid(warm?warm_valid:fetch_request_valid),.cpu_address(warm?warm_address:fetch_address),
 .cpu_response_ready(warm?warm_ready:fetch_response_ready),
 .memory_request_ready(memory_request_ready),.memory_response_valid(memory_response_valid),.memory_read_data(memory_read_data),.memory_error(memory_error),
 .cpu_request_ready(cache_request_ready),.cpu_response_valid(cache_response_valid),.cpu_read_data(cache_read_data),.cpu_error(cache_error),
 .memory_request_valid(memory_request_valid),.memory_address(memory_address),.memory_response_ready(memory_response_ready));
integer cycles=0,beat=0,exposures=0,scenario=0;
reg active=0,fail_enabled=0;reg [21:0] line=0,fail_line=0;
function [15:0] pattern;
 input [21:0] address;
 begin pattern=16'h5371 ^ address[15:0] ^ {address[6:0],9'b0};end
endfunction
function [31:0] next_word;
 input [31:0] address;
 begin next_word={address[31:16],address[15:0]+16'd1};end
endfunction
always @(posedge clk)begin
 cycles<=cycles+1;
 if(cycles>3000)$fatal(1,"integrated held-error watchdog");
 memory_response_valid<=0;memory_error<=0;
 if(reset)begin active<=0;beat<=0;end
 else if(memory_request_valid && memory_request_ready)begin
  if(active)$fatal(1,"overlapping line transactions");
  active<=1;beat<=0;line<=memory_address;
 end else if(active)begin
  memory_response_valid<=1;
  memory_read_data<={pattern(line+beat*4+3),pattern(line+beat*4+2),pattern(line+beat*4+1),pattern(line+beat*4)};
  memory_error<=fail_enabled && line==fail_line && beat==2;
  if(beat==3 || (fail_enabled && line==fail_line && beat==2))begin
   active<=0;
   if(fail_enabled && line==fail_line && beat==2)fail_enabled<=0;
  end else beat<=beat+1;
 end
end
// Independent assertion: a valid unaccepted error must survive accepting
// another lookup. This does not use the cycle model's response-error state.
always @(posedge clk)if(!reset && !warm && fetch.queue_count==2 &&
 cache_response_valid && cache_error && !fetch_response_ready &&
 fetch_request_valid && cache_request_ready)begin
 if(fetch.metadata_count!=1)$fatal(1,"expected independent q2/meta1 exposure");
 exposures=exposures+1;
 $display("INTEGRATED_EXPOSURE scenario=%0d q=2 metadata=1 held_error=1 successor=%h",scenario,fetch_address);
 #1;
 if(!cache_response_valid || !cache_error)$fatal(1,"INTEGRATED_HELD_ERROR_CLOBBERED scenario=%0d",scenario);
end
task warm_line;
 input [31:0] address;
 begin
  @(negedge clk);warm_address=address;warm_valid=1;warm_ready=1;
  while(!cache_request_ready)@(negedge clk);
  @(posedge clk);@(negedge clk);warm_valid=0;
  while(!cache_response_valid)@(negedge clk);
  if(cache_error || cache_read_data!==pattern(address[21:0]))$fatal(1,"warm line wrong");
  @(posedge clk);@(negedge clk);warm_ready=0;
 end
endtask
integer seen,old_exposures,passed=0;
reg expected_error;reg [15:0] expected_data;
initial begin
`ifdef HELD_ERROR_REFILL_ONLY
 scenario=1;
`else
 scenario=0;
`endif
 for(scenario=scenario;scenario<2;scenario=scenario+1)begin
  @(negedge clk);reset=1;warm=1;warm_valid=0;core_request_valid=0;core_response_ready=0;fail_enabled=0;
  repeat(3)@(negedge clk);reset=0;
  if(scenario==1)warm_line(32'h100e);
  core_address=scenario==0?32'h400000:32'h100e;
  fail_line=22'h1010;fail_enabled=scenario==1;
  old_exposures=exposures;
  warm=0;core_request_valid=1;
  while(exposures==old_exposures)@(negedge clk);
  if(fetch.queue_count!=2)$fatal(1,"FIFO was not kept full until exposure");
  core_response_ready=1;
  for(seen=0;seen<4;seen=seen+1)begin
   while(!core_response_valid)@(negedge clk);
   expected_error=scenario==0 || seen==2;
   expected_data=expected_error?16'd0:pattern(core_address[21:0]);
   if(core_error!==expected_error || core_read_data!==expected_data)
    $fatal(1,"CORE_LOST_ORIGINAL_ERROR_OR_WORD scenario=%0d word=%0d address=%h error=%b data=%h",scenario,seen,core_address,core_error,core_read_data);
   @(posedge clk);@(negedge clk);core_address=next_word(core_address);#1;
  end
  core_request_valid=0;core_response_ready=0;
  passed=passed+1;
  $display("INTEGRATED_HELD_ERROR_CASE scenario=%0d refill_error=%0d core_observed_four_ordered_words=1 PASS",scenario,scenario==1);
 end
`ifdef HELD_ERROR_REFILL_ONLY
 if(passed!=1)$fatal(1,"integrated coverage incomplete");
`else
 if(passed!=2)$fatal(1,"integrated coverage incomplete");
`endif
 $display("HELD_ERROR_INTEGRATED_PASS");$finish;
end
endmodule
