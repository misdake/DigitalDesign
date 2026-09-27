// Tags share the existing synchronous data lookup stage. Port A addresses way
// zero and port B way one; a refill writes only its selected port. Normal-mode
// writes hold that port's DO. No request resolves on a tag-install edge.
module CpuV3CacheTagBsram (
    input wire clk,
    input wire write_enable,
    input wire write_way,
    input wire [5:0] address,
    input wire [11:0] write_data,
    output wire [11:0] way_0_read_data,
    output wire [11:0] way_1_read_data
);
`ifdef __ICARUS__
`ifndef CPU_V3_CACHE_TAG_VENDOR
`define CPU_V3_CACHE_TAG_PORTABLE
`endif
`endif
`ifdef CPU_V3_CACHE_TAG_PORTABLE
    reg [11:0] tags_0 [0:63];
    reg [11:0] tags_1 [0:63];
    reg [11:0] qa=0,qb=0;
    integer initial_set;
    initial for(initial_set=0;initial_set<64;initial_set=initial_set+1) begin
        tags_0[initial_set]=0;
        tags_1[initial_set]=0;
    end
    assign way_0_read_data=qa;
    assign way_1_read_data=qb;
    always @(posedge clk) begin
        if(write_enable && !write_way) tags_0[address]<=write_data;
        else qa<=tags_0[address];
        if(write_enable && write_way) tags_1[address]<=write_data;
        else qb<=tags_1[address];
    end
`undef CPU_V3_CACHE_TAG_PORTABLE
`else
    wire [15:0] qa,qb;
    assign way_0_read_data=qa[11:0];
    assign way_1_read_data=qb[11:0];
    DPB #(.READ_MODE0(1'b0),.READ_MODE1(1'b0),.WRITE_MODE0(2'b00),.WRITE_MODE1(2'b00),
          .BIT_WIDTH_0(16),.BIT_WIDTH_1(16),.BLK_SEL_0(3'b0),.BLK_SEL_1(3'b0),.RESET_MODE("SYNC")) tags (
        .DOA(qa),.DOB(qb),.DIA({4'b0,write_data}),.DIB({4'b0,write_data}),
        .ADA({3'b0,1'b0,address,2'b0,2'b11}),.ADB({3'b0,1'b1,address,2'b0,2'b11}),
        .BLKSELA(3'b0),.BLKSELB(3'b0),.WREA(write_enable&&!write_way),.WREB(write_enable&&write_way),
        .CLKA(clk),.CLKB(clk),.CEA(1'b1),.CEB(1'b1),.OCEA(1'b0),.OCEB(1'b0),.RESETA(1'b0),.RESETB(1'b0));
`endif
endmodule
