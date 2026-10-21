// 固定quad日历小核：四条微指令依次为Ux/Uy/Vx/Vy；无就绪队列或端口仲裁。
// quad_end后16/20/24/28CE读G及anchor；33CE按完成编号读公共offset。
module rc_lod(
 input clk,reset,ce,input[1:0]phase,
 input read_bank,write_bank,
 input coefficient_bwe,coefficient_cwe,input[3:0]coefficient_bwa,coefficient_cwa,
 input[35:0]coefficient_bwd,coefficient_cwd,
 input cfg_valid,
 input[4:0]cfg_tex_u,cfg_tex_v,cfg_max_mip,input signed[7:0]cfg_bias,input cfg_rgb,
 input quad_end_valid,input[2:0]quad_end_qid,input[3:0]quad_end_mask,
 input reciprocal_valid,input[4:0]reciprocal_pid,input reciprocal_anchor,input[12:0]reciprocal_log_q8,
 input tap_valid,input[4:0]tap_pid,input[1:0]tap_index,input tap_anchor,input[17:0]tap_a,tap_b,
 output operand_we,output[3:0]operand_wa,output[17:0]operand_wd,
 output operand_re,output[3:0]operand_ra,input[17:0]operand_value,
 output wide_valid,output signed[35:0]wide_b,output signed[53:0]wide_c,output[4:0]wide_tag,
 input wide_result_valid,input[4:0]wide_result_tag,input signed[53:0]wide_result_data,
 output norm_input_valid,output[35:0]norm_input_value,
 input norm_valid,norm_zero,input[5:0]norm_exp,norm_index,input norm_pending,
 output log_valid,output[5:0]log_index,output[4:0]log_tag,
 input log_result_valid,input[4:0]log_result_tag,input[7:0]log_result_data,
 output reg lod_valid,output reg[2:0]lod_id,output reg[7:0]lod_value,
 output busy,output reg[3:0]error
);
 reg arrival,arrival_run;
 reg[2:0]calendar_valid,calendar_run;
 reg kernel_valid,kernel_run,issue_live;reg[2:0]kernel_id;reg[1:0]axis;
 reg[4:0]issue_tag;
 reg[1:0]finish_valid,finish_run;reg[2:0]result_id;
 reg finish_pending,pending_run;reg signed[15:0]finish_offset;
 reg signed[15:0]bias_base;reg[4:0]tex_u,tex_v,max_mip;reg rgb;
 // 系数指数Nc与覆盖D的E相同，公共校正不再减E。
 // quad公共校正在倒数产生时形成，结束前按完成编号只读一次。
 reg[4:0]log_context;
 reg sampled_zero;reg[5:0]sampled_exp;reg[14:0]maximum;reg have_gradient;
 assign busy=arrival||(|calendar_valid)||kernel_valid||issue_live||(|finish_valid)||
             norm_pending||wide_result_valid||log_result_valid||finish_pending;
 wire cfg_take=cfg_valid&&!reset;

 // 数据只驻留两个anchor页，页号来自quad地址最低位；U/V分别占一行。
 assign operand_we=tap_valid&&tap_anchor&&!rgb&&(tap_index==1||tap_index==2)&&!reset;
 assign operand_wa={2'b01,tap_pid[2],tap_index==2};
 assign operand_wd=tap_index==1 ? tap_b : tap_a;
 wire read_pending=kernel_valid&&kernel_run;
 wire issue_read=phase==2&&read_pending&&!reset;
 assign operand_re=issue_read;
 assign operand_ra={2'b01,kernel_id[0],axis[1]};
 wire meta_write=reciprocal_valid&&reciprocal_anchor&&!rgb;
 wire signed[15:0]quad_offset=bias_base-$signed({3'd0,reciprocal_log_q8});
 wire z_read=phase==0&&!reset;
 // 最后一项log返回前的phi3，预读正在完成quad的公共offset。
 wire meta_read=phase==3&&!reset&&finish_valid[1]&&finish_run[1];
 wire[2:0]meta_id=result_id;
 wire pw_re=z_read||meta_read||issue_read;
 wire[3:0]pw_ra=phase==0 ? 4'd4 : phase==3 ? {1'b1,meta_id} : {2'd0,axis};
 // Only final S20/G36/Z31 coefficients cross from prepare to runtime.
 // Each half uses inactive rows in one 16-word 1W1R bank; the offset bank
 // retains its independent writer, so no simultaneous write is arbitrated.
 reg[3:0]pw_read_address,pw_acc_read_address;
 reg[2:0]offset_read_address;wire[15:0]offset_q;
 wire[35:0]coefficient_b_q,coefficient_c_q;

 always @(posedge clk)begin
  if(reset)begin pw_read_address<=0;pw_acc_read_address<=0;offset_read_address<=0;end
  else if(ce)begin
   if(z_read||issue_read)begin
    pw_read_address<={read_bank,z_read?3'd4:{2'd0,axis[0]}};
    pw_acc_read_address<={read_bank,z_read?3'd4:{1'b0,axis}};
   end
   if(meta_read)offset_read_address<=meta_id;
  end
 end
 iz_params_async #(.WIDTH(16)) parameters_offset(clk,reset,ce,meta_write,
  {1'b0,reciprocal_pid[4:2]},quad_offset,{1'b0,offset_read_address},offset_q);
 iz_params_async #(.WIDTH(36)) coefficients_b(clk,reset,ce,coefficient_bwe,
  coefficient_bwa,coefficient_bwd,pw_read_address,coefficient_b_q);
 iz_params_async #(.WIDTH(36),.PC_CONSTANTS(2)) coefficients_c(clk,reset,ce,coefficient_cwe,
  coefficient_cwa,coefficient_cwd,pw_acc_read_address,coefficient_c_q);
 assign wide_b=coefficient_b_q;
 assign wide_c={{18{coefficient_c_q[35]}},coefficient_c_q};
 assign wide_valid=issue_live&&phase==3&&!reset;assign wide_tag=issue_tag;
 assign norm_input_valid=wide_result_valid;assign norm_input_value=wide_result_data[35:0];
 // 导数晚两CE发射，直接在phi3消费两CE归一化返回，删除旧lookup等待寄存。
 assign log_valid=norm_valid&&phase==3&&!reset;assign log_index=norm_index;assign log_tag=log_context;
 wire[4:0]texture_exp=log_result_tag[1] ? tex_v : tex_u;
 wire[6:0]score_exp={1'b0,sampled_exp}+{2'd0,texture_exp};
 wire[14:0]score={score_exp,log_result_data};
 reg[14:0]next_max;reg next_have;
 wire first=log_result_tag[1:0]==0;
 wire take=!sampled_zero&&(first||!have_gradient||score>maximum);
 always @*begin
  next_have=!sampled_zero||(!first&&have_gradient);
  if(take)next_max=score;else if(first)next_max=0;else next_max=maximum;
 end
 wire signed[15:0]final_lod=$signed({1'b0,maximum})+finish_offset;
 wire signed[15:0]max_level=$signed({3'd0,max_mip,8'd0});
 always @(posedge clk)begin
  if(reset)begin
   arrival<=0;arrival_run<=0;calendar_valid<=0;calendar_run<=0;
   kernel_valid<=0;kernel_run<=0;kernel_id<=0;axis<=0;issue_live<=0;issue_tag<=0;
   finish_valid<=0;finish_run<=0;result_id<=0;finish_pending<=0;pending_run<=0;finish_offset<=0;
   bias_base<=-16'sd3712;tex_u<=0;tex_v<=0;max_mip<=0;rgb<=0;
   log_context<=0;
   sampled_zero<=1;sampled_exp<=0;
   maximum<=0;have_gradient<=0;lod_valid<=0;lod_id<=0;lod_value<=0;error<=0;
  end else if(ce)begin
   lod_valid<=0;finish_pending<=0;
   // q34只归约score并捕获旧Q的offset；q35独立做公共校正与格式化。
   if(finish_pending)begin
    lod_valid<=1;lod_id<=result_id;result_id<=result_id+1'b1;
    if(!pending_run||!have_gradient||final_lod<0)lod_value<=0;
    else if(final_lod>max_level)lod_value<={max_mip,3'b0};else lod_value<=final_lod[12:5];
   end
   if(wide_result_valid)log_context<=wide_result_tag;
   if(cfg_take)begin
    bias_base<=-16'sd3712+$signed({{4{cfg_bias[7]}},cfg_bias,4'd0});
    tex_u<=cfg_tex_u;tex_v<=cfg_tex_v;max_mip<=cfg_max_mip;rgb<=cfg_rgb;
   end
   if(phase==2)begin
    arrival<=quad_end_valid&&!rgb;arrival_run<=quad_end_mask!=0;
    // RGB没有导数运算，按配置静态旁路；不让它等待不存在的四条微指令。
    if(quad_end_valid&&rgb)begin
     lod_valid<=1;lod_id<=result_id;lod_value<=0;
     kernel_id<=kernel_id+1'b1;result_id<=result_id+1'b1;
    end
   end
   if(phase==3)begin
    calendar_valid<={calendar_valid[1:0],arrival};calendar_run<={calendar_run[1:0],arrival_run};
    arrival<=0;
    if(calendar_valid[2])begin kernel_valid<=1;kernel_run<=calendar_run[2];axis<=0;end
   end
   if(phase==0)begin

    finish_valid<={finish_valid[0],kernel_valid&&axis==3};finish_run<={finish_run[0],kernel_run};
    if(finish_valid[1])begin
     finish_pending<=1;pending_run<=finish_run[1];finish_offset<=offset_q;
    end
   end
   if(phase==2)begin
    issue_live<=issue_read;issue_tag<={kernel_id,axis};
    if(kernel_valid)begin
     if(axis==3)begin kernel_valid<=0;kernel_id<=kernel_id+1'b1;end
     else axis<=axis+1'b1;
    end
   end
   if(wide_valid)issue_live<=0;
   if(log_valid)begin sampled_zero<=norm_zero;sampled_exp<=norm_exp;end
   if(log_result_valid)begin maximum<=next_max;have_gradient<=next_have;end
   if(quad_end_valid&&phase!=2)error[0]<=1;
  end
 end
`ifndef SYNTHESIS
 always @(posedge clk)if(ce&&!reset&&cfg_valid&&busy)$fatal(1,"固定配置提交时LOD未排空");
 reg[2:0]end_id;
 reg[2:0]anchor_stamp[0:3];reg[3:0]anchor_known;
 reg[2:0]dlog_stamp[0:7];reg[7:0]dlog_known;
 integer sim_i;
 always @(posedge clk)begin
  if(reset)begin
   end_id<=0;anchor_known<=0;dlog_known<=0;
   for(sim_i=0;sim_i<4;sim_i=sim_i+1)anchor_stamp[sim_i]<=0;
   for(sim_i=0;sim_i<8;sim_i=sim_i+1)dlog_stamp[sim_i]<=0;
  end else if(ce)begin
   if(quad_end_valid)begin
    if(quad_end_qid!=end_id)$fatal(1,"固定LOD地址次序错误");
    end_id<=end_id+1'b1;
   end
   if(operand_we)begin anchor_known[operand_wa[1:0]]<=1;anchor_stamp[operand_wa[1:0]]<=tap_pid[4:2];end
   if(meta_write)begin dlog_known[reciprocal_pid[4:2]]<=1;dlog_stamp[reciprocal_pid[4:2]]<=reciprocal_pid[4:2];end
   if(issue_read&&(!anchor_known[operand_ra[1:0]]||anchor_stamp[operand_ra[1:0]]!=kernel_id))
    $fatal(1,"anchor未产生或被后续quad覆盖");
   if(issue_read&&operand_we&&operand_ra==operand_wa)$fatal(1,"anchor读写同址");
   if(meta_read&&(!dlog_known[meta_id]||dlog_stamp[meta_id]!=meta_id))$fatal(1,"Dlog未产生");
  end
 end
 always @(posedge clk)if(ce&&!reset)begin
  if(wide_result_valid&&wide_result_data[53:35]!={19{wide_result_data[35]}})$fatal(1,"LOD导数溢出signed36");
  if(phase==3&&calendar_valid[2]&&kernel_valid)$fatal(1,"固定LOD核重叠：输入quad小于16CE");
  if(norm_valid&&phase!=3)$fatal(1,"LOD归一化未落在phi1");
  if(log_result_valid&&phase!=0)$fatal(1,"LOD查表未落在phi0");
  if(phase==0&&finish_valid[1]&&finish_run[1]&&(!log_result_valid||log_result_tag!={result_id,2'd3}))
   $fatal(1,"LOD完成日历与算术流水不一致");
 end
`endif
endmodule
