// Single Quad ingress. Both branches use its preflighted advance; paid RAM
// returns remain wall-clocked in the existing queue/UV leaves.
module gpu_v2_pixel_ingress(
 input wire clk,reset,ce,
 input wire in_valid,input wire [22:0] in_begin,
 input wire [31:0] in_basic,input wire [35:0] in_light,in_uv,
 output wire in_ready,advance,output wire [2:0] in_row,
 output wire published,output wire [4:0] published_slot,
 output wire [22:0] published_begin,
 output wire basic_write,output wire [7:0] basic_write_address,
 output wire [23:0] basic_write_data,
 input wire basic_read,input wire [7:0] basic_read_address,
 output wire basic_return_valid,output wire [23:0] basic_return_data,
 output wire basic_collision,
 output wire sample_ready,sample_valid,
 output wire [35:0] sample_data,output wire [1:0] sample_row,
 output wire sample_last,output wire [10:0] sample_descriptor,
 output wire sample_fire,
 input wire rop_row7_accepted,input wire [4:0] rop_slot,
 output wire [4:0] retire_slot,output wire [5:0] global_used,
 output wire [1:0] light_used,output wire [3:0] sample_used,
 output wire fault,
 input wire [2:0] phase,
 input wire context_credit,input wire [2:0] context_slot,
 input wire [26:0] draw_config,
 output wire raw_write,output wire [2:0] raw_slot,
 output wire [35:0] raw_metadata,
 output wire committed,output wire [2:0] committed_slot,
 input wire d_take,output wire d_valid,output wire [2:0] d_slot,
 output wire [143:0] d_uv,
 input wire c_request,input wire [2:0] c_slot,input wire [1:0] c_lane,
 input wire c_take,output wire c_empty,c_valid,
 output wire [2:0] c_owner_slot,output wire [1:0] c_owner_lane,
 output wire [31:0] c_uv,
 output wire uv_write,output wire [4:0] uv_write_address,
 output wire [35:0] uv_write_data,
 output wire uv_read,output wire [4:0] uv_read_address,
 output wire light_bank,
 input wire retire_valid,retire_bank,
 input wire [15:0] context_epoch,
 input wire [1:0] context_mode,input wire [4:0] context_code,
 input wire signed [15:0] light_x,light_y,light_z,
 input wire [8:0] ambient,directional,
 input wire signed [15:0] ray_x,ray_y,ray_k,
 output wire context_wait,context_accepted,pixel_accepted,
 output wire out_valid,input wire out_ready,
 output wire [6:0] out_key,output wire out_quad_last,
 output wire [15:0] out_epoch,output wire [8:0] out_g,out_h
);
wire light_ready,light_valid,light_last;
wire [35:0] light_data;
wire [2:0] light_row;
wire [9:0] light_descriptor;
assign light_bank=light_descriptor[9];
// All Sampling/Basic/Quad ports are passed directly. This is the only Quad
// instance; the consumer below owns no global descriptor or source queue.
gpu_v2_quad_sampling_ingress sampling(
 .clk(clk),.reset(reset),.ce(ce),.in_valid(in_valid),.in_begin(in_begin),
 .in_basic(in_basic),.in_light(in_light),.in_uv(in_uv),.in_ready(in_ready),
 .advance(advance),.in_row(in_row),.published(published),
 .published_slot(published_slot),.published_begin(published_begin),
 .basic_write(basic_write),.basic_write_address(basic_write_address),.basic_write_data(basic_write_data),
 .basic_read(basic_read),.basic_read_address(basic_read_address),
 .basic_return_valid(basic_return_valid),.basic_return_data(basic_return_data),.basic_collision(basic_collision),
 .light_ready(light_ready),.light_valid(light_valid),.light_data(light_data),.light_row(light_row),
 .light_last(light_last),.light_descriptor(light_descriptor),
 .sample_ready(sample_ready),.sample_valid(sample_valid),.sample_data(sample_data),
 .sample_row(sample_row),.sample_last(sample_last),.sample_descriptor(sample_descriptor),.sample_fire(sample_fire),
 .rop_row7_accepted(rop_row7_accepted),.rop_slot(rop_slot),.retire_slot(retire_slot),
 .global_used(global_used),.light_used(light_used),.sample_used(sample_used),.fault(fault),
 .phase(phase),.context_credit(context_credit),.context_slot(context_slot),.draw_config(draw_config),
 .raw_write(raw_write),.raw_slot(raw_slot),.raw_metadata(raw_metadata),
 .committed(committed),.committed_slot(committed_slot),
 .d_take(d_take),.d_valid(d_valid),.d_slot(d_slot),.d_uv(d_uv),
 .c_request(c_request),.c_slot(c_slot),.c_lane(c_lane),.c_take(c_take),
 .c_empty(c_empty),.c_valid(c_valid),.c_owner_slot(c_owner_slot),.c_owner_lane(c_owner_lane),.c_uv(c_uv),
 .uv_write(uv_write),.uv_write_address(uv_write_address),.uv_write_data(uv_write_data),
 .uv_read(uv_read),.uv_read_address(uv_read_address)
);
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
 .out_epoch(out_epoch),.out_g(out_g),.out_h(out_h)
);
endmodule
