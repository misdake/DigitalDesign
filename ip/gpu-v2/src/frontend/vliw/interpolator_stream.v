// R到计算无像素FIFO；仅最终quad槽有信用。
module gpu_v2_vliw_interpolator(
input clk,reset,ce,
 output bound_valid,bound_empty,output[4:0]bound_xlo,bound_xhi,output[3:0]bound_ylo,bound_yhi,
 
 input table_valid,output table_ready,input[7:0]table_index,input[35:0]table_data,
 input prepare_start,execution_idle,output raster_start,exec_idle,input[35:0]descriptor,
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
 wire cfg_apply,context_ready,sc_rgb;wire signed[7:0]sc_z_bias;wire[31:0]sc_far;
 wire sp_valid,sp_bank;wire[1:0]sp_index;wire[50:0]sp_data;
 wire[4:0]sc_shift0,sc_shift1,sc_shift2;
 wire[4:0]lc_tex_u,lc_tex_v,lc_max;wire signed[7:0]lc_bias;

 wire[3:0]s_error;wire[7:0]p_error;
 wire p_busy,s_raw_ready;reg rgb,active_bank,raster_pending;reg[2:0]current_id;reg replay;reg[3:0]replay_slot;
 wire engine_bound_valid,engine_bound_empty;

 wire quad_take=quad_valid&&quad_ready;
 wire[2:0]quad_id=pool_quad_id;
 wire[4:0]sample_pid={quad_take?quad_id:current_id,sample_lane};
 assign pool_rgb=rgb;
 assign pool_end_id=current_id;
 wire pixel_table_ready;
 assign busy=s_busy||p_busy||replay;
 assign drained=!busy;
 assign exec_idle=!p_busy;
 // R owns geometry only. It can import and traverse the next triangle while
 // old I returns still read the active coefficient/attribute context.
 wire discard_context=context_ready&&engine_bound_empty;
 assign raster_start=context_ready&&!engine_bound_empty&&execution_idle&&!raster_pending;
 // Empty work owns only the inactive page. Retire without touching the live
 // I context or waiting for old R/I; the next preparation reuses that page.
 wire execute_grant=discard_context||((execution_idle||raster_pending)&&!p_busy);
 wire canonical_we;wire[3:0]canonical_wa;wire[29:0]canonical_wd;
 wire[17:0]operand_value,dsp_a;wire dsp_asign;wire[35:0]dsp_b;wire[53:0]dsp_c;
 reg[3:0]pb_read_address,pc_read_address;
 wire[35:0]private_b;wire[47:0]private_c;
 wire replay_read=(replay&&replay_slot<9)||raster_start;
 wire[3:0]replay_index=raster_start?4'd0:replay_slot;
 reg[3:0]replay_address;
 always @*case(replay_index)
  0:replay_address=6;1:replay_address=9;2:replay_address=12;
  3:replay_address=7;4:replay_address=10;5:replay_address=13;
  6:replay_address=8;7:replay_address=11;default:replay_address=14;
 endcase
 wire[3:0]return_index=replay_slot-1'b1;
 reg[3:0]return_address;
 always @*case(return_index)
  0:return_address=0;1:return_address=4;2:return_address=8;
  3:return_address=1;4:return_address=5;5:return_address=9;
  6:return_address=2;7:return_address=6;default:return_address=10;
 endcase
 assign geo_we=replay&&replay_slot<=9&&!reset;
 assign geo_wa=return_address;
 assign geo_wd=return_address[3]?private_b[29:0]:{private_b[28:0],1'b0};
 assign bound_valid=((replay&&replay_slot==10)||discard_context)&&!reset;
 assign bound_empty=engine_bound_empty;
 wire coefficient_bwe=(pw_low0_we||pw_low1_we)&&(pw_wa==0||pw_wa==1||pw_wa==4);
 wire coefficient_cwe=pw_high_we&&pw_high_wa<4;
 always @(posedge clk)begin
  if(reset)begin pb_read_address<=0;pc_read_address<=0;replay<=0;replay_slot<=0;end
  else if(ce)begin
   if(replay_read)pb_read_address<=replay_address;
   else if(pw_re)pb_read_address<=pw_ra;
   if(pw_acc_re)pc_read_address<=pw_acc_ra;
   if(raster_start)begin replay<=1;replay_slot<=1;end
   else if(replay)begin
    if(replay_slot==10)begin replay<=0;replay_slot<=0;end
    else replay_slot<=replay_slot+1'b1;
   end
  end
 end
 iz_params_async #(.WIDTH(36)) work_b(clk,reset,ce,pw_low0_we||pw_low1_we,pw_wa,pw_low_wd,pb_read_address,private_b);
 iz_params_async #(.WIDTH(48),.PC_CONSTANTS(1)) work_c(clk,reset,ce,pw_high_we,pw_high_wa,pw_high_wd[47:0],pc_read_address,private_c);
 assign pw_q={ {6{private_c[47]}},private_c,private_b};
 iz_operand_table prepare_operand(clk,reset,ce,operand_we,operand_wa,operand_wd[16:0],operand_re,operand_ra,operand_value);
 iz_widemac prepare_dsp(clk,reset,ce,dsp_a,dsp_asign,dsp_b,dsp_c,result_data);
`ifndef SYNTHESIS
 always @(posedge clk)if(ce&&!reset)begin
  if(replay_read&&pw_re)$fatal(1,"prepare and geometry replay share PB read");
  if(replay_read&&(pw_low0_we||pw_low1_we)&&replay_address==pw_wa)$fatal(1,"PB replay overwritten before read");
  if(cfg_apply&&!execute_grant)$fatal(1,"configuration published before old execution drain");
  if(raster_start&&!execution_idle)$fatal(1,"geometry handoff before old raster drain");
  if(sample_valid&&raster_pending)$fatal(1,"new raster sample before I context switch");
  if(discard_context&&(cfg_apply||raster_start||replay))$fatal(1,"empty context acquired runtime ownership");
 end
`endif
 assign raw_ready=s_raw_ready;assign prepare_error=s_error;
 assign run_enable=!raster_pending;
 assign quad_ready=pool_ready&&run_enable;
 assign table_ready=pixel_table_ready&&drained;
 // 低四位保留外层诊断接口布局；确定性内部生产次序改由仿真断言验证。
 assign error={1'b0,p_error,s_error,4'b0};

 always @(posedge clk)begin
  if(reset)begin rgb<=0;current_id<=0;active_bank<=0;raster_pending<=0;end
  else if(ce)begin
   if(raster_start)raster_pending<=1;
   if(cfg_apply)raster_pending<=0;
   if(cfg_apply)begin rgb<=sc_rgb;active_bank<=!active_bank;end
   if(quad_take)current_id<=quad_id;
  end
 end
 rp_prepare prepare(
 .clk(clk),.reset(reset),.ce(ce),.start(prepare_start),.execute_grant(execute_grant),
 .operand_value(operand_value),.dsp_a(dsp_a),.dsp_asign(dsp_asign),.dsp_b(dsp_b),.dsp_c(dsp_c),.descriptor(descriptor),
 .cfg_far_z(cfg_far_z),.cfg_tex_u(cfg_tex_u),.cfg_tex_v(cfg_tex_v),.cfg_max_mip(cfg_max_mip),.cfg_bias(cfg_bias),
 .in_valid(raw_valid),.in_ready(s_raw_ready),.in_index(raw_index),.in_data(raw_data),
 .geo_we(canonical_we),.geo_wa(canonical_wa),.geo_wd(canonical_wd),
 .pw_low_we(pw_low_we),.pw_low0_we(pw_low0_we),.pw_low1_we(pw_low1_we),.pw_high_we(pw_high_we),.pw_wa(pw_wa),.pw_low_wd(pw_low_wd),.pw_high_wd(pw_high_wd),.pw_high_wa(pw_high_wa),.pw_re(pw_re),.pw_ra(pw_ra),.pw_acc_re(pw_acc_re),.pw_acc_ra(pw_acc_ra),.pw_q(pw_q),
 .cfg_apply(cfg_apply),.context_ready(context_ready),.stream_cfg_z_bias(sc_z_bias),.stream_cfg_far(sc_far),.stream_cfg_rgb(sc_rgb),
 .stream_cfg_shift0(sc_shift0),.stream_cfg_shift1(sc_shift1),.stream_cfg_shift2(sc_shift2),
 .stream_param_valid(sp_valid),.stream_param_index(sp_index),.stream_param_bank(sp_bank),.stream_param_data(sp_data),
 .lod_cfg_tex_u(lc_tex_u),.lod_cfg_tex_v(lc_tex_v),.lod_cfg_max_mip(lc_max),.lod_cfg_bias(lc_bias),
 .operand_we(operand_we),.operand_wa(operand_wa),.operand_wd(operand_wd),.operand_re(operand_re),.operand_ra(operand_ra),
 .wide_valid(wide_valid),.wide_asign(wide_asign),
 .bound_valid(engine_bound_valid),.bound_empty(engine_bound_empty),.bound_xlo(bound_xlo),.bound_xhi(bound_xhi),.bound_ylo(bound_ylo),.bound_yhi(bound_yhi),
 .wide_result_data(result_data),
 .busy(s_busy),.import_busy(prepare_import_busy),.done(s_done),.error(s_error));

 rc_compute compute(
 .clk(clk),.reset(reset),.ce(ce),.phase(phase),
 .read_bank(active_bank),.write_bank(!active_bank),
 .coefficient_bwe(coefficient_bwe),.coefficient_cwe(coefficient_cwe),
 .coefficient_bwa({!active_bank,pw_wa[2:0]}),.coefficient_cwa({!active_bank,pw_high_wa[2:0]}),
 .coefficient_bwd(pw_low_wd),.coefficient_cwd(pw_high_wd[35:0]),
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
