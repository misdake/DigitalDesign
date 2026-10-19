// 统一准备生产链：几何/权重直接驻留最终物理银行，属性发布/bbox/N/S/G仅一套ROM。
module rp_prepare(
 input clk,reset,ce,start,input[35:0]descriptor,
 input[31:0]cfg_far_z,input[4:0]cfg_tex_u,cfg_tex_v,cfg_max_mip,input signed[7:0]cfg_bias,
 input in_valid,output in_ready,input[4:0]in_index,input[53:0]in_data,
 output geo_we,output[3:0]geo_wa,output[29:0]geo_wd,
 output pw_low_we,output reg pw_low0_we,pw_low1_we,pw_high_we,output reg[3:0]pw_wa,output reg[35:0]pw_low_wd,output reg[53:0]pw_high_wd,output reg[3:0]pw_high_wa,
 output reg pw_re,output reg[3:0]pw_ra,output reg pw_acc_re,output reg[3:0]pw_acc_ra,input[89:0]pw_q,
 output cfg_apply,
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
 localparam IDLE=0,COLLECT=1,READ=2,DRAIN=4,TAIL=5,
 CORE=8,
 CFG_COMMIT=26,FINISH=28,FAULT=29;
 reg[4:0]state,expected;reg[1:0]edge_no,field;
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
 assign busy=state!=IDLE;assign import_busy=state>=COLLECT&&state<=TAIL;
 assign math_busy=state==CORE||state==CFG_COMMIT||state==FINISH;
 assign done=state==FINISH;
 wire input_normal,input_attr;
 assign in_ready=!reset&&(state==COLLECT||state==TAIL);
 wire input_take=in_valid&&in_ready;
 wire is_c=in_index==2||in_index==5||in_index==8;
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
 assign input_normal=in_index==10||in_index==12||in_index==14;
 assign input_attr=in_index==11||in_index==13||in_index==15;
 reg[1:0]input_vertex;
 always @*case(in_index)10,11:input_vertex=0;12,13:input_vertex=1;default:input_vertex=2;endcase
 // Reuse PC8..10 after the last geometry record has been consumed. Each
 // N36/UV32/RGB24 input writes three final 16-bit lanes in one cycle.
 reg[47:0]packed_input;
 always @*begin
  if(input_normal)packed_input={{4{in_data[35]}},in_data[35:24],{4{in_data[23]}},in_data[23:12],{4{in_data[11]}},in_data[11:0]};
  else if(rgb)packed_input={8'd0,in_data[23:16],8'd0,in_data[15:8],8'd0,in_data[7:0]};
  else packed_input={16'd0,in_data[31:0]};
 end
 wire normal_write=state==TAIL&&input_take&&input_normal;
 wire packed_write=state==TAIL&&input_take&&(input_normal||input_attr);
 assign pw_low_we=pw_low0_we||pw_low1_we;
 // PC8..10 hold the current edge's raw records; no separate quantization pool.
 wire[47:0]pool_q=pw_q[83:36];
 function[1:0]collect_field;input[4:0]i;begin
  case(i)0,3,6:collect_field=0;1,4,7:collect_field=1;default:collect_field=2;endcase
 end endfunction
 assign cfg_apply=state==CFG_COMMIT&&!normalizing&&!reset;
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
 wire core_a_re,core_b_re,core_b_we,core_c_re,core_c_we,core_issue,core_asign,core_active,core_done;
 wire core_attr0_take,core_attr1_take;
 reg[15:0]component;
 wire[6:0]core_ar;wire[3:0]core_br,core_bw,core_cr,core_cw;wire[35:0]core_bd;wire[53:0]core_cd;
 wire core_start=state==TAIL&&input_take&&in_index==15;
 rp_prepare_core core(clk,reset,ce,core_start,rgb,{seed_sign_y,seed_sign_x},stream_cfg_shift0,stream_cfg_shift1,stream_cfg_shift2,pw_q[35:0],pw_q[89:36],wide_result_data,
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
 assign operand_we=!reset&&(state==CORE?uv_write:(geo_we&&quant_field<2));
 wire[3:0]weight_address=quant_field==1?4'd9+{2'd0,quant_edge}:quant_edge==0?4'd0:4'd6+{2'd0,quant_edge};
 assign operand_wa=state==CORE?uv_address:weight_address;
 assign operand_wd=state==CORE?{2'd0,component}:{1'b0,normal_magnitude};
 assign operand_re=state==CORE&&core_a_re;assign operand_ra=core_ar;
 assign wide_valid=state==CORE&&core_issue;assign wide_asign=core_asign;
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
 always @*case(factor_index)
  0,3:component=pw_q[51:36];
  1,4:component=pw_q[67:52];
  default:component=pw_q[83:68];
 endcase
 always @(posedge clk)begin
  if(reset)begin attribute_base<=0;difference0<=0;end
  else if(ce)begin
   if(core_attr0_take)attribute_base<=component;
   if(core_attr1_take)difference0<=difference;
  end
 end
 assign stream_param_data={base_a,difference,difference0};

 always @*begin
  pw_low0_we=0;pw_low1_we=0;pw_high_we=0;pw_wa=0;pw_high_wa=0;pw_re=0;pw_ra=0;pw_acc_re=0;pw_acc_ra=0;
  if(state==COLLECT&&input_take&&in_index==expected)begin
   pw_high_we=1;pw_high_wa=4'd8+{2'd0,collect_field(in_index)};
  end
  if(quant_read)begin pw_acc_re=1;pw_acc_ra=4'd8+{2'd0,quant_read_field};end
  if(geo_we)begin
   pw_low0_we=1;pw_low1_we=1;
   pw_wa=(quant_field==0?4'd6:quant_field==1?4'd9:4'd12)+{2'd0,quant_edge};
  end
  if(packed_write)begin
   pw_high_we=1;pw_high_wa={1'b1,input_normal,2'b00}+{2'd0,input_vertex};
  end
  case(state)
   CORE:begin
    pw_re=core_b_re;pw_ra=core_br;pw_acc_re=core_c_re;pw_acc_ra=core_cr;
    pw_low0_we=core_b_we;pw_low1_we=core_b_we;pw_wa=core_bw;
    if(core_c_we)begin pw_high_we=1;pw_high_wa=core_cw;end
   end
   CFG_COMMIT:if(cfg_apply)begin
    pw_low0_we=1;pw_low1_we=1;pw_wa=4;
   end
   default:begin end
  endcase
  // Data is selected by the owner phase, independently of write-valid.
  // No reader observes a write-data value unless its unchanged enable fires.
  case(state)
   CORE:begin pw_low_wd=core_bd;pw_high_wd=core_cd;end
   CFG_COMMIT:begin pw_low_wd={5'd0,depth_k};pw_high_wd={{6{packed_input[47]}},packed_input};end
   COLLECT:begin pw_low_wd=canonical;pw_high_wd={6'd0,raw_value[46],raw_value};end
   default:begin pw_low_wd=canonical;pw_high_wd={{6{packed_input[47]}},packed_input};end
  endcase
  if(reset)begin pw_low0_we=0;pw_low1_we=0;pw_high_we=0;pw_re=0;pw_acc_re=0;end
 end

 // One physical work vector; Delta is stored in reversed bit order.
 wire[52:0]work_delta_input;
 genvar work_bit;
 generate for(work_bit=0;work_bit<53;work_bit=work_bit+1)begin:work_reverse
  assign delta[work_bit]=work[52-work_bit];
  assign work_delta_input[work_bit]=in_data[52-work_bit];
 end endgenerate
 wire work_load_delta=!start&&state==TAIL&&in_valid&&in_index==expected&&in_index==9;
 wire work_shift_delta=normalizing&&(|delta)&&!delta[52];
 wire[52:0]work_shifted=delta[51]?{1'b0,work[52:1]}:{2'b0,work[52:2]};
 always @(posedge clk)begin
  if(reset)work<=0;
  else if(ce)begin
   if(work_load_delta)work<=work_delta_input;
   else if(work_shift_delta)work<=work_shifted;
  end
 end

 // PC reads Nx/Ny/C in consecutive slots. C capture requests Nx immediately;
 // the final read releases collection while its Q still feeds the DSP.
 assign quant_read=(state==COLLECT&&input_take&&in_index==expected&&is_c)||state==READ;
 assign quant_read_field=state==READ?field:2'd0;
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
 reg[17:0]quant_scale;
 reg signed[8:0]plus_bias,minus_bias;
 always @*begin
  case({sigma,read_exp_q[2:0]})
   4'd0:quant_scale=18'h00080;
   4'd1:quant_scale=18'h00040;
   4'd2:quant_scale=18'h00020;
   4'd3:quant_scale=18'h00010;
   4'd4:quant_scale=18'h00008;
   4'd5:quant_scale=18'h00004;
   4'd6:quant_scale=18'h00002;
   4'd7:quant_scale=18'h00001;
   4'd8:quant_scale=18'h3ff80;
   4'd9:quant_scale=18'h3ffc0;
   4'd10:quant_scale=18'h3ffe0;
   4'd11:quant_scale=18'h3fff0;
   4'd12:quant_scale=18'h3fff8;
   4'd13:quant_scale=18'h3fffc;
   4'd14:quant_scale=18'h3fffe;
   4'd15:quant_scale=18'h3ffff;
  endcase
  case(read_exp_q[2:0])
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
 // stays live across all three launches. Only the lost-tail predicate is delayed.
 reg signed[8:0]geometry_bias9;
 always @*begin
  geometry_bias9=0;
  if(quant_tail)begin
   if(sigma)geometry_bias9=minus_bias;else geometry_bias9=plus_bias;
  end else if(quant_negative0^sigma)geometry_bias9=9'sd127;
 end
 iz_widemac geometry_dsp(clk,reset,ce,quant_scale,1'b1,quant_window,
  {{45{geometry_bias9[8]}},geometry_bias9},quant_result);
 always @(posedge clk)begin
  if(reset)begin
   read_valid<=0;read_exp_q<=0;quant_pipe<=0;quant_edge<=0;quant_field<=0;
   quant_negative0<=0;quant_negative1<=0;
  end else if(ce)begin
   if(start)begin read_valid<=0;quant_pipe<=0;quant_edge<=0;quant_field<=0;end
   else begin
    read_valid<=quant_read;quant_pipe<={quant_pipe[0],read_valid};
    if(quant_read)begin
     read_exp_q<=state==COLLECT?next_exp:edge_exp;
    end
    if(read_valid)begin
     quant_negative0<=pool_q[47];
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
   if(quant_pipe[0]&&read_exp_q[2:0]!==sim_issue_fraction)
    $fatal(1,"raw geometry scale changed before its C sampling edge");
   if(read_valid)sim_issue_fraction<=read_exp_q[2:0];
  end
 end
 always @(posedge clk)if(ce&&!reset)begin
  if(quant_valid&&(state==CORE||state==CFG_COMMIT))$fatal(1,"geometry pipeline overlaps parameter execution");
 end
`endif

 always @(posedge clk)begin
  if(reset)begin
   state<=IDLE;expected<=0;edge_no<=0;field<=0;edge_exp<=0;
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
    if(state!=IDLE)begin error[3]<=1;state<=FAULT;end
    else begin
     state<=COLLECT;expected<=0;edge_no<=0;field<=0;edge_exp<=0;normalizing<=0;
     exp0<=0;exp1<=0;exp2<=0;common_e<=0;error<=0;
     sigma<=descriptor[22];rgb<=descriptor[23];tri_shift<=descriptor[21:18];
     far_z<=cfg_far_z;tex_u<=cfg_tex_u;tex_v<=cfg_tex_v;max_mip<=cfg_max_mip;bias<=cfg_bias;
    end
   end else begin
    if(seed_take)begin
     if(quant_field==0)seed_sign_x[quant_edge]<=negative^sigma;
     else seed_sign_y[quant_edge]<=negative^sigma;
    end
    case(state)
     COLLECT:if(input_take)begin
      if(in_index!=expected)begin error[0]<=1;state<=FAULT;end
      else begin
       expected<=expected+1'b1;edge_exp<=next_exp;
       if(is_c)begin
        if(next_exp>19)error[1]<=1;
        case(edge_no)0:exp0<=next_exp;1:exp1<=next_exp;default:begin
         exp2<=next_exp;
         if(exp0>=exp1&&exp0>=next_exp)common_e<=exp0;
         else if(exp1>=next_exp)common_e<=exp1;else common_e<=next_exp;
        end endcase
        state<=READ;field<=1;
       end
      end
     end
     READ:begin
      if(field==2)begin
       field<=0;edge_exp<=0;
       if(edge_no==2)state<=DRAIN;
       else begin edge_no<=edge_no+1'b1;state<=COLLECT;end
      end else field<=field+1'b1;
     end
     DRAIN:if(quant_valid&&quant_field==2&&quant_edge==2)state<=TAIL;
     TAIL:if(in_valid)begin
      if(in_index!=expected)begin error[0]<=1;state<=FAULT;end
      else if(in_index==9)begin
       expected<=expected+1'b1;
       if(in_data==0||in_data[53])error[1]<=1;
       delta_e<=52;normalizing<=1;
      end else if(input_normal||input_attr)begin expected<=expected+1'b1;if(in_index==15)state<=CORE;end
      else begin error[0]<=1;state<=FAULT;end
     end
    CORE:if(core_done)state<=CFG_COMMIT;
    CFG_COMMIT:if(cfg_apply)state<=FINISH;
    FINISH:state<=IDLE;
     default:begin end
    endcase

   end
  end
 end
`ifndef SYNTHESIS
 always @(posedge clk)if(ce&&!reset&&geo_we)begin
  if(quant_field<2&&canonical[35:17]!={19{canonical[17]}})$fatal(1,"几何N不能精确存入18位");
  if(quant_field==2&&canonical[35:27]!={9{canonical[27]}})$fatal(1,"几何C不能精确存入28位");
 end
 always @(posedge clk)if(ce&&!reset)begin
  if(packed_write&&(pw_low0_we||pw_low1_we||in_index!=expected))$fatal(1,"packed import layout");
  if(state==CORE&&core_c_we&&core_cw>=8&&core_cw!=11)$fatal(1,"packed lifetime overwritten");
 end
`endif
endmodule

