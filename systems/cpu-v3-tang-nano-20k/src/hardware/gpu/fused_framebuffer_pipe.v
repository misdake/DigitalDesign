`timescale 1ns/1ps
// One in-service quad. The source holds RGBA/Z, state and its resident lease
// through the actual masked-write/input-ready edge. Execution is external.
module FusedFramebufferPipe #(
    parameter SLOT_BITS = 2,
    parameter HALF_CAPACITY = 1,
    parameter BANK_ORDER = 1,
    parameter PORTABLE_MODEL = 0
) (
    input wire clk, input wire reset,
    input wire input_valid, output wire input_ready, input wire resident,
    input wire input_group, input wire [SLOT_BITS-1:0] input_way,
    input wire [3:0] input_x, input wire [3:0] input_y,
    input wire [127:0] input_colors, input wire [63:0] input_depths,
    input wire [3:0] input_mask, input wire depth_enable, input wire depth_write,
    input wire [2:0] depth_func, input wire [1:0] blend_mode,
    output wire execute_valid, input wire execute_ready,
    output wire [127:0] execute_colors, output wire [63:0] execute_depths,
    output wire [3:0] execute_mask,
    output wire [63:0] old_colors, output wire [63:0] old_depths,
    input wire [63:0] result_colors, input wire [3:0] result_mask,
    output wire commit_valid, input wire commit_ready, output wire [3:0] commit_mask,
    input wire memory_write_valid, output wire memory_write_ready,
    input wire memory_write_group, input wire [9:0] memory_write_address,
    input wire [63:0] memory_write_data, input wire [7:0] memory_write_mask,
    input wire memory_read_valid, output wire memory_read_ready,
    input wire memory_read_group, input wire [9:0] memory_read_address,
    output wire memory_response_valid, input wire memory_response_ready,
    output wire [63:0] memory_response_data
);
    localparam IDLE = 0, EXECUTE = 1;
    reg state = IDLE;
    wire rsv, rrr, rwr;
    wire rrv = !reset && state == IDLE && input_valid && resident;
    // execute_ready means the external result is available, not a separate
    // request acceptance. Execution side effects advance on commit_valid only.
    assign execute_valid = !reset && state == EXECUTE && rsv;
    wire rwv = execute_valid && execute_ready && commit_ready;
    wire execute_fire = rwv && rwr;
    // Capture only the lane-order bit at the accepted read. No numeric payload
    // is copied; source fields and the resident lease still belong upstream.
    reg reorder = 0;
    always @(posedge clk) if (rrv && rrr) reorder <= BANK_ORDER && input_x[1];
    assign execute_colors = reorder ? {input_colors[63:0], input_colors[127:64]} : input_colors;
    assign execute_depths = reorder ? {input_depths[31:0], input_depths[63:32]} : input_depths;
    assign execute_mask = reorder ? {input_mask[1:0], input_mask[3:2]} : input_mask;
    wire [3:0] cm = execute_mask & result_mask;
    wire [3:0] zm = depth_enable && depth_write ? cm : 4'b0;
    wire [63:0] wc = result_colors;
    wire [63:0] wz = execute_depths;
    wire [3:0] wy = input_y;

    // Completion is an event on the actual-write edge. Permission may stall
    // execution; this is not a separately backpressured valid/result stream.
    assign commit_valid = execute_fire;
    assign input_ready = execute_fire;
    assign commit_mask = reorder ? {cm[1:0], cm[3:2]} : cm;

    FramebufferLaneArray #(.SLOT_BITS(SLOT_BITS), .HALF_CAPACITY(HALF_CAPACITY), .BANK_ORDER(BANK_ORDER), .PORTABLE_MODEL(PORTABLE_MODEL)) array (
        .clk(clk), .reset(reset),
        .memory_write_valid(memory_write_valid), .memory_write_ready(memory_write_ready),
        .memory_write_group(memory_write_group), .memory_write_address(memory_write_address),
        .memory_write_data(memory_write_data), .memory_write_mask(memory_write_mask),
        .memory_read_valid(memory_read_valid), .memory_read_ready(memory_read_ready),
        .memory_read_group(memory_read_group), .memory_read_address(memory_read_address),
        .memory_response_valid(memory_response_valid), .memory_response_ready(memory_response_ready),
        .memory_response_data(memory_response_data),
        .render_write_valid(rwv), .render_write_ready(rwr),
        .render_write_group(input_group), .render_write_way(input_way),
        .render_write_x(input_x), .render_write_y(wy),
        .render_write_colors(wc), .render_write_depths(wz),
        .render_write_color_mask(cm), .render_write_depth_mask(zm),
        .render_read_valid(rrv), .render_read_ready(rrr),
        .render_read_group(input_group), .render_read_way(input_way),
        .render_read_x(input_x), .render_read_y(input_y),
        .render_response_valid(rsv), .render_response_ready(execute_fire),
        .render_response_colors(old_colors), .render_response_depths(old_depths)
    );
    always @(posedge clk) begin
        if (reset) state <= IDLE;
        else begin
            if (rrv && rrr) state <= EXECUTE;
            if (execute_fire) state <= IDLE;
        end
    end

    // synthesis translate_off
    reg held = 0;
    reg [SLOT_BITS+211:0] snapshot;
    always @(posedge clk) begin
        if (reset) held <= 0;
        else begin
            if (held && (!input_valid ||
                {input_group,input_way,input_x,input_y,input_colors,input_depths,
                 input_mask,depth_enable,depth_write,depth_func,blend_mode} !== snapshot))
                $fatal(1, "fused source/lease changed before actual commit");
            if (state != IDLE && !input_valid) $fatal(1, "active fused request withdrawn");
            if (state != IDLE && !resident) $fatal(1, "active fused resident lease withdrawn");
            held <= input_valid && !input_ready;
            snapshot <= {input_group,input_way,input_x,input_y,input_colors,input_depths,
                         input_mask,depth_enable,depth_write,depth_func,blend_mode};
        end
    end
    // synthesis translate_on
endmodule
