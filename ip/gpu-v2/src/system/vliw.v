// Direct V/T/R/I -> one producer pool -> Lighting/Sampling/Final/ROP.
// Global configuration is immutable until pipeline_idle. T-local drain is not
// coupled to old quads in the backend. Memory return/drain is independent of CE.
module gpu_v2_vliw(
 input clk,reset,ce,abort,start,slot,
`ifdef GPU_V2_COMMAND_PORTS
 input cp_re,input[8:0]cp_addr,output[63:0]cp_q,output cp_ready,
`endif
 input dma_we,output dma_ready,input[9:0]dma_addr,input[63:0]dma_data,input output_ready,
 input[31:0]cfg_far_z,input[4:0]cfg_tex_w_log2,cfg_tex_h_log2,cfg_max_mip,input signed[7:0]cfg_lod_bias,
 input draw_bank,draw_unlit,input[26:0]draw_config,input[23:0]specular,input[7:0]alpha,
 input[2:0]depth_func,input depth_write,blend,draw_rebind,
 input[15:0]context_epoch,input[1:0]context_mode,input[4:0]context_code,
 input signed[15:0]light_x,light_y,light_z,ray_x,ray_y,ray_k,input[8:0]ambient,directional,
 input flush_request,output flush_complete,pipeline_idle,idle,fault,frontend_idle,
 output meshlet_done,meshlet_ready,output[6:0]published_vertices,
 output emit_valid,output[9:0]emit_addr,output[35:0]emit_data,
 output quad_admitted,quad_empty_skipped,quad_published,
 output light_result,sample_result,final_issue,final_result,quad_accepted,commit_valid,
 output[8:0]commit_x,output[7:0]commit_y,output[3:0]commit_mask,
 output[5:0]global_used,producer_used,output[4:0]work_reserved,work_count,
 output mem_req_valid,mem_req_write,output[31:0]mem_req_addr,input mem_req_ready,
 input[63:0]mem_read_data,input mem_read_valid,
 output mem_write_valid,output[63:0]mem_write_data,input mem_write_ready,mem_complete_valid,mem_complete_ok
);
 wire[3:0]frontend_error,raster_error;wire[16:0]interpolator_error;
 wire math_fault=|frontend_error|| |raster_error|| |interpolator_error;
 reg math_stopped;
 always @(posedge clk)begin
  if(reset)math_stopped<=0;
  else if(math_fault)math_stopped<=1;
 end
 wire backend_fault,backend_pipeline_idle,backend_idle;
 assign fault=abort||math_fault||math_stopped||backend_fault;
 wire front_ce=ce&&!abort&&!math_stopped&&!backend_fault;
 assign pipeline_idle=frontend_idle&&backend_idle;
 assign idle=frontend_idle&&backend_idle;
 wire qvalid,qready,qrgb,qend,result_we,depth_valid,depth_pass,lod_valid;
 wire[2:0]qid,qend_id,lod_id;wire[7:0]qx,lod_value;wire[6:0]qy,result_wa;
 wire[1:0]phase;wire[3:0]qmask;wire[35:0]result_wd;
 wire[4:0]depth_pid;wire[15:0]depth_z;
 assign quad_admitted=front_ce&&qvalid&&qready;
 gpu_v2_vliw_backend back(
 .clk(clk),.reset(reset),.ce(ce),.abort(abort||math_stopped),.front_ce(front_ce),.phase(phase),.draw_bank(draw_bank),.draw_unlit(draw_unlit),
 .quad_valid(qvalid),.quad_ready(qready),.quad_id(qid),.quad_x(qx),.quad_y(qy),.quad_rgb(qrgb),
 .quad_end_valid(qend),.quad_end_id(qend_id),.quad_end_mask(qmask),
 .result_we(result_we),.result_wa(result_wa),.result_wd(result_wd),
 .depth_event_valid(depth_valid),.depth_event_pid(depth_pid),.depth_event_z16(depth_z),.depth_event_pass(depth_pass),
 .lod_valid(lod_valid),.lod_id(lod_id),.lod_value(lod_value),.empty_skipped(quad_empty_skipped),.producer_used(producer_used),
 .draw_config(draw_config),.specular(specular),.alpha(alpha),.depth_func(depth_func),.depth_write(depth_write),.blend(blend),.draw_rebind(draw_rebind&&pipeline_idle),
 .context_epoch(context_epoch),.context_mode(context_mode),.context_code(context_code),
 .light_x(light_x),.light_y(light_y),.light_z(light_z),.ray_x(ray_x),.ray_y(ray_y),.ray_k(ray_k),.ambient(ambient),.directional(directional),
 .flush_request(flush_request&&frontend_idle),.flush_complete(flush_complete),.pipeline_idle(backend_pipeline_idle),.idle(backend_idle),.fault(backend_fault),
 .published(quad_published),.published_slot(),.light_result(light_result),.sample_result(sample_result),.final_issue(final_issue),.final_result(final_result),
 .quad_accepted(quad_accepted),.commit_valid(commit_valid),.commit_x(commit_x),.commit_y(commit_y),.commit_mask(commit_mask),
 .global_used(global_used),.work_reserved(work_reserved),.work_count(work_count),
 .mem_req_valid(mem_req_valid),.mem_req_write(mem_req_write),.mem_req_addr(mem_req_addr),.mem_req_ready(mem_req_ready),
 .mem_read_data(mem_read_data),.mem_read_valid(mem_read_valid),.mem_write_valid(mem_write_valid),.mem_write_data(mem_write_data),
 .mem_write_ready(mem_write_ready),.mem_complete_valid(mem_complete_valid),.mem_complete_ok(mem_complete_ok));
 gpu_v2_vliw_frontend front(
`ifdef GPU_V2_COMMAND_PORTS
 .cp_re(cp_re),.cp_addr(cp_addr),.cp_q(cp_q),.cp_ready(cp_ready),
`endif
 .clk(clk),.reset(reset),.ce(front_ce),.start(start&&!fault),.slot(slot),
 .dma_we(dma_we),.dma_ready(dma_ready),.dma_addr(dma_addr),.dma_data(dma_data),.output_ready(output_ready),
 .cfg_far_z(cfg_far_z),.cfg_tex_w_log2(cfg_tex_w_log2),.cfg_tex_h_log2(cfg_tex_h_log2),.cfg_max_mip(cfg_max_mip),.cfg_lod_bias(cfg_lod_bias),
 .quad_valid(qvalid),.quad_ready(qready),.quad_id(qid),.quad_x(qx),.quad_y(qy),.quad_rgb(qrgb),.phase(phase),
 .quad_end_valid(qend),.quad_end_id(qend_id),.quad_end_mask(qmask),
 .result_we(result_we),.result_wa(result_wa),.result_wd(result_wd),
 .depth_event_valid(depth_valid),.depth_event_pid(depth_pid),.depth_event_z16(depth_z),.depth_event_pass(depth_pass),
 .lod_valid(lod_valid),.lod_id(lod_id),.lod_value(lod_value),
 .frontend_busy(),.raster_busy(),.interpolator_busy(),.meshlet_done(meshlet_done),.meshlet_ready(meshlet_ready),.done_raster(),
 .frontend_error(frontend_error),.raster_error(raster_error),.interpolator_error(interpolator_error),
 .published_vertices(published_vertices),.idle(frontend_idle),.emit_valid(emit_valid),.emit_addr(emit_addr),.emit_data(emit_data));
`ifndef SYNTHESIS
 // Pin configurations obey the existing immutable-draw contract. This detects
 // accidental mutation; it does not add a second hardware configuration bank.
 reg[257:0]held_config;reg was_active;
 wire[257:0]live_config={cfg_far_z,cfg_tex_w_log2,cfg_tex_h_log2,cfg_max_mip,cfg_lod_bias,
  draw_bank,draw_unlit,draw_config,specular,alpha,depth_func,depth_write,blend,
  context_epoch,context_mode,context_code,light_x,light_y,light_z,ray_x,ray_y,ray_k,ambient,directional};
 always @(posedge clk)begin
  if(reset)begin was_active<=0;held_config<=0;end
  else begin
   if(was_active&&!pipeline_idle&&live_config!==held_config)$fatal(1,"global configuration changed before whole GPU drain");
   was_active<=!pipeline_idle||(front_ce&&start&&meshlet_ready);
   held_config<=live_config;
   if(draw_rebind&&!pipeline_idle)$fatal(1,"draw rebind before whole GPU drain");
  end
 end
`endif
endmodule
