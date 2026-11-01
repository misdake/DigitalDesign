`timescale 1ns/1ps
module tb;
 parameter NATIVE=1;
 reg clk=0,reset=1,ce=0,allow=0,enable=0,ready=0;always #5 clk=~clk;
 GSR GSR(.GSRI(1'b1));
 reg[71:0]stim[0:@LAST@];reg[75:0]members[0:@LAST@];
 reg[34:0]groups[0:@GLAST@];reg[77:0]work[0:@GLAST@];
 wire mv,mf,pf,advance,in_ready,proposal_ready,ack,front,next_valid,capture;
 wire[75:0]member;wire[34:0]payload;wire[14:0]next_key;
 wire[4:0]wc,pc,uw;wire[5:0]gc,rows,strobes;
 integer cycle=0,sent=0,returned=0,issued=0,written=0,got=0,acks=0;
 wire[71:0]s=sent<@COUNT@?stim[sent]:72'd0;
 integer holds=0,peeks=0,promotes=0,loads=0,paused=0,peak=0;
 reg[1:0]pipe=0;reg[34:0]held_head,held_q;reg hh=0,hq=0;
 wire iv=sent<@COUNT@&&wc<10&&cycle%5!=2;
 membership_leaf #(.SHORT_ALIGNMENT(1)) member_dut(
  .clk(clk),.reset(reset),.ce(advance),.in_valid(iv),
  .in_w0(s[8:0]),.in_w1(s[17:9]),.in_w2(s[26:18]),.in_w3(s[35:27]),
  .in_c0(s[45:36]),.in_c1(10'd0),.in_c2(s[55:46]),.in_c3(10'd0),
  .in_slot(s[59:56]),.in_level(s[63:60]),.in_key(s[69:64]),.in_fine(s[70]),.in_last_fine(s[71]),
  .out_valid(mv),.out_member(member),.fault(mf));
 gpu_v2_runtime_packet_backend_unique dut(
  .texture_base(16'h2000),.texture_mip(1'b@MIP@),.texture_fb400240(1'b@RTT@),
  .clk(clk),.reset(reset),.proposal_ce(ce),.commit_allow(allow),.abort(1'b0),.issue_enable(enable),
  .in_valid(mv),.in_member(member),.in_ready(in_ready),.proposal_ready(proposal_ready),.advance(advance),
  .out_ready(ready),.out_valid(front),.out_payload(payload),.next_valid(next_valid),.next_key(next_key),
  .capture_fire(capture),.work_ack(ack),.fault(pf),.work_count(wc),
  .producer_count(pc),.unwritten_count(uw),.group_count(gc),.row_count(rows),.pool_strobes(strobes));
 defparam dut.u_pool.NATIVE=NATIVE;
 always@(negedge clk)begin
  ce=!reset&&cycle%7!=2&&cycle%19!=6;allow=cycle%11!=3;
  enable=cycle%13!=5;ready=cycle>250&&cycle%17>4;
 end
 always@(posedge clk)begin
  cycle=cycle+1;if(cycle>30000)$fatal(1,"upstream watchdog");
  if(!reset)begin
   if(mf||pf)$fatal(1,"upstream fault");
   if(front&&next_valid)begin
    if(next_key!==groups[got+1][14:0])$fatal(1,"lookahead bits/order %0d",got);
    peeks=peeks+1;
   end
   if(hh&&(!front||payload!==held_head))$fatal(1,"head changed while held");
   if(hq&&dut.u_pool.data_do!==held_q)$fatal(1,"native Q changed without load");
   hh=front&&!(advance&&ready);held_head=payload;
   hq=!reset&&!dut.u_pool.load;held_q=dut.u_pool.data_do;
   // Q changes at this edge only for load, so sample its new value below.
   if(front&&!(advance&&ready))holds=holds+1;
   if(dut.u_pool.promote)promotes=promotes+1;if(dut.u_pool.load)loads=loads+1;
   if(!advance)paused=paused+1;if(rows>peak)peak=rows;
   if(advance)begin
    if(iv)sent=sent+1;
    if(mv)begin
     if(member!==members[returned])$fatal(1,"Member76 %0d %h %h",returned,member,members[returned]);
     returned=returned+1;
    end
    if(capture)begin
     if({dut.tap,dut.member}!==work[issued])$fatal(1,"Work metadata/mask/tap %0d",issued);
     issued=issued+1;
    end
    if(dut.packet_valid!==pipe[1])$fatal(1,"Packet publish edge");
    if(dut.packet_valid)begin
     if(dut.packet!==groups[written])$fatal(1,"Packet35 bits %0d %h %h",written,dut.packet,groups[written]);
     written=written+1;
    end
    pipe={pipe[0],capture};
    if(ack)acks=acks+1;
    if(front&&ready)begin
     if(payload!==groups[got])$fatal(1,"Group35 %0d",got);
     got=got+1;
    end
   end
   $display("TRACE %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %h %h",cycle,sent,returned,issued,written,got,wc,pc,gc,rows,strobes,front?payload:35'd0,next_valid?next_key:15'd0);
   if(got==@GROUPS@&&rows==0&&wc==0&&pc==0&&!mv)begin
    if(sent!=@COUNT@||returned!=sent||acks!=sent||issued!=got||written!=got||uw||front||next_valid||pipe||
       holds<100||peeks<100||promotes!=got||loads!=got||paused<100||peak!=48)$fatal(1,"upstream coverage/conservation");
    $display("PASS UPSTREAM rtt=@RTT@ native=%0d members=%0d groups=%0d cycles=%0d hold=%0d peek=%0d",NATIVE,sent,got,cycle,holds,peeks);$finish;
   end
  end
  #1;held_q=dut.u_pool.data_do;
 end
 initial begin
  $readmemh("stim.hex",stim);$readmemh("members.hex",members);$readmemh("groups.hex",groups);$readmemh("work.hex",work);
  repeat(4)@(negedge clk);reset=0;
 end
endmodule
