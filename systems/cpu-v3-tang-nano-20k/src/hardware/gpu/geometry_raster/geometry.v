// Serial store-side clip/projection. One shared registered 36x36 DSP,
// a synchronous reciprocal ROM, and two 512x36 scratch banks. Color is
// native RGB565 component code with eight fractional bits during clipping.
module GpuColorGeometry #(parameter EXTERNAL_MULTIPLIER=0)(input wire clk,input wire reset,
    input wire input_valid,output wire input_ready,input wire [207:0] input_vertex,
    input wire input_source_last,
    output wire output_valid,input wire output_ready,output wire output_end,
    output wire output_source_last,output wire [31:0] output_xy,
    output wire [47:0] output_color,output wire [31:0] output_w,output wire [23:0] output_invw,
    output reg error,
    output wire signed [35:0] external_multiply_a,output wire signed [35:0] external_multiply_b,
    input wire signed [71:0] external_multiply_product);
function signed [39:0] clip_dist;
    input [3:0] plane;input [31:0] vx,vy,vz,vw;
    reg signed [39:0] coordinate,scaled,left,right;
    reg signed [39:0] signed_w,signed_z;
    begin
        signed_w={{8{vw[31]}},vw};signed_z={{8{vz[31]}},vz};
        case(plane)
            1:coordinate=signed_z;
            4,5,8,9:coordinate={{8{vy[31]}},vy};
            default:coordinate={{8{vx[31]}},vx};
        endcase
        if(plane>=6) begin scaled=(coordinate<<<4)+(coordinate<<<3)+coordinate;left=signed_w<<<6;end
        else begin scaled=coordinate;left=signed_w;end
        if(plane==0) begin left=signed_z;scaled=0;end
        if(plane[0]) right=~scaled;else right=scaled;
        clip_dist=left+right+$signed({39'd0,plane[0]});
    end
endfunction

// The six planes that truly clip, in fixed order.
function [3:0] clip_plane;
    input [2:0] pos;
    begin
        case (pos)
            3'd0: clip_plane = 4'd0;
            3'd1: clip_plane = 4'd1;
            3'd2: clip_plane = 4'd6;
            3'd3: clip_plane = 4'd7;
            3'd4: clip_plane = 4'd8;
            default: clip_plane = 4'd9;
        endcase
    end
endfunction

// 10-bit outcode over all planes.


// Leading-zero count of a nonzero 32-bit word.
function [5:0] clz32;
    input [31:0] v;
    integer k;
    reg found;
    begin
        clz32 = 6'd32;
        found = 1'b0;
        for (k = 31; k >= 0; k = k - 1) begin
            if (!found && v[k]) begin
                clz32 = 31 - k[5:0];
                found = 1'b1;
            end
        end
    end
endfunction

// Reciprocal of a positive Q16.16 (raw u32): {shift[5:0], mag[17:0]} such
// that 1/w ~ mag * 2^(16-shift); LUT + slope lerp, one small multiply.



localparam LOAD=0,WRITE=1,PREV_ADDR=2,READ_WAIT=3,READ=4,
    PREV_DIST=5,CUR_ADDR=6,STEP=7,DIV=8,LERP_ISSUE=9,LERP_WAIT=10,LERP_CAPTURE=11,
    NEXT=12,PROJECT_ADDR=13,PROJECT_RCP=14,PROJECT_ISSUE=15,PROJECT_WAIT=16,
    PROJECT_CAPTURE=17,PROJECT_VIEW=18,EMIT=19,END_SOURCE=20,RCP_ISSUE=21,RCP_WAIT=22,RCP_CAPTURE=23,CLASSIFY=24,SELECT_PLANE=25;
reg [4:0] state;
reg bank,source_last;
reg [3:0] length,next_length,index,fan;
reg [2:0] plane;
// Quantized lerp remains a convex combination plus <= 0.5 raw unit per
// coordinate. A guard distance has coefficient norm 64+25=89, so each
// preceding plane can reduce an originally positive distance by <=44.5.
// There are at most five preceding planes: 224 raw units safely excludes
// crossings introduced by rounding. Far/near need less, but share this test.
reg [9:0] pending_planes;
wire [5:0] selected_planes={pending_planes[9:6],pending_planes[1:0]};
reg [3:0] classify_plane;
reg [9:0] classify_code,code_or,code_and;
reg [1:0] row,corner,emit_corner;
reg [6:0] write_vertex;
reg [4:0] write_return,read_return;
reg [175:0] previous,current,intersection;
reg [1:0] write_selection;
reg [175:0] record;
always @* begin
 case(write_selection) 0:record=current;1:record=previous;default:record=intersection;endcase
end
reg signed [39:0] previous_distance;
reg outside_previous,append_current;
reg [40:0] denominator,remainder;
reg [31:0] quotient;
reg [5:0] bits;
reg [2:0] component;
reg [2:0] delay;
wire signed [71:0] product;
// One synchronous base/slope table. The packed 32-bit word maps to pROM.
reg [31:0] reciprocal_table[0:255];
integer table_i;
integer table_base,table_next;
initial begin
    for(table_i=0;table_i<256;table_i=table_i+1) begin
        table_base=((1<<18)*256+(256+table_i)/2)/(256+table_i);
        table_next=((1<<18)*256+(257+table_i)/2)/(257+table_i);
        reciprocal_table[table_i]=((table_next-table_base)&8191)*524288+table_base;
    end
end
wire [31:0] reciprocal_w=(current[127:96]<8192) ? 32'd8192 : current[127:96];
wire [5:0] reciprocal_lz=clz32(reciprocal_w);
wire [31:0] normalized_w=reciprocal_w<<reciprocal_lz;
reg [31:0] reciprocal_word;
always @(posedge clk) reciprocal_word<=reciprocal_table[normalized_w[30:23]];
reg [18:0] reciprocal_base;
reg [11:0] reciprocal_offset;
wire signed [71:0] reciprocal_rounded=product+72'sd2048;
wire signed [20:0] reciprocal_value=$signed({2'd0,reciprocal_base})+(reciprocal_rounded >>> 12);
reg [23:0] reciprocal;
reg signed [17:0] ndcx,ndcy;
reg [135:0] projected[0:2];
reg invalid;
reg signed [35:0] multiply_a,multiply_b;

assign external_multiply_a=multiply_a;
assign external_multiply_b=multiply_b;
generate if(EXTERNAL_MULTIPLIER) begin: external_multiplier
    assign product=external_multiply_product;
end else begin: internal_multiplier
    FrontendMultiply36 multiplier(clk,reset,multiply_a,multiply_b,product);
end endgenerate
reg [8:0] read_address;
wire [71:0] read_data;
wire write_enable=state==WRITE;
wire [8:0] write_address={2'b00,write_vertex}+row;
reg [71:0] write_data;
always @* begin
    case(row)
        0:write_data=record[71:0];
        1:write_data=record[143:72];
        default:write_data={40'd0,record[175:144]};
    endcase
end
FrontendResultStore scratch(clk,read_address,read_data,write_enable,write_address,write_data);
function [6:0] vertex_address;
    input which;input [3:0] number;
    begin vertex_address={which,5'd0}+({3'd0,number}<<1)+number;end
endfunction
function signed [31:0] scalar;
    input [175:0] v;input [2:0] part;
    begin case(part)
        0:scalar=v[31:0];1:scalar=v[63:32];2:scalar=v[95:64];3:scalar=v[127:96];
        4:scalar={16'd0,v[143:128]};5:scalar={16'd0,v[159:144]};
        default:scalar={16'd0,v[175:160]};
    endcase end
endfunction
wire [3:0] distance_plane=(state==CLASSIFY) ? classify_plane : clip_plane(plane);
wire signed [39:0] distance=clip_dist(distance_plane,current[31:0],current[63:32],current[95:64],current[127:96]);
wire [9:0] complete_code=classify_code | (distance[39] ? (10'b1<<classify_plane) : 10'd0);
wire [9:0] complete_or=code_or | complete_code;
wire [9:0] complete_and=code_and & complete_code;
wire [41:0] doubled={remainder,1'b0};
wire subtract=doubled>={1'b0,denominator};
wire [41:0] reduced=doubled-{1'b0,denominator};
wire signed [31:0] outside_scalar=outside_previous ? scalar(previous,component) : scalar(current,component);
wire signed [31:0] inside_scalar=outside_previous ? scalar(current,component) : scalar(previous,component);
wire signed [32:0] difference={inside_scalar[31],inside_scalar}-{outside_scalar[31],outside_scalar};
wire signed [32:0] lerp_step=$signed(product[64:32])+$signed({32'd0,product[31]});
wire signed [32:0] lerp_value={outside_scalar[31],outside_scalar}+lerp_step;
reg [35:0] ndc_scaled;
reg signed [31:0] screen_x,screen_y;
reg signed [17:0] high_x,high_y;
always @* begin
    // Clamped raw W is >= 2^13, so shift=p+18 is always 31..49.
    // Only F29 NDC bits [31:14] reach the viewport. After the mandatory
    // two-bit right shift, this 36-bit window supplies every required bit
    // for the remaining 0..18 shifts, including negative coordinates.
    ndc_scaled=product[51:16] >> (reciprocal[23:18]-31);
    high_x=ndcx;high_y=ndcy;
    // Constant viewport products use shift/add; no additional DSP lane.
    screen_x=32'sd13107200 + (($signed(high_x)<<<8)+($signed(high_x)<<<7)+($signed(high_x)<<<4));
    screen_y=32'sd7864320 - (($signed(high_y)<<<8)-($signed(high_y)<<<4));
end
function signed [15:0] snap;
    input signed [31:0] value;reg signed [33:0] rounded;
    begin rounded=($signed({{2{value[31]}},value})+34'sd2048) >>> 12;
        if(rounded>32767) snap=32767;
        else if(rounded < -32767) snap=-32767;
        else snap=rounded[15:0];
    end
endfunction
assign input_ready=state==LOAD;
assign output_valid=state==EMIT || state==END_SOURCE;
assign output_end=state==END_SOURCE;
assign output_source_last=source_last;
reg [135:0] selected_projected;
always @* begin
    case(emit_corner) 0:selected_projected=projected[0];1:selected_projected=projected[1];default:selected_projected=projected[2];endcase
end
assign output_xy=selected_projected[31:0];
assign output_color=selected_projected[79:32];
assign output_w=selected_projected[111:80];
assign output_invw=selected_projected[135:112];
reg [23:0] projected_invw;
always @* begin
 // Prepend the maximum six fractional bits, then use the same 0..18
 // right-shift domain. This preserves the original 24-bit truncation.
 projected_invw={reciprocal[17:0],6'd0}>>(reciprocal[23:18]-31);
end
task write_record;
    input [1:0] selection;input which;input [3:0] number;input [4:0] continuation;
    begin write_selection<=selection;write_vertex<=vertex_address(which,number);row<=0;write_return<=continuation;state<=WRITE;end
endtask
task read_record;
    input which;input [3:0] number;input [4:0] continuation;
    begin read_address<={2'd0,vertex_address(which,number)};row<=0;read_return<=continuation;state<=READ_WAIT;end
endtask
task next_primitive;
    begin
        if(fan+1>=length-1) state<=END_SOURCE;
        else begin fan<=fan+1'b1;corner<=0;invalid<=0;state<=PROJECT_ADDR;end
    end
endtask
integer reset_i;
always @(posedge clk) begin
    if(reset) begin
        state<=LOAD;bank<=0;source_last<=0;length<=0;next_length<=0;index<=0;fan<=1;plane<=0;
        classify_plane<=0;classify_code<=0;code_or<=0;code_and<=1023;pending_planes<=0;row<=0;corner<=0;emit_corner<=0;write_vertex<=0;write_return<=0;read_return<=0;
        write_selection<=0;previous<=0;current<=0;intersection<=0;previous_distance<=0;
        outside_previous<=0;append_current<=0;denominator<=0;remainder<=0;quotient<=0;bits<=0;
        component<=0;delay<=0;reciprocal<=0;reciprocal_base<=0;reciprocal_offset<=0;ndcx<=0;ndcy<=0;invalid<=0;error<=0;
        multiply_a<=0;multiply_b<=0;read_address<=0;
        for(reset_i=0;reset_i<3;reset_i=reset_i+1) projected[reset_i]<=0;
    end else case(state)
        LOAD:if(input_valid) begin
            source_last<=input_source_last;
            current<={3'd0,input_vertex[168:164],8'd0,2'd0,input_vertex[174:169],8'd0,3'd0,input_vertex[179:175],8'd0,input_vertex[127:0]};
            write_record(0,bank,length,CLASSIFY);
            classify_plane<=0;classify_code<=0;
            length<=length+1'b1;
            if(length==0) begin code_or<=0;code_and<=1023;pending_planes<=0;end
        end
        WRITE:if(row==2) state<=write_return;else row<=row+1'b1;
        CLASSIFY:begin
            if(distance<40'sd224) pending_planes[classify_plane]<=1'b1;
            classify_code<=complete_code;
            if(classify_plane==9) begin
                code_or<=complete_or;code_and<=complete_and;
                if(length<3) state<=LOAD;
                else if((complete_or & 10'b11_1100_0011)==0) begin fan<=1;corner<=0;invalid<=0;state<=PROJECT_ADDR;end
                else if(complete_and!=0) state<=END_SOURCE;
                else begin plane<=0;index<=0;next_length<=0;state<=SELECT_PLANE;end
            end else classify_plane<=classify_plane+1'b1;
        end
        SELECT_PLANE:begin
            if(plane==6) begin fan<=1;corner<=0;invalid<=0;state<=PROJECT_ADDR;end
            else if(selected_planes[plane]) state<=PREV_ADDR;
            else plane<=plane+1'b1;
        end
        PREV_ADDR:read_record(bank,length-1'b1,PREV_DIST);
        READ_WAIT:state<=READ;
        READ:begin
            case(row) 0:current[71:0]<=read_data;1:current[143:72]<=read_data;2:current[175:144]<=read_data[31:0];endcase
            if(row==2) state<=read_return;
            else begin row<=row+1'b1;read_address<=read_address+1'b1;state<=READ_WAIT;end
        end
        PREV_DIST:begin previous<=current;previous_distance<=distance;state<=CUR_ADDR;end
        CUR_ADDR:read_record(bank,index,STEP);
        STEP:begin
            append_current<=!distance[39];outside_previous<=previous_distance[39];
            if(previous_distance[39]!=distance[39]) begin
                if((previous_distance==0 && !previous_distance[39]) || (distance==0 && !distance[39])) begin
                    if(previous_distance[39]) write_record(0,!bank,next_length,NEXT);
                    else write_record(1,!bank,next_length,NEXT);
                    next_length<=next_length+1'b1;
                end else begin
                    if(previous_distance[39]) begin remainder<=-previous_distance;denominator<=distance-previous_distance;end
                    else begin remainder<=-distance;denominator<=previous_distance-distance;end
                    quotient<=0;bits<=0;state<=DIV;
                end
            end else if(!distance[39]) begin
                write_record(0,!bank,next_length,NEXT);next_length<=next_length+1'b1;append_current<=0;
            end else state<=NEXT;
        end
        DIV:begin
            if(subtract) remainder<=reduced[40:0];else remainder<=doubled[40:0];
            quotient<={quotient[30:0],subtract};bits<=bits+1'b1;
            if(bits==31) begin component<=0;state<=LERP_ISSUE;end
        end
        LERP_ISSUE:begin multiply_a<={{3{difference[32]}},difference};multiply_b<={4'd0,quotient};delay<=0;state<=LERP_WAIT;end
        LERP_WAIT:if(delay==2) state<=LERP_CAPTURE;else delay<=delay+1'b1;
        LERP_CAPTURE:begin
            case(component)
                0:intersection[31:0]<=lerp_value[31:0];1:intersection[63:32]<=lerp_value[31:0];
                2:intersection[95:64]<=lerp_value[31:0];3:intersection[127:96]<=lerp_value[31:0];
                4:intersection[143:128]<=lerp_value[15:0];5:intersection[159:144]<=lerp_value[15:0];
                6:intersection[175:160]<=lerp_value[15:0];
            endcase
            if(component==6) begin
                write_record(2,!bank,next_length,NEXT);next_length<=next_length+1'b1;
            end else begin component<=component+1'b1;state<=LERP_ISSUE;end
        end
        NEXT:begin
            if(append_current) begin
                write_record(0,!bank,next_length,NEXT);next_length<=next_length+1'b1;append_current<=0;
            end else if(index+1<length) begin index<=index+1'b1;previous<=current;previous_distance<=distance;state<=CUR_ADDR;end
            else begin
                bank<=!bank;length<=next_length;next_length<=0;index<=0;plane<=plane+1'b1;
                if(next_length<3) state<=END_SOURCE;
                else if(plane==5) begin fan<=1;corner<=0;invalid<=0;state<=PROJECT_ADDR;end
                else state<=SELECT_PLANE;
            end
        end
        PROJECT_ADDR:begin
            case(corner) 0:read_record(bank,0,PROJECT_RCP);1:read_record(bank,fan,PROJECT_RCP);default:read_record(bank,fan+1'b1,PROJECT_RCP);endcase
        end
        PROJECT_RCP:begin
            if($signed(current[127:96])<8192 || $signed(current[95:64]) < -1024 || $signed({current[95],current[95:64]}) > $signed({current[127],current[127:96]})+33'sd1024) begin invalid<=1;error<=1;end
            reciprocal[23:18]<=49-reciprocal_lz;reciprocal_base<=reciprocal_word[18:0];
            reciprocal_offset<=normalized_w[22:11];component<=0;state<=RCP_ISSUE;
        end
        RCP_ISSUE:begin
            multiply_a<={{23{reciprocal_word[31]}},reciprocal_word[31:19]};multiply_b<={24'd0,reciprocal_offset};delay<=0;state<=RCP_WAIT;
        end
        RCP_WAIT:if(delay==2) state<=RCP_CAPTURE;else delay<=delay+1'b1;
        RCP_CAPTURE:begin
            if(reciprocal_value<131072) reciprocal[17:0]<=18'd131072;
            else if(reciprocal_value>262143) reciprocal[17:0]<=18'd262143;
            else reciprocal[17:0]<=reciprocal_value[17:0];
            state<=PROJECT_ISSUE;
        end
        PROJECT_ISSUE:begin
            // Projection visits X and Y only; avoid a dynamic 32-bit slice
            // over the full vertex record and a separate sign-bit mux.
            if(component==0) multiply_a<={{4{current[31]}},current[31:0]};
            else multiply_a<={{4{current[63]}},current[63:32]};
            multiply_b<={18'd0,reciprocal[17:0]};delay<=0;state<=PROJECT_WAIT;
        end
        PROJECT_WAIT:if(delay==2) state<=PROJECT_CAPTURE;else delay<=delay+1'b1;
        PROJECT_CAPTURE:begin
            if(component==0) begin ndcx<=ndc_scaled[17:0];component<=1;state<=PROJECT_ISSUE;end
            else begin ndcy<=ndc_scaled[17:0];state<=PROJECT_VIEW;end
        end
        PROJECT_VIEW:begin
            projected[corner]<={projected_invw,current[127:96],current[175:128],snap(screen_y),snap(screen_x)};
            if(corner==2) begin if(invalid) next_primitive();else begin emit_corner<=0;state<=EMIT;end end
            else begin corner<=corner+1'b1;state<=PROJECT_ADDR;end
        end
        EMIT:if(output_ready) begin if(emit_corner==2) next_primitive();else emit_corner<=emit_corner+1'b1;end
        END_SOURCE:if(output_ready) begin state<=LOAD;length<=0;next_length<=0;index<=0;end
    endcase
end
endmodule
