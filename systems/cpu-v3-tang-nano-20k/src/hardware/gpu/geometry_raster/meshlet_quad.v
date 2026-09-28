// Two committed raw meshlet slots -> shared-multiplier geometry microcore
// -> existing viewport raster, perspective RGB, and ordered quad queue.
// This independent top uses the manual slot-load interface; it is not the
// command processor, framebuffer cache, or fitted system GPU.
module GpuMeshletQuad(input wire clk,input wire reset,
    input wire load_valid,output wire load_ready,input wire load_slot,
    input wire [8:0] load_address,input wire [63:0] load_data,
    input wire commit_valid,output wire commit_ready,input wire commit_slot,
    input wire [6:0] commit_vertex_count,input wire [7:0] commit_triangle_count,
    output wire release_valid,output wire release_slot,
    output wire output_valid,input wire output_ready,output wire [30:0] output_work,
    output wire [3:0] output_mask,output wire [127:0] output_color,
    output wire output_error,output reg sticky_error);
wire source_valid,source_ready,source_end,source_last,source_error,source_slot;
wire [7:0] source_index;
wire [95:0] source_xy;
wire [707:0] source_planes;
wire [5:0] source_scale;
GpuMeshletMicrocore source(clk,reset,load_valid,load_ready,load_slot,
    load_address,load_data,commit_valid,commit_ready,commit_slot,
    commit_vertex_count,commit_triangle_count,
    source_valid,source_ready,source_end,source_last,source_error,
    source_slot,source_index,source_xy,source_planes,source_scale,
    release_valid,release_slot);

localparam IDLE=0,INSTALL=1,FEED=2,DRAIN=3;
reg [1:0] state;
reg [1:0] corner;
reg [95:0] xy;
reg [707:0] planes;
reg [5:0] scale;
reg slot;
reg [6:0] source_id;
reg [2:0] primitive_id;
reg raster_finished;
wire config_ready,interp_ready;
assign source_ready=state==IDLE && source_valid &&
    (!source_end || interp_ready);
wire accept_triangle=source_valid && source_ready && !source_end;
wire accept_end=source_valid && source_ready && source_end;

wire raster_input_ready,quad_valid,tile_end,retire_marker,retire_draw;
wire [31:0] quad_tri;
wire [15:0] quad_tile,quad_x,quad_y;
wire [3:0] quad_mask;
wire raster_done,prefetch_valid;
wire [15:0] prefetch_tile;
CpuV3GpuRaster #(.VIEWPORT_ONLY(1),.PREFETCH_LIMIT(1)) raster(clk,reset,
    state==FEED,corner==2,xy[corner*32+:32],raster_input_ready,
    interp_ready,quad_valid,tile_end,retire_marker,retire_draw,
    quad_tri,quad_tile,quad_x,quad_y,quad_mask,
    1'b1,prefetch_valid,prefetch_tile,raster_done);
wire [30:0] raster_work={slot,1'b0,
    (retire_marker ? 2'd3 : (tile_end ? 2'd1 : 2'd0)),
    primitive_id,source_id,quad_y[7:0],quad_x[8:0]};
wire [30:0] end_work={source_slot,source_last,2'd2,3'd0,
    source_index[6:0],17'd0};
wire pair_valid,pair_ready;
wire [31:0] pair_tag;
wire [1:0] pair_mask;
wire [47:0] pair_denominator;
wire [179:0] pair_numerator;
wire interp_idle;
GpuColorInterpolator interpolator(clk,reset,state==INSTALL,config_ready,
    planes,scale,quad_valid || accept_end,interp_ready,
    quad_valid ? raster_work : end_work,
    quad_valid ? quad_mask : 4'd0,
    pair_valid,pair_ready,pair_tag,pair_mask,
    pair_denominator,pair_numerator,interp_idle);
wire [63:0] compact_color;
wire queue_idle;
GpuColorPairQueue queue(clk,reset,pair_valid,pair_ready,pair_tag,
    pair_mask,pair_denominator,pair_numerator,
    output_valid,output_ready,output_work,output_mask,
    compact_color,output_error,queue_idle);
genvar lane;
generate for(lane=0;lane<4;lane=lane+1) begin: rgba_lane
    wire [15:0] color=compact_color[lane*16+:16];
    wire [7:0] r={color[15:11],color[15:13]};
    wire [7:0] g={color[10:5],color[10:9]};
    wire [7:0] b={color[4:0],color[4:2]};
    assign output_color[lane*32+:32]={8'hff,b,g,r};
end endgenerate

always @(posedge clk) begin
    if(reset) begin
        state<=IDLE;corner<=0;xy<=0;planes<=0;scale<=0;
        slot<=0;source_id<=0;primitive_id<=0;raster_finished<=0;
        sticky_error<=0;
    end else begin
        if(accept_end && source_error) sticky_error<=1;
        if(output_valid && output_ready && output_error) sticky_error<=1;
        if(raster_done) raster_finished<=1;
        case(state)
            IDLE:begin
                if(accept_triangle) begin
                    xy<=source_xy;planes<=source_planes;scale<=source_scale;
                    slot<=source_slot;source_id<=source_index[6:0];
                    corner<=0;raster_finished<=0;state<=INSTALL;
                    if(source_error) sticky_error<=1;
                end else if(accept_end) primitive_id<=0;
            end
            INSTALL:if(config_ready) state<=FEED;
            FEED:if(raster_input_ready) begin
                if(corner==2) state<=DRAIN;
                else corner<=corner+1'b1;
            end
            DRAIN:if(raster_finished && config_ready) begin
                primitive_id<=primitive_id+1'b1;state<=IDLE;
            end
        endcase
    end
end
endmodule
