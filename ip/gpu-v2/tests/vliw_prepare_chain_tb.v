`timescale 1ns/1ps
module tb;
`ifndef PORTABLE
GSR GSR(.GSRI(1'b1));
`endif
reg clk=0;always #5 clk=~clk;
reg reset=1,ce=0,start=0,raw_valid=0;integer tick=0,wall=0,p=0;
always @(negedge clk)begin tick=tick+1;ce=!reset&&(tick%11!=3&&tick%11!=4);end
reg[35:0]descriptor=0;reg[4:0]raw_index=0;reg[53:0]raw_data=0;
wire ready,sbusy,sdone,run_enable;wire[16:0]ie;wire[3:0]re;
wire gwe,bvalid,bempty;wire[3:0]gwa;wire[29:0]gwd;wire[4:0]xl,xh;wire[3:0]yl,yh;wire[1:0]phase;
wire rb,rd;
gpu_v2_vliw_interpolator I(.clk(clk),.reset(reset),.ce(ce),.prepare_start(start),.descriptor(descriptor),.raw_valid(raw_valid),.raw_ready(ready),.raw_index(raw_index),.raw_data(raw_data),
.cfg_far_z(32'd13041664),.cfg_tex_u(5'd6),.cfg_tex_v(5'd6),.cfg_max_mip(5'd6),.cfg_bias(8'sd0),
.table_valid(1'b0),.table_index(8'd0),.table_data(36'd0),.quad_valid(1'b0),.quad_x(8'd0),.quad_y(7'd0),
.sample_valid(1'b0),.sample_lane(2'd0),.sample_cover(1'b0),.sample_anchor(1'b0),.sample_h(30'd0),.quad_end_valid(1'b0),.quad_mask(4'd0),.pool_ready(1'b1),.pool_quad_id(3'd0),.s_busy(sbusy),.s_done(sdone),.error(ie),
.geo_we(gwe),.geo_wa(gwa),.geo_wd(gwd),.bound_valid(bvalid),.bound_empty(bempty),.bound_xlo(xl),.bound_xhi(xh),.bound_ylo(yl),.bound_yhi(yh),.phase(phase),.run_enable(run_enable));
rc_raster R(.clk(clk),.reset(reset),.ce(ce),.start(start),.geo_we(gwe),.geo_wa(gwa),.geo_wd(gwd),.bbox_done(bvalid),.bbox_empty(bempty),.bbox_xlo(xl),.bbox_xhi(xh),.bbox_ylo(yl),.bbox_yhi(yh),.run_enable(run_enable),.phase(phase),.quad_ready(1'b0),.busy(rb),.done(rd),.error(re));
reg[35:0]descs[0:@LAST@];reg[53:0]words[0:@WLAST@];
reg[53:0]params[0:@N@*6-1];
 wire[50:0]stored_param=I.prepare.stream_param_data;
 wire[53:0]expanded_param={stored_param[50],stored_param[50:34],stored_param[33],stored_param[33:17],stored_param[16],stored_param[16:0]};
reg signed[63:0]aa[0:2],bb[0:2],cc[0:2],expected[0:2],tmp;
reg[29:0]shadow[0:2];reg seen_node=0,seen_bound=0,empty_seen=0;reg[3:0]saved_level=0;
integer root,ox,oy,diff,j,e,origin_writes=0,roots9=0,nodes=0,early_cancel=0,zero_slots=0,ce0=0,resetcuts=0,fd;
reg[255:0]held;
reg[90:0]held_attribute;
reg[47:0]packed_expected;
reg[31:0]uv_word;
reg[17:0]uv_expected;
integer param_writes=0,normal_writes=0,attribute_writes=0,packed_reads=0,attr0_takes=0,attr1_takes=0,param_row,uv_writes=0,uv_v,uv_row;
always @(posedge clk)begin
 wall=wall+1;if(wall>80000)$fatal(1,"bounded root test");
 if(reset)begin seen_node=0;seen_bound=0;empty_seen=0;saved_level<=0;
  param_writes=0;normal_writes=0;attribute_writes=0;packed_reads=0;attr0_takes=0;attr1_takes=0;uv_writes=0;
  for(j=0;j<3;j=j+1)begin aa[j]=0;bb[j]=0;cc[j]=0;shadow[j]=0;end end
 else if(ce)begin
  if(ie||re)$fatal(1,"diagnostic packet %0d I%h R%h",p,ie,re);
  if(I.prepare.packed_write)begin
   if(raw_index[0])begin
    if(descriptor[23])packed_expected={8'd0,raw_data[23:16],8'd0,raw_data[15:8],8'd0,raw_data[7:0]};
    else packed_expected={16'd0,raw_data[31:0]};
    if(I.prepare.pw_high_wa!=8+(raw_index-11)/2)$fatal(1,"attribute row");
    attribute_writes=attribute_writes+1;
   end else begin
    packed_expected={{4{raw_data[35]}},raw_data[35:24],{4{raw_data[23]}},raw_data[23:12],{4{raw_data[11]}},raw_data[11:0]};
    if(I.prepare.pw_high_wa!=12+(raw_index-10)/2)$fatal(1,"normal row");
    normal_writes=normal_writes+1;
   end
   if(!ready||I.prepare.pw_high_wd!=={{6{packed_expected[47]}},packed_expected})$fatal(1,"packed direct write");
  end
  if(I.prepare.state==8&&I.prepare.core_c_re&&(I.prepare.core_cr>=12||(I.prepare.core_cr>=8&&I.prepare.core_cr<=10)))packed_reads=packed_reads+1;
  if(I.prepare.core_attr0_take)attr0_takes=attr0_takes+1;
  if(I.prepare.core_attr1_take)attr1_takes=attr1_takes+1;
  if(I.prepare.uv_write)begin
   uv_v=I.prepare.uv_vertex;uv_word=words[p*16+11+2*uv_v];
   if(I.prepare.factor_index==3)begin uv_row=1+uv_v;uv_expected={2'd0,uv_word[15:0]};end
   else begin uv_row=4+uv_v;uv_expected={2'd0,uv_word[31:16]};end
   if(!I.prepare.operand_we||I.prepare.operand_wa!=uv_row||I.prepare.operand_wd!==uv_expected)$fatal(1,"UV PA prefetch write");
   uv_writes=uv_writes+1;
  end
  if(I.prepare.stream_param_valid)begin
   param_row={I.prepare.stream_param_index,I.prepare.stream_param_bank};
   if(param_row!=param_writes||expanded_param!==params[p*6+param_row])
    $fatal(1,"parameter mismatch packet%0d row%0d got%h want%h",p,param_row,expanded_param,params[p*6+param_row]);
   param_writes=param_writes+1;
   $fdisplay(fd,"P %0d %0d %h",p,param_row,expanded_param);
  end
  if(gwe)begin
   e=gwa[1:0];case(gwa[3:2])0:aa[e]=$signed(gwd);1:bb[e]=$signed(gwd);2:begin cc[e]=$signed(gwd);shadow[e]=gwd;end default:$fatal(1,"geo row");endcase
  end
  if(bvalid)begin
   seen_bound=1;empty_seen=bempty;
   if(bempty&&R.w_pc<6)early_cancel=early_cancel+1;
   diff=(xl^xh)|(yl^yh);root=4;while(diff!=0)begin root=root+1;diff=diff>>1;end
   ox=(xl>>(root-4))<<root;oy=(yl>>(root-4))<<root;
   for(j=0;j<3;j=j+1)expected[j]=cc[j]+aa[j]*ox+bb[j]*oy;
  end
  if(R.rf_we&&R.rf_wa[3:2]==2&&!gwe&&!seen_node)begin
   e=R.rf_wa[1:0];
   if(!(R.r_pc==14||R.r_pc==15))$fatal(1,"unexpected root writer");
   if(R.work_q!==shadow[e])$fatal(1,"RF old Q packet%0d edge%0d",p,e);
   if(R.r_pc==14)tmp=aa[e];else tmp=bb[e];
   tmp=shadow[e]+(tmp<<saved_level);
   if(R.rf_wd!==tmp[29:0])$fatal(1,"origin add packet%0d edge%0d got%h want%h",p,e,R.rf_wd,tmp[29:0]);
   shadow[e]=tmp[29:0];origin_writes=origin_writes+1;
  end
  if(R.micro_issue&&R.w_pc==10&&R.w_edge==0&&!seen_node)begin
   if(!seen_bound||empty_seen)$fatal(1,"node before bound/empty");
   if(R.level!=root||R.nx!=ox||R.ny!=oy)$fatal(1,"root coordinates p%0d",p);
   for(j=0;j<3;j=j+1)if(shadow[j]!==expected[j][29:0])$fatal(1,"root formula p%0d edge%0d got%h expected%h",p,j,shadow[j],expected[j][29:0]);
   seen_node=1;nodes=nodes+1;if(root==9)roots9=roots9+1;
   $fdisplay(fd,"H %0d %0d %0d %0d %h %h %h",p,root,ox,oy,shadow[0],shadow[1],shadow[2]);
  end
  if(R.fast_w&&R.origin_op)zero_slots=zero_slots+1;
  if(R.micro_issue)saved_level<=R.level;
 end
 if(!reset&&!ce)begin
  ce0=ce0+1;held={R.life,R.w_pc,R.w_edge,R.level,R.rom_pc,R.rom_valid,R.r_valid,R.r_pc,R.r_edge,R.work_q,R.step_q,R.nx,R.ny};
  held_attribute={I.prepare.attribute_base,I.prepare.difference0,I.prepare.core.pc,I.compute.lod.pw_acc_read_address,I.compute.lod.pw_q[83:36]};
  #1;if(held!=={R.life,R.w_pc,R.w_edge,R.level,R.rom_pc,R.rom_valid,R.r_valid,R.r_pc,R.r_edge,R.work_q,R.step_q,R.nx,R.ny})$fatal(1,"CE hold");
  if(held_attribute!=={I.prepare.attribute_base,I.prepare.difference0,I.prepare.core.pc,I.compute.lod.pw_acc_read_address,I.compute.lod.pw_q[83:36]})$fatal(1,"attribute prefetch CE hold");
 end
end
task drive;input integer cutoff;integer wi,taken,n;begin: body
 descriptor=descs[p];start=1;@(posedge clk);while(!ce)@(posedge clk);@(negedge clk);start=0;
 wi=0;n=0;raw_valid=1;raw_index=0;raw_data=words[p*16];
 while(wi<16||sbusy||!(seen_node||(empty_seen&&!rb)))begin
  @(posedge clk);taken=ready&&ce;if(ce)n=n+1;
  @(negedge clk);
  if(cutoff!=0&&n>=cutoff)begin raw_valid=0;resetcuts=resetcuts+1;disable body;end
  if(taken&&wi<16)begin wi=wi+1;if(wi<16)begin raw_index=wi;raw_data=words[p*16+wi];end else raw_valid=0;end
  if(n>1200)$fatal(1,"per packet bound %0d",p);
 end
 if(normal_writes!=3||attribute_writes!=3||attr0_takes!=param_writes||attr1_takes!=param_writes||packed_reads!=3*param_writes)
  $fatal(1,"packed read/write conservation p%0d NW%0d AW%0d R%0d P%0d",p,normal_writes,attribute_writes,packed_reads,param_writes);
 if(uv_writes!=((descriptor[23]||param_writes<4)?0:(param_writes==4?3:6)))$fatal(1,"UV prefetch count");
 if(!empty_seen&&param_writes!=6)$fatal(1,"incomplete nonempty parameters");
end endtask
initial begin
 fd=$fopen("roots.txt","w");$readmemh("descriptors.hex",descs);$readmemh("words.hex",words);$readmemh("params.hex",params);
 for(p=0;p<@N@;p=p+1)begin
  reset=1;raw_valid=0;start=0;repeat(2)@(negedge clk);reset=0;
  if(p<24)begin drive(5+(p%8)*20);reset=1;raw_valid=0;start=0;repeat(2)@(negedge clk);reset=0;end
  drive(0);
 end
 if(nodes==0||roots9==0||origin_writes==0||early_cancel==0||zero_slots==0||ce0==0||resetcuts==0)$fatal(1,"required boundary not exercised");
 $display("PASS RROOT packets=@N@ nodes=%0d originwrites=%0d roots9=%0d earlycancel=%0d zeros=%0d ce0=%0d cuts=%0d wall=%0d",nodes,origin_writes,roots9,early_cancel,zero_slots,ce0,resetcuts,wall);$finish;
end
initial begin #800100;$fatal(1,"absolute bound");end
endmodule
