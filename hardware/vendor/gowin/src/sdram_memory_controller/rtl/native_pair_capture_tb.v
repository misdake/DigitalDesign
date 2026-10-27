`timescale 1ns/1ps
module tb;
reg controller_clk=0,logic_clk=0;
always #5 controller_clk=!controller_clk;
always @(posedge controller_clk)logic_clk=!logic_clk;
wire sdram_clk=controller_clk;
reg reset=1,cpu_request_valid=0,cpu_write=0,cpu_line=1;
reg [21:0] cpu_address=0;reg [1:0] cpu_line_count_minus_1=3;
reg [63:0] cpu_write_data=0;reg cpu_response_ready=1;
wire cpu_request_ready,cpu_lookahead_window,cpu_write_data_ready,cpu_response_valid,cpu_response_last,cpu_error;
wire [63:0] cpu_read_data;
wire request_valid,writing,write_valid,request_ready,stream_active,write_ready,read_valid,done,initialized,next_valid;
wire [20:0] address,next_address;wire [5:0] words;wire [3:0]mask;wire [63:0]write_data,read_data;
wire [31:0]IO_sdram_dq;
SharedSdramPort #(.EARLY_GRANT(1)) adapter(.clk(logic_clk),.reset(reset),.cpu_request_valid(cpu_request_valid),.cpu_write(cpu_write),.cpu_line(cpu_line),.cpu_address(cpu_address),.cpu_line_count_minus_1(cpu_line_count_minus_1),.cpu_write_data(cpu_write_data),.cpu_response_ready(cpu_response_ready),.cpu_request_ready(cpu_request_ready),.cpu_lookahead_window(cpu_lookahead_window),.cpu_write_data_ready(cpu_write_data_ready),.cpu_response_valid(cpu_response_valid),.cpu_read_data(cpu_read_data),.cpu_response_last(cpu_response_last),.cpu_error(cpu_error),.controller_read_data(read_data),.controller_read_valid(read_valid),.controller_init_done(initialized),.controller_request_ready(request_ready),.controller_stream_active(stream_active),.controller_done(done),.controller_write_data_ready(write_ready),.controller_request_valid(request_valid),.controller_next_valid(next_valid),.controller_next_address(next_address),.controller_write(writing),.controller_address(address),.controller_write_mask(mask),.controller_write_data(write_data),.controller_write_data_valid(write_valid),.controller_words(words));
TangNano20KSdramNativeBridge108M54M bridge(.logic_clk(logic_clk),.controller_clk(controller_clk),.sdram_clk(sdram_clk),.reset(reset),.request_valid(request_valid),.next_valid(next_valid),.next_address(next_address),.writing(writing),.address(address),.words(words),.write_mask(mask),.write_data(write_data),.write_data_valid(write_valid),.request_ready(request_ready),.stream_active(stream_active),.write_data_ready(write_ready),.read_data(read_data),.read_valid(read_valid),.done(done),.initialized(initialized),.IO_sdram_dq(IO_sdram_dq));
reg native_valid=0,native_done=0;reg [31:0]native_data=0;
initial begin force bridge.core_ready=1;force bridge.core_initialized=1;force bridge.core_phase=18;force bridge.core_read_valid=native_valid;force bridge.core_read_data=native_data;force bridge.core_done=native_done;end
function [63:0] pattern;input integer n;begin pattern={32'hcaf00000+n,32'hbee00000+n};end endfunction
integer cycles=0,received=0,expected_beats=16,hold=0,held=0,case_id=0,total=0;
reg scalar=0,expect_error=0;reg [15:0]scalar_value=0;reg holding=0;reg [63:0]held_data=0;
always @(negedge logic_clk)begin
if(reset)begin cpu_response_ready=1;holding=0;held=0;end
else if(cpu_response_valid && cpu_response_last && hold>0)begin
cpu_response_ready=0;hold=hold-1;
if(!holding)begin held_data=cpu_read_data;holding=1;end
else if(cpu_read_data!==held_data || !cpu_response_valid || !cpu_response_last)$fatal(1,"terminal hold changed");
held=held+1;
end else cpu_response_ready=1;
end
always @(posedge logic_clk)begin
cycles=cycles+1;if(cycles>4000)$fatal(1,"bounded phase probe");
if(!reset && cpu_response_valid && cpu_response_ready)begin
if(cpu_error!==expect_error)$fatal(1,"error flag case%0d",case_id);
if(!expect_error && cpu_read_data!==(scalar?{48'd0,scalar_value}:pattern(received)))$fatal(1,"pair shifted case%0d beat%0d got%h expected%h",case_id,received,cpu_read_data,scalar?{48'd0,scalar_value}:pattern(received));
if(cpu_response_last!==(received==expected_beats-1))$fatal(1,"LAST mismatch");
$display("ACK case%0d cycle%0d beat%0d data%h last%0d",case_id,cycles,received,cpu_read_data,cpu_response_last);
received=received+1;total=total+1;
end end
task offer;input ln;input [21:0] a;begin
@(negedge logic_clk);holding=0;held=0;cpu_line=ln;cpu_address=a;cpu_request_valid=1;
#1;
while(!cpu_request_ready)begin @(negedge logic_clk);#1;end
@(posedge logic_clk);#1;cpu_request_valid=0;
repeat(8)@(negedge controller_clk);
end endtask
task complete;begin
while(received<expected_beats)@(negedge logic_clk);
repeat(8)@(negedge logic_clk);
end endtask
integer k,p;
initial begin
repeat(4)@(negedge logic_clk);reset=0;
for(p=0;p<2;p=p+1)begin
case_id=p;received=0;expected_beats=16;scalar=0;hold=p?0:6;offer(1,0);
if(p)@(negedge controller_clk);
for(k=0;k<32;k=k+1)begin native_valid=1;native_data=k[0]?(32'hcaf00000+k/2):(32'hbee00000+k/2);@(negedge controller_clk);end
native_valid=0;
if(p)begin
// Drain LAST before done: retain transaction ownership while waiting.
repeat(16)begin @(negedge logic_clk);if(request_ready || request_valid)$fatal(1,"native owner released before done");end
end else repeat(2)@(negedge controller_clk);
@(negedge controller_clk);native_done=1;@(negedge controller_clk);native_done=0;complete();
end
for(p=0;p<2;p=p+1)begin
case_id=2+p;received=0;expected_beats=1;scalar=1;scalar_value=p?16'habcd:16'h1234;hold=4;offer(0,p);
native_valid=1;native_data=32'habcd1234;@(negedge controller_clk);native_valid=0;
repeat(6)@(negedge controller_clk);native_done=1;@(negedge controller_clk);native_done=0;complete();
end
// Bounded acceleration of the real 20-bit timeout, no fabricated response.
case_id=4;received=0;expected_beats=1;expect_error=1;hold=3;offer(0,0);
@(negedge logic_clk);adapter.timeout_count=20'hfffff;complete();
// A timeout does not cancel a native command; global reset owns recovery.
reset=1;repeat(4)@(negedge logic_clk);reset=0;repeat(4)@(negedge logic_clk);
// Reset cuts a partially assembled pair; stale native Q must not emit valid.
case_id=5;expect_error=0;received=0;offer(1,0);native_valid=1;native_data=32'hbadc0ffe;@(negedge controller_clk);native_valid=0;
reset=1;repeat(4)@(negedge logic_clk);reset=0;repeat(10)@(negedge logic_clk);
if(cpu_response_valid || read_valid)$fatal(1,"orphan reset response");
$display("PASS old-Q phase probe total%0d",total);$finish;
end
initial begin #100000;$fatal(1,"wall bound");end
endmodule
