`timescale 1ns/1ps
module tb;
reg logic_clk=0,controller_clk=0,sdram_clk=0;
reg reset=0,request_valid=0,request_continuation=0,writing=0;
reg[20:0]address=0,next_address=0;reg[5:0]words=0;
reg[3:0]write_mask=0;reg[63:0]write_data=0;
reg write_data_valid=0,next_valid=0;
reg p_ready=0,p_done=0,p_init=0,p_read_valid=0,p_chain=0,p_read_chain=0;
reg[31:0]p_data=0;reg[5:0]p_index=0;reg[7:0]p_phase=0;
wire request_ready,stream_active,write_data_ready,read_valid,write_done,initialized;
wire[63:0]read_data;
TangNano20KSdramNativeBridge108M54M #(.COLD_START_ONLY(__COLD__),.INIT_CYCLES(8)) dut(
 .logic_clk(logic_clk),.controller_clk(controller_clk),.sdram_clk(sdram_clk),.reset(reset),
 .request_valid(request_valid),.request_continuation(request_continuation),.writing(writing),
 .address(address),.words(words),.write_mask(write_mask),.write_data(write_data),
 .write_data_valid(write_data_valid),.next_valid(next_valid),.next_address(next_address),
 .request_ready(request_ready),.stream_active(stream_active),.write_data_ready(write_data_ready),
 .read_data(read_data),.read_valid(read_valid),.write_done(write_done),.initialized(initialized));
integer fd,code,step=0;reg[2:0]edges;
initial begin
 fd=$fopen("input.txt","r");if(!fd)$fatal(1,"input missing");
 while(!$feof(fd)&&step<2048)begin
  code=$fscanf(fd,"%h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h\n",
   edges,reset,request_valid,request_continuation,writing,address,words,write_mask,write_data,
   write_data_valid,next_valid,next_address,p_ready,p_done,p_init,p_read_valid,p_data,p_index,p_phase,p_chain,p_read_chain);
  if(code!=21)$fatal(1,"bad stimulus at %d",step);
  #5;
  if(edges[1])controller_clk=1;
  if(edges[2])controller_clk=0;
  if(edges[0])logic_clk=1;else if(edges[1])logic_clk=0;
  #1;
  $display("TRACE %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h",
   request_ready,stream_active,write_data_ready,read_data,read_valid,write_done,initialized,
   dut.u_controller.reset,dut.u_controller.request,dut.u_controller.writing,dut.u_controller.address,
   dut.u_controller.words,dut.u_controller.write_slot,dut.u_controller.next_valid,
   dut.u_controller.next_address,dut.u_controller.next_words,dut.u_controller.next_chainable,
   dut.u_controller.next_write_ready,dut.u_controller.next_generation,
   dut.u_controller.write_data,dut.u_controller.write_mask);
  step=step+1;
 end
 $display("COMPLETE %0d",step);$finish;
end
initial begin #20000;$fatal(1,"gear oracle watchdog");end
endmodule
