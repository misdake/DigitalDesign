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
 localparam IDLE=0,COLLECT=1,LOAD=2,DOWN=3,TAIL=5,
 CORE=8,
 CFG_COMMIT=26,FINISH=28,FAULT=29,Q_READ=30;
 reg[4:0]state,expected;reg[1:0]edge_no,field;
 reg quant_valid;reg[1:0]quant_edge,quant_field;wire quant_finish;
 reg[4:0]edge_exp,remaining,exp0,exp1,exp2,common_e;
 reg[52:0]work;wire[46:0]magnitude=work[46:0];reg negative,sigma,rgb;reg[3:0]tri_shift;
 reg[31:0]far_z;reg[4:0]tex_u,tex_v,max_mip;reg signed[7:0]bias;
 wire[52:0]delta;reg[5:0]delta_e;reg normalizing;

 reg[2:0]seed_sign_x,seed_sign_y;
 wire core_attr_send,core_attr_bank;wire[1:0]core_attr_index;
 wire[35:0]low_q=pw_q[35:0];
 assign busy=state!=IDLE;assign import_busy=(state>=COLLECT&&state<=TAIL)||state==Q_READ;
 assign math_busy=state==CORE||state==CFG_COMMIT||state==FINISH;
 assign done=state==FINISH;
 wire input_normal,input_attr;
 assign in_ready=!reset&&(state==COLLECT||state==TAIL);
 wire input_take=in_valid&&in_ready;
 wire is_c=in_index==2||in_index==5||in_index==8;
 wire[46:0]raw_value=is_c ? in_data[46:0] : {{11{in_data[35]}},in_data[35:0]};
 wire[46:0]raw_magnitude=raw_value[46] ? 47'd0-raw_value : raw_value;
 function [4:0] required_shift;
  input [46:0] value;input c;integer b;
  begin
   required_shift=0;
   if(c)begin for(b=27;b<47;b=b+1)if(value[b])required_shift=b-26;end
   else begin for(b=17;b<47;b=b+1)if(value[b])required_shift=b-16;end
  end
 endfunction
 wire [4:0] need=required_shift(raw_magnitude,is_c);
 wire [4:0] next_exp=need>edge_exp ? need : edge_exp;
 wire[35:0]canonical=(negative^sigma)?36'd0-magnitude[35:0]:magnitude[35:0];
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
 // PW6..8复用当前edge的三份48bit原记录，不另建量化池。
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
 assign operand_wd=state==CORE?{2'd0,component}:magnitude[17:0];
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
  pw_low0_we=0;pw_low1_we=0;pw_high_we=0;pw_wa=0;pw_low_wd=0;pw_high_wd=0;pw_high_wa=0;pw_re=0;pw_ra=0;pw_acc_re=0;pw_acc_ra=0;
  if(state==COLLECT&&input_take&&in_index==expected)begin
   pw_high_we=1;pw_high_wa=4'd8+{2'd0,collect_field(in_index)};
   pw_high_wd={6'd0,raw_value[46],raw_magnitude};
  end
  if(state==COLLECT&&input_take&&in_index==expected&&is_c)begin pw_acc_re=1;pw_acc_ra=8;end
  if(geo_we)begin
   pw_low0_we=1;pw_low1_we=1;
   pw_wa=(quant_field==0?4'd6:quant_field==1?4'd9:4'd12)+{2'd0,quant_edge};pw_low_wd=canonical;
  end
  if(quant_finish&&field<2)begin pw_acc_re=1;pw_acc_ra=4'd9+{2'd0,field};end
  if(packed_write)begin
   pw_high_we=1;pw_high_wa={1'b1,input_normal,2'b00}+{2'd0,input_vertex};pw_high_wd={{6{packed_input[47]}},packed_input};
  end
  case(state)
   Q_READ:begin pw_acc_re=1;pw_acc_ra=4'd8+{2'd0,field};end
   CORE:begin
    pw_re=core_b_re;pw_ra=core_br;pw_acc_re=core_c_re;pw_acc_ra=core_cr;
    pw_low0_we=core_b_we;pw_low1_we=core_b_we;pw_wa=core_bw;pw_low_wd=core_bd;
    if(core_c_we)begin pw_high_we=1;pw_high_wa=core_cw;pw_high_wd=core_cd;end
   end
   CFG_COMMIT:if(cfg_apply)begin
    pw_low0_we=1;pw_low1_we=1;pw_wa=4;pw_low_wd={5'd0,depth_k};
   end
   default:begin end
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
 wire work_load_geo=!start&&state==LOAD;
 wire work_load_delta=!start&&state==TAIL&&in_valid&&in_index==expected&&in_index==9;
 wire work_shift_geo=!start&&state==DOWN;
 // Keep the original invalid-start behavior: an active Delta may still shift.
 wire work_shift_delta=normalizing&&(|delta)&&!delta[52];
 // A geometry load uses the same shift window as subsequent steps. The first
 // 1/2/4-bit reduction is performed before writing work, rather than one edge
 // after the read. Delta keeps its reversed-bit, feedback-only lifetime.
 wire[52:0]work_source=work_load_geo?{6'b0,pool_q[46:0]}:work;
 reg[2:0]work_shift;
 reg[52:0]work_shifted;
 always @*begin
  work_shift=3'd2;
  if(work_load_geo)begin
   if(edge_exp>=4)work_shift=3'd4;
   else if(edge_exp>=2)work_shift=3'd2;
   else work_shift={2'd0,edge_exp[0]};
  end else if(work_shift_geo)begin
   if(remaining>=4)work_shift=3'd4;
   else if(remaining==1)work_shift=3'd1;
  end else if(delta[51])work_shift=3'd1;
  case(work_shift)
   3'd0:work_shifted=work_source;
   3'd1:work_shifted={1'b0,work_source[52:1]};
   3'd4:work_shifted={4'b0,work_source[52:4]};
   default:work_shifted={2'b0,work_source[52:2]};
  endcase
 end
 assign quant_finish=(state==LOAD&&edge_exp==work_shift)||
  (state==DOWN&&remaining<=4&&remaining!=3);
 // The completed work value is consumed on the following edge while LOAD
 // writes the next value into that same register. Only its address is delayed;
 // no second magnitude/canonical buffer or dedicated SEND state is needed.
 always @(posedge clk)begin
  if(reset)begin quant_valid<=0;quant_edge<=0;quant_field<=0;end
  else if(ce)begin
   quant_valid<=quant_finish&&!start;
   if(quant_finish)begin quant_edge<=edge_no;quant_field<=field;end
  end
 end
 always @(posedge clk)begin
  if(reset)work<=0;
  else if(ce)begin
   if(work_load_delta)work<=work_delta_input;
   else if(work_load_geo||work_shift_geo||work_shift_delta)work<=work_shifted;
  end
 end
`ifndef SYNTHESIS
 always @(posedge clk)if(ce&&!reset)begin
  if(normalizing&&(state==COLLECT||state==Q_READ||state==LOAD||state==DOWN))
   $fatal(1,"shared work lifetime overlap");
  if(work_load_delta&&(work_load_geo||work_shift_geo||work_shift_delta))
   $fatal(1,"shared work load priority collision");
  if(work_shift_geo&&work_shift_delta)$fatal(1,"shared work shift collision");
 end
`endif

 always @(posedge clk)begin
  if(reset)begin
   state<=IDLE;expected<=0;edge_no<=0;field<=0;edge_exp<=0;remaining<=0;
   exp0<=0;exp1<=0;exp2<=0;common_e<=0;negative<=0;sigma<=0;rgb<=0;tri_shift<=0;
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
        state<=LOAD;
       end
      end
     end
     Q_READ:state<=LOAD;
     LOAD:begin negative<=pool_q[47];remaining<=edge_exp-work_shift;
      state<=DOWN;
     end
     DOWN:begin
      if(remaining<=4)begin
       if(remaining==3)remaining<=1;
       else remaining<=0;
      end else remaining<=remaining-4;
     end
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
    if(quant_finish)begin
     if(field==2)begin
      field<=0;edge_exp<=0;
      if(edge_no==2)state<=TAIL;
      else begin edge_no<=edge_no+1'b1;state<=COLLECT;end
     end else begin field<=field+1'b1;state<=LOAD;end
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
  if(packed_write&&(pw_low0_we||pw_low1_we||in_index!=expected))$fatal(1,"packed import layout");
  if(state==CORE&&core_c_we&&core_cw>=8&&core_cw!=11)$fatal(1,"packed lifetime overwritten");
 end
`endif
endmodule

