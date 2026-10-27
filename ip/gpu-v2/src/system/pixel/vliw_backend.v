// Producer-pool sampling path; no row ingress or duplicate Basic/Light/UV payload.
module gpu_v2_vliw_sampling(
 input wire clk,reset,ce,light_compute_tick,
 input wire front_ce,input wire[1:0]producer_phase,input wire draw_bank,draw_unlit,
 input wire quad_valid,output wire quad_ready,output wire[2:0]quad_id,
 input wire[7:0]quad_x,input wire[6:0]quad_y,input wire quad_rgb,
 input wire quad_end_valid,input wire[2:0]quad_end_id,input wire[3:0]quad_end_mask,
 output wire prefetch_valid,output wire[9:0]prefetch_tile,
 input wire result_we,input wire[6:0]result_wa,input wire[35:0]result_wd,
 input wire depth_event_valid,input wire[4:0]depth_event_pid,input wire[15:0]depth_event_z16,input wire depth_event_pass,
 input wire lod_valid,input wire[2:0]lod_id,input wire[7:0]lod_value,
 input wire empty_skip,output wire empty_head,empty_skip_ready,
 output wire advance,preflight_bad,fault,
 output wire published,output wire[4:0] published_slot,
 output wire[22:0] published_begin,
 input wire[4:0]descriptor_issue_slot,output wire[22:0]descriptor_issue,descriptor_head,descriptor_next,output wire[8:0]descriptor_head_tile,descriptor_next_tile,
 input wire basic_read,input wire[6:0] basic_read_address,
 output wire[23:0] basic_return_data,
 output wire basic_collision,
 input wire depth_read,input wire[6:0]depth_read_address,output wire[15:0]depth_data,
 input wire rop_row7_accepted,input wire[4:0] rop_slot,
 output wire[4:0] retire_slot,output wire[5:0] global_used,
 output wire[5:0]light_used,sample_used,producer_used,
 output wire public_credit,input wire[26:0] draw_config,
 output wire sample_valid,sample_ready,sample_fire,
 output wire[1:0] sample_row,output wire[18:0] sample_descriptor,
 output wire[35:0] sample_data,raw_metadata,
 output wire raw_write,committed,output wire[2:0] context_slot,
 output wire[2:0] phase,
 output wire sample_out_valid,sample_out_ready,
 input wire packet_enable,abort,result_ready,coalition_allow,
 output wire packet_out_ready,result_valid,result_quad_last,result_fire,public_publish,
 output wire[5:0]result_key,output wire[23:0]result_rgb,output wire[15:0]public_live,
 output wire mem_req_valid,output wire[31:0]mem_req_addr,
 input wire mem_req_ready,mem_resp_valid,input wire[3:0]mem_resp_index,
 input wire[63:0]mem_resp_data,input wire mem_resp_complete,mem_resp_ok,
 output wire cache_drained,
 output wire packet_out_valid,coefficient_accept,output wire[33:0]packet_out_payload,
 output wire[4:0]work_reserved,work_count,producer_count,unwritten_count,
 output wire[5:0]group_count,row_count,
 output wire[37:0] sample_out_owner,output wire[35:0] sample_out_result,
 output wire[72:0] context_state,
 output wire[4:0] c_occupied,c_completed,
 output wire d_issue,l_issue,c_issue,d_return,l_return,c_return,
 output wire uv_write,uv_read,
 output wire[4:0] uv_write_address,uv_read_address,
 output wire light_bank,input wire retire_valid,retire_bank,
 input wire[15:0] context_epoch,
 input wire[1:0] context_mode,input wire[4:0] context_code,
 input wire signed[15:0] light_x,light_y,light_z,
 input wire[8:0] ambient,directional,
 input wire signed[15:0] ray_x,ray_y,ray_k,
 output wire context_wait,context_accepted,pixel_accepted,
 output wire out_valid,input wire out_ready,
 output wire[6:0] out_key,output wire out_quad_last,
 output wire[15:0] out_epoch,output wire[8:0] out_g,out_h
);
 wire quad_bad,sampling_bad,quad_fault,sampling_fault,backend_bad,backend_fault;
 // Each leaf suppresses its own illegal proposal; the parent latches the
 // diagnostic union into its existing sticky stopped bit on the physical edge.
 // No cross-leaf same-edge rollback gate is inserted into the common CE.
 wire allow=coalition_allow;
 wire cache_bad,cache_fault,color_bad,color_fault;
 assign preflight_bad=quad_bad||sampling_bad||backend_bad||cache_bad||color_bad;
 assign fault=abort||quad_fault||sampling_fault||backend_fault||cache_fault||color_fault;
 assign advance=ce&&!reset&&allow;
 wire light_ready,light_valid,light_context_valid,sample_last;
 wire[35:0]light_normal;wire[31:0]light_position;wire[1:0]light_lane;
 wire[9:0] light_descriptor;
 assign light_bank=light_descriptor[9];
 assign sample_fire=advance&&sample_valid&&sample_ready;
 assign raw_metadata={draw_config[26:25],draw_config[24],draw_config[23:20],
   draw_config[19:16],sample_descriptor[8:5],sample_descriptor[3:0],
   1'b0,8'd0,sample_descriptor[18:11]};
 assign quad_bad=1'b0;
 gpu_v2_vliw_quad_pool quad(
 .clk(clk),.reset(reset),.front_ce(front_ce&&!fault),.backend_ce(advance),.phase(producer_phase),
 .draw_bank(draw_bank),.draw_unlit(draw_unlit),
 .quad_valid(quad_valid),.quad_ready(quad_ready),.quad_id(quad_id),.quad_x(quad_x),.quad_y(quad_y),.quad_rgb(quad_rgb),
 .quad_end_valid(quad_end_valid),.quad_end_id(quad_end_id),.quad_end_mask(quad_end_mask),
 .prefetch_valid(prefetch_valid),.prefetch_tile(prefetch_tile),
 .result_we(result_we),.result_wa(result_wa),.result_wd(result_wd),
 .depth_event_valid(depth_event_valid),.depth_event_pid(depth_event_pid),.depth_event_z16(depth_event_z16),.depth_event_pass(depth_event_pass),
 .lod_valid(lod_valid),.lod_id(lod_id),.lod_value(lod_value),
 .publish_ready(1'b1),.published(published),.published_slot(published_slot),.published_begin(published_begin),
 .descriptor_issue_slot(descriptor_issue_slot),
 .descriptor_issue(descriptor_issue),.descriptor_head(descriptor_head),.descriptor_next(descriptor_next),.descriptor_head_tile(descriptor_head_tile),.descriptor_next_tile(descriptor_next_tile),
 .basic_read(basic_read),.basic_read_address(basic_read_address),
 .basic_return_data(basic_return_data),.basic_collision(basic_collision),
 .depth_read(depth_read),.depth_read_address(depth_read_address),.depth_data(depth_data),
 .light_ready(light_ready),.sample_ready(sample_ready),.light_valid(light_valid),.light_context_valid(light_context_valid),.sample_valid(sample_valid),
 .light_normal(light_normal),.light_position(light_position),.light_lane(light_lane),.sample_data(sample_data),.sample_row(sample_row),
 .sample_last(sample_last),.light_descriptor(light_descriptor),.sample_descriptor(sample_descriptor),
 .retire_valid(rop_row7_accepted),.retire_id(rop_slot),.empty_skip(empty_skip),.empty_head(empty_head),.empty_skip_ready(empty_skip_ready),
 .retire_slot(retire_slot),.global_used(global_used),.producer_used(producer_used),.light_pending(light_used),.sample_pending(sample_used),.busy(),.fault(quad_fault));
 wire sample_first=sample_descriptor[10];
 wire coordinate_ready;
 wire sample_admit=!sample_first||public_credit;
 wire[1:0]coordinate_phase;
 assign sample_ready=coordinate_ready&&sample_admit;
 assign raw_write=sample_fire&&sample_first;assign committed=sample_fire&&sample_last;
 assign phase={1'b0,coordinate_phase};assign context_slot=0;assign context_state=0;
 assign d_issue=0;assign l_issue=0;assign d_return=0;assign l_return=0;
 assign uv_write=0;assign uv_read=c_issue;assign uv_write_address=0;assign uv_read_address={3'd0,sample_row};
 gpu_v2_texture_direct_lod sampling(
 .clk(clk),.reset(reset),.ce(ce),.commit_allow(allow),
 .in_valid(sample_valid&&sample_admit),.in_uv({sample_data[33:18],sample_data[15:0]}),
 .in_quad(sample_descriptor[3:0]),.in_mask(sample_descriptor[8:5]),.in_slot(draw_config[19:16]),.in_lane(sample_row),
 .in_lod(sample_descriptor[18:11]),.in_max_n(draw_config[23:20]),.in_has_mip(draw_config[24]),.in_filter(draw_config[26:25]),
 .in_ready(coordinate_ready),.advance(),.preflight_bad(sampling_bad),.fault(sampling_fault),
 .out_valid(sample_out_valid),.out_ready(sample_out_ready),.out_owner(sample_out_owner),.out_result(sample_out_result),
 .phase(coordinate_phase),.issue(c_issue),.returned(c_return),.occupied(c_occupied),.completed(c_completed));
 gpu_v2_pixel_lighting_consumer light(
 .clk(clk),.reset(reset),.ce(advance),.compute_tick(light_compute_tick),
 .source_context_valid(light_context_valid),.source_valid(light_valid),.source_ready(light_ready),
 .source_lane(light_lane),.source_normal(light_normal),.source_position(light_position),
 .source_slot(light_descriptor[4:0]),.source_mask(light_descriptor[8:5]),.source_bank(light_bank),
 .retire_valid(retire_valid),.retire_bank(retire_bank),
 .context_epoch(context_epoch),.context_mode(context_mode),.context_code(context_code),
 .light_x(light_x),.light_y(light_y),.light_z(light_z),.ambient(ambient),.directional(directional),
 .ray_x(ray_x),.ray_y(ray_y),.ray_k(ray_k),
 .context_wait(context_wait),.context_accepted(context_accepted),.pixel_accepted(pixel_accepted),
 .out_valid(out_valid),.out_ready(out_ready),.out_key(out_key),.out_quad_last(out_quad_last),
 .out_epoch(out_epoch),.out_g(out_g),.out_h(out_h));


 wire packet_next_valid;wire[17:0]packet_next_key;
 wire weight_write;wire[6:0]weight_address;wire[35:0]weight_value;
 gpu_v2_texture_runtime_backend_unique backend(
 .clk(clk),.reset(reset),.ce(ce),.commit_allow(allow),.abort(1'b0),
 .in_valid(sample_out_valid),.in_owner(sample_out_owner),.in_result(sample_out_result),
 .in_ready(sample_out_ready),.in_accept(coefficient_accept),.advance(),.preflight_bad(backend_bad),.fault(backend_fault),
 .packet_enable(packet_enable),.out_ready(packet_out_ready),.out_valid(packet_out_valid),.out_payload(packet_out_payload),
 .next_valid(packet_next_valid),.next_key(packet_next_key),
 .work_reserved(work_reserved),.work_count(work_count),.producer_count(producer_count),.unwritten_count(unwritten_count),
 .group_count(group_count),.row_count(row_count),.pool_strobes(),
 .coefficient_valid(),.coefficient_consume(),.member_issue(),.member_return(),.reserved_cost(),
 .coefficient_weights(),.coefficient_meta(),.member_payload(),.packet_capture(),.work_ack(),
 .weight_write(weight_write),.weight_address(weight_address),.weight_value(weight_value));

 wire cache_valid,color_ready,color_valid,color_last,color_result;
 wire[71:0]cache_payload;wire[15:0]cache_tex0,cache_tex1,cache_tex2,cache_tex3;
 wire[5:0]color_key;wire[23:0]color_rgb;
 wire publish_proposal=ce&&!reset&&!fault&&sample_valid&&sample_ready&&sample_first;
 gpu_v2_texture_runtime_cache_unique demand(
 .clk(clk),.reset(reset),.proposal_ce(ce),.commit_allow(allow),
 .abort(abort),
 .weight_write(weight_write),.weight_address(weight_address),.weight_value(weight_value),
 .draw_slot(draw_config[19:16]),.in_valid(packet_out_valid),.in_payload(packet_out_payload),.next_valid(packet_next_valid),.next_key({packet_next_key,draw_config[19:16]}),.proposal_ready(packet_out_ready),.in_ready(),
 .preflight_bad(cache_bad),.advance(),.out_ready(color_ready),.out_valid(cache_valid),.out_payload(cache_payload),
 .out_tex0(cache_tex0),.out_tex1(cache_tex1),.out_tex2(cache_tex2),.out_tex3(cache_tex3),
 .mem_req_valid(mem_req_valid),.mem_req_addr(mem_req_addr),.mem_req_ready(mem_req_ready),
 .mem_resp_valid(mem_resp_valid),.mem_resp_index(mem_resp_index),.mem_resp_data(mem_resp_data),
 .mem_resp_complete(mem_resp_complete),.mem_resp_ok(mem_resp_ok),.drained(cache_drained),.fault(cache_fault));
 gpu_v2_texture_runtime_color color(
 .clk(clk),.reset(reset),.proposal_ce(ce&&!abort),.commit_allow(allow),
 .check_quad(sample_descriptor[3:0]),.public_free(public_credit),
 .publish_valid(publish_proposal),.publish_quad(sample_descriptor[3:0]),.publish_mask(sample_descriptor[8:5]),
 .publish_ready(),.publish_fire(public_publish),
 .in_valid(cache_valid),.in_payload(cache_payload),.in_texels({cache_tex3,cache_tex2,cache_tex1,cache_tex0}),
 .in_ready(color_ready),.input_fire(),.out_ready(result_ready),.out_valid(color_valid),
 .out_key(color_key),.out_rgb(color_rgb),.quad_last(color_last),.result_fire(color_result),
 .eligible(),.advance(),.preflight_fault(color_bad),.fault(color_fault),.public_live(public_live));
 assign result_valid=!reset&&!fault&&color_valid;
 assign result_key=color_key;assign result_rgb=color_rgb;
 assign result_quad_last=result_valid&&color_last;
 assign result_fire=color_result;
endmodule
