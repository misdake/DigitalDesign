// Explicit Icarus co-simulation of the handwritten GPU command FSM.
//
// The testbench substitutes a small direct-memory responder for the arbiter and
// SharedSdramPort: one 32-byte line read streams four 64-bit beats, and one
// 32-byte framebuffer write captures four beats. It then drives the real device
// register sequence for one submission whose three draws total 375 tiles and
// checks the counter/status contract and the first/last tile pixel.
`timescale 1ns/1ps
module tb;
reg clk = 0; always #5 clk = ~clk;
integer cycles = 0;
always @(posedge clk) begin
    cycles <= cycles + 1;
    if (cycles > 4000000) $fatal(1, "GPU testbench cycle limit exceeded");
end

// Command buffer at word address 0x100; memory spans both framebuffer slots.
localparam CMD_BASE = 22'h000100;
reg [15:0] memory [0:22'h240000];

reg [2:0] device_index = 0;
reg [3:0] device_channel = 0;
reg device_read_enable = 0, device_write_enable = 0;
reg [15:0] device_write_data = 0;
wire [15:0] device_read_data;

reg gpu_ro_request_ready = 0, gpu_ro_write_data_ready = 0, gpu_ro_response_valid = 0, gpu_ro_response_last = 0, gpu_ro_error = 0;
reg [63:0] gpu_ro_read_data = 0;
reg gpu_fb_w_request_ready = 0, gpu_fb_w_write_data_ready = 0, gpu_fb_w_response_valid = 0, gpu_fb_w_response_last = 0, gpu_fb_w_error = 0;

wire gpu_ro_request_valid, gpu_ro_write;
wire [21:0] gpu_ro_address;
wire [63:0] gpu_ro_write_data;
wire gpu_fb_w_request_valid, gpu_fb_w_write;
wire [21:0] gpu_fb_w_address;
wire [63:0] gpu_fb_w_write_data;
wire gpu_fb_r_request_valid, gpu_fb_r_write;
wire [21:0] gpu_fb_r_address;
wire [63:0] gpu_fb_r_write_data;

reg reset = 1;
CpuV3Gpu dut(.clk(clk), .reset(reset), .device_index(device_index), .device_channel(device_channel),
    .device_read_enable(device_read_enable), .device_write_enable(device_write_enable),
    .device_write_data(device_write_data), .device_read_data(device_read_data),
    .gpu_ro_request_ready(gpu_ro_request_ready), .gpu_ro_write_data_ready(gpu_ro_write_data_ready), .gpu_ro_response_valid(gpu_ro_response_valid),
    .gpu_ro_read_data(gpu_ro_read_data), .gpu_ro_response_last(gpu_ro_response_last),
    .gpu_ro_error(gpu_ro_error),
    .gpu_fb_w_request_ready(gpu_fb_w_request_ready), .gpu_fb_w_write_data_ready(gpu_fb_w_write_data_ready), .gpu_fb_w_response_valid(gpu_fb_w_response_valid),
    .gpu_fb_w_response_last(gpu_fb_w_response_last), .gpu_fb_w_error(gpu_fb_w_error),
    .gpu_fb_r_request_ready(1'b1), .gpu_fb_r_write_data_ready(1'b0), .gpu_fb_r_response_valid(1'b0),
    .gpu_fb_r_read_data(64'h0), .gpu_fb_r_response_last(1'b0), .gpu_fb_r_error(1'b0),
    .gpu_ro_request_valid(gpu_ro_request_valid), .gpu_ro_write(gpu_ro_write),
    .gpu_ro_address(gpu_ro_address), .gpu_ro_write_data(gpu_ro_write_data),
    .gpu_fb_w_request_valid(gpu_fb_w_request_valid), .gpu_fb_w_write(gpu_fb_w_write),
    .gpu_fb_w_address(gpu_fb_w_address), .gpu_fb_w_write_data(gpu_fb_w_write_data),
    .gpu_fb_r_request_valid(gpu_fb_r_request_valid), .gpu_fb_r_write(gpu_fb_r_write),
    .gpu_fb_r_address(gpu_fb_r_address), .gpu_fb_r_write_data(gpu_fb_r_write_data));

// ---- memory responder ----
integer beat;
always @(posedge clk) begin
    gpu_ro_response_valid <= 0;
    gpu_fb_w_response_valid <= 0;
    if (gpu_ro_request_valid && gpu_ro_request_ready) begin
        // Stream four beats on the following four clocks.
        for (beat = 0; beat < 4; beat = beat + 1) begin
            @(negedge clk);
            gpu_ro_read_data[15:0] = memory[gpu_ro_address + 4 * beat];
            gpu_ro_read_data[31:16] = memory[gpu_ro_address + 4 * beat + 1];
            gpu_ro_read_data[47:32] = memory[gpu_ro_address + 4 * beat + 2];
            gpu_ro_read_data[63:48] = memory[gpu_ro_address + 4 * beat + 3];
            gpu_ro_response_last = beat == 3;
            gpu_ro_response_valid = 1;
            @(posedge clk);
        end
        @(negedge clk);
        gpu_ro_response_valid = 0;
        gpu_ro_response_last = 0;
    end else if (gpu_fb_w_request_valid && gpu_fb_w_request_ready) begin
        for (beat = 0; beat < 4; beat = beat + 1) begin
            @(negedge clk);
            gpu_fb_w_response_valid = 1;
            gpu_fb_w_response_last = beat == 3;
            if (beat == 0) begin
                memory[gpu_fb_w_address] = gpu_fb_w_write_data[15:0];
                memory[gpu_fb_w_address + 1] = gpu_fb_w_write_data[31:16];
                memory[gpu_fb_w_address + 2] = gpu_fb_w_write_data[47:32];
                memory[gpu_fb_w_address + 3] = gpu_fb_w_write_data[63:48];
            end else begin
                memory[gpu_fb_w_address + 4 * beat] = gpu_fb_w_write_data[15:0];
                memory[gpu_fb_w_address + 4 * beat + 1] = gpu_fb_w_write_data[31:16];
                memory[gpu_fb_w_address + 4 * beat + 2] = gpu_fb_w_write_data[47:32];
                memory[gpu_fb_w_address + 4 * beat + 3] = gpu_fb_w_write_data[63:48];
            end
            @(posedge clk);
        end
        @(negedge clk);
        gpu_fb_w_response_valid = 0;
        gpu_fb_w_response_last = 0;
    end
end

reg [15:0] read_result;

task device_write; input [3:0] channel; input [15:0] value; begin
    @(negedge clk);
    device_index = 3'd4; device_channel = channel;
    device_write_enable = 1; device_write_data = value;
    @(posedge clk);
    @(negedge clk);
    device_write_enable = 0;
end endtask

task device_read; input [3:0] channel; begin
    @(negedge clk);
    device_index = 3'd4; device_channel = channel; device_read_enable = 1;
    #1;
    read_result = device_read_data;
    device_read_enable = 0;
end endtask

integer index;
reg [4:0] tile_x;
reg [3:0] tile_y;
reg [4:0] r5;
reg [5:0] g6;
reg [4:0] b5;
reg [15:0] expected;
// Store one 64-bit qword as four little-endian 16-bit words.
task set_qword; input [21:0] address; input [63:0] value; begin
    memory[address] = value[15:0];
    memory[address + 1] = value[31:16];
    memory[address + 2] = value[47:32];
    memory[address + 3] = value[63:48];
end endtask

initial begin
    for (index = 0; index < 22'h240001; index = index + 1) memory[index] = 16'h0000;

    // Three FAKE_DRAW packets, each 125 tiles, with distinct parameters.
    // SET_TARGET slot A = 0x0020_0000: opcode e0, count 1, arg0 = 0x00200000.
    set_qword(CMD_BASE, 64'h00200000_000001e0);
    // draw 0: count 2, arg0 = 125 tiles, color mode 0; phase 3, biases 5/7/9
    set_qword(CMD_BASE + 4, 64'h0000007d_000002e1);
    set_qword(CMD_BASE + 8, 64'h0009_0007_0005_0003);
    // draw 1: phase 100, biases 1/2/3
    set_qword(CMD_BASE + 12, 64'h0000007d_000002e1);
    set_qword(CMD_BASE + 16, 64'h0003_0002_0001_0064);
    // draw 2: phase 200, biases 11/12/13
    set_qword(CMD_BASE + 20, 64'h0000007d_000002e1);
    set_qword(CMD_BASE + 24, 64'h000d_000c_000b_00c8);
    // END
    set_qword(CMD_BASE + 28, 64'h00000000_000001ff);

    // Response-ready is always asserted; the responder above is the master.
    gpu_ro_request_ready = 1;
    gpu_fb_w_request_ready = 1;

    repeat(4) @(posedge clk);
    reset = 0;
    repeat(2) @(posedge clk);

    device_write(4'd0, CMD_BASE[15:0]);
    device_write(4'd1, CMD_BASE[21:16]);
    device_write(4'd2, 16'd32);
    device_write(4'd3, 16'd0);
    device_write(4'd4, 16'd0);

    device_read(4'd0);
    if (read_result != 16'd1) $fatal(1, "received_count is not 1");

    // Wait for retirement (bounded).
    index = 0;
    device_read(4'd1);
    while (read_result != 16'd1) begin
        index = index + 1;
        if (index > 100000) $fatal(1, "submission did not retire");
        device_read(4'd1);
    end
    device_read(4'd2);
    if (read_result & 16'h0008) $fatal(1, "unexpected command error");
    device_read(4'd3);
    if (read_result != 16'd0) $fatal(1, "fifo not empty after retirement");

    // Check the first tile (0,0, phase 3, biases 5/7/9).
    tile_x = 5'd0; tile_y = 4'd0;
    r5 = (tile_x + 5'd5 + 5'd3) & 5'd31;
    g6 = ({tile_y, 2'b00} + 6'd7 + 6'd0) & 6'd63;
    b5 = (tile_x + tile_y + 5'd9 + 5'd3) & 5'd31;
    expected = {r5, g6, b5};
    if (memory[22'h200000] != expected) $fatal(1, "first tile pixel mismatch");

    // Check the last tile (24,14, phase 200, biases 11/12/13).
    tile_x = 5'd24; tile_y = 4'd14;
    r5 = (tile_x + 5'd11 + 5'd8) & 5'd31;      // 200[4:0] = 8
    g6 = ({tile_y, 2'b00} + 6'd12 + 6'd50) & 6'd63;  // 200[7:2] = 50
    b5 = (tile_x + tile_y + 5'd13 + 5'd8) & 5'd31;
    expected = {r5, g6, b5};
    if (memory[22'h200000 + 374 * 256] != expected) $fatal(1, "last tile pixel mismatch");

    $display("DIGITAL_DESIGN_PASS");
    $finish;
end
endmodule
