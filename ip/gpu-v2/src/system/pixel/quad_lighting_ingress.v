// Actual registered Quad source queues feeding the selected Lighting core.
// The parent supplies context from light_bank and owns results/ROP retirement.
module gpu_v2_quad_lighting_ingress(
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
 input wire sample_ready,output wire sample_valid,
 output wire [35:0] sample_data,output wire [1:0] sample_row,
 output wire sample_last,output wire [10:0] sample_descriptor,
 input wire rop_row7_accepted,input wire [4:0] rop_slot,
 output wire [4:0] retire_slot,output wire [5:0] global_used,
 output wire [1:0] light_used,output wire [3:0] sample_used,
 output wire fault,
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
wire light_valid,light_ready,light_last;
wire [35:0] light_data;
wire [2:0] light_row;
wire [9:0] light_descriptor;
assign light_bank=light_descriptor[9];
gpu_v2_quad_ingress quad(
 .clk(clk),.reset(reset),.ce(ce),.in_valid(in_valid),.in_begin(in_begin),
 .in_basic(in_basic),.in_light(in_light),.in_uv(in_uv),
 .in_ready(in_ready),.advance(advance),.in_row(in_row),
 .published(published),.published_slot(published_slot),.published_begin(published_begin),
 .basic_write(basic_write),.basic_write_address(basic_write_address),.basic_write_data(basic_write_data),
 .basic_read(basic_read),.basic_read_address(basic_read_address),
 .basic_return_valid(basic_return_valid),.basic_return_data(basic_return_data),.basic_collision(basic_collision),
 .light_ready(light_ready),.sample_ready(sample_ready),.light_valid(light_valid),.sample_valid(sample_valid),
 .light_data(light_data),.sample_data(sample_data),.light_row(light_row),.sample_row(sample_row),
 .light_last(light_last),.sample_last(sample_last),.light_descriptor(light_descriptor),.sample_descriptor(sample_descriptor),
 .rop_row7_accepted(rop_row7_accepted),.rop_slot(rop_slot),.retire_slot(retire_slot),
 .global_used(global_used),.light_used(light_used),.sample_used(sample_used),.fault(fault)
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
