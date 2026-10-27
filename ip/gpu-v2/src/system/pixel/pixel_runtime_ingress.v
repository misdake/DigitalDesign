// No added payload bank, owner table or phase counter at the composition edge.
module gpu_v2_pixel_runtime_ingress(
 input wire clk,reset,ce,
 input wire in_valid,input wire[22:0] in_begin,
 input wire[31:0] in_basic,input wire[35:0] in_light,in_uv,
 output wire in_ready,advance,preflight_bad,fault,
 output wire[2:0] in_row,
 output wire published,output wire[4:0] published_slot,
 output wire[22:0] published_begin,
 output wire basic_write,output wire[7:0] basic_write_address,
 output wire[23:0] basic_write_data,
 input wire basic_read,input wire[7:0] basic_read_address,
 output wire basic_return_valid,output wire[23:0] basic_return_data,
 output wire basic_collision,
 input wire rop_row7_accepted,input wire[4:0] rop_slot,
 output wire[4:0] retire_slot,output wire[5:0] global_used,
 output wire[1:0] light_used,output wire[3:0] sample_used,
 input wire public_credit,input wire[26:0] draw_config,
 output wire sample_valid,sample_ready,sample_fire,
 output wire[1:0] sample_row,output wire[10:0] sample_descriptor,
 output wire[35:0] sample_data,raw_metadata,
 output wire raw_write,committed,output wire[2:0] context_slot,
 output wire[2:0] phase,
 output wire sample_out_valid,input wire sample_out_ready,
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
 wire quad_bad,sampling_bad,quad_fault,sampling_fault;
 wire allow=!preflight_bad&&!fault;
 assign preflight_bad=quad_bad||sampling_bad;
 assign fault=quad_fault||sampling_fault;
 assign advance=ce&&!reset&&allow;
 wire light_ready,light_valid,light_last,sample_last;
 wire[35:0] light_data;
 wire[2:0] light_row;
 wire[9:0] light_descriptor;
 assign light_bank=light_descriptor[9];
 assign sample_fire=advance&&sample_valid&&sample_ready;
 assign raw_metadata={draw_config[26:25],draw_config[24],draw_config[23:20],
   draw_config[19:16],sample_descriptor[8:5],sample_descriptor[3:0],
   sample_descriptor[10],draw_config[15:0]};
 gpu_v2_quad_ingress_shared quad(
 .clk(clk),.reset(reset),.proposal_ce(ce),.commit_allow(allow),
 .in_valid(in_valid),.in_begin(in_begin),.in_basic(in_basic),.in_light(in_light),.in_uv(in_uv),
 .in_ready(in_ready),.advance(),.preflight_bad(quad_bad),.in_row(in_row),
 .published(published),.published_slot(published_slot),.published_begin(published_begin),
 .basic_write(basic_write),.basic_write_address(basic_write_address),.basic_write_data(basic_write_data),
 .basic_read(basic_read),.basic_read_address(basic_read_address),
 .basic_return_valid(basic_return_valid),.basic_return_data(basic_return_data),.basic_collision(basic_collision),
 .light_ready(light_ready),.sample_ready(sample_ready),.light_valid(light_valid),.sample_valid(sample_valid),
 .light_data(light_data),.sample_data(sample_data),.light_row(light_row),.sample_row(sample_row),
 .light_last(light_last),.sample_last(sample_last),.light_descriptor(light_descriptor),.sample_descriptor(sample_descriptor),
 .rop_row7_accepted(rop_row7_accepted),.rop_slot(rop_slot),.retire_slot(retire_slot),
 .global_used(global_used),.light_used(light_used),.sample_used(sample_used),.fault(quad_fault));
 gpu_v2_texture_runtime_rows sampling(
 .clk(clk),.reset(reset),.ce(ce),.commit_allow(allow),.admission_allow(public_credit),
 .in_valid(sample_valid),.in_raw(raw_metadata),.in_uv(sample_data),.in_ready(sample_ready),
 .advance(),.preflight_bad(sampling_bad),.fault(sampling_fault),.in_row(),.in_slot(context_slot),
 .raw_write(raw_write),.admitted(committed),.reclaim_fire(),.reclaim_slot(),
 .out_valid(sample_out_valid),.out_ready(sample_out_ready),.out_owner(sample_out_owner),.out_result(sample_out_result),
 .phase(phase),.d_issue(d_issue),.l_issue(l_issue),.c_issue(c_issue),.c_empty(),
 .d_return(d_return),.l_return(l_return),.c_return(c_return),
 .d_slot(),.l_slot(),.c_slot(),.d_return_slot(),.l_return_slot(),.c_lane(),
 .context_state(context_state),.c_occupied(c_occupied),.c_completed(c_completed),
 .uv_write(uv_write),.uv_read(uv_read),.uv_write_address(uv_write_address),.uv_read_address(uv_read_address),
 .store_reads(),.store_writes());
 gpu_v2_pixel_lighting_consumer light(
 .clk(clk),.reset(reset),.ce(advance),
 .source_valid(light_valid),.source_ready(light_ready),.source_row(light_row),.source_data(light_data),
 .source_slot(light_descriptor[4:0]),.source_mask(light_descriptor[8:5]),.source_bank(light_bank),
 .retire_valid(retire_valid),.retire_bank(retire_bank),
 .context_epoch(context_epoch),.context_mode(context_mode),.context_code(context_code),
 .light_x(light_x),.light_y(light_y),.light_z(light_z),.ambient(ambient),.directional(directional),
 .ray_x(ray_x),.ray_y(ray_y),.ray_k(ray_k),
 .context_wait(context_wait),.context_accepted(context_accepted),.pixel_accepted(pixel_accepted),
 .out_valid(out_valid),.out_ready(out_ready),.out_key(out_key),.out_quad_last(out_quad_last),
 .out_epoch(out_epoch),.out_g(out_g),.out_h(out_h));
endmodule
