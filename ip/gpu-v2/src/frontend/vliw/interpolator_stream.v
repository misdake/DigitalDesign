// R到计算无像素FIFO；仅最终quad槽有信用。
module gpu_v2_vliw_interpolator(
input clk,reset,ce,
 output bound_valid,bound_empty,output[4:0]bound_xlo,bound_xhi,output[3:0]bound_ylo,bound_yhi,
 
 input table_valid,output table_ready,input[7:0]table_index,input[35:0]table_data,
 input prepare_start,input[35:0]descriptor,
 input raw_valid,output raw_ready,input[4:0]raw_index,input[53:0]raw_data,
 output geo_we,output[3:0]geo_wa,output[29:0]geo_wd,
 input[31:0]cfg_far_z,input[4:0]cfg_tex_u,cfg_tex_v,cfg_max_mip,input signed[7:0]cfg_bias,
 output prepare_import_busy,output[3:0]prepare_error,
 input quad_valid,output quad_ready,input[7:0]quad_x,input[6:0]quad_y,
 input sample_valid,input[1:0]sample_lane,input sample_cover,sample_anchor,input[29:0]sample_h,
 input quad_end_valid,input[3:0]quad_mask,output[1:0]phase,output run_enable,
 input pool_ready,input[2:0]pool_quad_id,
 output pool_rgb,output[2:0]pool_end_id,
 output result_we,output[6:0]result_wa,output[35:0]result_wd,
 output depth_event_valid,output[4:0]depth_event_pid,output[15:0]depth_event_z16,output depth_event_pass,
 output lod_valid,output[2:0]lod_id,output[7:0]lod_value,
 output busy,drained,output[16:0]error,output s_busy,s_done
);
 wire pw_low_we,pw_low0_we,pw_low1_we,pw_high_we,pw_re,pw_acc_re;wire[3:0]pw_acc_ra;wire[3:0]pw_wa,pw_ra;wire[35:0]pw_low_wd;wire[53:0]pw_high_wd;wire[3:0]pw_high_wa;wire[89:0]pw_q;
 wire operand_we,operand_re,wide_valid,wide_asign;wire[3:0]operand_wa;wire[6:0]operand_ra;
 wire[17:0]operand_wd;wire[53:0]result_data;
 wire cfg_apply,sc_rgb;wire signed[7:0]sc_z_bias;wire[31:0]sc_far;
 wire sp_valid,sp_bank;wire[1:0]sp_index;wire[50:0]sp_data;
 wire[4:0]sc_shift0,sc_shift1,sc_shift2;
 wire[4:0]lc_tex_u,lc_tex_v,lc_max;wire signed[7:0]lc_bias;

 wire[3:0]s_error;wire[7:0]p_error;
 wire p_busy,s_raw_ready;reg configured,rgb;reg[2:0]current_id;

 wire quad_take=quad_valid&&quad_ready;
 wire[2:0]quad_id=pool_quad_id;
 wire[4:0]sample_pid={quad_take?quad_id:current_id,sample_lane};
 assign pool_rgb=rgb;
 assign pool_end_id=current_id;
 wire pixel_table_ready;
 assign busy=s_busy||p_busy;
 assign drained=!busy;
 assign raw_ready=s_raw_ready;assign prepare_error=s_error;
 assign run_enable=configured&&!s_busy;
 assign quad_ready=pool_ready&&run_enable;
 assign table_ready=pixel_table_ready&&drained;
 // 低四位保留外层诊断接口布局；确定性内部生产次序改由仿真断言验证。
 assign error={1'b0,p_error,s_error,4'b0};

 always @(posedge clk)begin
  if(reset)begin configured<=0;rgb<=0;current_id<=0;end
  else if(ce)begin
   if(prepare_start)configured<=0;
   if(s_done)configured<=1;
   if(cfg_apply)rgb<=sc_rgb;
   if(quad_take)current_id<=quad_id;
  end
 end
 rp_prepare prepare(
 .clk(clk),.reset(reset),.ce(ce),.start(prepare_start),.descriptor(descriptor),
 .cfg_far_z(cfg_far_z),.cfg_tex_u(cfg_tex_u),.cfg_tex_v(cfg_tex_v),.cfg_max_mip(cfg_max_mip),.cfg_bias(cfg_bias),
 .in_valid(raw_valid),.in_ready(s_raw_ready),.in_index(raw_index),.in_data(raw_data),
 .geo_we(geo_we),.geo_wa(geo_wa),.geo_wd(geo_wd),
 .pw_low_we(pw_low_we),.pw_low0_we(pw_low0_we),.pw_low1_we(pw_low1_we),.pw_high_we(pw_high_we),.pw_wa(pw_wa),.pw_low_wd(pw_low_wd),.pw_high_wd(pw_high_wd),.pw_high_wa(pw_high_wa),.pw_re(pw_re),.pw_ra(pw_ra),.pw_acc_re(pw_acc_re),.pw_acc_ra(pw_acc_ra),.pw_q(pw_q),
 .cfg_apply(cfg_apply),.stream_cfg_z_bias(sc_z_bias),.stream_cfg_far(sc_far),.stream_cfg_rgb(sc_rgb),
 .stream_cfg_shift0(sc_shift0),.stream_cfg_shift1(sc_shift1),.stream_cfg_shift2(sc_shift2),
 .stream_param_valid(sp_valid),.stream_param_index(sp_index),.stream_param_bank(sp_bank),.stream_param_data(sp_data),
 .lod_cfg_tex_u(lc_tex_u),.lod_cfg_tex_v(lc_tex_v),.lod_cfg_max_mip(lc_max),.lod_cfg_bias(lc_bias),
 .operand_we(operand_we),.operand_wa(operand_wa),.operand_wd(operand_wd),.operand_re(operand_re),.operand_ra(operand_ra),
 .wide_valid(wide_valid),.wide_asign(wide_asign),
 .bound_valid(bound_valid),.bound_empty(bound_empty),.bound_xlo(bound_xlo),.bound_xhi(bound_xhi),.bound_ylo(bound_ylo),.bound_yhi(bound_yhi),
 .wide_result_data(result_data),
 .busy(s_busy),.import_busy(prepare_import_busy),.done(s_done),.error(s_error));

 rc_compute compute(
 .clk(clk),.reset(reset),.ce(ce),.phase(phase),
 .prepare_owner(s_busy),.prep_pw_re(pw_re),.prep_pw_ra(pw_ra),.prep_pw_acc_re(pw_acc_re),.prep_pw_acc_ra(pw_acc_ra),
 .prep_pw_low0_we(pw_low0_we),.prep_pw_low1_we(pw_low1_we),.prep_pw_high_we(pw_high_we),.prep_pw_wa(pw_wa),
 .prep_pw_low_wd(pw_low_wd),.prep_pw_high_wd(pw_high_wd),.prep_pw_high_wa(pw_high_wa),.prep_pw_q(pw_q),
 .prep_operand_we(operand_we),.prep_operand_re(operand_re),.prep_operand_wa(operand_wa),.prep_operand_ra(operand_ra),.prep_operand_wd(operand_wd),
 .prep_wide_valid(wide_valid),.prep_wide_asign(wide_asign),.prep_result_data(result_data),
 .sample_valid(sample_valid),.sample_pid(sample_pid),.sample_cover(sample_cover),.sample_anchor(sample_anchor),.sample_h(sample_h),
 .cfg_valid(cfg_apply),.cfg_z_bias(sc_z_bias),.cfg_far(sc_far),.cfg_rgb(sc_rgb),
 .cfg_shift0(sc_shift0),.cfg_shift1(sc_shift1),.cfg_shift2(sc_shift2),
 .table_valid(table_valid&&drained),.table_ready(pixel_table_ready),.table_index(table_index),.table_data(table_data),
 .param_valid(sp_valid),.param_index(sp_index),.param_bank(sp_bank),.param_data(sp_data),
 .cfg_tex_u(lc_tex_u),.cfg_tex_v(lc_tex_v),.cfg_max_mip(lc_max),.cfg_bias(lc_bias),
 .quad_end_valid(quad_end_valid),.quad_end_qid(current_id),.quad_end_mask(quad_mask),
 .result_we(result_we),.result_wa(result_wa),.result_wd(result_wd),
 .depth_event_valid(depth_event_valid),.depth_event_pid(depth_event_pid),.depth_event_z16(depth_event_z16),.depth_event_pass(depth_event_pass),
 .lod_valid(lod_valid),.lod_id(lod_id),.lod_value(lod_value),.busy(p_busy),.error(p_error));
endmodule
