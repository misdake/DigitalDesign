`timescale 1ns/1ps
module tb;
reg clk=0,sdram_clk=0,reset=0,request=0,writing=0;
reg[20:0]address=0,next_address=0;reg[5:0]words=0,next_words=0;
reg write_slot=0,next_valid=0,next_chainable=0,next_write_ready=0,next_generation=0,read_boundary=0;
reg[31:0]write_data=0,external_data=0;reg[3:0]write_mask=0;
wire ready,done,read_valid,initialized,chain_accept,read_chain_accept,read_chain_pending;
wire[31:0]read_data;wire[5:0]write_index;wire[7:0]phase;wire[23:0]stream_stats;
wire cke,cs,ras,cas,we;wire[3:0]dqm;wire[10:0]a;wire[1:0]ba;wire[31:0]dq;
SdramController #(.BANK_BIT(5),.CAS(3),.RCD_CYCLES(3),.RP_CYCLES(3),.CAPTURE_LATENCY(3),.DQ_INPUT_DELAY_TAPS(32),.INIT_CYCLES(__INIT__),.CHAIN(1),.READ_CHAIN(1),.CONTINUE(1),.PREPARE_NEXT(1),.NEXT_TAGGED(1))dut(
 .clk(clk),.sdram_clk(sdram_clk),.reset(reset),.request(request),.writing(writing),.address(address),.words(words),
 .write_slot(write_slot),.next_valid(next_valid),.next_address(next_address),.next_words(next_words),
 .next_chainable(next_chainable),.next_write_ready(next_write_ready),.next_generation(next_generation),
 .chain_accept(chain_accept),.read_chain_accept(read_chain_accept),.read_boundary(read_boundary),.read_chain_pending(read_chain_pending),
 .ready(ready),.done(done),.write_data(write_data),.write_mask(write_mask),.write_index(write_index),
 .read_data(read_data),.read_valid(read_valid),.initialized(initialized),.phase(phase),.stream_stats(stream_stats),
 .O_sdram_clk(),.O_sdram_cke(cke),.O_sdram_cs_n(cs),.O_sdram_cas_n(cas),.O_sdram_ras_n(ras),.O_sdram_wen_n(we),
 .O_sdram_dqm(dqm),.O_sdram_addr(a),.O_sdram_ba(ba),.IO_sdram_dq(dq));
// Boundary injection, not an SDRAM device: stable data reaches the real IODELAY.
// Never contend with the controller's actual pad OE, including pre-launch cycles.
assign dq=(dut.dq_oe_n_pins==32'hffffffff)?external_data:32'bz;
integer fd,code,step=0;
initial begin
 fd=$fopen("input.txt","r");if(!fd)$fatal(1,"input missing");
 while(!$feof(fd)&&step<30000)begin
  code=$fscanf(fd,"%h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h\n",
   reset,request,writing,address,words,write_slot,next_valid,next_address,next_words,next_chainable,next_write_ready,next_generation,write_data,write_mask,read_boundary,external_data);
  if(code!=16)$fatal(1,"bad stimulus at %d",step);
  #4;clk=1;#2;
  $display("TRACE %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h",
   ready,done,initialized,read_valid,read_data,write_index,phase,chain_accept,read_chain_accept,
   read_chain_pending,stream_stats,{ras,cas,we},cke,a,ba,dqm,dut.dq_oe_n_pins,dut.dq_out);
  #4;clk=0;step=step+1;
 end
 $display("COMPLETE %0d",step);$finish;
end
initial begin #300010;$fatal(1,"native controller oracle watchdog");end
endmodule
