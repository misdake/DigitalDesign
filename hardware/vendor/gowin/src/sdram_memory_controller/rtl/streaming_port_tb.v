`timescale 1ns/1ps
module tb;
reg clk=0;always #5 clk=!clk;
reg reset=1,cpu_request_valid=0,cpu_write=0,cpu_line=1,cpu_response_ready=1;
reg[21:0]cpu_address=0;reg[1:0]cpu_line_count_minus_1=0,cpu_chain_class=1;
reg[63:0]cpu_write_data=0,controller_read_data=0;
reg controller_read_valid=0,controller_init_done=1,controller_request_ready=1;
reg controller_stream_active=1,controller_write_done=0,controller_write_data_ready=1;
wire cpu_request_ready,cpu_lookahead_window,cpu_write_data_ready,cpu_response_valid,cpu_response_last,cpu_error;
wire[63:0]cpu_read_data,controller_write_data;
wire controller_request_valid,controller_next_valid,controller_request_continuation,controller_write,controller_write_data_valid;
wire[20:0]controller_address,controller_next_address;wire[3:0]controller_write_mask;wire[5:0]controller_words;
__DUT__ dut(.*);
integer cycle=0,requests=0,reads=0,terms=0,writes=0,i;
always @(posedge clk)begin
 cycle=cycle+1;if(cycle>500)$fatal(1,"endpoint watchdog");
 if(!reset)begin
  if(controller_request_valid&&controller_request_ready)begin
   if(requests<2)begin
    if(controller_write||controller_words!=8||controller_address!=(21'h400+requests*8))$fatal(1,"read descriptor");
    if(controller_request_continuation!=(requests==1))$fatal(1,"successor proof");
   end else if(!controller_write||controller_words!=1||controller_write_mask!=12)$fatal(1,"scalar descriptor");
   requests=requests+1;
  end
  if(controller_write_data_valid&&controller_write_data_ready)begin
   if(controller_write_data!==64'h1234)$fatal(1,"scalar payload");writes=writes+1;
  end
  if(cpu_response_valid&&cpu_response_ready)begin
   if(cpu_error)$fatal(1,"unexpected endpoint error");
   if(reads<8)begin
    if(cpu_read_data!==(64'hfeed0000+reads)||cpu_response_last!=(reads%4==3))$fatal(1,"read owner/framing");
    reads=reads+1;
   end
   if(cpu_response_last)terms=terms+1;
  end
 end
end
task offer;input[21:0]address;begin
 @(negedge clk);cpu_address=address;cpu_request_valid=1;
 @(posedge clk);while(!cpu_request_ready)@(posedge clk);
 @(negedge clk);cpu_request_valid=0;
end endtask
initial begin
 repeat(3)@(negedge clk);reset=0;
 offer(22'h800);offer(22'h810);
 wait(requests==2);if(reads!=0)$fatal(1,"no early request coverage");
 for(i=0;i<8;i=i+1)begin
  @(negedge clk);controller_read_valid=1;controller_read_data=64'hfeed0000+i;
 end
 @(negedge clk);controller_read_valid=0;
 wait(terms==2);@(negedge clk);cpu_line=0;cpu_write=1;cpu_chain_class=0;cpu_write_data=16'h1234;
 offer(22'h40);wait(writes==1);
 @(negedge clk);cpu_response_ready=0;controller_write_done=1;
 @(negedge clk);controller_write_done=0;
 repeat(3)begin @(negedge clk);if(!cpu_response_valid||!cpu_response_last||cpu_read_data!=0)$fatal(1,"held write terminal");end
 cpu_response_ready=1;wait(terms==3);@(negedge clk);
 if(requests!=3||reads!=8||writes!=1)$fatal(1,"endpoint conservation");
 $display("early=1 read_words=%0d terminals=%0d",reads,terms);$display("DIGITAL_DESIGN_PASS");$finish;
end
endmodule
