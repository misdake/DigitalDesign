// Sequential one-owner v6 vertex leaf; caller owns private result rows.
module gpu_v2_vertex_single_edge(
    input wire clk, reset, ce,
    input wire config_valid,
    output wire config_ready,
    input wire signed [31:0] base_x, base_y, base_z,
    input wire [4:0] grid_shift,
    input wire coefficient_valid,
    output wire coefficient_ready,
    output wire [4:0] coefficient_index,
    input wire signed [31:0] coefficient_data,
    input wire vertex_valid,
    output wire vertex_ready,
    input wire [95:0] vertex_data,
    input wire vertex_slot,
    input wire [31:0] vertex_epoch,
    input wire [5:0] vertex_index,
    output wire write_valid,
    input wire write_ready,
    output wire [2:0] row_index,
    output wire [35:0] row_data,
    output wire publish_valid,
    input wire publish_ready,
    output wire result_slot,
    output wire [31:0] result_epoch,
    output wire [5:0] result_vertex,
    output wire [2:0] fault
);
localparam IDLE=0, LOAD=1, POSITION=2, INIT_READ=3, INIT_RETURN=4,
    TERM_READ=5, TERM_RETURN=6,
    ACCUMULATE=7, ROUND=8, WRITE=9, PUBLISH=10, FAULT=11;
reg [3:0] state;
reg loaded;
reg signed [31:0] bases [0:2];
reg [4:0] spacing, config_index;
reg [95:0] raw;
reg signed [31:0] position [0:2];
reg [2:0] row;
reg [1:0] normal_row, col;
wire signed [31:0] coefficient_q;
reg signed [65:0] accumulator;
reg [35:0] normal_pack, offered_row;
reg slot;
reg [31:0] epoch;
reg [5:0] destination;
reg [2:0] error;
assign config_ready = state == IDLE;
assign coefficient_ready = state == LOAD;
assign coefficient_index = config_index;
assign vertex_ready = state == IDLE && loaded && !config_valid;
assign write_valid = state == WRITE;
assign publish_valid = state == PUBLISH;
assign row_index = row;
assign row_data = offered_row;
assign result_slot = slot;
assign result_epoch = epoch;
assign result_vertex = destination;
assign fault = error;

wire normal_word_ok = coefficient_data[31:16] == {16{coefficient_data[15]}};
wire context_write = ce && !reset && state == LOAD && coefficient_valid
    && (config_index < 16 || normal_word_ok);
wire context_read = ce && !reset && (state == INIT_READ || state == TERM_READ);
wire [4:0] clip_address = {row[1:0],2'b00} + {3'b000,col};
wire [4:0] normal_address = 5'd16 + {2'b00,normal_row,1'b0} + {3'b000,normal_row} + {3'b000,col};
wire [4:0] read_address = state == INIT_READ ? {row[1:0],2'b11}
    : row < 4 ? clip_address : normal_address;
`ifdef GPU_VERTEX_SINGLE_EDGE_NATIVE
wire [35:0] context_q;
SDPX9B #(.BIT_WIDTH_0(36),.BIT_WIDTH_1(36),.READ_MODE(1'b0),.BLK_SEL_0(3'b000),.BLK_SEL_1(3'b000),.RESET_MODE("SYNC")) context_bank (
    .DO(context_q),.DI({4'b0000,coefficient_data}),
    .ADA({4'b0000,config_index,1'b0,4'b1111}),.ADB({4'b0000,read_address,5'b00000}),
    .BLKSELA(3'b000),.BLKSELB(3'b000),.CLKA(clk),.CLKB(clk),
    .CEA(context_write),.CEB(context_read),.OCE(1'b1),.RESETA(reset),.RESETB(reset));
assign coefficient_q = context_q[31:0];
`else
reg signed [31:0] context_bank [0:31];
reg signed [31:0] context_q;
always @(posedge clk) begin
    if (context_write) context_bank[config_index] <= coefficient_data;
    if (reset) context_q <= 0;
    else if (context_read) context_q <= context_bank[read_address];
end
assign coefficient_q = context_q;
`endif

reg [9:0] xyz;
reg signed [31:0] selected_base, selected_position;
reg signed [7:0] selected_normal;
always @* begin
    xyz=0; selected_base=0; selected_position=0; selected_normal=0;
    case (col)
        0: begin xyz=raw[11:2]; selected_base=bases[0]; selected_position=position[0]; selected_normal=raw[39:32]; end
        1: begin xyz=raw[21:12]; selected_base=bases[1]; selected_position=position[1]; selected_normal=raw[47:40]; end
        2: begin xyz=raw[31:22]; selected_base=bases[2]; selected_position=position[2]; selected_normal=raw[55:48]; end
        default: begin end
    endcase
end
// A positive delta >= 2^32 cannot yield signed32 even with base == -2^31.
// Reject that high part before a low32 shift and a checked 33-bit sum.
reg position_high_overflow;
always @* begin
    position_high_overflow=1'b0;
    case(spacing)
        23:position_high_overflow=xyz[9];
        24:position_high_overflow=|xyz[9:8];
        25:position_high_overflow=|xyz[9:7];
        26:position_high_overflow=|xyz[9:6];
        27:position_high_overflow=|xyz[9:5];
        28:position_high_overflow=|xyz[9:4];
        29:position_high_overflow=|xyz[9:3];
        30:position_high_overflow=|xyz[9:2];
        default:begin end
    endcase
end
wire [31:0] position_delta = {22'b0,xyz} << spacing;
wire signed [32:0] position_sum = $signed({selected_base[31],selected_base}) + $signed({1'b0,position_delta});
wire position_ok = !position_high_overflow && position_sum[32] == position_sum[31];
wire signed [15:0] normal_operand = {selected_normal[7],selected_normal,7'b0};
wire wide_ce = ce && !reset && row < 4 && state == TERM_RETURN;
wire narrow_ce = ce && !reset && row == 4 && state == TERM_RETURN;
wire signed [71:0] wide_result;
wire signed [35:0] narrow_result;
`ifdef GPU_VERTEX_SINGLE_EDGE_NATIVE
MULT36X36 #(.AREG(0),.BREG(0),.ASIGN_REG(0),.BSIGN_REG(0),.PIPE_REG(0),.OUT0_REG(1),.OUT1_REG(0),.MULT_RESET_MODE("SYNC")) wide_multiplier (
    .A({{4{coefficient_q[31]}},coefficient_q}),.B({{4{selected_position[31]}},selected_position}),
    .ASIGN(1'b1),.BSIGN(1'b1),.CE(wide_ce),.CLK(clk),.RESET(reset),.DOUT(wide_result));
MULT18X18 #(.AREG(0),.BREG(0),.ASIGN_REG(0),.BSIGN_REG(0),.PIPE_REG(0),.OUT_REG(1),.MULT_RESET_MODE("SYNC")) narrow_multiplier (
    .A({{2{coefficient_q[15]}},coefficient_q[15:0]}),.B({{2{normal_operand[15]}},normal_operand}),
    .SIA(18'b0),.SIB(18'b0),.ASEL(1'b0),.BSEL(1'b0),.SOA(),.SOB(),
    .ASIGN(1'b1),.BSIGN(1'b1),.CE(narrow_ce),.CLK(clk),.RESET(reset),.DOUT(narrow_result));
`else
reg signed [71:0] wide_product;
reg signed [35:0] narrow_product;
wire signed [35:0] wide_a = {{4{coefficient_q[31]}},coefficient_q};
wire signed [35:0] wide_b = {{4{selected_position[31]}},selected_position};
wire signed [17:0] narrow_a = {{2{coefficient_q[15]}},coefficient_q[15:0]};
wire signed [17:0] narrow_b = {{2{normal_operand[15]}},normal_operand};
always @(posedge clk) begin
    if (reset) begin wide_product<=0; narrow_product<=0; end
    else begin
        if (wide_ce) wide_product<=wide_a*wide_b;
        if (narrow_ce) narrow_product<=narrow_a*narrow_b;
    end
end
assign wide_result=wide_product;
assign narrow_result=narrow_product;
`endif

// Compute signed results separately; no signed shift/comparison in an unsigned ?:.
wire signed [65:0] clip_term = {{2{wide_result[63]}},wide_result[63:0]};
wire signed [65:0] normal_term = {{34{narrow_result[31]}},narrow_result[31:0]};
wire signed [65:0] selected_term = row < 4 ? clip_term : normal_term;
wire signed [65:0] accumulator_next = accumulator + selected_term;
wire clip_increment = accumulator[15] && ((|accumulator[14:0]) || accumulator[16]);
wire normal_increment = accumulator[17] && ((|accumulator[16:0]) || accumulator[18]);
wire signed [32:0] clip_rounded = $signed(accumulator[48:16]) + $signed({32'b0,clip_increment});
wire signed [12:0] normal_rounded = $signed(accumulator[30:18]) + $signed({12'b0,normal_increment});
wire clip_ok = accumulator[65:49] == {17{accumulator[48]}} && clip_rounded[32] == clip_rounded[31];
wire normal_ok = accumulator[65:31] == {35{accumulator[30]}} && normal_rounded[12] == normal_rounded[11];
integer k;
always @(posedge clk) begin
    if (reset) begin
        state<=IDLE; loaded<=0; spacing<=0; config_index<=0; raw<=0;
        row<=0; normal_row<=0; col<=0; accumulator<=0;
        normal_pack<=0; offered_row<=0; slot<=0; epoch<=0; destination<=0; error<=0;
        for(k=0;k<3;k=k+1) begin bases[k]<=0; position[k]<=0; end
    end else if (ce) begin
        case(state)
            IDLE: begin
                if(config_valid) begin
                    if(grid_shift==31) begin error<=1; state<=FAULT; end
                    else begin bases[0]<=base_x; bases[1]<=base_y; bases[2]<=base_z; spacing<=grid_shift; loaded<=0; config_index<=0; state<=LOAD; end
                end else if(vertex_valid && loaded) begin
                    if(vertex_data[1:0]!=0) begin error<=3; state<=FAULT; end
                    else begin raw<=vertex_data; slot<=vertex_slot; epoch<=vertex_epoch; destination<=vertex_index;
                        col<=0; row<=0; normal_row<=0; normal_pack<=0; state<=POSITION; end
                end
            end
            LOAD: if(coefficient_valid) begin
                if(config_index>=16 && !normal_word_ok) begin error<=2; state<=FAULT; end
                else if(config_index==24) begin loaded<=1; state<=IDLE; end
                else config_index<=config_index+1'b1;
            end
            POSITION: begin
                if(!position_ok) begin error<=4; state<=FAULT; end
                else begin
                    case(col) 0:position[0]<=position_sum[31:0]; 1:position[1]<=position_sum[31:0]; 2:position[2]<=position_sum[31:0]; default:begin end endcase
                    if(col==2) begin col<=0; state<=INIT_READ; end else col<=col+1'b1;
                end
            end
            INIT_READ: state<=INIT_RETURN;
            INIT_RETURN: begin accumulator<={{18{coefficient_q[31]}},coefficient_q,16'b0}; col<=0; state<=TERM_READ; end
            TERM_READ: state<=TERM_RETURN;
            TERM_RETURN: state<=ACCUMULATE;
            ACCUMULATE: begin
                accumulator<=accumulator_next;
                if(col==2) state<=ROUND; else begin col<=col+1'b1; state<=TERM_READ; end
            end
            ROUND: begin
                if(row<4) begin
                    if(!clip_ok) begin error<=5; state<=FAULT; end
                    else begin offered_row<={4'b0,clip_rounded[31:0]}; state<=WRITE; end
                end else if(!normal_ok) begin error<=6; state<=FAULT; end
                else begin
                    case(normal_row)
                        0:normal_pack[11:0]<=normal_rounded[11:0];
                        1:normal_pack[23:12]<=normal_rounded[11:0];
                        2:normal_pack[35:24]<=normal_rounded[11:0];
                        default:begin end
                    endcase
                    if(normal_row==2) begin offered_row<={normal_rounded[11:0],normal_pack[23:0]}; state<=WRITE; end
                    else begin normal_row<=normal_row+1'b1; col<=0; accumulator<=0; state<=TERM_READ; end
                end
            end
            WRITE: if(write_ready) begin
                case(row)
                    0,1,2: begin row<=row+1'b1; state<=INIT_READ; end
                    3: begin row<=4; col<=0; normal_row<=0; accumulator<=0; state<=TERM_READ; end
                    4: begin row<=5; offered_row<={12'b0,raw[79:68],raw[67:56]}; end
                    5: begin row<=6; offered_row<={20'b0,raw[95:80]}; end
                    6: state<=PUBLISH;
                    default:begin end
                endcase
            end
            PUBLISH: if(publish_ready) state<=IDLE;
            FAULT: begin end
            default: begin error<=7; state<=FAULT; end
        endcase
    end
end
endmodule
