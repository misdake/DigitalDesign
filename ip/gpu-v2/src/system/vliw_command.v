// Current GPU with fixed input-page mapping and the main three-master memory contract.
// Direct V/T/R/I -> one producer pool -> Lighting/Sampling/Final/ROP.
// Global configuration is immutable until pipeline_idle. T-local drain is not
// coupled to old quads in the backend. Memory return/drain is independent of CE.
module gpu_v2_vliw_command(
 input clk,reset,ce,abort,
 input[2:0]device_index,input[3:0]device_channel,
 input device_read_enable,device_write_enable,input[15:0]device_write_data,
 output[15:0]device_read_data,output busy,
 input output_ready,output pipeline_idle,idle,fault,frontend_idle,
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
 wire compute_fault,compute_pipeline_idle,compute_idle,memory_busy,memory_fault;
 wire device_selected=device_index==3'd4;
 wire load_count=device_selected&&device_write_enable&&device_channel==0;
 wire submit=device_selected&&device_write_enable&&device_channel==1;
 wire submit_ready;
 // Observe existing transport errors without changing execution or retirement.
 // Reset-only sticky status preserves evidence until controlled system recovery.
 reg transport_error;
 always @(posedge clk)begin
  if(reset)transport_error<=0;
  else if((ro_response_valid&&ro_response_error)||
          (fb_r_response_valid&&fb_r_response_error)||
          (fb_w_response_valid&&fb_w_response_error))transport_error<=1;
 end
 assign device_read_data=device_selected&&device_read_enable&&device_channel==2?
  {14'd0,transport_error,busy}:16'd0;
 wire start,slot,stop,cp_re,cp_ready;wire[8:0]cp_addr;wire[63:0]cp_q;
 wire processor_cp_re;wire[8:0]processor_cp_addr;
 wire processor_work_valid,processor_work_mesh,processor_work_slot;
 wire[21:0]processor_work_source;wire[6:0]processor_work_destination;
 wire[1:0]processor_work_size;wire[2:0]processor_work_blocks;
 wire ui_start,ui_active,ui_done,ui_cp_re,ui_work_valid;
 wire[9:0]ui_first,ui_last;
 wire[8:0]ui_cp_addr,ui_tile;wire[21:0]ui_work_source;wire[6:0]ui_work_destination;
 wire ui_cmd_valid,ui_cmd_write,ui_lower_ready,ui_cmd_ready,ui_cmd_done,ui_pixel_valid,ui_pixel_half;
 wire[4:0]ui_pixel_word;wire[63:0]ui_pixels;
 assign cp_re=ui_active?ui_cp_re:processor_cp_re;
 assign cp_addr=ui_active?ui_cp_addr:processor_cp_addr;
 wire start_ready=meshlet_ready;wire start_fire=ce&&start&&start_ready;
 wire dma_we,dma_ready;wire[8:0]dma_local_addr;wire[9:0]dma_addr={1'b0,dma_local_addr};wire[63:0]dma_data;
 wire work_valid,work_ready,work_mesh,work_slot,work_busy,work_done,dma_idle,dma_fault,mesh_loading;
 wire[21:0]work_source;wire[6:0]work_destination;wire[1:0]work_size;wire[2:0]work_blocks;
 wire cache_valid,cache_ready,cache_bank,cache_done,cache_done_bank;wire[21:0]cache_source;
 wire mesh_begin,mesh_done,mesh_slot,mesh_begin_slot,mesh_done_slot,uniform_done;wire[1:0]uniform_bank;
 wire header_we=dma_we&&dma_addr[8:3]==6'd32&&dma_addr[1:0]==0;
 wire[31:0]cfg_far_z;wire signed[7:0]cfg_lod_bias;
 wire[15:0]texture_base;wire[3:0]texture_n;wire texture_mip;wire[1:0]texture_filter;
 wire draw_unlit;wire[23:0]specular;
 wire[13:0]color_base,depth_base;wire[31:0]mesh_color;
 wire[2:0]depth_func;wire depth_write,blend;
 wire[15:0]context_epoch;wire[1:0]context_mode;wire[4:0]context_code;
 wire signed[15:0]light_x,light_y,light_z,ray_x,ray_y,ray_k;wire[8:0]ambient,directional;
 wire flush_request,flush_complete,texture_invalidate;
 wire clear_active,clear_request,clear_ready,clear_complete,clear_ok;
 wire[31:0]clear_address;wire[15:0]clear_value;
 assign work_valid=ui_active?ui_work_valid:processor_work_valid;
 assign work_mesh=!ui_active&&processor_work_mesh;
 assign work_source=ui_active?ui_work_source:processor_work_source;
 assign work_destination=ui_active?ui_work_destination:processor_work_destination;
 assign work_size=ui_active?2'd2:processor_work_size;
 assign work_blocks=ui_active?3'd0:processor_work_blocks;
 assign work_slot=!ui_active&&processor_work_slot;
 wire control_request_valid,control_request_ready;wire[21:0]control_request_address;wire[1:0]control_request_count;
 wire control_response_valid,control_response_last,control_response_error,control_response_bad;wire[63:0]control_data;
 reg input_active,active_page;
 always @(posedge clk)begin
  if(reset)begin input_active<=0;active_page<=0;end
  else begin if(meshlet_done)input_active<=0;if(start_fire)begin input_active<=1;active_page<=slot;end end
 end
 assign pipeline_idle=compute_pipeline_idle&&!memory_busy&&dma_idle&&!clear_active&&!ui_active;
 assign idle=!busy&&compute_idle&&!memory_busy&&dma_idle;
 assign fault=stop||compute_fault||memory_fault||dma_fault;
 gpu_command_processor processor(
 .clk(clk),.reset(reset),.ce(ce),.abort(abort),.load_count(load_count),.submit(submit),.control_data(device_write_data),
 .submit_ready(submit_ready),.busy(busy),.stop(stop),
 .render_idle(compute_idle),.render_fault(compute_fault||memory_fault),.memory_busy(memory_busy),.meshlet_ready(meshlet_ready),.meshlet_done(meshlet_done),
 .start(start),.draw_slot(slot),.cp_re(processor_cp_re),.cp_addr(processor_cp_addr),.cp_q(cp_q),.cp_ready(cp_ready),
 .cache_valid(cache_valid),.cache_ready(cache_ready),.cache_source(cache_source),.cache_bank(cache_bank),.cache_done(cache_done),.cache_done_bank(cache_done_bank),
 .work_valid(processor_work_valid),.work_ready(work_ready),.work_mesh(processor_work_mesh),.work_source(processor_work_source),.work_destination(processor_work_destination),.work_size(processor_work_size),.work_blocks(processor_work_blocks),.work_slot(processor_work_slot),
 .work_busy(work_busy),.dma_idle(dma_idle),.dma_fault(dma_fault),.mesh_loading(mesh_loading),.mesh_begin(mesh_begin),.mesh_done(mesh_done),.mesh_slot(mesh_slot),.mesh_begin_slot(mesh_begin_slot),.mesh_done_slot(mesh_done_slot),
 .uniform_done(uniform_done),.uniform_bank(uniform_bank),.header_we(header_we),.header_slot(dma_addr[2]),.header_data(dma_data),
 .cfg_far_z(cfg_far_z),.cfg_lod_bias(cfg_lod_bias),
 .texture_base(texture_base),.texture_n(texture_n),.texture_mip(texture_mip),.texture_filter(texture_filter),
 .color_base(color_base),.depth_base(depth_base),.mesh_color(mesh_color),
 .draw_unlit(draw_unlit),.specular(specular),.depth_func(depth_func),.depth_write(depth_write),.blend(blend),.texture_invalidate(texture_invalidate),
 .context_epoch(context_epoch),.context_mode(context_mode),.context_code(context_code),.light_x(light_x),.light_y(light_y),.light_z(light_z),.ray_x(ray_x),.ray_y(ray_y),.ray_k(ray_k),
 .ambient(ambient),.directional(directional),.flush_request(flush_request),.flush_complete(flush_complete),
 .clear_active(clear_active),.clear_request(clear_request),.clear_address(clear_address),.clear_value(clear_value),
 .clear_ready(clear_ready),.clear_complete(clear_complete),.clear_ok(clear_ok),.ui_start(ui_start),.ui_done(ui_done),.ui_first(ui_first),.ui_last(ui_last));
 gpu_ui_grid ui(.clk(clk),.reset(reset),.ce(ce),.start(ui_start),.stop(stop),.grid_base(cp_q[15:0]),.atlas_base(cp_q[22:16]),
 .first_cell(ui_first),.last_cell(ui_last),
 .active(ui_active),.done(ui_done),.cp_re(ui_cp_re),.cp_addr(ui_cp_addr),.cp_ready(cp_ready),.cp_q(cp_q),
 .work_valid(ui_work_valid),.work_ready(work_ready),.work_source(ui_work_source),.work_destination(ui_work_destination),.dma_idle(dma_idle),
 .cmd_valid(ui_cmd_valid),.cmd_write(ui_cmd_write),.lower_ready(ui_lower_ready),.tile(ui_tile),.cmd_ready(ui_cmd_ready),.cmd_done(ui_cmd_done),
 .pixel_valid(ui_pixel_valid),.pixel_half(ui_pixel_half),.pixel_word(ui_pixel_word),.pixels(ui_pixels));
 gpu_command_dma dma(
 .clk(clk),.reset(reset),.ce(ce),.stop(stop||compute_fault||memory_fault),
 .cache_valid(cache_valid),.cache_ready(cache_ready),.cache_source(cache_source),.cache_bank(cache_bank),.cache_done(cache_done),.cache_done_bank(cache_done_bank),
 .work_valid(work_valid),.work_ready(work_ready),.work_mesh(work_mesh),.work_source(work_source),.work_destination(work_destination),.work_size(work_size),.work_blocks(work_blocks),.work_slot(work_slot),
 .work_busy(work_busy),.work_done(work_done),.mesh_begin(mesh_begin),.mesh_done(mesh_done),.mesh_slot(mesh_slot),.mesh_begin_slot(mesh_begin_slot),.mesh_done_slot(mesh_done_slot),.mesh_loading(mesh_loading),
 .uniform_done(uniform_done),.uniform_bank(uniform_bank),.dma_we(dma_we),.dma_addr(dma_local_addr),.dma_data(dma_data),
 .request_valid(control_request_valid),.request_ready(control_request_ready),.request_address(control_request_address),.request_count(control_request_count),
 .response_valid(control_response_valid),.response_last(control_response_last),.response_error(control_response_error),.response_bad(control_response_bad),.response_data(control_data),.idle(dma_idle),.fault(dma_fault));
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
 wire bus_framebuffer_ready,bus_framebuffer_write_ready,bus_framebuffer_read_valid;
 wire bus_framebuffer_complete,bus_framebuffer_ok;
 assign framebuffer_request_ready=bus_framebuffer_ready&&!clear_active;
 assign framebuffer_write_ready=bus_framebuffer_write_ready&&!clear_active;
 assign framebuffer_read_valid=bus_framebuffer_read_valid&&!clear_active;
 assign framebuffer_complete=bus_framebuffer_complete&&!clear_active;
 assign framebuffer_ok=bus_framebuffer_ok;
 assign clear_ready=fb_w_request_ready;
 assign clear_complete=fb_w_response_valid&&(fb_w_response_last||fb_w_response_error);
 assign clear_ok=!fb_w_response_error;
 gpu_v2_vliw_three_port core(
 .ui_active(ui_active),.ui_cmd_valid(ui_cmd_valid),.ui_cmd_write(ui_cmd_write),.ui_lower_ready(ui_lower_ready),.ui_tile(ui_tile),.ui_cmd_ready(ui_cmd_ready),.ui_cmd_done(ui_cmd_done),
 .ui_pixel_valid(ui_pixel_valid),.ui_pixel_half(ui_pixel_half),.ui_pixel_word(ui_pixel_word),.ui_pixels(ui_pixels),
 .cp_re(cp_re),.cp_addr(cp_addr),.cp_q(cp_q),.cp_ready(cp_ready),
 .clk(clk),
 .reset(reset),
 .ce(ce),
 .abort(stop),
 .start(start_fire),
 .slot(slot),
 .dma_we(dma_we),
 .dma_ready(dma_ready),
 .dma_addr(dma_addr),
 .dma_data(dma_data),
 .output_ready(output_ready),
 .cfg_far_z(cfg_far_z),
 .texture_base(texture_base),.texture_n(texture_n),.texture_mip(texture_mip),.texture_filter(texture_filter),.texture_invalidate(texture_invalidate),
 .cfg_lod_bias(cfg_lod_bias),
 .color_base(color_base),.depth_base(depth_base),.mesh_color(mesh_color),
 .draw_unlit(draw_unlit),
 .specular(specular),
 .depth_func(depth_func),
 .depth_write(depth_write),
 .blend(blend),
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
 wire cache_w_valid,cache_w_ready,cache_w_data_ready;
 wire[21:0]cache_w_address;wire[1:0]cache_w_count;wire[63:0]cache_w_data;
 assign fb_w_request_valid=clear_active?clear_request:cache_w_valid;
 assign fb_w_address=clear_active?clear_address[22:1]:cache_w_address;
 assign fb_w_count=clear_active?2'd3:cache_w_count;
 assign fb_w_data=clear_active?{4{clear_value}}:cache_w_data;
 assign cache_w_ready=fb_w_request_ready&&!clear_active;
 assign cache_w_data_ready=fb_w_data_ready&&!clear_active;
 gpu_vliw_memory_router memory(
 .clk(clk),.reset(reset),
 .geometry_request_valid(control_request_valid),.geometry_request_ready(control_request_ready),
 .geometry_request_address(control_request_address),.geometry_request_count(control_request_count),
 .geometry_response_valid(control_response_valid),.geometry_response_last(control_response_last),
 .geometry_response_error(control_response_error),.geometry_response_bad(control_response_bad),.geometry_response_data(control_data),
 .control_request_valid(1'b0),.control_request_address(22'd0),.control_request_count(2'd0),
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
 .framebuffer_request_ready(bus_framebuffer_ready),
 .framebuffer_read_valid(bus_framebuffer_read_valid),
 .framebuffer_read_data(framebuffer_read_data),
 .framebuffer_write_valid(framebuffer_write_valid),
 .framebuffer_write_data(framebuffer_write_data),
 .framebuffer_write_ready(bus_framebuffer_write_ready),
 .framebuffer_complete(bus_framebuffer_complete),
 .framebuffer_ok(bus_framebuffer_ok),
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
 .fb_w_request_valid(cache_w_valid),
 .fb_w_request_ready(cache_w_ready),
 .fb_w_address(cache_w_address),
 .fb_w_count(cache_w_count),
 .fb_w_data(cache_w_data),
 .fb_w_data_ready(cache_w_data_ready),
 .fb_w_response_valid(fb_w_response_valid&&!clear_active),
 .fb_w_response_last(fb_w_response_last),
 .fb_w_response_error(fb_w_response_error),
 .busy(memory_busy),.fault(memory_fault));
`ifndef SYNTHESIS
 always @(posedge clk)if(!reset)begin
  if(dma_we&&!dma_ready)$fatal(1,"DMA sink lost its statically independent write port");
  if(cp_re&&input_active)$fatal(1,"CP read while V/T owns input Q");
  if(input_active&&dma_we&&dma_addr<256&&dma_addr[7]==active_page)$fatal(1,"DMA overwrote active input page");
  if(clear_active&&(!compute_idle||framebuffer_request_valid))$fatal(1,"CLEAR overlapped compute/cache work");
 end
`endif
endmodule
