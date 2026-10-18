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
 output stream_param_bank,output[53:0]stream_param_data,
 
 output[4:0]lod_cfg_tex_u,lod_cfg_tex_v,lod_cfg_max_mip,output signed[7:0]lod_cfg_bias,
 output operand_we,output[3:0]operand_wa,output[17:0]operand_wd,
 output operand_re,output[3:0]operand_ra,
 output wide_valid,output wide_asign,
 output bound_valid,bound_empty,output[4:0]bound_xlo,bound_xhi,output[3:0]bound_ylo,bound_yhi,
 input[53:0]wide_result_data,
 output busy,import_busy,math_busy,done,output reg[3:0]error
);
 localparam IDLE=0,COLLECT=1,LOAD=2,DOWN=3,Q_SEND=4,TAIL=5,SECOND=6,THIRD=7,
 CORE=8,
 CFG_COMMIT=26,FINISH=28,FAULT=29,Q_READ=30;
 reg[4:0]state,expected;reg[1:0]edge_no,field;
 reg[4:0]edge_exp,remaining,exp0,exp1,exp2,common_e;
 reg[52:0]work;wire[46:0]magnitude=work[46:0];reg negative,sigma,rgb,normal_mode;reg[3:0]tri_shift;
 reg[31:0]far_z;reg[4:0]tex_u,tex_v,max_mip;reg signed[7:0]bias;
 wire[52:0]delta;reg[5:0]delta_e;reg normalizing;

 reg[2:0]seed_sign_x,seed_sign_y;
 wire core_attr_read,core_attr_send,core_attr_bank,core_round_init;wire[1:0]core_attr_index;
 wire[35:0]low_q=pw_q[35:0];
 assign busy=state!=IDLE;assign import_busy=(state>=COLLECT&&state<=THIRD)||state==Q_READ;
 assign math_busy=state==CORE||state==CFG_COMMIT||state==FINISH;
 assign done=state==FINISH;
 wire input_xy,input_z,input_attr;
 assign in_ready=!reset&&(state==COLLECT||(state==TAIL&&(in_index==9||input_z))||
  (state==SECOND&&input_xy)||state==THIRD);
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
 assign geo_we=!reset&&state==Q_SEND;
 // 按最终R工作池的物理行发射；符号方向直接复用PB已经需要的canonical。
 assign geo_wa={field,edge_no};
 assign geo_wd=field==2 ? canonical[29:0] : {canonical[28:0],1'b0};
 wire q_take=geo_we;
 assign input_xy=in_index==10||in_index==13||in_index==16;
 assign input_z=in_index==11||in_index==14||in_index==17;
 assign input_attr=in_index==12||in_index==15||in_index==18;
 reg[1:0]input_vertex;
 always @*case(in_index)10,11,12:input_vertex=0;13,14,15:input_vertex=1;default:input_vertex=2;endcase
 function [11:0] normal12;
  input [17:0] raw;input mode;
  reg [17:0] value;reg signed [12:0] rounded;
  begin
   if(mode)value=raw;else value={raw[15:0],2'b0};
   rounded=$signed({value[17],value[17:6]});
   if(value[5]&&((|value[4:0])||value[6]))rounded=rounded+13'sd1;
   if(rounded>13'sd2047)normal12=12'h7ff;
   else if(rounded< -13'sd2048)normal12=12'h800;
   else normal12=rounded[11:0];
  end
 endfunction
 // 直接消费前端保持Q；在本字最后固定写槽确认，所有子项只存最终参数池。
 wire[17:0]normal_input=state==SECOND ? in_data[35:18] : in_data[17:0];
 wire[11:0]normal_value=normal12(normal_input,normal_mode);
 reg[15:0]raw_attribute;
 always @*begin
  if(state==TAIL)begin
   if(input_xy||input_z)raw_attribute={{4{normal_value[11]}},normal_value};
   else if(rgb)raw_attribute={8'd0,in_data[7:0]};else raw_attribute=in_data[15:0];
  end else if(state==SECOND)begin
   if(!input_attr)raw_attribute={{4{normal_value[11]}},normal_value};
   else if(rgb)raw_attribute={8'd0,in_data[15:8]};else raw_attribute=in_data[31:16];
  end else begin
   if(rgb)raw_attribute={8'd0,in_data[23:16]};else raw_attribute=0;
  end
 end
 wire tail_first=state==TAIL&&in_valid&&!reset;
 wire attribute_write=(tail_first&&(input_xy||input_z||input_attr))||state==SECOND||state==THIRD;
 wire[3:0]attribute_row=state==TAIL ? (input_xy ? 4'd0 : input_z ? 4'd2 : 4'd3) :
  state==SECOND ? (input_attr ? 4'd4 : 4'd1) : 4'd5;
 wire[1:0]attribute_vertex=input_vertex;
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
 wire seed_take=state==Q_SEND&&q_take&&field<2;
 // 在已有A_READ空闲写槽形成三份公共指数对齐常量；不增加阶段。
 wire[2:0]factor_index={core_attr_index,core_attr_bank};
 wire factor_write=core_attr_read&&factor_index<3&&!rgb;
 reg[4:0]factor_shift;
 always @*case(factor_index)
  0:factor_shift=common_e-exp0;
  1:factor_shift=common_e-exp1;
  default:factor_shift=common_e-exp2;
 endcase
 reg[17:0]factor;
 always @*begin
  case(factor_shift)
   5'd0:factor=18'd131072;
   5'd1:factor=18'd65536;
   5'd2:factor=18'd32768;
   5'd3:factor=18'd16384;
   5'd4:factor=18'd8192;
   5'd5:factor=18'd4096;
   5'd6:factor=18'd2048;
   5'd7:factor=18'd1024;
   5'd8:factor=18'd512;
   5'd9:factor=18'd256;
   5'd10:factor=18'd128;
   5'd11:factor=18'd64;
   5'd12:factor=18'd32;
   5'd13:factor=18'd16;
   5'd14:factor=18'd8;
   5'd15:factor=18'd4;
   5'd16:factor=18'd2;
   5'd17:factor=18'd1;
   default:factor=0;
  endcase
 end
 wire core_a_re,core_a_we,core_b_re,core_b_we,core_c_re,core_c_we,core_issue,core_asign,core_active,core_done;
 wire[3:0]core_ar,core_br,core_bw,core_cr,core_cw;wire[17:0]core_ad;wire[35:0]core_bd;wire[53:0]core_cd;
 wire core_start=state==THIRD&&in_index==18&&!reset;
 rp_prepare_core core(clk,reset,ce,core_start,rgb,{seed_sign_y,seed_sign_x},pw_q[35:0],pw_q[89:36],wide_result_data,
  core_a_re,core_ar,core_a_we,core_ad,core_b_re,core_br,core_b_we,core_bw,core_bd,
  core_c_re,core_cr,core_c_we,core_cw,core_cd,core_issue,core_asign,core_active,core_done,
  bound_valid,bound_empty,bound_xlo,bound_xhi,bound_ylo,bound_yhi,core_attr_read,core_attr_send,core_attr_index,core_attr_bank,core_round_init);
 wire attr_first=tail_first&&input_attr;
 wire attr_second=state==SECOND&&input_attr;
 assign operand_we=!reset&&(state==CORE?(core_a_we||factor_write):factor_write||attr_first||attr_second||(state==Q_SEND&&q_take&&field<2));
 wire[3:0]weight_address=field==1?4'd9+{2'd0,edge_no}:edge_no==0?4'd0:4'd6+{2'd0,edge_no};
 assign operand_wa=state==CORE?(factor_write?4'd12+{1'b0,factor_index}:4'd15):state==Q_SEND?weight_address:
  factor_write?4'd12+{1'b0,factor_index}:attr_first?4'd1+{2'd0,input_vertex}:4'd4+{2'd0,input_vertex};
 // PA属性直接来自同一份保持Q的低/高16位，包含RGB无消费者部分也保持原值。
 assign operand_wd=state==CORE?(factor_write?factor:core_ad):state==Q_SEND?magnitude[17:0]:
  factor_write?factor:attr_first?{2'd0,in_data[15:0]}:{2'd0,in_data[31:16]};
 assign operand_re=state==CORE&&core_a_re;assign operand_ra=core_ar;
 assign wide_valid=state==CORE&&core_issue;assign wide_asign=core_asign;
 // 尾属性第一项在接受沿落池，余项独占SECOND/THIRD；最后写入沿启动准备ROM。
 // 输入直接形成列：B低16为A0、高16为A1，C低16为A2；4位切片填充不参与运算。
 wire signed_attribute=core_attr_index==0||(core_attr_index==1&&!core_attr_bank);
 reg signed[17:0]a0,a1,current_a;
 always @*begin
  a0={2'd0,pw_q[15:0]};a1={2'd0,pw_q[35:20]};current_a={2'd0,pw_q[51:36]};
  if(signed_attribute)begin
   a0={{2{pw_q[15]}},pw_q[15:0]};a1={{2{pw_q[35]}},pw_q[35:20]};current_a={{2{pw_q[51]}},pw_q[51:36]};
  end
 end
 wire signed[17:0]da0=a0-current_a,da1=a1-current_a;
 assign stream_param_data={current_a,da1,da0};

 always @*begin
  pw_low0_we=0;pw_low1_we=0;pw_high_we=0;pw_wa=0;pw_low_wd=0;pw_high_wd=0;pw_high_wa=0;pw_re=0;pw_ra=0;pw_acc_re=0;pw_acc_ra=0;
  if(state==COLLECT&&input_take&&in_index==expected)begin
   pw_high_we=1;pw_high_wa=4'd8+{2'd0,collect_field(in_index)};
   pw_high_wd={6'd0,raw_value[46],raw_magnitude};
  end
  if(state==COLLECT&&input_take&&in_index==expected&&is_c)begin pw_acc_re=1;pw_acc_ra=8;end
  if(state==Q_SEND&&q_take)begin
   pw_low0_we=1;pw_low1_we=1;
   pw_wa=(field==0?4'd6:field==1?4'd9:4'd12)+{2'd0,edge_no};pw_low_wd=canonical;
   if(field<2)begin pw_acc_re=1;pw_acc_ra=4'd9+{2'd0,field};end
  end
  if(state==TAIL&&input_take&&in_index==9)begin pw_high_we=1;pw_high_wa=6;end
  if(core_round_init)begin pw_high_we=1;pw_high_wa=7;pw_high_wd=54'd131071;end
  if(attribute_write)begin
   pw_wa=attribute_row;pw_high_wa=attribute_row;
   pw_low_wd={raw_attribute,4'd0,raw_attribute};pw_high_wd={20'd0,raw_attribute};
   if(attribute_vertex==0)pw_low0_we=1;
   else if(attribute_vertex==1)pw_low1_we=1;
   else pw_high_we=1;
  end
  case(state)
   Q_READ:begin pw_acc_re=1;pw_acc_ra=4'd8+{2'd0,field};end
   CORE:begin
    pw_re=core_b_re;pw_ra=core_br;pw_acc_re=core_c_re;pw_acc_ra=core_cr;
    pw_low0_we=core_b_we;pw_low1_we=core_b_we;pw_wa=core_bw;pw_low_wd=core_bd;
    if(core_c_we)begin pw_high_we=1;pw_high_wa=core_cw;pw_high_wd=core_cd;end
   end
   CFG_COMMIT:if(cfg_apply)begin
    pw_low0_we=1;pw_low1_we=1;pw_high_we=1;pw_wa=4;pw_high_wa=4;pw_low_wd={5'd0,depth_k};
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
 reg[2:0]work_shift;
 reg[52:0]work_shifted;
 always @*begin
  work_shift=3'd2;
  if(work_shift_geo)begin
   if(remaining>=4)work_shift=3'd4;
   else if(remaining==1)work_shift=3'd1;
  end else if(delta[51])work_shift=3'd1;
  case(work_shift)
   3'd1:work_shifted={1'b0,work[52:1]};
   3'd4:work_shifted={4'b0,work[52:4]};
   default:work_shifted={2'b0,work[52:2]};
  endcase
 end
 always @(posedge clk)begin
  if(reset)work<=0;
  else if(ce)begin
   if(work_load_delta)work<=work_delta_input;
   else if(work_load_geo)work<={6'b0,pool_q[46:0]};
   else if(work_shift_geo||work_shift_delta)work<=work_shifted;
  end
 end
`ifndef SYNTHESIS
 always @(posedge clk)if(ce&&!reset)begin
  if(normalizing&&(state==COLLECT||state==Q_READ||state==LOAD||state==DOWN||state==Q_SEND))
   $fatal(1,"shared work lifetime overlap");
  if(work_load_delta&&(work_load_geo||work_shift_geo||work_shift_delta))
   $fatal(1,"shared work load priority collision");
  if(work_shift_geo&&work_shift_delta)$fatal(1,"shared work shift collision");
 end
`endif

 always @(posedge clk)begin
  if(reset)begin
   state<=IDLE;expected<=0;edge_no<=0;field<=0;edge_exp<=0;remaining<=0;
   exp0<=0;exp1<=0;exp2<=0;common_e<=0;negative<=0;sigma<=0;rgb<=0;normal_mode<=0;tri_shift<=0;
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
     sigma<=descriptor[22];rgb<=descriptor[23];normal_mode<=descriptor[24];tri_shift<=descriptor[21:18];
     far_z<=cfg_far_z;tex_u<=cfg_tex_u;tex_v<=cfg_tex_v;max_mip<=cfg_max_mip;bias<=cfg_bias;
    end
   end else begin
    if(seed_take)begin
     if(field==0)seed_sign_x[edge_no]<=negative^sigma;
     else seed_sign_y[edge_no]<=negative^sigma;
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
     LOAD:begin negative<=pool_q[47];remaining<=edge_exp;
      if(edge_exp==0)state<=Q_SEND;else state<=DOWN;
     end
     DOWN:begin
      if(remaining<=4)begin
       if(remaining==3)remaining<=1;
       else begin remaining<=0;state<=Q_SEND;end
      end else remaining<=remaining-4;
     end
     Q_SEND:if(q_take)begin
      if(field==2)begin
       field<=0;edge_exp<=0;
       if(edge_no==2)state<=TAIL;
       else begin edge_no<=edge_no+1'b1;state<=COLLECT;end
      end else begin field<=field+1'b1;state<=LOAD;end
     end
     TAIL:if(in_valid)begin
      if(in_index!=expected)begin error[0]<=1;state<=FAULT;end
      else if(in_index==9)begin
       expected<=expected+1'b1;
       if(in_data==0||in_data[53])error[1]<=1;
       delta_e<=52;normalizing<=1;
      end else if(input_xy||input_attr)state<=SECOND;
      else if(input_z)expected<=expected+1'b1;
      else begin error[0]<=1;state<=FAULT;end
     end
     SECOND:if(input_attr)state<=THIRD;
      else begin expected<=expected+1'b1;state<=TAIL;end
     THIRD:begin expected<=expected+1'b1;if(in_index==18)state<=CORE;else state<=TAIL;end
    CORE:if(core_done)state<=CFG_COMMIT;
    CFG_COMMIT:if(cfg_apply)state<=FINISH;
    FINISH:state<=IDLE;
     default:begin end
    endcase
   end
  end
 end
`ifndef SYNTHESIS
 // 总线由源单一保持，必须覆盖本字全部写槽；不是逐字申请或端口共享。
 reg sim_hold;reg[58:0]sim_word;
 always @(posedge clk)begin
  if(reset)begin sim_hold<=0;sim_word<=0;end
  else begin
   if(sim_hold&&(!in_valid||{in_index,in_data}!==sim_word))$fatal(1,"尾字固定子调度期间源Q改变");
   if(ce)begin
    if(state==TAIL&&in_valid&&!in_ready)begin sim_hold<=1;sim_word<={in_index,in_data};end
    else if(in_ready)sim_hold<=0;
    if((state==SECOND||state==THIRD)&&(!in_valid||in_index!=expected))$fatal(1,"尾字子调度缺少稳定源");
    if(attribute_write&&(attribute_vertex==3||in_index!=expected))$fatal(1,"尾属性最终地址错误");
   end
  end
 end
 always @(posedge clk)if(ce&&!reset&&geo_we)begin
  if(field<2&&canonical[35:17]!={19{canonical[17]}})$fatal(1,"几何N不能精确存入18位");
  if(field==2&&canonical[35:27]!={9{canonical[27]}})$fatal(1,"几何C不能精确存入28位");
 end
 always @(posedge clk)if(ce&&!reset)begin
  if(core_a_we&&factor_write)$fatal(1,"统一PA写日历冲突");
  if(attribute_write&&state==CORE&&core_c_we)$fatal(1,"尾属性与统一PC退休重叠");
 end
`endif
endmodule

