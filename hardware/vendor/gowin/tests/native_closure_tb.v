`timescale 1ns/1ps
module tb;
reg logic_clk=0,controller_clk=0,sdram_clk=0,reset=0,request_valid=0,request_continuation=0,writing=0;
reg[20:0]address=0,next_address=0;reg[5:0]words=0;reg[3:0]write_mask=0;reg[63:0]write_data=0;
reg write_data_valid=0,next_valid=0;
wire request_ready,stream_active,write_data_ready,read_valid,write_done,initialized;wire[63:0]read_data;
wire O_sdram_clk,O_sdram_cke,O_sdram_cs_n,O_sdram_cas_n,O_sdram_ras_n,O_sdram_wen_n;
wire[3:0]O_sdram_dqm;wire[10:0]O_sdram_addr;wire[1:0]O_sdram_ba;wire[31:0]IO_sdram_dq;
always #4.629629 controller_clk=~controller_clk;
always @(posedge controller_clk)logic_clk=~logic_clk;
initial begin #12.430556;forever begin sdram_clk=~sdram_clk;#4.629629;end end
TangNano20KSdramNativeBridge108M54M #(.COLD_START_ONLY(1),.INIT_CYCLES(32))dut(.*);
LabPinModel #(.PERIOD_NS(9.259259),.CAS_LATENCY(3),.RCD_MIN(3),.RP_MIN(3),.RETURN_MIN(1),.RETURN_MAX(4))pin(
 .sclk(O_sdram_clk),.reset(reset),.cke(O_sdram_cke),.cs(O_sdram_cs_n),.ras(O_sdram_ras_n),.cas(O_sdram_cas_n),.we(O_sdram_wen_n),
 .dqm(O_sdram_dqm),.a(O_sdram_addr),.ba(O_sdram_ba),.dq(IO_sdram_dq),.cycle(),.refreshes());
function[20:0]native;input[20:0]a;begin native={a[6:5],a[20:7],a[4:0]};end endfunction
function[20:0]logical;input[20:0]a;begin logical={a[18:5],a[20:19],a[4:0]};end endfunction
integer fd,code,loaded=0,seen=0,n,write_log,final_log;reg[20:0]committed;
task load_row;begin
 if(loaded<__COUNT__)begin
 code=$fscanf(fd,"%h %h %h %h %h %h %h %h %h %h %h\n",reset,request_valid,request_continuation,writing,address,words,write_mask,write_data,write_data_valid,next_valid,next_address);
 if(code!=11)$fatal(1,"input row %d",loaded);loaded=loaded+1;
 end
end endtask
initial begin
 fd=$fopen("input.txt","r");if(!fd)$fatal(1,"input missing");write_log=$fopen("writes.txt","w");
 for(n=0;n<4096;n=n+1)pin.seed_word(native(n),32'h10000000|n);
 load_row;
end
// Change a descriptor only after the preceding full logic cycle's native/pin events.
always @(negedge controller_clk)if(seen>0&&!logic_clk)begin #1.5;load_row;end
always @(posedge O_sdram_clk)begin
 #0.001;
 if(!reset&&pin_cke&&!pin_cs&&pin.writing_bus)begin
  committed={pin.burst_bank,pin.rows[pin.burst_bank],(pin.col-8'd1)};
  $fdisplay(write_log,"%h %h %h",logical(committed),pin_dqm,pin.physical_word(committed));
 end
end
always @(posedge logic_clk)begin
 $display("INPUT %h %h %h %h %h %h %h %h %h %h %h",reset,request_valid,request_continuation,writing,address,words,write_mask,write_data,write_data_valid,next_valid,next_address);
 #0.001;
 $display("TRACE %h %h %h %h %h %h %h",request_ready,stream_active,write_data_ready,read_data,read_valid,write_done,initialized);
 seen=seen+1;
 if(seen==__COUNT__)begin
  #16;final_log=$fopen("final.txt","w");for(n=0;n<4096;n=n+1)$fdisplay(final_log,"%h",pin.physical_word(native(n)));
  $display("COMPLETE %0d",seen);$finish;
 end
end
initial begin #100000;$fatal(1,"native closure watchdog");end
endmodule
