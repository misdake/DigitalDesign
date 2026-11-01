`timescale 1ns/1ps
module tb;
 parameter CHECKED=0,MONITORS=0;
 localparam COUNT=96;
 reg clk=0;always #5 clk=~clk;
 GSR GSR(.GSRI(1'b1));
 reg reset=1,ce=0,commit=0,offer=0,ready=0,abort=0,shade=0;
 reg[89:0]pixels[0:COUNT-1];reg[23:0]gold[0:COUNT-1];reg[89:0]payload;
 wire in_ready,proposal,in_accept,advance,bad,fault,out_valid,out_fire;wire[23:0]rgb;wire[3:0]credits,queued;
 wire spec_ce;wire[8:0]spec_a;wire[23:0]spec_b;wire[50:0]spec_product;wire[53:0]rop_product;
 wire rop_ce=advance&&shade;
 gpu_v2_final_rop_bank bank(.clk(clk),.reset(reset),.rop_ce(rop_ce),.rop_a(27'h7f103ff),.rop_b(8'd173),.rop_product(rop_product),.spec_ce(spec_ce),.spec_a(spec_a),.spec_b(spec_b),.spec_product(spec_product));
 gpu_v2_final_ordered_two_cycle dut(.shade_phase(shade),.clk(clk),.reset(reset),.ce(ce),.proposal_ce(ce),.commit_allow(commit),.abort(abort),.in_valid(offer),.shared_rop_offer(rop_ce),.shared_spec_product(spec_product),.shared_spec_ce(spec_ce),.shared_spec_a(spec_a),.shared_spec_b(spec_b),.in_tint(payload[23:0]),.in_tex(payload[47:24]),.in_spec(payload[71:48]),.in_g(payload[80:72]),.in_h(payload[89:81]),.in_ready(in_ready),.proposal_ready(proposal),.in_accept(in_accept),.advance(advance),.preflight_bad(bad),.fault(fault),.out_ready(ready),.out_valid(out_valid),.out_fire(out_fire),.out_rgb(rgb),.credits(credits),.queued(queued));
 reg[1023:0]input_file,gold_file;reg[127:0]scenario;
 integer wall=0,sent=0,received=0,full=0,overlap=0,direct=0,held=0,noborrow=0,blocked=0;
 integer abort_stage=0,abort_age=0,epoch=0,bad_started=0;
 reg[167:0]snapshot;
 wire[167:0]state_q={dut.q_count,dut.q_head,dut.q_tail,dut.arithmetic_live,dut.result0,dut.result1,dut.channels[0].product,dut.channels[1].product,dut.channels[2].product,dut.channels[0].sh,dut.channels[1].sh,dut.channels[2].sh,shade};
 initial begin
  if(!$value$plusargs("INPUT=%s",input_file)||!$value$plusargs("GOLD=%s",gold_file)||!$value$plusargs("CASE=%s",scenario))$fatal(1,"inputs");
  $readmemh(input_file,pixels);$readmemh(gold_file,gold);payload=pixels[0];
  repeat(3)@(negedge clk);#1;reset=0;
 end
 always@(negedge clk)if(!reset||abort_stage==3)begin
  wall=wall+1;if(wall>3000)$fatal(1,"bounded Final watchdog");
  ce=wall%13<10;commit=wall%17!=5&&wall%17!=6;ready=wall%31>=10&&wall%7!=2;offer=sent<COUNT;
  if(scenario=="abort")begin
   if(abort_stage==0&&sent>=3&&shade&&dut.arithmetic_live)begin abort_stage=1;abort_age=0;end
   if(abort_stage==1)begin ce=0;commit=1;abort=1;abort_age=abort_age+1;if(abort_age==4)begin abort_stage=2;abort_age=0;abort=0;ce=1;end end
   else if(abort_stage==2)begin ce=1;abort=0;abort_age=abort_age+1;if(abort_age==4)begin abort_stage=3;abort_age=0;reset=1;end end
   else if(abort_stage==3)begin abort_age=abort_age+1;if(abort_age==3)begin abort_stage=4;reset=0;epoch=1;end end
  end
  if((scenario=="h"||scenario=="credit")&&!bad_started)begin
   ce=1;commit=1;offer=1;ready=1;bad_started=1;
   if(scenario=="h")force dut.in_h=9'd257;
   else begin force dut.q_count=3;force dut.arithmetic_live=0;end
   #2;$display("OBS t=%0t case=%0s bad=%b accept=%b advance=%b",$time,scenario,bad,in_accept,advance);
   if(CHECKED)begin if(!bad||in_accept||advance)$fatal(1,"checked rejection");end
   else if(bad||!advance||(scenario=="h"&&!in_accept))$fatal(1,"command diagnostic feedback");
  end
 end
 always@(posedge clk)begin
  if(reset)begin shade<=0;payload<=pixels[0];sent=0;received=0;full=0;overlap=0;direct=0;held=0;noborrow=0;blocked=0;end
  else begin
   if(advance)shade<=!shade;
   // Model the parent's READ_MODE0 replacement: SPEC consumes old Q on the
   // same edge that updates the source for the following phase0 admission.
   if(advance&&shade&&sent<COUNT)payload<=pixels[sent];
   if(scenario=="h"||scenario=="credit")begin
    if(bad_started)begin
    #2;
    if(CHECKED&&!fault)$fatal(1,"checked sticky missing");
    if(!CHECKED&&fault)$fatal(1,"command sticky diagnostic");
    $display("PASS FINAL diagnostic case=%0s checked=%0d monitors=%0d",scenario,CHECKED,MONITORS);$finish;
    end
   end else if(abort_stage==1||abort_stage==2)begin
    if(!fault||advance||out_valid||spec_ce||rop_ce)$fatal(1,"abort sticky/CE0");
   end else begin
    if(credits!=sent-received)$fatal(1,"external reservation conservation");
    if(out_valid&&(received>=sent||rgb!==gold[received]))$fatal(1,"RGB order/value index=%0d got=%h expected=%h",received,rgb,gold[received]);
    if(credits>=2&&ready&&offer&&!shade)begin noborrow=noborrow+1;if(in_accept)$fatal(1,"borrowed ACK credit");end
    if(in_accept)begin if(payload!==pixels[sent])$fatal(1,"input loan age");sent=sent+1;end
    if(out_fire)begin $display("RGB %0d %0d %h",epoch,received,rgb);received=received+1;end
    if(dut.q_count==2)full=full+1;if(in_accept&&out_fire)overlap=overlap+1;if(out_fire&&dut.q_count==0)direct=direct+1;if(out_valid&&!ready)blocked=blocked+1;
    $display("E %0d %0d %b%b%b%b%b%b%b%b%b %h %0d %0d",epoch,wall,shade,in_ready,proposal,in_accept,advance,out_valid,out_fire,bad,fault,out_valid?rgb:24'd0,credits,queued);
    snapshot=state_q;
    if(!advance)begin held=held+1;#1;if(state_q!==snapshot)$fatal(1,"held arithmetic");end
    if(received==COUNT)begin
     if(full==0||overlap==0||direct==0||held==0||noborrow==0||blocked==0||(scenario=="abort"&&abort_stage!=4))$fatal(1,"coverage full%0d overlap%0d direct%0d held%0d noborrow%0d blocked%0d",full,overlap,direct,held,noborrow,blocked);
     $display("PASS FINAL numeric count=%0d cycles=%0d epoch=%0d",received,wall,epoch);$finish;
    end
   end
  end
 end
endmodule
