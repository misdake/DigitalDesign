// Two raw meshlet slots feed the serial geometry microcore in commit order.
// The producer may fill the free slot while the other slot is being consumed.
// Slot layout (64-bit words): matrix 0..7, vertex i at 8+3*i (128-bit
// position then RGB565 in the low bits of word 2), triangle i at 200+i
// (three 8-bit vertex indices in bits 23:0). Each slot occupies 512 words.
// Commit publishes a complete slot. Release occurs only when the downstream
// consumer accepts the final source-end record of that meshlet.
module GpuMeshletMicrocore(input wire clk,input wire reset,
    input wire load_valid,output wire load_ready,input wire load_slot,
    input wire [8:0] load_address,input wire [63:0] load_data,
    input wire commit_valid,output wire commit_ready,input wire commit_slot,
    input wire [6:0] commit_vertex_count,input wire [7:0] commit_triangle_count,
    output wire output_valid,input wire output_ready,output wire output_end,
    output wire output_source_last,output wire output_error,
    output wire output_slot,output wire [7:0] output_source,
    output wire [95:0] output_xy,output wire [707:0] output_planes,
    output wire [5:0] output_scale,
    output reg release_valid,output reg release_slot);
localparam [3:0] IDLE=0,TRI_READ=1,TRI_CAPTURE=2,
    VERT_READ=3,VERT_CAPTURE=4,LAUNCH=5,RUN=6,BAD_INDEX=7;
reg [3:0] state;
reg [1:0] locked,pending;
reg first_pending,active_slot;
reg [6:0] vertex_counts[0:1];
reg [7:0] triangle_counts[0:1];
reg [7:0] triangle_index;
reg [63:0] triangle_word;
reg [1:0] corner,word_phase;
reg [383:0] positions;
reg [47:0] colors;

assign load_ready=!locked[load_slot];
// Avoid enqueue/dequeue of the pending head on the same edge. This is a
// one-cycle control bubble only; the other slot remains writable.
assign commit_ready=!locked[commit_slot] && !(state==IDLE && (|pending));
reg [63:0] words[0:1023];
reg [63:0] read_data;
reg [8:0] local_address;
wire [9:0] core_scratch_address;
wire [7:0] vertex_index=triangle_word[corner*8+:8];
always @* begin
    local_address=0;
    case(state)
        TRI_READ:local_address=9'd200+{1'b0,triangle_index};
        VERT_READ:local_address=9'd8+({1'b0,vertex_index}*9'd3)+word_phase;
        LAUNCH,RUN:local_address=core_scratch_address[8:0];
    endcase
end
wire [9:0] read_address={active_slot,local_address};
always @(posedge clk) begin
    read_data<=words[read_address];
    if(load_valid && load_ready) words[{load_slot,load_address}]<=load_data;
end

wire core_ready,core_valid,core_end,core_last,core_error;
wire [95:0] core_xy;
wire [707:0] core_planes;
wire [5:0] core_scale;
wire core_start=state==LAUNCH;
wire core_output_ready=state==RUN && output_ready;
wire final_source=triangle_index+8'd1==triangle_counts[active_slot];
GpuGeometryMicrocore core(clk,reset,core_start,core_ready,positions,colors,
    10'd0,final_source,core_scratch_address,read_data,
    core_valid,core_output_ready,core_end,core_last,core_error,
    core_xy,core_planes,core_scale);

assign output_valid=(state==RUN && core_valid) || state==BAD_INDEX;
assign output_end=state==BAD_INDEX || core_end;
assign output_source_last=state==BAD_INDEX ? final_source : core_last;
assign output_error=state==BAD_INDEX || core_error;
assign output_slot=active_slot;
assign output_source=triangle_index;
assign output_xy=state==BAD_INDEX ? 96'd0 : core_xy;
assign output_planes=state==BAD_INDEX ? 708'd0 : core_planes;
assign output_scale=state==BAD_INDEX ? 6'd0 : core_scale;
wire source_retired=output_valid && output_ready && output_end;

always @(posedge clk) begin
    release_valid<=0;
    if(reset) begin
        state<=IDLE;locked<=0;pending<=0;first_pending<=0;
        active_slot<=0;triangle_index<=0;triangle_word<=0;
        corner<=0;word_phase<=0;positions<=0;colors<=0;
        vertex_counts[0]<=0;vertex_counts[1]<=0;
        triangle_counts[0]<=0;triangle_counts[1]<=0;
        release_valid<=0;release_slot<=0;
    end else begin
        if(commit_valid && commit_ready) begin
            locked[commit_slot]<=1;
            pending[commit_slot]<=1;
            if(pending==0) first_pending<=commit_slot;
            vertex_counts[commit_slot]<=commit_vertex_count;
            triangle_counts[commit_slot]<=commit_triangle_count;
        end
        case(state)
            IDLE:if(|pending) begin
                active_slot<=first_pending;
                pending[first_pending]<=0;
                if(pending[!first_pending]) first_pending<=!first_pending;
                triangle_index<=0;
                if(triangle_counts[first_pending]==0) begin
                    locked[first_pending]<=0;
                    release_valid<=1;release_slot<=first_pending;
                end else state<=TRI_READ;
            end
            TRI_READ:state<=TRI_CAPTURE;
            TRI_CAPTURE:begin
                triangle_word<=read_data;corner<=0;word_phase<=0;
                state<=VERT_READ;
            end
            VERT_READ:if(vertex_index>=vertex_counts[active_slot]) state<=BAD_INDEX;
                else state<=VERT_CAPTURE;
            VERT_CAPTURE:begin
                case(word_phase)
                    0:positions[corner*128+:64]<=read_data;
                    1:positions[corner*128+64+:64]<=read_data;
                    default:colors[corner*16+:16]<=read_data[15:0];
                endcase
                if(word_phase==2) begin
                    word_phase<=0;
                    if(corner==2) state<=LAUNCH;
                    else begin corner<=corner+1'b1;state<=VERT_READ;end
                end else begin word_phase<=word_phase+1'b1;state<=VERT_READ;end
            end
            LAUNCH:if(core_ready) state<=RUN;
            RUN,BAD_INDEX:if(source_retired) begin
                if(final_source) begin
                    locked[active_slot]<=0;
                    release_valid<=1;release_slot<=active_slot;
                    state<=IDLE;
                end else begin
                    triangle_index<=triangle_index+1'b1;
                    state<=TRI_READ;
                end
            end
        endcase
    end
end
endmodule
