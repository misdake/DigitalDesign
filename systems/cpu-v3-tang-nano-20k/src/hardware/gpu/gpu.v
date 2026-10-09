// Retired GPU integration leaf. All memory ports are inactive and the legacy
// status register reports submit rejected while the v2 cmodel is developed.
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
    assign device_read_data = device_read_enable && device_index == 3'd4 &&
        device_channel == 4'd2 ? 16'h0004 : 16'h0000;
    assign gpu_ro_request_valid = 1'b0;
    assign gpu_ro_write = 1'b0;
    assign gpu_ro_address = 22'b0;
    assign gpu_ro_line_count_minus_1 = 2'b0;
    assign gpu_ro_write_data = 64'b0;
    assign gpu_fb_w_request_valid = 1'b0;
    assign gpu_fb_w_write = 1'b0;
    assign gpu_fb_w_address = 22'b0;
    assign gpu_fb_w_line_count_minus_1 = 2'b0;
    assign gpu_fb_w_write_data = 64'b0;
    assign gpu_fb_r_request_valid = 1'b0;
    assign gpu_fb_r_write = 1'b0;
    assign gpu_fb_r_address = 22'b0;
    assign gpu_fb_r_line_count_minus_1 = 2'b0;
    assign gpu_fb_r_write_data = 64'b0;
endmodule
