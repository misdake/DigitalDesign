// Two pixels/clock, six-stage color-only perspective restoration. The input
// interpolator supplies N=(RGB/w), D=(1/w) in one common fixed-point scale.
// Each pair is independently tagged; held valid/data survive arbitrary stalls.
module GpuColorVarying(input wire clk,input wire reset,
    input wire input_valid,output wire input_ready,input wire [31:0] input_tag,
    input wire [1:0] input_mask,input wire [47:0] input_denominator,
    input wire [179:0] input_numerator,
    output wire output_valid,input wire output_ready,output wire [31:0] output_tag,
    output wire [1:0] output_mask,output wire [31:0] output_color,output wire output_error,output wire idle);
reg [5:0] valid;
wire enable=!valid[5] || output_ready;
assign idle=valid==0;
assign input_ready=enable;assign output_valid=valid[5];
reg [31:0] tags[0:5];reg [1:0] masks[0:5];reg [1:0] errors[0:5];
assign output_tag=tags[5];assign output_mask=masks[5];assign output_error=|errors[5];
reg [23:0] denominator0[0:1];
reg [29:0] numerator[0:3][0:5];reg [5:0] shift[1:4][0:1];
reg [31:0] table_word[0:1];reg [11:0] offset1[0:1];reg [18:0] base2[0:1];
reg signed [27:0] slope_product2[0:1];reg [17:0] magnitude3[0:1];
reg [47:0] product4[0:5];reg [15:0] color5[0:1];
assign output_color={color5[1],color5[0]};
function [4:0] clz24;
    input [23:0] value;integer bit_i;reg seen;
    begin clz24=0;seen=0;
        for(bit_i=23;bit_i>=0;bit_i=bit_i-1) begin
            if(!seen) begin if(value[bit_i]) seen=1;else clz24=clz24+1'b1;end
        end
    end
endfunction
function [5:0] channel;
    input [47:0] product;input [5:0] count;input green;
    reg [7:0] guarded;reg [8:0] rounded;reg [7:0] value;
    begin
        guarded=product>>(count-1'b1);rounded={1'b0,guarded}+9'd1;value=rounded>>1;
        if(green && value>63) channel=63;
        else if(!green && value>31) channel=31;
        else channel=value[5:0];
    end
endfunction
function [15:0] pixel;
    input [47:0] red,green,blue;input [5:0] count;
    reg [5:0] r,g,b;
    begin r=channel(red,count,0);g=channel(green,count,1);b=channel(blue,count,0);pixel={r[4:0],g,b[4:0]};end
endfunction
genvar lane;
generate for(lane=0;lane<2;lane=lane+1) begin: reciprocal_lane
    reg [31:0] table_memory[0:255];integer table_i,table_base,table_next;
    initial begin
        for(table_i=0;table_i<256;table_i=table_i+1) begin
            table_base=((1<<18)*256+(256+table_i)/2)/(256+table_i);
            table_next=((1<<18)*256+(257+table_i)/2)/(257+table_i);
            table_memory[table_i]=((table_next-table_base)&8191)*524288+table_base;
        end
    end
    wire [23:0] nonzero=(denominator0[lane]==0) ? 24'd1 : denominator0[lane];
    wire [4:0] leading=clz24(nonzero);wire [23:0] normalized=nonzero<<leading;
    wire signed [20:0] restored=$signed({2'd0,base2[lane]})+((slope_product2[lane]+28'sd2048) >>> 12);
    always @(posedge clk) if(enable) begin
        table_word[lane]<=table_memory[normalized[22:15]];
        offset1[lane]<=normalized[14:3];shift[1][lane]<=41-leading;
        slope_product2[lane]<=$signed(table_word[lane][31:19])*$signed({1'b0,offset1[lane]});
        base2[lane]<=table_word[lane][18:0];
        if(restored<131072) magnitude3[lane]<=131072;
        else if(restored>262143) magnitude3[lane]<=262143;
        else magnitude3[lane]<=restored[17:0];
    end
end endgenerate
integer i,j;
always @(posedge clk) begin
    if(reset) begin
        valid<=0;
        for(i=0;i<6;i=i+1) begin tags[i]<=0;masks[i]<=0;errors[i]<=0;product4[i]<=0;end
        for(i=0;i<2;i=i+1) begin denominator0[i]<=0;color5[i]<=0;end
        for(i=0;i<4;i=i+1) for(j=0;j<6;j=j+1) numerator[i][j]<=0;
        for(i=2;i<5;i=i+1) for(j=0;j<2;j=j+1) shift[i][j]<=0;
    end else if(enable) begin
        valid<={valid[4:0],input_valid};tags[0]<=input_tag;masks[0]<=input_mask;
        errors[0]<={(input_mask[1] && input_denominator[47:24]==0),(input_mask[0] && input_denominator[23:0]==0)};
        denominator0[0]<=input_denominator[23:0];denominator0[1]<=input_denominator[47:24];
        for(i=1;i<6;i=i+1) begin tags[i]<=tags[i-1];masks[i]<=masks[i-1];errors[i]<=errors[i-1];end
        for(i=0;i<6;i=i+1) begin
            numerator[0][i]<=input_numerator[i*30+:30];
            for(j=1;j<4;j=j+1) numerator[j][i]<=numerator[j-1][i];
            product4[i]<=numerator[3][i]*magnitude3[i/3];
        end
        for(i=2;i<5;i=i+1) for(j=0;j<2;j=j+1) shift[i][j]<=shift[i-1][j];
        for(i=0;i<2;i=i+1) begin
            if(masks[4][i] && !errors[4][i]) color5[i]<=pixel(product4[i*3],product4[i*3+1],product4[i*3+2],shift[4][i]);
            else color5[i]<=0;
        end
    end
end
endmodule
