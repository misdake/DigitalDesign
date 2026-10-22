// Device4 production binding to the autonomous single-command-buffer GPU.
module CpuV3Gpu (
    input wire clk, input wire reset,
    input wire [2:0] device_index, input wire [3:0] device_channel,
    input wire device_read_enable, input wire device_write_enable,
    input wire [15:0] device_write_data,
    input wire gpu_ro_request_ready, input wire gpu_ro_write_data_ready,
    input wire gpu_ro_response_valid, input wire [63:0] gpu_ro_read_data,
    input wire gpu_ro_response_last, input wire gpu_ro_error,
    input wire gpu_fb_w_request_ready, input wire gpu_fb_w_write_data_ready,
    input wire gpu_fb_w_response_valid, input wire gpu_fb_w_response_last,
    input wire gpu_fb_w_error,
    input wire gpu_fb_r_request_ready, input wire gpu_fb_r_write_data_ready,
    input wire gpu_fb_r_response_valid, input wire [63:0] gpu_fb_r_read_data,
    input wire gpu_fb_r_response_last, input wire gpu_fb_r_error,
    output wire [15:0] device_read_data,
    output wire gpu_ro_request_valid, output wire gpu_ro_write,
    output wire [21:0] gpu_ro_address, output wire [1:0] gpu_ro_line_count_minus_1,
    output wire [63:0] gpu_ro_write_data,
    output wire gpu_fb_w_request_valid, output wire gpu_fb_w_write,
    output wire [21:0] gpu_fb_w_address, output wire [1:0] gpu_fb_w_line_count_minus_1,
    output wire [63:0] gpu_fb_w_write_data,
    output wire gpu_fb_r_request_valid, output wire gpu_fb_r_write,
    output wire [21:0] gpu_fb_r_address, output wire [1:0] gpu_fb_r_line_count_minus_1,
    output wire [63:0] gpu_fb_r_write_data
);
    // GPU byte addressing is internal; shared-memory ports are 16-bit words.
    assign gpu_ro_write=1'b0;assign gpu_ro_write_data=64'd0;
    assign gpu_fb_r_write=1'b0;assign gpu_fb_r_write_data=64'd0;
    assign gpu_fb_w_write=1'b1;
    gpu_v2_vliw_command core(
     .clk(clk),.reset(reset),.ce(1'b1),.abort(1'b0),.output_ready(1'b1),
     .device_index(device_index),.device_channel(device_channel),
     .device_read_enable(device_read_enable),.device_write_enable(device_write_enable),
     .device_write_data(device_write_data),.device_read_data(device_read_data),
     .ro_request_valid(gpu_ro_request_valid),.ro_request_ready(gpu_ro_request_ready),
     .ro_address(gpu_ro_address),.ro_count(gpu_ro_line_count_minus_1),
     .ro_response_valid(gpu_ro_response_valid),.ro_response_last(gpu_ro_response_last),
     .ro_response_error(gpu_ro_error),.ro_data(gpu_ro_read_data),
     .fb_r_request_valid(gpu_fb_r_request_valid),.fb_r_request_ready(gpu_fb_r_request_ready),
     .fb_r_address(gpu_fb_r_address),.fb_r_count(gpu_fb_r_line_count_minus_1),
     .fb_r_response_valid(gpu_fb_r_response_valid),.fb_r_response_last(gpu_fb_r_response_last),
     .fb_r_response_error(gpu_fb_r_error),.fb_r_data(gpu_fb_r_read_data),
     .fb_w_request_valid(gpu_fb_w_request_valid),.fb_w_request_ready(gpu_fb_w_request_ready),
     .fb_w_address(gpu_fb_w_address),.fb_w_count(gpu_fb_w_line_count_minus_1),
     .fb_w_data(gpu_fb_w_write_data),.fb_w_data_ready(gpu_fb_w_write_data_ready),
     .fb_w_response_valid(gpu_fb_w_response_valid),.fb_w_response_last(gpu_fb_w_response_last),
     .fb_w_response_error(gpu_fb_w_error));
endmodule
