// h/Z按phi0/1/2/3共用30位移位；Z提前两CE发射，窄结果静态保持到原输出沿。
// 标量sample_h按phi0/1/2依次消费；cover/anchor仅phi2有效。
module rc_compute(
 input clk,reset,ce,output reg[1:0]phase,
 input read_bank,write_bank,
 input coefficient_bwe,coefficient_cwe,input[3:0]coefficient_bwa,coefficient_cwa,
 input[35:0]coefficient_bwd,coefficient_cwd,
 input sample_valid,input[4:0]sample_pid,input sample_cover,sample_anchor,input[29:0]sample_h,
 input cfg_valid,input signed[7:0]cfg_z_bias,input[31:0]cfg_far,input cfg_rgb,
 input[4:0]cfg_shift0,cfg_shift1,cfg_shift2,
 input param_valid,input[1:0]param_index,input param_bank,input[50:0]param_data,
 input[4:0]cfg_tex_u,cfg_tex_v,cfg_max_mip,input signed[7:0]cfg_bias,
 input quad_end_valid,input[2:0]quad_end_qid,input[3:0]quad_end_mask,
 output result_we,output[6:0]result_wa,output reg[35:0]result_wd,
 output depth_event_valid,output[4:0]depth_event_pid,output[15:0]depth_event_z16,output depth_event_pass,
 output lod_valid,output[2:0]lod_id,output[7:0]lod_value,
 output tap_valid,output[4:0]tap_pid,output[1:0]tap_index,output[17:0]tap_a,tap_b,
 output busy,output[7:0]error
);
 reg[3:0]valid_pipe,cover_pipe,anchor_pipe;
 reg[4:0]pid0,pid1,pid2,pid3;
 reg signed[7:0]z_bias;reg[31:0]far_z;reg rgb;
 reg[4:0]shift0,shift1,shift2;
 reg[31:0]d_acc;reg d_zero;
 reg[8:0]fraction;reg[17:0]seed,r,weight0,weight1;reg[16:0]clipped0;
 reg signed[8:0]z_shift_early,z_shift;reg z_dzero_early,z_dzero;
 reg[15:0]z_new,z_early;reg z_new_pass,z_early_pass;reg[3:0]compute_error;
 wire lod_busy;wire[3:0]lod_error;
 assign busy=(|valid_pipe)||lod_busy;
 assign error={lod_error,compute_error};
 wire launch=sample_valid&&phase==0&&!reset;

 wire signed[53:0]wide_result;
 wire[5:0]output_shift=z_shift[5:0]-6'd15;
 wire[29:0]raw_h=sample_h;
 reg[4:0]align_shift;
 always @*begin
  case(phase)
   0:align_shift=shift0;
   1:align_shift=shift1;
   2:align_shift=shift2;
   default:align_shift=output_shift[4:0];
  endcase
 end
 // cover到phi2才成立：前两项原样写M1高区，dummy仅在D和窗口入口选择。
 reg negative_prefix;
 wire[29:0]shift_input=phase==3 ? {5'd0,wide_result[47:23]} : raw_h;
 wire[29:0]shifted=shift_input>>align_shift;
 wire[31:0]aligned_h={2'd0,shifted};
 // 两个交替pixel各H0/H1：M1地址256..259，低区0..255仍为只读数值表。
 wire h_we=launch||(phase==1&&valid_pipe[0]);
 wire[1:0]h_wa={(phase==0 ? sample_pid[0] : pid0[0]),(phase==1)};
 wire[35:0]table_q;
 // H<=D及窗口定义保证hmant bit17恒0；保留17位无损值与旧pixel的e/D0。
 reg[16:0]hm0,hm1;reg[4:0]hm_exponent;reg hm_zero;
 wire[31:0]positive_d=d_acc==0 ? 32'd1 : d_acc;
 wire[4:0]exponent;
 wire[31:0]normalized_h=table_q[31:0]&{32{cover_pipe[1]}};
 wire[17:0]mant;wire shared_norm_valid,shared_norm_zero,shared_norm_pending;
 wire[5:0]shared_norm_exp;wire[35:0]lod_norm_value;wire lod_norm_valid;
 iz_four_phase_window normalize(clk,reset,ce,phase,valid_pipe[0],valid_pipe[1],
  positive_d,normalized_h,lod_norm_valid,lod_norm_value,mant,exponent,
  shared_norm_valid,shared_norm_zero,shared_norm_exp,shared_norm_pending);

 wire log_valid,log_result_valid;wire[5:0]log_index;wire[4:0]log_tag,log_result_tag;
 wire log_take=log_valid&&phase==3&&!reset;
 wire table_re=log_take||(phase==0&&valid_pipe[0])||((phase==1||phase==2)&&valid_pipe[1]);
 reg[8:0]table_ra;
 always @*begin
  case(phase)
   0:table_ra={7'b1000000,pid0[0],1'b0};
   1:table_ra={1'b0,mant[16:9]};
   2:table_ra={7'b1000000,pid1[0],1'b1};
   default:table_ra={1'b0,log_index,2'b00};
  endcase
 end
 iz_numeric_ram reciprocal_ram(clk,reset,ce,h_we,h_wa,aligned_h,table_re,table_ra,table_q);
 reg log_return_valid;reg[4:0]log_return_tag;
 assign log_result_valid=log_return_valid;
 assign log_result_tag=log_return_tag;
 wire reciprocal_valid=phase==2&&valid_pipe[1]&&!reset;
 wire[12:0]reciprocal_log_q8={exponent,8'd0}+{5'd0,table_q[34:27]};

 wire[35:0]mul_result;
 // 舍入作为减法借位输入，严格保留RNE；不先生成q+inc。
 wire correction_inc=mul_result[8]&&((|mul_result[7:0])||mul_result[9]);
 wire[17:0]correction_q=mul_result[26:9];
 // 低位附加borrow，单次19位减法后固定去掉低位：
 // floor((2*seed-2*q-inc)/2) = seed-q-inc。
 wire[18:0]correction_sub={seed,1'b0}-{correction_q,correction_inc};
 wire[17:0]corrected_r=correction_sub[18:1];
 reg[17:0]mul_a,mul_b;
 always @*begin
  mul_a=0;mul_b=0;
  case(phase)
   0:begin mul_a=corrected_r;mul_b={1'b0,hm0};end
   1:begin mul_a=r;mul_b={1'b0,hm1};end
   2:begin mul_a={9'd0,fraction};mul_b={9'd0,table_q[8:0]};end
   default:begin mul_a=0;mul_b=0;end
  endcase
 end
 iz_mul multiply(clk,reset,ce,mul_a,mul_b,mul_result);
 wire[18:0]raw_weight=mul_result[35:17];
 wire[16:0]remaining=17'd65536-clipped0;
 reg[1:0]param_read_row;
 wire param_re=((phase==3||phase==0)&&valid_pipe[2])||(phase==1&&valid_pipe[3]);
 always @(posedge clk)begin
  if(reset)param_read_row<=0;
  else if(ce&&param_re)begin
   case(phase)
    3:param_read_row<=0;
    0:param_read_row<=1;
    default:param_read_row<=2;
   endcase
  end
 end
 // S17 base/differences are exact for signed N12 and unsigned UV16/RGB8.
 // Store 51 bits per row; restore the redundant sign bits at the DSP pins.
 wire[50:0]p0q,p1q;
 iz_params_async #(.WIDTH(51)) param0(clk,reset,ce,param_valid&&!reset&&!param_bank,
  {1'b0,write_bank,param_index},param_data,{1'b0,read_bank,param_read_row},p0q);
 iz_params_async #(.WIDTH(51)) param1(clk,reset,ce,param_valid&&!reset&&param_bank,
  {1'b0,write_bank,param_index},param_data,{1'b0,read_bank,param_read_row},p1q);
 wire signed[53:0]base0={{21{p0q[50]}},p0q[50:34],16'h8000};
 wire signed[53:0]base1={{21{p1q[50]}},p1q[50:34],16'h8000};
 wire signed[53:0]pair0_result,pair1_result;
 iz_pairmac pair0(clk,reset,ce,weight0,weight1,{p0q[16],p0q[16:0]},{p0q[33],p0q[33:17]},base0,pair0_result);
 iz_pairmac pair1(clk,reset,ce,weight0,weight1,{p1q[16],p1q[16:0]},{p1q[33],p1q[33:17]},base1,pair1_result);
 // C低16位已加半LSB；仅半分位由取偶改成向正无穷取整。
 wire[17:0]attr0=pair0_result[33:16],attr1=pair1_result[33:16];
 wire operand_we,operand_re;wire[3:0]operand_wa,operand_ra;wire[17:0]operand_wd,operand_value;
 // Runtime A only holds four per-quad anchors. Preparation owns its
 // independent BSRAM table; no read/write owner mux joins these lifetimes.
 iz_params #(.WIDTH(18)) wide_anchor(clk,reset,ce,operand_we,
  {2'd0,operand_wa[1:0]},operand_wd,operand_re,{2'd0,operand_ra[1:0]},operand_value);
 wire wide_valid;wire signed[35:0]wide_b;wire signed[53:0]wide_c;wire[4:0]wide_tag;
 reg[1:0]wide_valid_pipe;reg[4:0]wide_tag0,wide_tag1;
 wire[17:0]wide_a=phase==1 ? r : operand_value;
 iz_widemac wide(clk,reset,ce,wide_a,1'b0,wide_b,wide_c,wide_result);
 wire wide_result_valid=wide_valid_pipe[1];
 wire wide_issue=wide_valid;
 wire[4:0]wide_issue_tag=wide_tag;

 wire z_overflow=z_shift<9'sd15||(z_shift==9'sd15&&wide_result[47]);
 wire[24:0]depth_word=(z_shift>=9'sd48||output_shift[5]) ? 25'd0:shifted[24:0];
 wire[15:0]z16=depth_word[15:0];
 wire numerical_pass=cover_pipe[2]&&!z_dzero&&!z_overflow&&depth_word[24:16]==0&&z16<=far_z[23:8];
 assign depth_event_valid=phase==1&&valid_pipe[3]&&!reset;
 assign depth_event_pid=pid3;
 assign depth_event_z16=z_early;
 assign depth_event_pass=z_early_pass;
 assign result_we=valid_pipe[3]&&(phase==2||phase==3||phase==0)&&!reset;
 wire[1:0]result_index=phase==2 ? 2'd0 : phase==3 ? 2'd1 : 2'd2;
 assign result_wa={pid3,result_index};
 assign tap_valid=result_we;assign tap_pid=pid3;assign tap_index=result_index;
 assign tap_a=attr0;assign tap_b=attr1;
 always @*begin
  result_wd=0;
  case(phase)
   2:result_wd={z_new_pass,11'd0,attr1[11:0],attr0[11:0]};
   3:begin
    if(rgb)result_wd={16'd0,attr1[7:0],attr0[11:0]};
    else result_wd={6'd0,attr1,attr0[11:0]};
   end
   0:begin
    if(rgb)result_wd={4'd0,z_new,attr1[7:0],attr0[7:0]};
    else result_wd={2'd0,z_new,attr0};
   end
   default:result_wd=0;
  endcase
 end

 rc_lod lod(
  .clk(clk),.reset(reset),.ce(ce),.phase(phase),
  .read_bank(read_bank),.write_bank(write_bank),
 .coefficient_bwe(coefficient_bwe),.coefficient_cwe(coefficient_cwe),
 .coefficient_bwa(coefficient_bwa),.coefficient_cwa(coefficient_cwa),
 .coefficient_bwd(coefficient_bwd),.coefficient_cwd(coefficient_cwd),
 .cfg_valid(cfg_valid),
  .cfg_tex_u(cfg_tex_u),.cfg_tex_v(cfg_tex_v),.cfg_max_mip(cfg_max_mip),.cfg_bias(cfg_bias),.cfg_rgb(cfg_rgb),
  .quad_end_valid(quad_end_valid),.quad_end_qid(quad_end_qid),.quad_end_mask(quad_end_mask),
  .reciprocal_valid(reciprocal_valid),.reciprocal_pid(pid1),.reciprocal_anchor(anchor_pipe[1]),.reciprocal_log_q8(reciprocal_log_q8),
  .tap_valid(tap_valid),.tap_pid(tap_pid),.tap_index(tap_index),.tap_anchor(anchor_pipe[3]),.tap_a(tap_a),.tap_b(tap_b),
  .operand_we(operand_we),.operand_wa(operand_wa),.operand_wd(operand_wd),.operand_re(operand_re),.operand_ra(operand_ra),.operand_value(operand_value),
  .wide_valid(wide_valid),.wide_b(wide_b),.wide_c(wide_c),.wide_tag(wide_tag),
  .wide_result_valid(wide_result_valid),.wide_result_tag(wide_tag1),.wide_result_data(wide_result),
  .norm_input_valid(lod_norm_valid),.norm_input_value(lod_norm_value),
  .norm_valid(shared_norm_valid),.norm_zero(shared_norm_zero),.norm_exp(shared_norm_exp),
  .norm_index(mant[16:11]),.norm_pending(shared_norm_pending),
  .log_valid(log_valid),.log_index(log_index),.log_tag(log_tag),
  .log_result_valid(log_result_valid),.log_result_tag(log_result_tag),.log_result_data(table_q[34:27]),
  .lod_valid(lod_valid),.lod_id(lod_id),.lod_value(lod_value),.busy(lod_busy),.error(lod_error));

 always @(posedge clk)begin
  if(reset)begin
   phase<=0;valid_pipe<=0;cover_pipe<=0;anchor_pipe<=0;pid0<=0;pid1<=0;pid2<=0;pid3<=0;
   z_bias<=0;far_z<=32'd13041664;rgb<=0;shift0<=0;shift1<=0;shift2<=0;
   d_acc<=0;d_zero<=0;negative_prefix<=0;fraction<=0;seed<=0;r<=0;clipped0<=0;weight0<=0;weight1<=0;
   hm0<=0;hm1<=0;hm_exponent<=0;hm_zero<=0;
   z_shift_early<=0;z_shift<=0;z_dzero_early<=0;z_dzero<=0;z_new<=0;z_new_pass<=0;z_early<=0;z_early_pass<=0;
   wide_valid_pipe<=0;wide_tag0<=0;wide_tag1<=0;log_return_valid<=0;log_return_tag<=0;compute_error<=0;
  end else if(ce)begin
   phase<=phase+1'b1;
   wide_valid_pipe<={wide_valid_pipe[0],wide_issue};wide_tag1<=wide_tag0;if(wide_issue)wide_tag0<=wide_issue_tag;
   log_return_valid<=log_take;if(log_take)log_return_tag<=log_tag;
   if(cfg_valid)begin z_bias<=cfg_z_bias;far_z<=cfg_far;rgb<=cfg_rgb;shift0<=cfg_shift0;shift1<=cfg_shift1;shift2<=cfg_shift2;end
   if(sample_valid&&phase!=0)compute_error[0]<=1;
   if(quad_end_valid&&(phase!=2||!valid_pipe[0]||pid0[1:0]!=3||pid0[4:2]!=quad_end_qid))compute_error[3]<=1;
   if(phase==2&&valid_pipe[0]&&sample_cover&&(negative_prefix||raw_h[29]))compute_error[1]<=1;
   if(launch&&(cfg_valid||(param_valid&&write_bank==read_bank)))compute_error[2]<=1;
   if(phase==0)begin
    valid_pipe<={valid_pipe[2:0],launch};cover_pipe<={cover_pipe[2:0],1'b0};
    anchor_pipe<={anchor_pipe[2:0],1'b0};
    pid3<=pid2;pid2<=pid1;pid1<=pid0;
    if(launch)begin pid0<=sample_pid;d_acc<=aligned_h;negative_prefix<=raw_h[29];end
    if(valid_pipe[1])begin
     hm1<=mant[16:0];r<=corrected_r;
     z_shift_early<=$signed(z_bias)+$signed({4'd0,hm_exponent});z_dzero_early<=hm_zero;
    end
   end
   if(phase==1)begin
    if(valid_pipe[0])begin d_acc<=d_acc+aligned_h;negative_prefix<=negative_prefix||raw_h[29];end
    if(valid_pipe[1])fraction<=mant[8:0];
    if(valid_pipe[3])begin z_new<=z_early;z_new_pass<=z_early_pass;end
   end
   if(phase==2)begin
    if(valid_pipe[0])begin
     if(sample_cover)d_acc<=d_acc+aligned_h;else d_acc<=32'd1;
     cover_pipe[0]<=sample_cover;anchor_pipe[0]<=sample_anchor&&sample_cover;
    end
    if(valid_pipe[1])begin seed<=table_q[26:9];hm0<=mant[16:0];hm_exponent<=exponent;hm_zero<=d_zero;end
    if(valid_pipe[2])begin
     if(raw_weight>19'd65536)clipped0<=17'd65536;else clipped0<=raw_weight[16:0];
    end
    z_shift<=z_shift_early;z_dzero<=z_dzero_early;
   end
   if(phase==3)begin
    if(valid_pipe[2])begin z_early<=z16;z_early_pass<=numerical_pass;end
    if(valid_pipe[0])d_zero<=d_acc==0;
    if(valid_pipe[2])begin
     weight0<={1'b0,clipped0};
     if(raw_weight>{2'd0,remaining})weight1<={1'b0,remaining};else weight1<=raw_weight[17:0];
    end
   end
  end
 end
`ifndef SYNTHESIS
 always @(posedge clk)if(ce&&!reset)begin
  if(operand_we&&operand_wd[17])$fatal(1,"anchor does not fit unsigned17");
  if(param_valid&&busy&&write_bank==read_bank)$fatal(1,"attribute write to live context");
  if(cfg_valid&&busy)$fatal(1,"configuration commit before pixel drain");
  if(log_take&&phase!=3)$fatal(1,"M1 log calendar");
  if(wide_valid&&phase!=3)$fatal(1,"LOD occupied the Z phase");
 end
`endif
endmodule
