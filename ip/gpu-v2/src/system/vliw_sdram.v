// Current GPU with fixed input-page mapping and the main three-master memory contract.
// Direct V/T/R/I -> one producer pool -> Lighting/Sampling/Final/ROP.
// Global configuration is immutable until pipeline_idle. T-local drain is not
// coupled to old quads in the backend. Memory return/drain is independent of CE.
module gpu_v2_vliw_sdram(
 input clk,reset,ce,abort,start,slot,
 input output_ready,output start_ready,
 input descriptor_valid,output descriptor_ready,input descriptor_page,input[7:0]descriptor_tag,
 input[21:0]config_source,metadata_source,payload_source,input[10:0]config_bytes,payload_bytes,
 output completion_valid,input completion_ready,output completion_publish,completion_page,
 output[7:0]completion_tag,output[3:0]completion_error,
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
 output ro_request_valid,input ro_request_ready,output[21:0]ro_address,output[1:0]ro_count,
 input ro_response_valid,ro_response_last,ro_response_error,input[63:0]ro_data,
 output fb_r_request_valid,input fb_r_request_ready,output[21:0]fb_r_address,output[1:0]fb_r_count,
 input fb_r_response_valid,fb_r_response_last,fb_r_response_error,input[63:0]fb_r_data,
 output fb_w_request_valid,input fb_w_request_ready,output[21:0]fb_w_address,output[1:0]fb_w_count,
 output[63:0]fb_w_data,input fb_w_data_ready,
 input fb_w_response_valid,fb_w_response_last,fb_w_response_error
);
 wire dma_we,dma_ready;wire[9:0]dma_addr;wire[63:0]dma_data;
 wire compute_fault,compute_pipeline_idle,compute_idle,memory_busy,memory_fault;
 reg active_page,input_active,job_active,job_page,job_config;reg[1:0]page_valid;
 // Input ownership ends at V/T completion, independently of retained attributes.
 wire page_writable=!input_active||descriptor_page!=active_page;
 wire config_drained=compute_idle&&!memory_busy&&!job_active;
 wire descriptor_take=descriptor_valid&&descriptor_ready;
 wire job_writing=job_active&&!completion_valid;
 wire incoming_conflict=descriptor_valid&&(config_bytes!=0||descriptor_page==slot);
 assign start_ready=meshlet_ready&&page_valid[slot]&&!fault&&
  (!job_writing||(!job_config&&job_page!=slot))&&!incoming_conflict;
 wire start_fire=ce&&start&&start_ready;
 assign pipeline_idle=compute_pipeline_idle&&!memory_busy&&!job_active;
 assign idle=compute_idle&&!memory_busy&&!job_active;
 assign fault=abort||compute_fault||memory_fault;
 wire texture_request_valid;
 wire[31:0] texture_request_address;
 wire texture_request_ready;
 wire texture_read_valid;
 wire[3:0] texture_read_index;
 wire[63:0] texture_read_data;
 wire texture_complete;
 wire texture_ok;
 wire framebuffer_request_valid;
 wire framebuffer_request_write;
 wire[31:0] framebuffer_request_address;
 wire framebuffer_request_ready;
 wire framebuffer_read_valid;
 wire[63:0] framebuffer_read_data;
 wire framebuffer_write_valid;
 wire[63:0] framebuffer_write_data;
 wire framebuffer_write_ready;
 wire framebuffer_complete;
 wire framebuffer_ok;
 always @(posedge clk)begin
  if(reset)begin active_page<=0;input_active<=0;job_active<=0;job_page<=0;job_config<=0;page_valid<=0;end
  else begin
   if(meshlet_done)input_active<=0;
   if(start_fire)begin active_page<=slot;input_active<=1;end
   if(descriptor_take)begin
    job_active<=1;job_page<=descriptor_page;job_config<=config_bytes!=0;
    page_valid[descriptor_page]<=0;
   end
   if(completion_valid)begin
    if(completion_publish)page_valid[completion_page]<=1;
    else if(job_config)page_valid<=0;
    if(completion_ready)job_active<=0;
   end
  end
 end
 gpu_v2_vliw_three_port core(
 .clk(clk),
 .reset(reset),
 .ce(ce),
 .abort(abort),
 .start(start_fire),
 .slot(slot),
 .dma_we(dma_we),
 .dma_ready(dma_ready),
 .dma_addr(dma_addr),
 .dma_data(dma_data),
 .output_ready(output_ready),
 .cfg_far_z(cfg_far_z),
 .cfg_tex_w_log2(cfg_tex_w_log2),
 .cfg_tex_h_log2(cfg_tex_h_log2),
 .cfg_max_mip(cfg_max_mip),
 .cfg_lod_bias(cfg_lod_bias),
 .draw_bank(draw_bank),
 .draw_unlit(draw_unlit),
 .draw_config(draw_config),
 .specular(specular),
 .alpha(alpha),
 .depth_func(depth_func),
 .depth_write(depth_write),
 .blend(blend),
 .draw_rebind(draw_rebind&&pipeline_idle),
 .context_epoch(context_epoch),
 .context_mode(context_mode),
 .context_code(context_code),
 .light_x(light_x),
 .light_y(light_y),
 .light_z(light_z),
 .ray_x(ray_x),
 .ray_y(ray_y),
 .ray_k(ray_k),
 .ambient(ambient),
 .directional(directional),
 .flush_request(flush_request),
 .flush_complete(flush_complete),
 .pipeline_idle(compute_pipeline_idle),
 .idle(compute_idle),
 .fault(compute_fault),
 .frontend_idle(frontend_idle),
 .meshlet_done(meshlet_done),
 .meshlet_ready(meshlet_ready),
 .published_vertices(published_vertices),
 .emit_valid(emit_valid),
 .emit_addr(emit_addr),
 .emit_data(emit_data),
 .quad_admitted(quad_admitted),
 .quad_empty_skipped(quad_empty_skipped),
 .quad_published(quad_published),
 .light_result(light_result),
 .sample_result(sample_result),
 .final_issue(final_issue),
 .final_result(final_result),
 .quad_accepted(quad_accepted),
 .commit_valid(commit_valid),
 .commit_x(commit_x),
 .commit_y(commit_y),
 .commit_mask(commit_mask),
 .global_used(global_used),
 .producer_used(producer_used),
 .work_reserved(work_reserved),
 .work_count(work_count),
 .texture_request_valid(texture_request_valid),
 .texture_request_address(texture_request_address),
 .texture_request_ready(texture_request_ready),
 .texture_read_valid(texture_read_valid),
 .texture_read_index(texture_read_index),
 .texture_read_data(texture_read_data),
 .texture_complete(texture_complete),
 .texture_ok(texture_ok),
 .framebuffer_request_valid(framebuffer_request_valid),
 .framebuffer_request_write(framebuffer_request_write),
 .framebuffer_request_address(framebuffer_request_address),
 .framebuffer_request_ready(framebuffer_request_ready),
 .framebuffer_read_valid(framebuffer_read_valid),
 .framebuffer_read_data(framebuffer_read_data),
 .framebuffer_write_valid(framebuffer_write_valid),
 .framebuffer_write_data(framebuffer_write_data),
 .framebuffer_write_ready(framebuffer_write_ready),
 .framebuffer_complete(framebuffer_complete),
 .framebuffer_ok(framebuffer_ok));
 gpu_vliw_memory memory(
 .clk(clk),.reset(reset),.geometry_abort(abort||compute_fault||memory_fault),
 .descriptor_valid(descriptor_valid),.descriptor_ready(descriptor_ready),.page_writable(page_writable),.gpu_drained(config_drained),
 .descriptor_page(descriptor_page),.descriptor_tag(descriptor_tag),
 .config_source(config_source),.metadata_source(metadata_source),.payload_source(payload_source),
 .config_destination(config_bytes>512?9'd511:9'd320),
 .metadata_destination(9'd256+{6'd0,descriptor_page,2'd0}),.payload_destination({1'b0,descriptor_page,7'd0}),
 .config_bytes(config_bytes),.payload_bytes(payload_bytes),
 .dma_we(dma_we),.dma_addr(dma_addr),.dma_data(dma_data),
 .completion_valid(completion_valid),.completion_ready(completion_ready),.completion_publish(completion_publish),
 .completion_page(completion_page),.completion_tag(completion_tag),.completion_error(completion_error),
 .texture_request_valid(texture_request_valid),
 .texture_request_address(texture_request_address),
 .texture_request_ready(texture_request_ready),
 .texture_read_valid(texture_read_valid),
 .texture_read_index(texture_read_index),
 .texture_read_data(texture_read_data),
 .texture_complete(texture_complete),
 .texture_ok(texture_ok),
 .framebuffer_request_valid(framebuffer_request_valid),
 .framebuffer_request_write(framebuffer_request_write),
 .framebuffer_request_address(framebuffer_request_address),
 .framebuffer_request_ready(framebuffer_request_ready),
 .framebuffer_read_valid(framebuffer_read_valid),
 .framebuffer_read_data(framebuffer_read_data),
 .framebuffer_write_valid(framebuffer_write_valid),
 .framebuffer_write_data(framebuffer_write_data),
 .framebuffer_write_ready(framebuffer_write_ready),
 .framebuffer_complete(framebuffer_complete),
 .framebuffer_ok(framebuffer_ok),
 .ro_request_valid(ro_request_valid),
 .ro_request_ready(ro_request_ready),
 .ro_address(ro_address),
 .ro_count(ro_count),
 .ro_response_valid(ro_response_valid),
 .ro_response_last(ro_response_last),
 .ro_response_error(ro_response_error),
 .ro_data(ro_data),
 .fb_r_request_valid(fb_r_request_valid),
 .fb_r_request_ready(fb_r_request_ready),
 .fb_r_address(fb_r_address),
 .fb_r_count(fb_r_count),
 .fb_r_response_valid(fb_r_response_valid),
 .fb_r_response_last(fb_r_response_last),
 .fb_r_response_error(fb_r_response_error),
 .fb_r_data(fb_r_data),
 .fb_w_request_valid(fb_w_request_valid),
 .fb_w_request_ready(fb_w_request_ready),
 .fb_w_address(fb_w_address),
 .fb_w_count(fb_w_count),
 .fb_w_data(fb_w_data),
 .fb_w_data_ready(fb_w_data_ready),
 .fb_w_response_valid(fb_w_response_valid),
 .fb_w_response_last(fb_w_response_last),
 .fb_w_response_error(fb_w_response_error),
 .busy(memory_busy),.fault(memory_fault));
`ifndef SYNTHESIS
 always @(posedge clk)if(!reset)begin
  if(dma_we&&!dma_ready)$fatal(1,"accepted DMA sink lost its fixed write port");
  if(start_fire&&job_writing&&(job_config||job_page==slot))$fatal(1,"start overlaps its input writer");
  if(descriptor_take&&!page_writable)$fatal(1,"DMA accepted an active input page");
 end
`endif
endmodule
