`timescale 1ns/1ps
module tb;
`ifdef NATIVE
GSR GSR(.GSRI(1'b1));
`endif
reg clk=0;always #5 clk=~clk;
reg reset=1,proposal_ce=0,commit_allow=0,publish_valid=0,in_valid=0,out_ready=1;
reg[3:0]check_quad=0,publish_quad=0,publish_mask=1;
reg[71:0]in_payload=0;reg[63:0]in_texels=0;
wire public_free,publish_ready,publish_fire,in_ready,input_fire,out_valid,quad_last,result_fire,eligible,advance,preflight_fault,fault;
wire[5:0]out_key;wire[23:0]out_rgb;wire[15:0]public_live;
gpu_v2_texture_runtime_color dut(.*);
reg public_upper[0:15];reg publish_upper=0;
wire[6:0]write_address={public_upper[out_key[5:2]],out_key};
reg read_en=0;reg[6:0]read_address=0;wire[23:0]return_data;wire collision;
gpu_v2_postquad_sample_bank sample_bank(.clk(clk),.reset(reset),.read_en(read_en),.write_en(result_fire),.read_address(read_address),.write_address(write_address),.write_data(out_rgb),.return_data(return_data),.collision(collision));
integer cycle=0,fd,got=0,releases=0,expected_count=0,ce0=0,commithold=0,q,l,g,t,c,i,k,groups,w,kind=0;
reg edge_input,edge_publish;reg[23:0]expected[0:127];reg[6:0]expected_addr[0:127];reg[3:0]masks[0:31];
reg[15:0]tex;integer accum[0:2],col,term;
function[3:0]mask;input integer n;begin case(n%6)0:mask=1;1:mask=10;2:mask=5;3:mask=15;4:mask=8;5:mask=2;endcase end endfunction
function integer highest;input[3:0]m;begin if(m[3])highest=3;else if(m[2])highest=2;else if(m[1])highest=1;else highest=0;end endfunction
function[15:0]texel;input integer n,lane,group,tap;begin texel=(n*7919+lane*997+group*109+tap*12347)^16'ha159;end endfunction
always @(negedge clk)if(!reset&&kind==0)begin proposal_ce=(cycle%23<15);commit_allow=(cycle%17<13);end
always @(posedge clk)begin
 cycle=cycle+1;if(cycle>10000)$fatal(1,"Color ownership watchdog");
 edge_input=input_fire;edge_publish=publish_fire;
 if(!reset)begin
  if(!proposal_ce)ce0=ce0+1;if(proposal_ce&&!commit_allow)commithold=commithold+1;
  $fdisplay(fd,"%h %h %h %h %h %h %h %h %h %h",{proposal_ce,commit_allow,publish_valid,publish_quad,publish_mask,publish_ready,publish_fire,in_valid,in_ready,input_fire},in_valid?in_payload:72'd0,in_valid?in_texels:64'd0,{out_valid,quad_last,result_fire,advance,eligible,preflight_fault,fault},out_valid?{out_key,out_rgb}:30'd0,public_live,{read_en,read_address,collision},result_fire?write_address:7'd0,read_en?return_data:24'd0,{dut.color.busy,dut.color.done,dut.color.q_done});
  if(kind==0)begin
   if(fault||preflight_fault||collision)$fatal(1,"Color legal protocol");
   if(publish_fire)public_upper[publish_quad]<=publish_upper;
   if(result_fire)begin
    if(got>=expected_count||write_address!==expected_addr[got]||out_rgb!==expected[write_address])$fatal(1,"Color bank gold got%0d addr%0d expected%0d RGB%h gold%h",got,write_address,expected_addr[got],out_rgb,expected[write_address]);
    if(!public_live[out_key[5:2]])$fatal(1,"Color premature public ID release");
    if(quad_last!==(out_key[1:0]==highest(mask(write_address>>2))))$fatal(1,"Color last covered lane");
    if(quad_last)begin
     if(public_free||publish_ready)$fatal(1,"Color borrowed same-edge public credit");
     releases=releases+1;
    end
    got=got+1;
   end
  end
 end
end
task tick;begin @(posedge clk);#2;@(negedge clk);#1;end endtask
task publish;input integer n;begin
 publish_quad=n%16;check_quad=n%16;publish_upper=(n/16);publish_mask=mask(n);publish_valid=1;
 tick;while(!edge_publish)tick;
 publish_valid=0;
 if(!public_live[n%16]||public_free||publish_ready)$fatal(1,"Color missing public ownership");
end endtask
task offer;input integer n,lane,group;begin
 in_payload=0;in_payload[7:4]=5;in_payload[69:66]=n%16;in_payload[71:70]=lane;
 in_payload[64]=(group==0);in_payload[65]=(group==((n%2)?1:0));
 w=(n%2)?63:127;
 for(t=0;t<4;t=t+1)begin in_payload[28+9*t+:9]=w;in_texels[16*t+:16]=texel(n,lane,group,t);end
 in_valid=1;tick;while(!edge_input)tick;in_valid=0;
end endtask
initial begin
 fd=$fopen("edges.trace","w");if($value$plusargs("KIND=%d",kind))begin end
 for(q=0;q<32;q=q+1)begin
  masks[q]=mask(q);groups=(q%2)?2:1;
  for(l=0;l<4;l=l+1)if(masks[q][l])begin
   for(c=0;c<3;c=c+1)begin
    accum[c]=0;w=(q%2)?63:127;
    for(g=0;g<groups;g=g+1)for(t=0;t<4;t=t+1)begin
     tex=texel(q,l,g,t);
     case(c)0:col={tex[15:11],tex[15:13]};1:col={tex[10:5],tex[10:9]};2:col={tex[4:0],tex[4:2]};endcase
     term=(w*col+256)/512;accum[c]=accum[c]+term;
    end
   end
   expected[{q[4:0],l[1:0]}]={accum[2][7:0],accum[1][7:0],accum[0][7:0]};expected_addr[expected_count]={q[4:0],l[1:0]};expected_count=expected_count+1;
  end
 end
 repeat(3)tick;reset=0;proposal_ce=1;commit_allow=1;tick;
 if(kind==0)begin
  for(q=0;q<16;q=q+1)publish(q);
  if(public_live!==16'hffff)$fatal(1,"Color capacity16");
  for(q=0;q<32;q=q+1)begin
   if(q>=16)publish(q);
   check_quad=q%16;publish_quad=q%16;publish_mask=mask(q);
   for(l=0;l<4;l=l+1)if(masks[q][l])for(g=0;g<((q%2)?2:1);g=g+1)offer(q,l,g);
   while(public_live[q%16])tick;
   if(!public_free||!publish_ready)$fatal(1,"Color final bank write failed to free ID");
  end
  repeat(8)tick;
  if(got!=expected_count||releases!=32||public_live||ce0<10||commithold<10)$fatal(1,"Color ownership coverage");
  for(i=0;i<expected_count;i=i+1)begin
   read_address=expected_addr[i];read_en=1;tick;
   if(return_data!==expected[read_address])$fatal(1,"Color physical Sample bank contents");
   read_en=0;read_address=~read_address;tick;
   if(return_data!==expected[expected_addr[i]])$fatal(1,"Color Sample bank CE0 Q");
  end
  $display("PASS Color ownership pixels=%0d releases=%0d CE0=%0d commit_hold=%0d",got,releases,ce0,commithold);$finish;
 end
 // Reserved source suppresses only duplicate-publication checking. An input
 // without a live public owner remains an actual wrapper diagnostic proposal.
 if(kind==1)begin
  in_payload[7:4]=5;in_valid=1;tick;#1;
  if(!dut.color_ready)$fatal(1,"Color diagnostic missing accepted half");
  tick;
`ifdef COMMAND
  if(fault||preflight_fault||public_live)$fatal(1,"Color command diagnostic feedback");
`else
  if(!fault)$fatal(1,"Color checked wrapper failed sticky terminal");
`endif
  $display("PASS Color wrapper diagnostic");$finish;
 end
 if(kind==3)begin
  publish(0);offer(0,0,0);out_ready=0;repeat(8)tick;
  if(fault||preflight_fault||public_live!==16'h0001||dut.color.q_done)$fatal(1,"Color result rejection lifetime");
  $display("PASS Color monitor-free closed result contract");$finish;
 end
 // A bad direct packet is functional, including with command monitors removed.
 publish(0);in_payload=0;in_payload[7:4]=11;in_valid=1;tick;
 `ifdef PHYSICAL_GROUPS
 if(dut.color.packet_bad||dut.color.preflight_bad||!dut.color.advance)$fatal(1,"Color physical Group35 contract");
 tick;
 if(fault||dut.color.busy||!dut.color.done)$fatal(1,"Color physical accepted group");
 $display("PASS Color physical no-level group");$finish;
`endif
 if(!dut.color.packet_bad||!dut.color.preflight_bad||dut.color.advance)$fatal(1,"Color packet functional gate removed");
 tick;
`ifdef COMMAND
 if(fault||!dut.color.busy||dut.color.advance||dut.color.done)$fatal(1,"Color command bad-packet branch advanced");
`else
 if(!fault)$fatal(1,"Color checked packet failed sticky terminal");
`endif
 $display("PASS Color packet functional branch");$finish;
end
endmodule
