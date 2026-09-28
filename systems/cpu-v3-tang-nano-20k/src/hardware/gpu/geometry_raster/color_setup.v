// Serial, pipelined setup for unnormalized perspective planes. Area cancels
// between N and D, so no per-triangle barycentric divider is needed.
module GpuColorSetup #(parameter EXTERNAL_MULTIPLIER=0)(input wire clk,input wire reset,input wire input_valid,
    output wire input_ready,input wire [95:0] input_xy,input wire [143:0] input_color,
    input wire [71:0] input_invw,output wire output_valid,input wire output_ready,
    output reg [707:0] output_planes,output reg [5:0] output_scale,
    output wire signed [35:0] external_multiply_a,output wire signed [35:0] external_multiply_b,
    input wire signed [71:0] external_multiply_product);
localparam IDLE=0,ATTR=1,FLUSH_ATTR=2,EDGE=3,FLUSH_EDGE=4,PLANES=5,FLUSH_PLANES=6,DONE=7,BOUND=8,FLUSH_BOUND=9;
reg [3:0] state;
reg [95:0] xy;reg [143:0] colors;reg [71:0] invw;
reg [5:0] issue,tag0,tag1,tag2;
reg [2:0] valid;
reg [29:0] attributes[0:11];
reg signed [33:0] edge_origin[0:2];
reg signed [20:0] edge_x[0:2],edge_y[0:2];
reg signed [66:0] accumulator;
reg [33:0] area;reg [23:0] largest_inverse;
integer highest;
reg signed [35:0] a,b;
wire signed [71:0] product;
assign external_multiply_a=a;
assign external_multiply_b=b;
generate if(EXTERNAL_MULTIPLIER) begin: external_multiplier
    assign product=external_multiply_product;
end else begin: internal_multiplier
    FrontendMultiply36 multiplier(clk,reset,a,b,product);
end endgenerate
wire signed [66:0] term=product[66:0];
reg signed [66:0] total;
reg first_term;
always @* begin
    first_term=0;
    case(tag2)
0,3,6,9,12,15,18,21,24,27,30,33:first_term=1;
    endcase
    if(first_term) total=term;else total=accumulator+term;
end
assign input_ready=state==IDLE;assign output_valid=state==DONE;
function signed [16:0] dx;
    input [1:0] i;reg signed [15:0] x0,x1;
    begin case(i) 0:begin x0=xy[15:0];x1=xy[47:32];end
        1:begin x0=xy[47:32];x1=xy[79:64];end
        default:begin x0=xy[79:64];x1=xy[15:0];end endcase
        dx={x1[15],x1}-{x0[15],x0};end
endfunction
function signed [16:0] dy;
    input [1:0] i;reg signed [15:0] y0,y1;
    begin case(i) 0:begin y0=xy[31:16];y1=xy[63:48];end
        1:begin y0=xy[63:48];y1=xy[95:80];end
        default:begin y0=xy[95:80];y1=xy[31:16];end endcase
        dy={y1[15],y1}-{y0[15],y0};end
endfunction
function signed [16:0] delta_center;
    input [1:0] i;input which_y;reg signed [15:0] coordinate;
    begin case(i) 0:if(which_y) coordinate=xy[31:16];else coordinate=xy[15:0];
        1:if(which_y) coordinate=xy[63:48];else coordinate=xy[47:32];
        default:if(which_y) coordinate=xy[95:80];else coordinate=xy[79:64];endcase
        delta_center=17'sd8-$signed({coordinate[15],coordinate});end
endfunction
reg signed [16:0] delta_a,delta_b;
always @* begin
    a=0;b=0;delta_a=0;delta_b=0;
    if(state==ATTR) begin case(issue)
0:begin a={12'd0,invw[0+:24]};b={20'd0,colors[0+:16]};end
1:begin a={12'd0,invw[0+:24]};b={20'd0,colors[16+:16]};end
2:begin a={12'd0,invw[0+:24]};b={20'd0,colors[32+:16]};end
3:begin a={12'd0,invw[24+:24]};b={20'd0,colors[48+:16]};end
4:begin a={12'd0,invw[24+:24]};b={20'd0,colors[64+:16]};end
5:begin a={12'd0,invw[24+:24]};b={20'd0,colors[80+:16]};end
6:begin a={12'd0,invw[48+:24]};b={20'd0,colors[96+:16]};end
7:begin a={12'd0,invw[48+:24]};b={20'd0,colors[112+:16]};end
8:begin a={12'd0,invw[48+:24]};b={20'd0,colors[128+:16]};end
    endcase end
    if(state==EDGE) begin case(issue)
0:begin delta_a=dx(0);delta_b=delta_center(0,1);end
1:begin delta_a=dy(0);delta_b=delta_center(0,0);end
2:begin delta_a=dx(1);delta_b=delta_center(1,1);end
3:begin delta_a=dy(1);delta_b=delta_center(1,0);end
4:begin delta_a=dx(2);delta_b=delta_center(2,1);end
5:begin delta_a=dy(2);delta_b=delta_center(2,0);end
    endcase a={{19{delta_a[16]}},delta_a};b={{19{delta_b[16]}},delta_b};end
    if(state==BOUND) begin a={2'd0,area};b={12'd0,largest_inverse};end
    if(state==PLANES) begin case(issue)
0:begin a={{2{edge_origin[1][33]}},edge_origin[1]};b={6'd0,attributes[0]};end
1:begin a={{2{edge_origin[2][33]}},edge_origin[2]};b={6'd0,attributes[4]};end
2:begin a={{2{edge_origin[0][33]}},edge_origin[0]};b={6'd0,attributes[8]};end
3:begin a={{15{edge_x[1][20]}},edge_x[1]};b={6'd0,attributes[0]};end
4:begin a={{15{edge_x[2][20]}},edge_x[2]};b={6'd0,attributes[4]};end
5:begin a={{15{edge_x[0][20]}},edge_x[0]};b={6'd0,attributes[8]};end
6:begin a={{15{edge_y[1][20]}},edge_y[1]};b={6'd0,attributes[0]};end
7:begin a={{15{edge_y[2][20]}},edge_y[2]};b={6'd0,attributes[4]};end
8:begin a={{15{edge_y[0][20]}},edge_y[0]};b={6'd0,attributes[8]};end
9:begin a={{2{edge_origin[1][33]}},edge_origin[1]};b={6'd0,attributes[1]};end
10:begin a={{2{edge_origin[2][33]}},edge_origin[2]};b={6'd0,attributes[5]};end
11:begin a={{2{edge_origin[0][33]}},edge_origin[0]};b={6'd0,attributes[9]};end
12:begin a={{15{edge_x[1][20]}},edge_x[1]};b={6'd0,attributes[1]};end
13:begin a={{15{edge_x[2][20]}},edge_x[2]};b={6'd0,attributes[5]};end
14:begin a={{15{edge_x[0][20]}},edge_x[0]};b={6'd0,attributes[9]};end
15:begin a={{15{edge_y[1][20]}},edge_y[1]};b={6'd0,attributes[1]};end
16:begin a={{15{edge_y[2][20]}},edge_y[2]};b={6'd0,attributes[5]};end
17:begin a={{15{edge_y[0][20]}},edge_y[0]};b={6'd0,attributes[9]};end
18:begin a={{2{edge_origin[1][33]}},edge_origin[1]};b={6'd0,attributes[2]};end
19:begin a={{2{edge_origin[2][33]}},edge_origin[2]};b={6'd0,attributes[6]};end
20:begin a={{2{edge_origin[0][33]}},edge_origin[0]};b={6'd0,attributes[10]};end
21:begin a={{15{edge_x[1][20]}},edge_x[1]};b={6'd0,attributes[2]};end
22:begin a={{15{edge_x[2][20]}},edge_x[2]};b={6'd0,attributes[6]};end
23:begin a={{15{edge_x[0][20]}},edge_x[0]};b={6'd0,attributes[10]};end
24:begin a={{15{edge_y[1][20]}},edge_y[1]};b={6'd0,attributes[2]};end
25:begin a={{15{edge_y[2][20]}},edge_y[2]};b={6'd0,attributes[6]};end
26:begin a={{15{edge_y[0][20]}},edge_y[0]};b={6'd0,attributes[10]};end
27:begin a={{2{edge_origin[1][33]}},edge_origin[1]};b={6'd0,attributes[3]};end
28:begin a={{2{edge_origin[2][33]}},edge_origin[2]};b={6'd0,attributes[7]};end
29:begin a={{2{edge_origin[0][33]}},edge_origin[0]};b={6'd0,attributes[11]};end
30:begin a={{15{edge_x[1][20]}},edge_x[1]};b={6'd0,attributes[3]};end
31:begin a={{15{edge_x[2][20]}},edge_x[2]};b={6'd0,attributes[7]};end
32:begin a={{15{edge_x[0][20]}},edge_x[0]};b={6'd0,attributes[11]};end
33:begin a={{15{edge_y[1][20]}},edge_y[1]};b={6'd0,attributes[3]};end
34:begin a={{15{edge_y[2][20]}},edge_y[2]};b={6'd0,attributes[7]};end
35:begin a={{15{edge_y[0][20]}},edge_y[0]};b={6'd0,attributes[11]};end
    endcase end
end
integer i;
always @(posedge clk) begin
    if(reset) begin state<=IDLE;valid<=0;issue<=0;tag0<=0;tag1<=0;tag2<=0;accumulator<=0;end
    else begin
        valid<={valid[1:0],state==ATTR || state==EDGE || state==PLANES || state==BOUND};
        tag0<=issue;tag1<=tag0;tag2<=tag1;
        case(state)
            IDLE:if(input_valid) begin xy<=input_xy;colors<=input_color;invw<=input_invw;
                attributes[0]<={6'd0,input_invw[23:0]};attributes[4]<={6'd0,input_invw[47:24]};attributes[8]<={6'd0,input_invw[71:48]};
                issue<=0;state<=ATTR;
            end
            ATTR:begin issue<=issue+1'b1;if(issue==8) state<=FLUSH_ATTR;end
            EDGE:begin issue<=issue+1'b1;if(issue==5) state<=FLUSH_EDGE;end
            BOUND:state<=FLUSH_BOUND;
            PLANES:begin issue<=issue+1'b1;if(issue==35) state<=FLUSH_PLANES;end
            DONE:if(output_ready) state<=IDLE;
        endcase
        if(valid[2]) begin
            if(state==ATTR || state==FLUSH_ATTR) begin case(tag2)
0:attributes[1]<=(product+72'd128)>>8;
1:attributes[2]<=(product+72'd128)>>8;
2:attributes[3]<=(product+72'd128)>>8;
3:attributes[5]<=(product+72'd128)>>8;
4:attributes[6]<=(product+72'd128)>>8;
5:attributes[7]<=(product+72'd128)>>8;
6:attributes[9]<=(product+72'd128)>>8;
7:attributes[10]<=(product+72'd128)>>8;
8:attributes[11]<=(product+72'd128)>>8;
                endcase
                if(tag2==8) begin issue<=0;state<=EDGE;
                    for(i=0;i<3;i=i+1) begin edge_x[i]<=-$signed(dy(i))<<<4;edge_y[i]<=$signed(dx(i))<<<4;end
                end
            end else if(state==EDGE || state==FLUSH_EDGE) begin
                if(!tag2[0]) accumulator<=term;
                else begin case(tag2)
                    1:edge_origin[0]<=accumulator-term;
                    3:edge_origin[1]<=accumulator-term;
                    5:edge_origin[2]<=accumulator-term;
                endcase end
                if(tag2==5) begin
                    area<=edge_origin[0]+edge_origin[1]+accumulator-term;
                    largest_inverse<=attributes[0][23:0];
                    if(attributes[4]>attributes[0] && attributes[4]>=attributes[8]) largest_inverse<=attributes[4][23:0];
                    else if(attributes[8]>attributes[0]) largest_inverse<=attributes[8][23:0];
                    issue<=0;state<=BOUND;end
            end else if(state==FLUSH_BOUND) begin
                output_scale<=0;
                for(highest=24;highest<67;highest=highest+1) if(product[highest]) output_scale<=highest-23;
                issue<=0;state<=PLANES;
            end else if(state==PLANES || state==FLUSH_PLANES) begin
                accumulator<=total;
                case(tag2)
2:output_planes[0 +:67]<=total[66:0];
5:output_planes[67 +:55]<=total[54:0];
8:output_planes[122 +:55]<=total[54:0];
11:output_planes[177 +:67]<=total[66:0];
14:output_planes[244 +:55]<=total[54:0];
17:output_planes[299 +:55]<=total[54:0];
20:output_planes[354 +:67]<=total[66:0];
23:output_planes[421 +:55]<=total[54:0];
26:output_planes[476 +:55]<=total[54:0];
29:output_planes[531 +:67]<=total[66:0];
32:output_planes[598 +:55]<=total[54:0];
35:output_planes[653 +:55]<=total[54:0];
                endcase
                if(tag2==35) state<=DONE;
            end
        end
    end
end
endmodule
