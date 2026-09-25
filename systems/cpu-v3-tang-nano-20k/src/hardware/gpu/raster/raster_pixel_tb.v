`timescale 1ns/1ps
module tb;
reg clk = 0;
always #5 clk = ~clk;
reg reset = 1;
reg input_valid = 0, input_last = 0;
reg [31:0] input_data = 0;
wire input_ready;
reg pixel_ready = 1;
integer throttle_mode = 0, cycles = 0, cycle_limit = 2000000;
integer scenes_started = 0, scenes_done = 0, tb_seq = 0;
wire pixel_valid, pixel_is_retire_marker, pixel_marker_draw;
wire [31:0] pixel_tri;
wire [15:0] pixel_tile, pixel_x, pixel_y, pixel_color;
wire prefetch_acquire_valid;
wire [15:0] prefetch_acquire_tile;
wire scene_done;
reg held_valid = 0;
reg [97:0] held_item = 0;
wire [97:0] output_item = {pixel_is_retire_marker, pixel_marker_draw,
                           pixel_tri, pixel_tile, pixel_x, pixel_y,
                           pixel_color};

CpuV3GpuRasterPixel dut(
    .clk(clk), .reset(reset),
    .input_valid(input_valid), .input_last(input_last),
    .input_data(input_data), .input_ready(input_ready),
    .pixel_ready(pixel_ready), .pixel_valid(pixel_valid),
    .pixel_is_retire_marker(pixel_is_retire_marker),
    .pixel_marker_draw(pixel_marker_draw), .pixel_tri(pixel_tri),
    .pixel_tile(pixel_tile), .pixel_x(pixel_x), .pixel_y(pixel_y),
    .pixel_color(pixel_color),
    .prefetch_acquire_valid(prefetch_acquire_valid),
    .prefetch_acquire_tile(prefetch_acquire_tile),
    .scene_done(scene_done));

always @(*) begin
    case (throttle_mode)
        1: pixel_ready = (cycles % 5) < 3;
        2: pixel_ready = (cycles % 3) == 0;
        default: pixel_ready = 1;
    endcase
end

always @(posedge clk) begin
    cycles = cycles + 1;
    if (cycles > cycle_limit)
        $fatal(1, "pixel raster cycle limit exceeded");
    if (reset) begin
        held_valid <= 0;
    end else begin
        if (held_valid && (!pixel_valid || output_item !== held_item))
            $fatal(1, "pixel raster output changed while stalled");
        held_valid <= pixel_valid && !pixel_ready;
        if (pixel_valid && !pixel_ready)
            held_item <= output_item;
        if (pixel_valid && pixel_ready) begin
            if (pixel_is_retire_marker)
                $display("RAST_OUT RETIRE_MARKER %0d DRAW %0d",
                         pixel_tri, pixel_marker_draw);
            else
                $display("RAST_OUT PIXEL %0d %0d %0d %0d %04x",
                         pixel_tri, pixel_tile, pixel_x, pixel_y, pixel_color);
        end
    end
end

always @(negedge clk) begin
    if (scene_done)
        scenes_done = scenes_done + 1;
end

task send_beat;
    input [31:0] data;
    input last;
    begin
        @(negedge clk);
        input_data = data;
        input_last = last;
        input_valid = 1;
        @(posedge clk);
        while (!input_ready) @(posedge clk);
        @(negedge clk);
        input_valid = 0;
        input_last = 0;
    end
endtask

task drive_tri;
    input [31:0] v0, v1, v2;
    input last;
    begin
        send_beat(v0, 0);
        send_beat(v1, 0);
        send_beat(v2, last);
    end
endtask

task scene_begin;
    input [8*48:1] name;
    begin
        while (scenes_done < scenes_started) @(negedge clk);
        scenes_started = scenes_started + 1;
        @(negedge clk);
        $display("RAST %0d SCENE %0s", tb_seq, name);
        tb_seq = tb_seq + 1;
    end
endtask

initial begin
    if (!$value$plusargs("THROTTLE=%d", throttle_mode)) throttle_mode = 0;
    if (!$value$plusargs("CYCLE_LIMIT=%d", cycle_limit)) cycle_limit = 2000000;
    repeat (4) @(negedge clk);
    reset = 0;
`ifdef RASTER_COSIM
`include "raster_vectors.vh"
`else
    scene_begin("viewport-smoke");
    drive_tri({16'd90 << 4, 16'd150 << 4},
              {16'd90 << 4, 16'd250 << 4},
              {16'd150 << 4, 16'd200 << 4}, 1);
`endif
    while (scenes_done < scenes_started) @(negedge clk);
    repeat (8) @(negedge clk);
    if (pixel_valid !== 1'b0)
        $fatal(1, "pixel raster stream not drained");
    $display("DIGITAL_DESIGN_PASS");
    $finish;
end
endmodule
