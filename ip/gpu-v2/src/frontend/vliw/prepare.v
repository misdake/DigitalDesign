// 统一准备生产链：几何/权重直接驻留最终物理银行，属性发布/bbox/N/S/G仅一套ROM。
module rp_prepare(
 input clk,reset,ce,start,execute_grant,input[35:0]descriptor,
 input[17:0]operand_value,output[17:0]dsp_a,output dsp_asign,
 output[35:0]dsp_b,output[53:0]dsp_c,
 input[31:0]cfg_far_z,input[4:0]cfg_tex_u,cfg_tex_v,cfg_max_mip,input signed[7:0]cfg_bias,
 input in_valid,output in_ready,input[4:0]in_index,input[53:0]in_data,
 output geo_we,output[3:0]geo_wa,output[29:0]geo_wd,
 output pw_low_we,output reg pw_low0_we,pw_low1_we,pw_high_we,output reg[3:0]pw_wa,output reg[35:0]pw_low_wd,output reg[53:0]pw_high_wd,output reg[3:0]pw_high_wa,
 output reg pw_re,output reg[3:0]pw_ra,output reg pw_acc_re,output reg[3:0]pw_acc_ra,input[89:0]pw_q,
 output cfg_apply,context_ready,
 output signed[7:0]stream_cfg_z_bias,output[31:0]stream_cfg_far,output stream_cfg_rgb,
 output[4:0]stream_cfg_shift0,stream_cfg_shift1,stream_cfg_shift2,
 output stream_param_valid,output[1:0]stream_param_index,
 output stream_param_bank,output[50:0]stream_param_data,
 
 output[4:0]lod_cfg_tex_u,lod_cfg_tex_v,lod_cfg_max_mip,output signed[7:0]lod_cfg_bias,
 output operand_we,output[3:0]operand_wa,output[17:0]operand_wd,
 output operand_re,output[6:0]operand_ra,
 output wide_valid,output wide_asign,
 output bound_valid,bound_empty,output[4:0]bound_xlo,bound_xhi,output[3:0]bound_ylo,bound_yhi,
 input[53:0]wide_result_data,
 output busy,import_busy,math_busy,done,output reg[3:0]error
);
 localparam IDLE=0,COLLECT=1,TAIL=5,
 CORE=8,
 CFG_COMMIT=26,FINISH=28,FAULT=29;
 wire[5:0]operation;wire core_active;
 wire input_phase=operation>=48;
 wire[4:0]expected={1'b0,operation[3:0]};
 wire collecting=core_active&&input_phase&&expected<9;
 wire tail_input=core_active&&input_phase&&expected>=9;
 wire commit_phase=core_active&&operation==23;
 wire finish_phase=core_active&&operation==24;
 wire math_phase=core_active&&!input_phase&&!commit_phase&&!finish_phase;
`ifndef SYNTHESIS
 wire[4:0]state=!core_active?IDLE:input_phase?(expected<9?COLLECT:TAIL):
  commit_phase?CFG_COMMIT:finish_phase?FINISH:CORE;
`endif
 reg[1:0]edge_no,field;wire[3:0]import_row;wire[2:0]import_kind;
 reg read_valid;reg[4:0]read_exp_q;
 reg[1:0]quant_pipe;reg[1:0]quant_edge,quant_field;
 reg quant_negative0,quant_negative1;
 wire quant_read;wire[1:0]quant_read_field;
 wire quant_valid=quant_pipe[1];
 wire negative=quant_negative1;
 reg[4:0]edge_exp,exp0,exp1,exp2,common_e;
 reg[52:0]work;reg sigma,rgb;reg[3:0]tri_shift;
 reg[31:0]far_z;reg[4:0]tex_u,tex_v,max_mip;reg signed[7:0]bias;
 wire[52:0]delta;reg[5:0]delta_e;reg normalizing;

 reg[2:0]seed_sign_x,seed_sign_y;
 wire core_attr_send,core_attr_bank;wire[1:0]core_attr_index;
 wire[35:0]low_q=pw_q[35:0];
 assign busy=core_active;assign import_busy=core_active&&input_phase;
 assign math_busy=core_active&&!input_phase;
 assign done=finish_phase;
 wire input_normal,input_attr;
 assign in_ready=!reset&&core_active&&input_phase&&!(|{error[3],error[0]});
 wire input_take=in_valid&&in_ready;
 wire is_c=import_kind[0];
 wire[46:0]raw_value=is_c ? in_data[46:0] : {{11{in_data[35]}},in_data[35:0]};
 // Raw signed coefficients are stored directly. Only the small exponent
 // detector needs a ones-magnitude high word and a low-zero carry.
 wire[19:0]excess_signed=is_c?raw_value[46:27]:{raw_value[35],raw_value[35:17]};
 wire[19:0]excess_ones=excess_signed^{20{raw_value[46]}};
 wire excess_carry=raw_value[46]&&(is_c?(~|raw_value[26:0]):(~|raw_value[16:0]));
 wire[20:0]leading_any,low_carry;
 assign leading_any[20]=1'b0;assign low_carry[0]=excess_carry;
 genvar prefix_bit;
 generate for(prefix_bit=0;prefix_bit<20;prefix_bit=prefix_bit+1)begin:exponent_prefix
  assign leading_any[prefix_bit]=leading_any[prefix_bit+1]||excess_ones[prefix_bit];
  assign low_carry[prefix_bit+1]=low_carry[prefix_bit]&&excess_ones[prefix_bit];
 end endgenerate
 reg[4:0]need;integer need_bit;
 always @*begin
  need=0;
  for(need_bit=0;need_bit<20;need_bit=need_bit+1)
   if(leading_any[need_bit]||low_carry[need_bit])need=need_bit+1;
 end
 wire [4:0] next_exp=need>edge_exp ? need : edge_exp;
 wire[53:0]quant_result;
 wire[27:0]canonical28=quant_result[34:7];
 // Only PA's N coefficients need an unsigned magnitude; PB/R use signed Q.
 wire[16:0]normal_magnitude=canonical28[27]?17'd0-canonical28[16:0]:canonical28[16:0];
 wire[35:0]canonical={{8{canonical28[27]}},canonical28};
 assign geo_we=!reset&&quant_valid;
 // 按最终R工作池的物理行发射；符号方向直接复用PB已经需要的canonical。
 assign geo_wa={quant_field,quant_edge};
 assign geo_wd=quant_field==2 ? canonical[29:0] : {canonical[28:0],1'b0};
 assign input_normal=import_kind[1];
 assign input_attr=import_kind[2];
 // Reuse PC8..10 after the last geometry record has been consumed.
 // Store N36/UV32/RGB24 unchanged; select a narrow component at consumption.
 wire[47:0]packed_input={12'd0,in_data[35:0]};
 wire packed_write=tail_input&&input_take&&(input_normal||input_attr);
 assign pw_low_we=pw_low0_we||pw_low1_we;
 // PC8..10 hold the current edge's raw records; no separate quantization pool.
 wire[47:0]pool_q=pw_q[83:36];
 assign context_ready=commit_phase&&!normalizing&&!reset;
 assign cfg_apply=context_ready&&execute_grant;
 wire[30:0]depth_k=delta[52:22];
 assign stream_cfg_z_bias=8'sd38+$signed({3'd0,common_e})-$signed({2'd0,delta_e})-$signed({4'd0,tri_shift});
 assign stream_cfg_far=far_z;assign stream_cfg_rgb=rgb;
 assign stream_cfg_shift0=common_e-exp0;assign stream_cfg_shift1=common_e-exp1;assign stream_cfg_shift2=common_e-exp2;
 assign stream_param_valid=core_attr_send&&!reset;
 assign stream_param_index=core_attr_index;assign stream_param_bank=core_attr_bank;
 
 assign lod_cfg_tex_u=tex_u;assign lod_cfg_tex_v=tex_v;assign lod_cfg_max_mip=max_mip;assign lod_cfg_bias=bias;
 wire seed_take=geo_we&&quant_field<2;
 // Component selection also identifies the UV return slots.
 wire[2:0]factor_index={core_attr_index,core_attr_bank};
 wire core_a_re,core_b_re,core_b_we,core_c_re,core_c_we,core_issue,core_asign,core_done;
 wire core_attr0_take,core_attr1_take;
 reg[15:0]component;
 wire[6:0]core_ar;wire[3:0]core_br,core_bw,core_cr,core_cw;wire[35:0]core_bd;wire[53:0]core_cd;
 wire core_start=start&&!busy;
 rp_prepare_core core(clk,reset,ce&&!(|{error[3],error[0]}),core_start,
  input_take,normalizing||!execute_grant,operation,import_row,import_kind,rgb,{seed_sign_y,seed_sign_x},stream_cfg_shift0,stream_cfg_shift1,stream_cfg_shift2,pw_q[35:0],pw_q[89:36],wide_result_data,
  core_a_re,core_ar,core_b_re,core_br,core_b_we,core_bw,core_bd,
  core_c_re,core_cr,core_c_we,core_cw,core_cd,core_issue,core_asign,core_active,core_done,
  bound_valid,bound_empty,bound_xlo,bound_xhi,bound_ylo,bound_yhi,core_attr_send,core_attr_index,core_attr_bank,
  core_attr0_take,core_attr1_take);
 // U/V reach PA in the same prefetch return slots used by parameter export.
 // Constants are read-only table rows; only actual UV data reach this write port.
 // RGB has no PA/analytic-LOD consumer.
 wire uv_component=factor_index==3||factor_index==4;
 wire uv_write=!rgb&&uv_component&&(core_attr0_take||core_attr1_take||core_attr_send);
 wire[1:0]uv_vertex=core_attr0_take?2'd2:core_attr1_take?2'd0:2'd1;
 wire[3:0]uv_address=(core_attr_index==1?4'd1:4'd4)+{2'd0,uv_vertex};
 assign operand_we=!reset&&(math_phase?uv_write:(geo_we&&quant_field<2));
 wire[3:0]weight_address=quant_field==1?4'd9+{2'd0,quant_edge}:quant_edge==0?4'd0:4'd6+{2'd0,quant_edge};
 assign operand_wa=math_phase?uv_address:weight_address;
 assign operand_wd=math_phase?{2'd0,component}:{1'b0,normal_magnitude};
 assign operand_re=(math_phase&&core_a_re)||quant_read;
 assign operand_ra=math_phase?core_ar:
  {3'b110,sigma,(collecting&&input_take&&is_c?next_exp[2:0]:read_exp_q[2:0])};
 assign wide_valid=math_phase&&core_issue;assign wide_asign=core_asign;
 // Read the consumer's base first: op0/1/2 read vertices 2/0/1.
 // op1 captures a2, op2 stores a0-a2, op3 (or op42) consumes a1-a2.
 // One subtractor serves both differences; all three stored fields are S17.
 wire signed_attribute=core_attr_index==0||(core_attr_index==1&&!core_attr_bank);
 reg[15:0]attribute_base;
 reg signed[16:0]difference0;
 // Unsigned16 differences fit signed17 exactly; normals and RGB are narrower.
 wire signed[16:0]current_a={signed_attribute&&component[15],component};
 wire signed[16:0]base_a={signed_attribute&&attribute_base[15],attribute_base};
 wire signed[16:0]difference=current_a-base_a;
 reg[11:0]normal_component;reg[7:0]rgb_component;
 always @*begin
  case({core_attr_index[0],core_attr_bank})
   0:normal_component=pw_q[47:36];1:normal_component=pw_q[59:48];default:normal_component=pw_q[71:60];
  endcase
  if(!core_attr_index[1])rgb_component=pw_q[43:36];
  else if(!core_attr_bank)rgb_component=pw_q[51:44];else rgb_component=pw_q[59:52];
  if(signed_attribute)component={{4{normal_component[11]}},normal_component};
  else if(rgb)component={8'd0,rgb_component};
  else if(core_attr_index[1]&&core_attr_bank)component=0;
  else if(core_attr_index[1])component=pw_q[67:52];else component=pw_q[51:36];
 end
 always @(posedge clk)begin
  if(reset)begin attribute_base<=0;difference0<=0;end
  else if(ce)begin
   if(core_attr0_take)attribute_base<=component;
   if(core_attr1_take)difference0<=difference;
  end
 end
 assign stream_param_data={base_a,difference,difference0};

 // Input rows, math rows and the depth row have disjoint lifetimes.
 // The ROM supplies the input destination directly; no legacy state decoder
 // or second input writer survives alongside the common write path.
 wire input_write=core_active&&input_phase&&input_take&&expected!=9;
 wire[11:0]input_high=collecting?{raw_value[46],raw_value[46:36]}:12'd0;
 always @*begin
  pw_low0_we=geo_we;pw_low1_we=geo_we;
  pw_wa=(quant_field==0?4'd6:quant_field==1?4'd9:4'd12)+{2'd0,quant_edge};
  pw_high_we=input_write;pw_high_wa=import_row;
  pw_re=0;pw_ra=core_br;pw_acc_re=quant_read;pw_acc_ra=4'd8+{2'd0,quant_read_field};
  pw_low_wd=canonical;
  pw_high_wd={6'd0,input_high,in_data[35:0]};
  if(math_phase)begin
   pw_low0_we=core_b_we;pw_low1_we=core_b_we;pw_wa=core_bw;pw_low_wd=core_bd;
   pw_high_we=core_c_we;pw_high_wa=core_cw;pw_high_wd=core_cd;
   pw_re=core_b_re;pw_acc_re=core_c_re;pw_acc_ra=core_cr;
  end else if(commit_phase)begin
   pw_low0_we=cfg_apply;pw_low1_we=cfg_apply;pw_wa=4;pw_low_wd={5'd0,depth_k};
  end
  if(reset)begin pw_low0_we=0;pw_low1_we=0;pw_high_we=0;pw_re=0;pw_acc_re=0;end
 end

 // One physical work vector; Delta is stored in reversed bit order.
 wire[52:0]work_delta_input;
 genvar work_bit;
 generate for(work_bit=0;work_bit<53;work_bit=work_bit+1)begin:work_reverse
  assign delta[work_bit]=work[52-work_bit];
  assign work_delta_input[work_bit]=in_data[52-work_bit];
 end endgenerate
 wire work_load_delta=!start&&tail_input&&in_valid&&in_index==expected&&in_index==9;
 wire work_shift_delta=normalizing&&(|delta)&&!delta[52];
 wire[52:0]work_shifted=delta[51]?{1'b0,work[52:1]}:{2'b0,work[52:2]};
 always @(posedge clk)begin
  if(reset)work<=0;
  else if(ce)begin
   if(work_load_delta)work<=work_delta_input;
   else if(work_shift_delta)work<=work_shifted;
  end
 end

 // C acceptance starts three consecutive reads. The next edge writes its
 // Nx/Ny while the old edge reads Ny/C: both physical ports remain independent.
 // A three-bit fraction follows A/B to the following live-C sampling edge.
 wire edge_capture=collecting&&input_take&&in_index==expected&&is_c;
 assign quant_read=edge_capture||field!=0;
 assign quant_read_field=field;
 reg signed[35:0]quant_window;
 always @*begin
  case(read_exp_q[4:3])
   0:quant_window=pool_q[35:0];
   1:quant_window=pool_q[43:8];
   default:quant_window={{5{pool_q[46]}},pool_q[46:16]};
  endcase
 end
 // Sigma changes only once per triangle. The DSP consumes signed raw Q and
 // a signed power of two, returning canonical coefficients in their final form.
 wire[17:0]quant_scale=operand_value;
 reg signed[8:0]plus_bias,minus_bias;reg[2:0]quant_fraction;
 always @*begin
  case(quant_fraction)
   3'd0:begin plus_bias=9'd255;minus_bias=-9'sd128;end
   3'd1:begin plus_bias=9'd191;minus_bias=-9'sd64;end
   3'd2:begin plus_bias=9'd159;minus_bias=-9'sd32;end
   3'd3:begin plus_bias=9'd143;minus_bias=-9'sd16;end
   3'd4:begin plus_bias=9'd135;minus_bias=-9'sd8;end
   3'd5:begin plus_bias=9'd131;minus_bias=-9'sd4;end
   3'd6:begin plus_bias=9'd129;minus_bias=-9'sd2;end
   3'd7:begin plus_bias=9'd128;minus_bias=-9'sd1;end
  endcase
 end
 reg tail_nonzero;
 always @*case(read_exp_q[4:3])
  0:tail_nonzero=0;
  1:tail_nonzero=pool_q[47]&&(|pool_q[7:0]);
  default:tail_nonzero=pool_q[47]&&(|pool_q[15:0]);
 endcase
 reg quant_tail;
 always @(posedge clk)begin
  if(reset)quant_tail<=0;
  else if(ce)begin
   if(start)quant_tail<=0;
   else if(read_valid)quant_tail<=tail_nonzero;
  end
 end
 // A/B are registered; C is live on their output edge. The common exponent
 // is latched per edge; its fraction, sign and tail follow the fixed C edge.
 reg signed[8:0]geometry_bias9;
 always @*begin
  geometry_bias9=0;
  if(quant_tail)begin
   if(sigma)geometry_bias9=minus_bias;else geometry_bias9=plus_bias;
  end else if(quant_negative0^sigma)geometry_bias9=9'sd127;
 end
 // One private DSP serves geometry quantization and the following ROM.
 // It never switches to a live pixel consumer.
 assign dsp_a=operand_value;
 assign dsp_asign=math_phase?core_asign:1'b1;
 assign dsp_b=math_phase?pw_q[35:0]:quant_window;
 assign dsp_c=math_phase?pw_q[89:36]:{{45{geometry_bias9[8]}},geometry_bias9};
 assign quant_result=wide_result_data;
 always @(posedge clk)begin
  if(reset)begin
   read_valid<=0;read_exp_q<=0;quant_pipe<=0;quant_edge<=0;quant_field<=0;
   quant_negative0<=0;quant_negative1<=0;quant_fraction<=0;
  end else if(ce)begin
   if(start)begin read_valid<=0;quant_pipe<=0;quant_edge<=0;quant_field<=0;quant_fraction<=0;end
   else begin
    read_valid<=quant_read;quant_pipe<={quant_pipe[0],read_valid};
    if(edge_capture)read_exp_q<=next_exp;
    if(read_valid)begin
     quant_negative0<=pool_q[47];quant_fraction<=read_exp_q[2:0];
    end
    quant_negative1<=quant_negative0;
    // Every return is Nx, Ny, C in edge order. Its final physical address
    // advances only at writeback; no edge/field tags travel through the DSP.
    if(quant_valid)begin
     if(quant_field==2)begin quant_field<=0;quant_edge<=quant_edge+1'b1;end
     else quant_field<=quant_field+1'b1;
    end
   end
  end
 end
`ifndef SYNTHESIS
 reg[2:0]sim_issue_fraction;
 always @(posedge clk)begin
  if(reset)sim_issue_fraction<=0;
  else if(ce)begin
   if(quant_pipe[0]&&quant_fraction!==sim_issue_fraction)
    $fatal(1,"raw geometry scale changed before its C sampling edge");
   if(read_valid)sim_issue_fraction<=read_exp_q[2:0];
  end
 end
 always @(posedge clk)if(ce&&!reset)begin
  if(quant_valid&&(math_phase||commit_phase))$fatal(1,"geometry pipeline overlaps parameter execution");
 end
`endif

 always @(posedge clk)begin
  if(reset)begin
   edge_no<=0;field<=0;edge_exp<=0;
   exp0<=0;exp1<=0;exp2<=0;common_e<=0;sigma<=0;rgb<=0;tri_shift<=0;
   far_z<=0;tex_u<=0;tex_v<=0;max_mip<=0;bias<=0;
   delta_e<=0;normalizing<=0;
   seed_sign_x<=0;seed_sign_y<=0;error<=0;
  end else if(ce)begin
   if(normalizing)begin
    if(delta==0)begin normalizing<=0;error[1]<=1;end
    else if(!delta[52])begin
     if(delta[51])begin delta_e<=delta_e-1'b1;end
     else begin delta_e<=delta_e-2;end
    end else normalizing<=0;
   end
   if(start)begin
    if(busy)begin error[3]<=1;end
    else begin
     edge_no<=0;field<=0;edge_exp<=0;normalizing<=0;
     exp0<=0;exp1<=0;exp2<=0;common_e<=0;error<=0;
     sigma<=descriptor[22];rgb<=descriptor[23];tri_shift<=descriptor[21:18];
     far_z<=cfg_far_z;tex_u<=cfg_tex_u;tex_v<=cfg_tex_v;max_mip<=cfg_max_mip;bias<=cfg_bias;
    end
   end else begin
    if(quant_read)begin if(field==2)field<=0;else field<=field+1'b1;end
    if(seed_take)begin
     if(quant_field==0)seed_sign_x[quant_edge]<=negative^sigma;
     else seed_sign_y[quant_edge]<=negative^sigma;
    end
    if(input_take)begin
     if(in_index!=expected)error[0]<=1;
     else if(collecting)begin
      edge_exp<=next_exp;
      if(is_c)begin
       if(next_exp>19)error[1]<=1;
       case(edge_no)
        0:exp0<=next_exp;1:exp1<=next_exp;
        default:begin
         exp2<=next_exp;
         if(exp0>=exp1&&exp0>=next_exp)common_e<=exp0;
         else if(exp1>=next_exp)common_e<=exp1;else common_e<=next_exp;
        end
       endcase
       edge_exp<=0;edge_no<=edge_no+1'b1;
      end
     end else if(expected==9)begin
      if(in_data==0||in_data[53])error[1]<=1;
      delta_e<=52;normalizing<=1;
     end
    end

   end
  end
 end
`ifndef SYNTHESIS
 always @(posedge clk)if(ce&&!reset&&geo_we)begin
  if(quant_field<2&&canonical[35:17]!={19{canonical[17]}})$fatal(1,"几何N不能精确存入18位");
  if(quant_field==2&&canonical[35:27]!={9{canonical[27]}})$fatal(1,"几何C不能精确存入28位");
 end
 always @(posedge clk)if(ce&&!reset)begin
  if(packed_write&&(in_index!=expected||pw_high_wa<8))$fatal(1,"packed import layout");
  if(!math_phase&&pw_high_we&&pw_acc_re&&pw_high_wa==pw_acc_ra)$fatal(1,"streaming PC address collision");
  if(math_phase&&core_c_we&&core_cw>=8&&core_cw!=11)$fatal(1,"packed lifetime overwritten");
 end
`endif
endmodule

