// V/T/R/I arithmetic is adopted unchanged; output pool lifetime is external.
// 全V→全T；按edge寻址微核与标量h插值流水，无raw quad FIFO。
// 本T的准备与算术返回排空后换T；已发布结果由后半独立持有。
module gpu_v2_vliw_frontend(
 input clk,reset,ce,start,slot,
 input dma_we,output dma_ready,input [9:0] dma_addr,input [63:0] dma_data,input output_ready,
 input table_valid,output table_ready,input [7:0] table_index,input [35:0] table_data,
 input [31:0] cfg_far_z,input [4:0] cfg_tex_w_log2,cfg_tex_h_log2,cfg_max_mip,input signed [7:0] cfg_lod_bias,
 output quad_valid,input quad_ready,input[2:0]quad_id,
 output[7:0]quad_x,output[6:0]quad_y,output quad_rgb,output[1:0]phase,
 output quad_end_valid,output[2:0]quad_end_id,output[3:0]quad_end_mask,
 output result_we,output[6:0]result_wa,output[35:0]result_wd,
 output depth_event_valid,output[4:0]depth_event_pid,output[15:0]depth_event_z16,output depth_event_pass,
 output lod_valid,output[2:0]lod_id,output[7:0]lod_value,
 output frontend_busy,raster_busy,interpolator_busy,output meshlet_done,meshlet_ready,done_raster,
 output [3:0] frontend_error,raster_error,output [16:0] interpolator_error,
 output [6:0] published_vertices,
 output idle,
 output emit_valid,output [9:0] emit_addr,output [35:0] emit_data
);
 wire fpv,fpr,fwv,fwr;wire [35:0] fd;wire [4:0] fwi;wire [53:0] fwd;
 wire bridge_input_ready,geo_we;wire [3:0]geo_wa;wire [29:0]geo_wd;
 wire bridge_busy;wire [3:0] bridge_error,core_error;
 wire rqv,rqr,i_drained;
 wire[1:0]sample_lane;wire run_enable,sample_valid,sample_cover,sample_anchor;
 wire[29:0]sample_h;wire[7:0]qx;wire[6:0]qy;wire[3:0]qm;
 wire s_busy,s_done;wire header_take;
 wire bound_valid,bound_empty,origin_we;wire[4:0]bound_xlo,bound_xhi;wire[3:0]bound_ylo,bound_yhi;wire[1:0]origin_edge;wire[29:0]origin_data;
 reg active_config,table_window,packet_window;
 assign quad_valid=rqv&&run_enable;
 assign quad_x=qx;assign quad_y=qy;assign quad_end_mask=qm;
 wire local_drain=!raster_busy&&i_drained;
 wire allow_table=table_window&&!reset;
 wire permit_packet=packet_window&&!reset;
 assign fpr=permit_packet;
 wire packet_valid=fpv&&fpr;
 assign header_take=ce&&packet_valid;
 wire raw_word_valid=fwv&&active_config&&!reset;
 wire i_table_ready;
 assign table_ready=i_table_ready&&allow_table;
 assign fwr=active_config&&bridge_input_ready&&!reset;
 assign idle=packet_window&&!frontend_busy&&!fpv&&!fwv;
 assign raster_error=core_error|bridge_error;
 shared_meshlet meshlet(
  .clk(clk),.reset(reset),.ce(ce),.start(start&&idle&&!table_valid),.slot(slot),
  .dma_we(dma_we),.dma_ready(dma_ready),.dma_addr(dma_addr),.dma_data(dma_data),.output_ready(output_ready),
  .packet_ready(fpr),.word_ready(fwr),.busy(frontend_busy),.done(meshlet_done),.ready(meshlet_ready),.error(frontend_error),
  .published_vertices(published_vertices),.packet_valid(fpv),.descriptor(fd),.word_valid(fwv),.word_index(fwi),.word_data(fwd),
  .emit_valid(emit_valid),.emit_addr(emit_addr),.emit_data(emit_data)
 );
 rc_raster raster(
 .bbox_done(bound_valid),.bbox_empty(bound_empty),.bbox_xlo(bound_xlo),.bbox_xhi(bound_xhi),.bbox_ylo(bound_ylo),.bbox_yhi(bound_yhi),.bbox_rf_we(origin_we),.bbox_edge(origin_edge),.bbox_rf_wd(origin_data),
  .clk(clk),.reset(reset),.ce(ce),.start(packet_valid),
  .geo_we(geo_we),.geo_wa(geo_wa),.geo_wd(geo_wd),
  .phase(phase),.run_enable(run_enable),.quad_valid(rqv),.quad_ready(rqr),.quad_x(qx),.quad_y(qy),.quad_mask(qm),
 .sample_valid(sample_valid),.sample_lane(sample_lane),.sample_cover(sample_cover),.sample_anchor(sample_anchor),.sample_h(sample_h),.quad_end_valid(quad_end_valid),
  .busy(raster_busy),.done(done_raster),.error(core_error));
 gpu_v2_vliw_interpolator interpolator(
 .bound_valid(bound_valid),.bound_empty(bound_empty),.bound_xlo(bound_xlo),.bound_xhi(bound_xhi),.bound_ylo(bound_ylo),.bound_yhi(bound_yhi),.origin_we(origin_we),.origin_edge(origin_edge),.origin_data(origin_data),
  .clk(clk),.reset(reset),.ce(ce),.table_valid(table_valid&&allow_table),.table_ready(i_table_ready),.table_index(table_index),.table_data(table_data),
  .prepare_start(header_take),.descriptor(fd),.raw_valid(raw_word_valid),.raw_ready(bridge_input_ready),.raw_index(fwi),.raw_data(fwd),
  .geo_we(geo_we),.geo_wa(geo_wa),.geo_wd(geo_wd),
  .cfg_far_z(cfg_far_z),.cfg_tex_u(cfg_tex_w_log2),.cfg_tex_v(cfg_tex_h_log2),.cfg_max_mip(cfg_max_mip),.cfg_bias(cfg_lod_bias),
  .prepare_import_busy(bridge_busy),.prepare_error(bridge_error),
  .quad_valid(rqv),.quad_ready(rqr),.quad_x(qx),.quad_y(qy),.phase(phase),.run_enable(run_enable),
 .sample_valid(sample_valid),.sample_lane(sample_lane),.sample_cover(sample_cover),.sample_anchor(sample_anchor),.sample_h(sample_h),.quad_end_valid(quad_end_valid),.quad_mask(qm),
 .s_busy(s_busy),.s_done(s_done),
  .pool_ready(quad_ready),.pool_quad_id(quad_id),.pool_rgb(quad_rgb),.pool_end_id(quad_end_id),
  .result_we(result_we),.result_wa(result_wa),.result_wd(result_wd),
  .depth_event_valid(depth_event_valid),.depth_event_pid(depth_event_pid),.depth_event_z16(depth_event_z16),.depth_event_pass(depth_event_pass),
  .lod_valid(lod_valid),.lod_id(lod_id),.lod_value(lod_value),
  .busy(interpolator_busy),.drained(i_drained),.error(interpolator_error));
 always @(posedge clk)begin
  if(reset)begin active_config<=0;table_window<=0;packet_window<=0;end
  else if(ce)begin
   if(start&&idle&&!table_valid)table_window<=0;else table_window<=idle;
   if(header_take)packet_window<=0;else packet_window<=!active_config&&local_drain;
   if(header_take)active_config<=1;
   else if(active_config&&local_drain)active_config<=0;
  end
 end
`ifndef SYNTHESIS
 // 旧阶段事实只做断言影子；模块生命周期本身负责运行时屏障。
 reg sim_import_complete,sim_raster_done_seen;
 always @(posedge clk)begin
  if(reset)begin sim_import_complete<=0;sim_raster_done_seen<=0;end
  else if(ce)begin
   if(header_take)begin sim_import_complete<=0;sim_raster_done_seen<=0;end
   if(fwv&&fwr&&fwi==18)sim_import_complete<=1;
   if(done_raster)sim_raster_done_seen<=1;
   if(local_drain&&bridge_busy)$fatal(1,"模块排空未涵盖导入");
   if(header_take&&(active_config||!local_drain))$fatal(1,"T配置生命周期重叠");
   if(active_config&&local_drain&&(!sim_import_complete||!(sim_raster_done_seen||done_raster)))
    $fatal(1,"T屏障在导入或光栅完成前释放");
   if(fwv&&fwr&&sim_import_complete)$fatal(1,"结束导入后仍消费原始字");
  end
 end
`endif
endmodule
