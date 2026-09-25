`timescale 1ns/1ps
// Stage-5 serial pixel interface. The raster core keeps its compact quad
// FIFO; this leaf expands covered lanes at the cache boundary. A marker uses
// the same valid/ready stream, so accepting it acknowledges every preceding
// pixel at this boundary. The framebuffer cache must withhold ready until its
// preceding pixel read/modify/writes have committed.
module CpuV3GpuRasterPixel (
    input wire clk,
    input wire reset,
    input wire input_valid,
    input wire input_last,
    input wire [31:0] input_data,
    output wire input_ready,
    input wire pixel_ready,
    output wire pixel_valid,
    output wire pixel_is_retire_marker,
    output wire pixel_marker_draw,
    output wire [31:0] pixel_tri,
    output wire [15:0] pixel_tile,
    output wire [15:0] pixel_x,
    output wire [15:0] pixel_y,
    output wire [15:0] pixel_color,
    output wire prefetch_acquire_valid,
    output wire [15:0] prefetch_acquire_tile,
    output wire scene_done
);
    wire quad_valid, quad_is_tile_end, quad_is_retire_marker;
    wire retire_marker_draw;
    wire [31:0] quad_tri;
    wire [15:0] quad_tile, quad_x, quad_y;
    wire [3:0] quad_mask;
    wire quad_ready;
    reg [1:0] lane = 2'd0;

    CpuV3GpuRaster core (
        .clk(clk), .reset(reset),
        .input_valid(input_valid), .input_last(input_last),
        .input_data(input_data), .input_ready(input_ready),
        .sink_ready(quad_ready),
        .quad_valid(quad_valid), .quad_is_tile_end(quad_is_tile_end),
        .quad_is_retire_marker(quad_is_retire_marker),
        .retire_marker_draw(retire_marker_draw),
        .quad_tri(quad_tri), .quad_tile(quad_tile),
        .quad_x(quad_x), .quad_y(quad_y), .quad_mask(quad_mask),
        .prefetch_acquire_valid(prefetch_acquire_valid),
        .prefetch_acquire_tile(prefetch_acquire_tile),
        .scene_done(scene_done)
    );

    wire [3:0] remaining = quad_mask & (4'b1111 << lane);
    wire [1:0] selected_lane = remaining[0] ? 2'd0
                             : remaining[1] ? 2'd1
                             : remaining[2] ? 2'd2 : 2'd3;
    wire [3:0] later = quad_mask & (4'b1110 << selected_lane);
    wire has_pixel = |remaining;
    wire last_lane = later == 4'd0;
    wire pixel_fire = pixel_valid && pixel_ready && !quad_is_retire_marker;

    assign pixel_valid = quad_valid && !quad_is_tile_end
                       && (quad_is_retire_marker || has_pixel);
    assign pixel_is_retire_marker = quad_is_retire_marker;
    assign pixel_marker_draw = quad_is_retire_marker && retire_marker_draw;
    assign pixel_tri = quad_tri;
    assign pixel_tile = quad_tile;
    assign pixel_x = quad_x + {15'd0, selected_lane[0]};
    assign pixel_y = quad_y + {15'd0, selected_lane[1]};
    // Reference stage-5 gradient: RGB565 with a per-triangle blue offset.
    wire [4:0] blue = (pixel_tri[4:0] << 3) - pixel_tri[4:0]
                      + pixel_x[8:4];
    assign pixel_color = {pixel_x[7:3], pixel_y[7:2], blue};
    assign quad_ready = quad_valid &&
        (quad_is_tile_end || (!quad_is_retire_marker && !has_pixel)
         || (pixel_ready && (quad_is_retire_marker || last_lane)));

    always @(posedge clk) begin
        if (reset || (quad_valid && quad_ready))
            lane <= 2'd0;
        else if (pixel_fire)
            lane <= selected_lane + 2'd1;
    end
endmodule
