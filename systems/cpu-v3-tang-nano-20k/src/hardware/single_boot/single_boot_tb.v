`timescale 1ns/1ps
module tb;
parameter CASE=0;
reg clk=0;always #5 clk=!clk;
reg ready_to_start=0;
wire flash_start;wire [23:0]flash_address,flash_length;wire flash_data_ready;
reg flash_ready=CASE!=15,flash_data_valid=0,flash_done=0,flash_error=0;reg [7:0]flash_data=0;
wire request_valid,response_ready;wire [21:0]address;wire [15:0]data;
wire done,error;wire [3:0]error_code;
reg pending=0;reg [21:0]saved_address=0;reg [15:0]saved_data=0;integer delay=0;
reg grant_open=0;
wire request_ready=!pending && (CASE!=14 || grant_open);
wire response_valid=pending && delay==0 && CASE!=10;
wire memory_error=(CASE==7 && saved_address==8191) ||
                  ((CASE==8 || CASE==17) && saved_address==22'h70003);
SingleBootLoader #(.TIMEOUT_BITS(8)) dut(clk,ready_to_start,
 flash_ready,flash_data_valid,flash_data,flash_done,flash_error,
 request_ready,response_valid,memory_error,flash_start,flash_address,flash_length,flash_data_ready,
 request_valid,address,data,response_ready,done,error,error_code);
integer cycles=0,writes=0,code_writes=0,zero_writes=0,stream_index=0,stream_length=0;
integer completion_delay=0,final_write_cycle=0,final_flash_cycle=0;
integer stalled_cycles=0;
reg request_stalled=0;reg [21:0]held_address=0;reg [15:0]held_data=0;
reg held_done,held_error;reg [3:0]held_error_code;
reg payload=0,stream=0;
reg [15:0]statics[0:8192];reg [15:0]code[0:65536];integer n;
localparam CODE_BYTES=CASE==12 ? 131072 : CASE==3 ? 7 : CASE==4 ? 0 : CASE==5 ? 131074 : 8;
function [15:0]word_pattern;input integer index;begin word_pattern=16'ha500 ^ index;end endfunction
function [7:0]header_byte;input integer index;integer corrupt_index;begin
 case(index)
 0:header_byte=CASE==1 ? 8'h00 : 8'h43;1:header_byte=8'h50;2:header_byte=8'h55;3:header_byte=8'h33;
 4:header_byte=8'h42;5:header_byte=8'h4f;6:header_byte=8'h4f;7:header_byte=8'h54;
 8:header_byte=CASE==2 ? 4 : 5;10:header_byte=32;
 12:header_byte=CODE_BYTES;13:header_byte=CODE_BYTES>>8;14:header_byte=CODE_BYTES>>16;
 16:header_byte=CASE==6 ? 1 : 0;default:header_byte=0;
 endcase
 // Mutate every fixed header byte, including the high length/reserved byte.
 corrupt_index=CASE-18;
 if(corrupt_index>=12)corrupt_index=corrupt_index+3;
 if(CASE>=18 && CASE<=46 && index==corrupt_index)header_byte=header_byte^1;
end endfunction
function [3:0]expected_error;input integer test_case;begin
 case(test_case)
  1,2,6:expected_error=1;
  3,4,5:expected_error=2;
  7,8:expected_error=4;
  9,17:expected_error=3;
  10,11,15,16:expected_error=5;
  default:expected_error=test_case>=18 ? 1 : 0;
 endcase
end endfunction
reg [15:0]next_word;
always @(negedge clk)begin
 grant_open=cycles%11>=7;
 flash_data_valid=stream && cycles%3!=0;
 next_word=word_pattern(stream_index/2);
 flash_data=payload ? (stream_index%2 ? next_word[15:8] : next_word[7:0]) : header_byte(stream_index);
end
always @(posedge clk)begin
 cycles=cycles+1;flash_done<=0;flash_error<=0;
 if(completion_delay>0)begin
  completion_delay<=completion_delay-1;
  if(completion_delay==1)flash_done<=1;
 end
 if(flash_done && payload)final_flash_cycle=cycles;
 if(cycles==5)ready_to_start<=1;
 if(cycles>1200000)$fatal(1,"bounded loader case%0d",CASE);
 if(flash_start)begin
  if(!flash_ready)$fatal(1,"Flash start while occupied");
  if(flash_address!=24'h100000 && flash_address!=24'h100080)$fatal(1,"Flash address");
  payload<=flash_address==24'h100080;
  stream<=1;flash_ready<=0;stream_index<=0;
  stream_length<=CASE==11 && flash_address==24'h100000 ? 31 : flash_length;
 end
 if(flash_data_valid && flash_data_ready)begin
  if(CASE==9 && payload && stream_index==2)flash_error<=1;
  stream_index<=stream_index+1;
  if(stream_index+1==stream_length)begin
   stream<=0;flash_ready<=1;
   if(payload && CASE==13)completion_delay<=70;
   else if(!(payload && CASE==16))flash_done<=1;
  end
 end
 if(CASE==14)begin
  if(request_stalled && (!request_valid || address!==held_address || data!==held_data))
   $fatal(1,"request changed under backpressure");
  request_stalled=request_valid && !request_ready;
  held_address=address;held_data=data;
  if(request_stalled)stalled_cycles=stalled_cycles+1;
 end
 if(request_valid && request_ready)begin
  pending<=1;saved_address<=address;saved_data<=data;
  delay<=address==22'h70003 ? 25 : 2+(writes%5);
 end else if(pending && delay>0)begin
  delay<=delay-1;
  // Exercise Flash-error priority on the same edge as a final memory error.
  if(CASE==17 && saved_address==22'h70003 && delay==1)flash_error<=1;
 end
 if(response_valid && response_ready)begin
  pending<=0;
  if(saved_address==22'h70000+CODE_BYTES/2-1)final_write_cycle=cycles;
  if(!memory_error)begin
   writes=writes+1;
   if(saved_address<8192)begin
    if(saved_address!=zero_writes || saved_data!=0)$fatal(1,"zero order/value");
    statics[saved_address]=saved_data;zero_writes=zero_writes+1;
   end else begin
    if(zero_writes!=8192 || saved_address!=22'h70000+code_writes || saved_data!==word_pattern(code_writes))$fatal(1,"code address/data");
    code[code_writes]=saved_data;code_writes=code_writes+1;
   end
  end
 end
 if(done || error)begin
  if(CASE==0 || CASE==12 || CASE==13 || CASE==14)begin
   if(!done || error || writes!=8192+CODE_BYTES/2 || pending || !flash_ready)$fatal(1,"release before final write/Flash completion");
   for(n=0;n<8192;n=n+1)if(statics[n]!==0)$fatal(1,"nonzero initial static survived");
   if(statics[8192]!==16'ha55a || code[CODE_BYTES/2]!==16'ha55a)$fatal(1,"outside clear/copy changed");
   if(CASE==13 && !(final_write_cycle>0 && final_flash_cycle>final_write_cycle))
    $fatal(1,"late Flash completion was not exercised");
   if(CASE==0 && !(final_flash_cycle>0 && final_write_cycle>final_flash_cycle))
    $fatal(1,"late final write was not exercised");
   if(CASE==14 && stalled_cycles==0)$fatal(1,"request backpressure was not exercised");
  end else begin
   if(!error || done)$fatal(1,"failed boot released CPU");
   if(error_code!==expected_error(CASE))$fatal(1,"wrong failure code %0d",error_code);
   if(CASE<=6 && writes!=0)$fatal(1,"invalid header wrote SDRAM");
   if((CASE==15 || CASE>=18) && writes!=0)$fatal(1,"pre-copy failure wrote SDRAM");
  end
  held_done=done;held_error=error;held_error_code=error_code;
  repeat(10)begin
   @(negedge clk);
   ready_to_start=!ready_to_start;
   if(done!==held_done || error!==held_error || error_code!==held_error_code ||
      request_valid || response_ready || flash_start || flash_data_ready)
    $fatal(1,"terminal boot state restarted or lost its result");
  end
  $display("PASS single boot case%0d cycles%0d writes%0d error%0d",CASE,cycles,writes,error_code);$finish;
 end
end
initial begin
 for(n=0;n<=8192;n=n+1)statics[n]=16'ha55a;
 for(n=0;n<=65536;n=n+1)code[n]=16'ha55a;
 #13000000;$fatal(1,"wall watchdog");
end
endmodule
