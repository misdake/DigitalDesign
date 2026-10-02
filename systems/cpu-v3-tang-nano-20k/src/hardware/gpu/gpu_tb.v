`timescale 1ns/1ps
module tb;
reg clk=0, reset=0;
reg [2:0] device_index=0;
reg [3:0] device_channel=0;
reg device_read_enable=0, device_write_enable=0;
reg [15:0] device_write_data=16'hffff;
reg gpu_ro_request_ready=1, gpu_ro_write_data_ready=1;
reg gpu_ro_response_valid=1, gpu_ro_response_last=1, gpu_ro_error=1;
reg [63:0] gpu_ro_read_data=64'h0123456789abcdef;
reg gpu_fb_w_request_ready=1, gpu_fb_w_write_data_ready=1;
reg gpu_fb_w_response_valid=1, gpu_fb_w_response_last=1, gpu_fb_w_error=1;
reg gpu_fb_r_request_ready=1, gpu_fb_r_write_data_ready=1;
reg gpu_fb_r_response_valid=1, gpu_fb_r_response_last=1, gpu_fb_r_error=1;
reg [63:0] gpu_fb_r_read_data=64'hfedcba9876543210;
wire [15:0] device_read_data;
wire gpu_ro_request_valid, gpu_ro_write;
wire [21:0] gpu_ro_address;
wire [1:0] gpu_ro_line_count_minus_1;
wire [63:0] gpu_ro_write_data;
wire gpu_fb_w_request_valid, gpu_fb_w_write;
wire [21:0] gpu_fb_w_address;
wire [1:0] gpu_fb_w_line_count_minus_1;
wire [63:0] gpu_fb_w_write_data;
wire gpu_fb_r_request_valid, gpu_fb_r_write;
wire [21:0] gpu_fb_r_address;
wire [1:0] gpu_fb_r_line_count_minus_1;
wire [63:0] gpu_fb_r_write_data;
CpuV3Gpu dut(.*);
always #5 clk=~clk;
integer sample;
reg [15:0] expected_status;
initial begin
    // Exhaust every device/channel/read/write/reset combination. Ready,
    // responses and errors on all memory inputs must never cause a request.
    for(sample=0; sample<1024; sample=sample+1) begin
        device_index=sample>>7;
        device_channel=(sample>>3)&15;
        device_read_enable=(sample>>2)&1;
        device_write_enable=(sample>>1)&1;
        reset=sample&1;
        device_write_data=sample^16'hffff;
        #1;
        expected_status=(device_index==4 && device_channel==2 && device_read_enable) ? 16'h0004 : 16'h0000;
        if(device_read_data !== expected_status) $fatal(1,"legacy status sample=%0d got=%h",sample,device_read_data);
        if({gpu_ro_request_valid,gpu_ro_write,gpu_ro_address,gpu_ro_line_count_minus_1,gpu_ro_write_data,
            gpu_fb_w_request_valid,gpu_fb_w_write,gpu_fb_w_address,gpu_fb_w_line_count_minus_1,gpu_fb_w_write_data,
            gpu_fb_r_request_valid,gpu_fb_r_write,gpu_fb_r_address,gpu_fb_r_line_count_minus_1,gpu_fb_r_write_data} !== 270'd0)
            $fatal(1,"retired GPU issued memory activity sample=%0d",sample);
    end
    $display("DIGITAL_DESIGN_PASS"); $finish;
end
initial begin #2048; $fatal(1,"retired GPU test watchdog"); end
endmodule
