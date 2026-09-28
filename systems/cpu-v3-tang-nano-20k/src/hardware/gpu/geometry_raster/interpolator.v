// Common-scale Q12 planes for normal triangles; a serial full-precision
// fallback preserves extreme thin triangles. Two four-lane adders are reused
// for jump walking, adjacent-quad/row updates, and the two pixel columns.
module GpuColorInterpolator(input wire clk,input wire reset,
    input wire config_valid,output wire config_ready,input wire [707:0] config_planes,input wire [5:0] config_scale,
    input wire input_valid,output wire input_ready,input wire [30:0] input_work,input wire [3:0] input_mask,
    output wire output_valid,input wire output_ready,output wire [31:0] output_tag,
    output reg [1:0] output_mask,output reg [47:0] output_denominator,output reg [179:0] output_numerator,output wire idle);
localparam IDLE=0,EVAL=1,PAIR0=2,PAIR1=3,NORMAL=4,RELOAD_WAIT=5,RELOAD_READ=6,FALLBACK=7;
reg [2:0] state;
reg configured,anchor_valid,slow;
reg [707:0] planes;
reg [66:0] position[0:3],row_origin[0:3],row1[0:3],shifted_gradient[0:3];
reg [30:0] work;reg [3:0] mask;reg [8:0] x,anchor_x;reg [7:0] y;
reg [4:0] bit_index;reg y_phase;reg [8:0] coordinate;
reg [3:0] normalize_index;reg [5:0] common_scale;
reg [2:0] fallback_index;reg fallback_row;
reg [47:0] fallback_denominator;reg [179:0] fallback_numerator;
wire marker=input_work[28:27]!=0;
assign idle=state==IDLE;
assign config_ready=state==IDLE && !configured;
assign input_ready=(state==IDLE || (state==PAIR1 && output_ready && work[28:27]!=3)) && (marker || configured);
assign output_valid=state==PAIR0 || state==PAIR1;
assign output_tag={state==PAIR1,work};
wire fast_x=anchor_valid && input_work[16:9]==y && input_work[8:0]==x+9'd2;
wire fast_y=anchor_valid && input_work[16:9]==y+8'd2 && input_work[8:0]==anchor_x;
wire accept=input_valid && input_ready;
wire signed [54:0] gx[0:3],gy[0:3];wire signed [66:0] origin[0:3];
genvar field;
generate for(field=0;field<4;field=field+1) begin: parameter_field
    assign origin[field]=planes[field*177+:67];
    assign gx[field]=planes[field*177+67+:55];assign gy[field]=planes[field*177+122+:55];
end endgenerate
reg signed [66:0] selected_coefficient;
always @* begin case(normalize_index)
0:selected_coefficient=planes[0+:67];
1:selected_coefficient={{12{planes[121]}},planes[67+:55]};
2:selected_coefficient={{12{planes[176]}},planes[122+:55]};
3:selected_coefficient=planes[177+:67];
4:selected_coefficient={{12{planes[298]}},planes[244+:55]};
5:selected_coefficient={{12{planes[353]}},planes[299+:55]};
6:selected_coefficient=planes[354+:67];
7:selected_coefficient={{12{planes[475]}},planes[421+:55]};
8:selected_coefficient={{12{planes[530]}},planes[476+:55]};
9:selected_coefficient=planes[531+:67];
10:selected_coefficient={{12{planes[652]}},planes[598+:55]};
11:selected_coefficient={{12{planes[707]}},planes[653+:55]};
    default:selected_coefficient=0;
endcase end
wire [71:0] cached_coefficient;
FrontendResultStore parameter_cache(clk,{5'd0,normalize_index},cached_coefficient,state==NORMAL,
    {5'd0,normalize_index},{5'd0,selected_coefficient});
reg signed [66:0] sample0[0:3],sample1[0:3],updated[0:3];
reg signed [66:0] base,delta,barrel_input,barrel_result;
reg [66:0] maximum;reg [6:0] pair_scale;reg found;
integer i,k;
wire row_select=(state==PAIR1)|| (state==FALLBACK && fallback_row);
always @* begin
    output_mask=mask[1:0];if(row_select) output_mask=mask[3:2];
    maximum=0;pair_scale=0;found=0;
    for(i=0;i<4;i=i+1) begin
        sample0[i]=$signed(position[i]);if(row_select) sample0[i]=$signed(row1[i]);
        sample1[i]=sample0[i]+$signed(gx[i]);
        base=$signed(position[i]);delta=0;
        if(state==EVAL && coordinate[0]) delta=$signed(shifted_gradient[i]);
        else if(state==PAIR0) delta=$signed(gy[i]);
        else if(accept && !marker) begin
            if(fast_y) begin base=$signed(row_origin[i]);delta=$signed(gy[i])<<<1;end
            else if(fast_x) delta=$signed(gx[i])<<<1;
        end
        updated[i]=base+delta;
    end
    if(output_mask[0] && !sample0[0][66]) maximum=sample0[0];
    if(output_mask[1] && !sample1[0][66] && $unsigned(sample1[0])>maximum) maximum=sample1[0];
    for(k=66;k>=24;k=k-1) begin if(!found && maximum[k]) begin pair_scale=k-23;found=1;end end
    barrel_input=0;
    case(fallback_index)
        0:barrel_input=sample0[0];1:barrel_input=sample0[1];2:barrel_input=sample0[2];3:barrel_input=sample0[3];
        4:barrel_input=sample1[0];5:barrel_input=sample1[1];6:barrel_input=sample1[2];7:barrel_input=sample1[3];
    endcase
    if(state==NORMAL) begin
        barrel_input=selected_coefficient;
        if(common_scale>=12) barrel_result=barrel_input >>> (common_scale-12);
        else barrel_result=barrel_input <<< (12-common_scale);
    end else barrel_result=barrel_input >>> pair_scale;
    output_denominator=0;output_numerator=0;
    if(work[28:27]!=0) begin output_mask=0;output_denominator={24'd1,24'd1};end
    else if(slow) begin output_denominator=fallback_denominator;output_numerator=fallback_numerator;end
    else begin
        if(output_mask[0]) begin
            if(sample0[0]>0) output_denominator[23:0]=(sample0[0]+67'd2048)>>12;
            for(i=1;i<4;i=i+1) output_numerator[(i-1)*30+:30]=(sample0[i]+67'd2048)>>12;
        end
        if(output_mask[1]) begin
            if(sample1[0]>0) output_denominator[47:24]=(sample1[0]+67'd2048)>>12;
            for(i=1;i<4;i=i+1) output_numerator[(i+2)*30+:30]=(sample1[i]+67'd2048)>>12;
        end
    end
end
wire normalized_overflow=barrel_result[66:51]!={16{barrel_result[51]}};
integer j;
task store_coefficient;
    input signed [66:0] value;
    begin case(normalize_index)
0:planes[0+:67]<=value[66:0];
1:planes[67+:55]<=value[54:0];
2:planes[122+:55]<=value[54:0];
3:planes[177+:67]<=value[66:0];
4:planes[244+:55]<=value[54:0];
5:planes[299+:55]<=value[54:0];
6:planes[354+:67]<=value[66:0];
7:planes[421+:55]<=value[54:0];
8:planes[476+:55]<=value[54:0];
9:planes[531+:67]<=value[66:0];
10:planes[598+:55]<=value[54:0];
11:planes[653+:55]<=value[54:0];
    endcase end
endtask
always @(posedge clk) begin
    if(reset) begin state<=IDLE;configured<=0;anchor_valid<=0;slow<=0;work<=0;mask<=0;
        x<=0;y<=0;anchor_x<=0;bit_index<=0;y_phase<=0;coordinate<=0;
        normalize_index<=0;common_scale<=0;fallback_index<=0;fallback_row<=0;fallback_denominator<=0;fallback_numerator<=0;
    end else begin
        if(config_valid && config_ready) begin planes<=config_planes;common_scale<=config_scale;configured<=0;
            anchor_valid<=0;slow<=0;normalize_index<=0;state<=NORMAL;end
        case(state)
            NORMAL:begin
                store_coefficient({{15{barrel_result[51]}},barrel_result[51:0]});
                if(normalized_overflow) slow<=1;
                normalize_index<=normalize_index+1'b1;
                if(normalize_index==11) begin
                    normalize_index<=0;
                    if(slow || normalized_overflow) state<=RELOAD_WAIT;
                    else begin configured<=1;state<=IDLE;end
                end
            end
            RELOAD_WAIT:state<=RELOAD_READ;
            RELOAD_READ:begin
                store_coefficient(cached_coefficient[66:0]);
                if(normalize_index==11) begin configured<=1;state<=IDLE;end
                else begin normalize_index<=normalize_index+1'b1;state<=RELOAD_WAIT;end
            end
            EVAL:begin
                for(j=0;j<4;j=j+1) begin
                    position[j]<=updated[j];shifted_gradient[j]<=shifted_gradient[j]<<1;
                    if(y_phase && bit_index==7) row_origin[j]<=updated[j];
                end
                coordinate<=coordinate>>1;bit_index<=bit_index+1'b1;
                if(!y_phase && bit_index==8) begin
                    y_phase<=1;bit_index<=0;coordinate<={1'b0,y};
                    for(j=0;j<4;j=j+1) shifted_gradient[j]<={{12{gy[j][54]}},gy[j]};
                end else if(y_phase && bit_index==7) begin
                    if(slow) begin state<=FALLBACK;fallback_row<=0;fallback_index<=0;fallback_denominator<=0;fallback_numerator<=0;end
                    else state<=PAIR0;
                    anchor_valid<=1;anchor_x<=x;
                end
            end
            FALLBACK:begin
                case(fallback_index)
                    0:if(output_mask[0] && sample0[0]>0) fallback_denominator[23:0]<=barrel_result[23:0];
                    1:if(output_mask[0]) fallback_numerator[29:0]<=barrel_result[29:0];
                    2:if(output_mask[0]) fallback_numerator[59:30]<=barrel_result[29:0];
                    3:if(output_mask[0]) fallback_numerator[89:60]<=barrel_result[29:0];
                    4:if(output_mask[1] && sample1[0]>0) fallback_denominator[47:24]<=barrel_result[23:0];
                    5:if(output_mask[1]) fallback_numerator[119:90]<=barrel_result[29:0];
                    6:if(output_mask[1]) fallback_numerator[149:120]<=barrel_result[29:0];
                    7:if(output_mask[1]) fallback_numerator[179:150]<=barrel_result[29:0];
                endcase
                fallback_index<=fallback_index+1'b1;
                if(fallback_index==7) begin if(fallback_row) state<=PAIR1;else state<=PAIR0;end
            end
            PAIR0:if(output_ready) begin
                for(j=0;j<4;j=j+1) row1[j]<=updated[j];
                if(slow && work[28:27]==0) begin state<=FALLBACK;fallback_row<=1;fallback_index<=0;fallback_denominator<=0;fallback_numerator<=0;end
                else state<=PAIR1;
            end
            PAIR1:if(output_ready) begin state<=IDLE;if(work[28:27]==3) begin configured<=0;anchor_valid<=0;end end
        endcase
        if(accept) begin
            work<=input_work;mask<=input_mask;
            if(marker) state<=PAIR0;
            else begin
                x<=input_work[8:0];y<=input_work[16:9];
                if(fast_x || fast_y) begin
                    for(j=0;j<4;j=j+1) begin position[j]<=updated[j];if(fast_y) row_origin[j]<=updated[j];end
                    if(slow) begin state<=FALLBACK;fallback_row<=0;fallback_index<=0;fallback_denominator<=0;fallback_numerator<=0;end
                    else state<=PAIR0;
                end else begin
                    for(j=0;j<4;j=j+1) begin position[j]<=origin[j];shifted_gradient[j]<={{12{gx[j][54]}},gx[j]};end
                    y_phase<=0;bit_index<=0;coordinate<=input_work[8:0];state<=EVAL;
                end
            end
        end
    end
end
endmodule
