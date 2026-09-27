`timescale 1ns/1ps
// Eight true dual-port 1024x16 DPBs: C0..C3 and Z0..Z3, not mirrored.
// A belongs to memory, B to render. Group is only a compatibility address bit;
// there is no checkerboard port grouping or group-wide data selection.
// bank=(x+2*y)%4 permits both aligned quads and horizontal memory beats.
module FramebufferLaneArray #(parameter SLOT_BITS=2,parameter HALF_CAPACITY=1) (
    input wire clk,input wire reset,
    input wire memory_write_valid,output wire memory_write_ready,
    input wire memory_write_group,input wire [9:0] memory_write_address,
    input wire [63:0] memory_write_data,input wire [7:0] memory_write_mask,
    input wire memory_read_valid,output wire memory_read_ready,
    input wire memory_read_group,input wire [9:0] memory_read_address,
    output reg memory_response_valid=0,input wire memory_response_ready,output wire [63:0] memory_response_data,
    input wire render_write_valid,output wire render_write_ready,
    input wire render_write_group,input wire [SLOT_BITS-1:0] render_write_way,
    input wire [3:0] render_write_x,input wire [3:0] render_write_y,
    input wire [63:0] render_write_colors,input wire [63:0] render_write_depths,
    input wire [3:0] render_write_color_mask,input wire [3:0] render_write_depth_mask,
    input wire render_read_valid,output wire render_read_ready,
    input wire render_read_group,input wire [SLOT_BITS-1:0] render_read_way,
    input wire [3:0] render_read_x,input wire [3:0] render_read_y,
    output reg render_response_valid=0,input wire render_response_ready,
    output wire [63:0] render_response_colors,output wire [63:0] render_response_depths
);
    localparam PLANE_BIT=9-SLOT_BITS-HALF_CAPACITY;
    localparam MEMORY_BITS=10-HALF_CAPACITY;
    wire [9:0] mwa={memory_write_group,memory_write_address[MEMORY_BITS-1:PLANE_BIT+1],memory_write_address[PLANE_BIT-1:0]};
    wire [9:0] mra={memory_read_group,memory_read_address[MEMORY_BITS-1:PLANE_BIT+1],memory_read_address[PLANE_BIT-1:0]};
    wire wp=memory_write_address[PLANE_BIT],rp=memory_read_address[PLANE_BIT];
    wire [9:0] wt={render_write_group,render_write_way,render_write_y[6-SLOT_BITS-HALF_CAPACITY:0],render_write_x[3:2]};
    wire [9:0] rt={render_read_group,render_read_way,render_read_y[6-SLOT_BITS-HALF_CAPACITY:0],render_read_x[3:2]};
    reg mrow=0,mplane=0,rswap=0;
    wire mw=memory_write_valid && memory_write_ready;
    wire mr=memory_read_valid && memory_read_ready;
    wire rw=render_write_valid && render_write_ready;
    wire rr=render_read_valid && render_read_ready;
    wire mh=memory_response_valid && !memory_response_ready;
    wire rh=render_response_valid && !render_response_ready;
    wire same_read_write_quad=mra[9:3]==wt[9:3] && mra[1:0]==wt[1:0];
    wire same_write_write_quad=mwa[9:3]==wt[9:3] && mwa[1:0]==wt[1:0];
    wire same_write_read_quad=mwa[9:3]==rt[9:3] && mwa[1:0]==rt[1:0];
    // Each plane's A port performs one operation; different planes may read
    // and write together. B can read or write all eight banks each clock.
    assign memory_write_ready=!reset;
    assign memory_read_ready=!reset && !mh && !(mw && memory_write_mask!=0 && wp==rp);
    assign render_write_ready=!reset && !render_write_x[0] && !render_write_y[0]
        && !(mw && memory_write_mask!=0 && same_write_write_quad
             && (wp ? render_write_depth_mask!=0 : render_write_color_mask!=0))
        && !(mr && same_read_write_quad && (rp ? render_write_depth_mask!=0 : render_write_color_mask!=0));
    assign render_read_ready=!reset && !render_read_x[0] && !render_read_y[0] && !rh
        && !(rw && (render_write_color_mask!=0 || render_write_depth_mask!=0))
        && !(mw && memory_write_mask!=0 && same_write_read_quad);
    wire [63:0] memory_data [0:1],render_data [0:1];
    wire [63:0] mraw=mplane ? memory_data[1] : memory_data[0];
    assign memory_response_data=mrow ? {mraw[31:0],mraw[63:32]} : mraw;
    assign render_response_colors=rswap ? {render_data[0][31:0],render_data[0][63:32]} : render_data[0];
    assign render_response_depths=rswap ? {render_data[1][31:0],render_data[1][63:32]} : render_data[1];
    wire [63:0] mwd=memory_write_address[2] ? {memory_write_data[31:0],memory_write_data[63:32]} : memory_write_data;
    wire [7:0] mwm=memory_write_address[2] ? {memory_write_mask[3:0],memory_write_mask[7:4]} : memory_write_mask;
    wire [63:0] rwd [0:1];wire [3:0] rwm [0:1];
    assign rwd[0]=render_write_x[1] ? {render_write_colors[31:0],render_write_colors[63:32]} : render_write_colors;
    assign rwd[1]=render_write_x[1] ? {render_write_depths[31:0],render_write_depths[63:32]} : render_write_depths;
    assign rwm[0]=render_write_x[1] ? {render_write_color_mask[1:0],render_write_color_mask[3:2]} : render_write_color_mask;
    assign rwm[1]=render_write_x[1] ? {render_write_depth_mask[1:0],render_write_depth_mask[3:2]} : render_write_depth_mask;
    always @(posedge clk) begin
        if(reset) begin memory_response_valid<=0;render_response_valid<=0;end
        else begin
            if(memory_response_ready) memory_response_valid<=0;
            if(render_response_ready) render_response_valid<=0;
            if(mr) begin memory_response_valid<=1;mrow<=memory_read_address[2];mplane<=rp;end
            if(rr) begin render_response_valid<=1;rswap<=render_read_x[1];end
        end
    end
    genvar plane,bank;
    generate for(plane=0;plane<2;plane=plane+1) begin: planes
        for(bank=0;bank<4;bank=bank+1) begin: banks
            wire mw_here=mw && wp==plane && memory_write_mask!=0;
            wire mr_here=mr && rp==plane;
            wire rw_here=rw && rwm[plane][bank];
            wire [9:0] aa=mw_here ? mwa : mra;
            wire [9:0] read_b={rt[9:3],render_read_x[1]!=(bank/2),rt[1:0]};
            wire [9:0] write_b={wt[9:3],render_write_x[1]!=(bank/2),wt[1:0]};
            wire [9:0] ab=rw_here ? write_b : read_b;
            DPB #(.READ_MODE0(1'b0),.READ_MODE1(1'b0),.WRITE_MODE0(2'b00),.WRITE_MODE1(2'b00),
                  .BIT_WIDTH_0(16),.BIT_WIDTH_1(16),.BLK_SEL_0(3'b0),.BLK_SEL_1(3'b0),.RESET_MODE("SYNC")) ram (
                .DOA(memory_data[plane][16*bank +:16]),.DOB(render_data[plane][16*bank +:16]),
                .DIA(mwd[16*bank +:16]),.DIB(rwd[plane][16*bank +:16]),
                .ADA({aa,2'b0,mwm[2*bank +:2]}),.ADB({ab,2'b0,2'b11}),
                .BLKSELA(3'b0),.BLKSELB(3'b0),.WREA(mw_here),.WREB(rw_here),
                .CLKA(clk),.CLKB(clk),.CEA(mw_here||mr_here),.CEB(rr||rw_here),
                .OCEA(1'b0),.OCEB(1'b0),.RESETA(1'b0),.RESETB(1'b0));
        end
    end endgenerate
endmodule
