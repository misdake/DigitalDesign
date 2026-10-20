// Private next-triangle prepare overlaps live R/I. Runtime coefficient and
// attribute pages switch after old I returns drain. New R geometry and traversal
// may overlap that tail; samples wait for the context switch.
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
 wire bound_valid,bound_empty;wire[4:0]bound_xlo,bound_xhi;wire[3:0]bound_ylo,bound_yhi;
 wire raster_start,exec_idle;reg live_empty;reg[4:0]live_xlo,live_xhi;reg[3:0]live_ylo,live_yhi;
 always @(posedge clk)begin
  if(reset)begin live_empty<=0;live_xlo<=0;live_xhi<=24;live_ylo<=0;live_yhi<=14;end
  else if(ce&&raster_start)begin
   live_empty<=bound_empty;live_xlo<=bound_xlo;live_xhi<=bound_xhi;live_ylo<=bound_ylo;live_yhi<=bound_yhi;
  end
 end
 reg table_window,packet_window;
 assign quad_valid=rqv&&run_enable;
 assign quad_x=qx;assign quad_y=qy;assign quad_end_mask=qm;
 wire local_drain=!raster_busy&&exec_idle;
 wire allow_table=table_window&&!reset;
 wire permit_packet=packet_window&&!reset;
 assign fpr=permit_packet;
 wire packet_valid=fpv&&fpr;
 assign header_take=ce&&packet_valid;
 wire raw_word_valid=fwv&&s_busy&&!reset;
 wire i_table_ready;
 assign table_ready=i_table_ready&&allow_table;
 assign fwr=s_busy&&bridge_input_ready&&!reset;
 assign idle=packet_window&&local_drain&&!s_busy&&!frontend_busy&&!fpv&&!fwv;
 assign raster_error=core_error|bridge_error;
 shared_meshlet meshlet(
  .clk(clk),.reset(reset),.ce(ce),.start(start&&idle&&!table_valid),.slot(slot),
  .dma_we(dma_we),.dma_ready(dma_ready),.dma_addr(dma_addr),.dma_data(dma_data),.output_ready(output_ready),
  .packet_ready(fpr),.word_ready(fwr),.busy(frontend_busy),.done(meshlet_done),.ready(meshlet_ready),.error(frontend_error),
  .published_vertices(published_vertices),.packet_valid(fpv),.descriptor(fd),.word_valid(fwv),.word_index(fwi),.word_data(fwd),
  .emit_valid(emit_valid),.emit_addr(emit_addr),.emit_data(emit_data)
 );
 rc_raster raster(
 .bbox_done(bound_valid),.bbox_empty(live_empty),.bbox_xlo(live_xlo),.bbox_xhi(live_xhi),.bbox_ylo(live_ylo),.bbox_yhi(live_yhi),
  .clk(clk),.reset(reset),.ce(ce),.start(raster_start),
  .geo_we(geo_we),.geo_wa(geo_wa),.geo_wd(geo_wd),
  .phase(phase),.run_enable(run_enable),.quad_valid(rqv),.quad_ready(rqr),.quad_x(qx),.quad_y(qy),.quad_mask(qm),
 .sample_valid(sample_valid),.sample_lane(sample_lane),.sample_cover(sample_cover),.sample_anchor(sample_anchor),.sample_h(sample_h),.quad_end_valid(quad_end_valid),
  .busy(raster_busy),.done(done_raster),.error(core_error));
 gpu_v2_vliw_interpolator interpolator(
 .bound_valid(bound_valid),.bound_empty(bound_empty),.bound_xlo(bound_xlo),.bound_xhi(bound_xhi),.bound_ylo(bound_ylo),.bound_yhi(bound_yhi),
  .clk(clk),.reset(reset),.ce(ce),.table_valid(table_valid&&allow_table),.table_ready(i_table_ready),.table_index(table_index),.table_data(table_data),
  .prepare_start(header_take),.execution_idle(!raster_busy),.raster_start(raster_start),.exec_idle(exec_idle),.descriptor(fd),.raw_valid(raw_word_valid),.raw_ready(bridge_input_ready),.raw_index(fwi),.raw_data(fwd),
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
  if(reset)begin table_window<=0;packet_window<=0;end
  else if(ce)begin
   if(start&&idle&&!table_valid)table_window<=0;else table_window<=idle;
   if(header_take)packet_window<=0;else packet_window<=!s_busy;
  end
 end
`ifndef SYNTHESIS
 always @(posedge clk)if(ce&&!reset)begin
  if(header_take&&s_busy)$fatal(1,"second lookahead packet before prepare completion");
  if(raster_start&&raster_busy)$fatal(1,"geometry handoff before old R drain");
  if(raster_start&&!s_busy)$fatal(1,"execution handoff without pending prepare");
 end
`endif
endmodule
