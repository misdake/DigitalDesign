module CpuV3DataCache (
    input wire clk,
    input wire reset,
    input wire clean_all,
    input wire invalidate_all,
    input wire cpu_request_valid,
    input wire cpu_write,
    input wire [31:0] cpu_address,
    input wire [15:0] cpu_write_data,
    input wire cpu_response_ready,
    input wire memory_request_ready,
    input wire memory_response_valid,
    input wire [63:0] memory_read_data,
    input wire memory_error,
    output wire cpu_request_ready,
    output wire cpu_response_valid,
    output wire [15:0] cpu_read_data,
    output wire cpu_error,
    output wire memory_request_valid,
    output wire memory_write,
    output wire memory_line,
    output wire [21:0] memory_address,
    output wire [63:0] memory_write_data,
    output wire memory_response_ready,
    output wire maintenance_busy,
    output reg maintenance_done = 0,
    output reg maintenance_error = 0
);

localparam [3:0] ST_IDLE=0, ST_LOOKUP=1, ST_LINE_REQUEST=2,
    ST_LINE_RECEIVE=3, ST_WB_PRIME=5,
    ST_WB_CAPTURE=6, ST_WB_REQUEST=7, ST_WB_STREAM=8,
    ST_WB_RESPONSE=9;

reg [3:0] state = ST_IDLE;
reg pending_write = 0;
reg [31:0] pending_address = 0;
reg [15:0] pending_write_data = 0;
reg pending_way = 0;
reg [2:0] refill_beat = 0;
reg [2:0] wb_beat = 0;
reg wb_way = 0;
reg [5:0] wb_set = 0;
reg [21:0] wb_address = 0;
reg wb_for_maintenance = 0;
reg maintenance_active = 0;
reg maintenance_invalidate = 0;
reg [15:0] response_data = 0;
reg response_error = 0;
reg response_valid = 0;
reg [63:0] way_0_valid = 0;
reg [63:0] way_1_valid = 0;
reg [63:0] victim = 0;
reg [15:0] refill_response_data = 0;
reg [63:0] wb_first_data = 0;

wire [5:0] pending_set = pending_address[9:4];
wire [11:0] pending_tag = pending_address[21:10];
wire [3:0] pending_word = pending_address[3:0];
wire pending_address_valid = pending_address[31:22] == 0;
wire [5:0] tag_read_set = (state == ST_WB_PRIME || state == ST_WB_CAPTURE) ?
    wb_set : pending_set;
wire [11:0] way_0_tag;
wire [11:0] way_1_tag;
wire way_0_hit = way_0_valid[pending_set] && way_0_tag == pending_tag;
wire way_1_hit = way_1_valid[pending_set] && way_1_tag == pending_tag;
wire pending_hit = way_0_hit || way_1_hit;
wire hit_way = !way_0_hit && way_1_hit;
wire selected_victim = !way_0_valid[pending_set] ? 1'b0 :
                       !way_1_valid[pending_set] ? 1'b1 : victim[pending_set];

wire refill_commit = state == ST_LINE_RECEIVE && memory_response_valid &&
    !memory_error && refill_beat == 3;
wire tag_write_enable = refill_commit;
__CACHE_TAGS__ u_tags (
    .clk(clk), .write_enable(tag_write_enable), .write_way(pending_way),
    .address(tag_read_set), .write_data(pending_tag),
    .way_0_read_data(way_0_tag), .way_1_read_data(way_1_tag)
);

wire writeback_read_mode = state == ST_WB_PRIME || state == ST_WB_CAPTURE ||
    state == ST_WB_REQUEST || state == ST_WB_STREAM;
wire [1:0] writeback_read_beat = state == ST_WB_PRIME || state == ST_WB_CAPTURE ? 2'd0 :
    state == ST_WB_REQUEST ? 2'd1 : wb_beat[1:0] + 2'd1;
wire [5:0] data_read_set = writeback_read_mode ? wb_set :
    (state == ST_IDLE && cpu_request_valid ? cpu_address[9:4] : pending_set);
wire [3:0] data_read_word = state == ST_IDLE && cpu_request_valid ?
    cpu_address[3:0] : pending_word;
wire [9:0] lookup_way_0_address = {1'b0, data_read_set, data_read_word[3:1]};
wire [9:0] lookup_way_1_address = {1'b1, data_read_set, data_read_word[3:1]};
wire [9:0] wb_read_address_a = {wb_way, wb_set, writeback_read_beat, 1'b0};
wire [9:0] wb_read_address_b = {wb_way, wb_set, writeback_read_beat, 1'b1};
wire [15:0] bank_0_a_read_data, bank_0_b_read_data;
wire [15:0] bank_1_a_read_data, bank_1_b_read_data;
wire [15:0] way_0_read_data = pending_word[0] ? bank_1_a_read_data : bank_0_a_read_data;
wire [15:0] way_1_read_data = pending_word[0] ? bank_1_b_read_data : bank_0_b_read_data;
wire [15:0] hit_read_data = hit_way ? way_1_read_data : way_0_read_data;
wire [63:0] wb_read_data = {bank_1_b_read_data, bank_0_b_read_data,
                            bank_1_a_read_data, bank_0_a_read_data};

wire hit_store = state == ST_LOOKUP && pending_write && pending_hit;
wire refill_write = state == ST_LINE_RECEIVE && memory_response_valid && !memory_error;
wire [9:0] refill_address_a = {pending_way, pending_set, refill_beat[1:0], 1'b0};
wire [9:0] refill_address_b = {pending_way, pending_set, refill_beat[1:0], 1'b1};
wire [9:0] hit_store_address = {hit_way, pending_set, pending_word[3:1]};
wire [15:0] refill_word_0 = pending_write && pending_word[3:2] == refill_beat && pending_word[1:0] == 0 ? pending_write_data : memory_read_data[15:0];
wire [15:0] refill_word_1 = pending_write && pending_word[3:2] == refill_beat && pending_word[1:0] == 1 ? pending_write_data : memory_read_data[31:16];
wire [15:0] refill_word_2 = pending_write && pending_word[3:2] == refill_beat && pending_word[1:0] == 2 ? pending_write_data : memory_read_data[47:32];
wire [15:0] refill_word_3 = pending_write && pending_word[3:2] == refill_beat && pending_word[1:0] == 3 ? pending_write_data : memory_read_data[63:48];

__CACHE_DATA_BANKS__ u_data_banks (
    .clk(clk),
    .bank_0_a_write_enable(refill_write || (hit_store && !pending_word[0])),
    .bank_0_a_address(refill_write ? refill_address_a : writeback_read_mode ? wb_read_address_a :
        (hit_store && !pending_word[0] ? hit_store_address : lookup_way_0_address)),
    .bank_0_a_write_data(refill_write ? refill_word_0 : pending_write_data),
    .bank_0_a_read_data(bank_0_a_read_data),
    .bank_0_b_write_enable(refill_write),
    .bank_0_b_address(refill_write ? refill_address_b : writeback_read_mode ? wb_read_address_b : lookup_way_1_address),
    .bank_0_b_write_data(refill_word_2), .bank_0_b_read_data(bank_0_b_read_data),
    .bank_1_a_write_enable(refill_write || (hit_store && pending_word[0])),
    .bank_1_a_address(refill_write ? refill_address_a : writeback_read_mode ? wb_read_address_a :
        (hit_store && pending_word[0] ? hit_store_address : lookup_way_0_address)),
    .bank_1_a_write_data(refill_write ? refill_word_1 : pending_write_data),
    .bank_1_a_read_data(bank_1_a_read_data),
    .bank_1_b_write_enable(refill_write),
    .bank_1_b_address(refill_write ? refill_address_b : writeback_read_mode ? wb_read_address_b : lookup_way_1_address),
    .bank_1_b_write_data(refill_word_3), .bank_1_b_read_data(bank_1_b_read_data)
);

wire [63:0] way_0_dirty;
wire [63:0] way_1_dirty;
wire dirty_write_hit = state == ST_LOOKUP && pending_write && pending_hit;
wire dirty_write_install = refill_commit;
wire dirty_write_back = state == ST_WB_RESPONSE && memory_response_valid && !memory_error;
wire dirty_write_enable = dirty_write_hit || dirty_write_install || dirty_write_back;
wire dirty_write_way = dirty_write_hit ? hit_way :
    dirty_write_install ? pending_way : wb_way;
wire [5:0] dirty_write_set = dirty_write_hit ? pending_set :
    dirty_write_install ? pending_set : wb_set;
wire dirty_write_value = dirty_write_hit ? 1'b1 :
    dirty_write_install ? pending_write : 1'b0;
wire dirty_clear_all = reset ||
    ((state == ST_LINE_RECEIVE || state == ST_WB_RESPONSE) &&
     memory_response_valid && memory_error);
__DIRTY_RAM__ u_dirty (
    .clk(clk), .write_enable(dirty_write_enable),
    .write_way(dirty_write_way), .write_set(dirty_write_set),
    .write_value(dirty_write_value), .clear_all(dirty_clear_all),
    .way_0(way_0_dirty), .way_1(way_1_dirty)
);

wire [127:0] dirty_bits = {way_1_dirty, way_0_dirty};
wire selected_victim_dirty = selected_victim ?
    way_1_dirty[pending_set] : way_0_dirty[pending_set];
function [6:0] find_first_dirty;
    input [127:0] bits;
    integer index;
    reg found;
    begin
        find_first_dirty = 0;
        found = 0;
        for (index = 0; index < 128; index = index + 1)
            if (bits[index] && !found) begin
                find_first_dirty = index[6:0];
                found = 1;
            end
    end
endfunction
wire [6:0] first_dirty = find_first_dirty(dirty_bits);
wire [127:0] completed_dirty_mask = 128'b1 << {wb_way, wb_set};
wire [127:0] remaining_dirty_bits = dirty_bits & ~completed_dirty_mask;
wire [6:0] next_dirty = find_first_dirty(remaining_dirty_bits);

assign cpu_request_ready = state == ST_IDLE && !response_valid &&
    !maintenance_active && !clean_all && !invalidate_all;
assign cpu_response_valid = response_valid;
assign cpu_read_data = response_data;
assign cpu_error = response_valid && response_error;
assign memory_request_valid = state == ST_LINE_REQUEST || state == ST_WB_REQUEST;
assign memory_write = state == ST_WB_REQUEST || state == ST_WB_STREAM ||
    state == ST_WB_RESPONSE;
assign memory_line = state != ST_IDLE && state != ST_LOOKUP;
assign memory_address = memory_write ? wb_address : {pending_address[21:4],4'b0};
assign memory_write_data = state == ST_WB_REQUEST ? wb_first_data : wb_read_data;
assign memory_response_ready = state == ST_LINE_RECEIVE || state == ST_WB_RESPONSE;
assign maintenance_busy = maintenance_active;

always @(posedge clk) begin
    maintenance_done <= 0;
    if (reset) begin
        state <= ST_IDLE;
        response_valid <= 0;
        response_error <= 0;
        way_0_valid <= 0;
        way_1_valid <= 0;
        victim <= 0;
        maintenance_active <= 0;
        maintenance_error <= 0;
    end else begin
        if (response_valid && cpu_response_ready)
            response_valid <= 0;
        case (state)
            ST_IDLE: begin
                if (!response_valid && (clean_all || invalidate_all)) begin
                    maintenance_active <= 1;
                    maintenance_invalidate <= invalidate_all;
                    maintenance_error <= 0;
                    if (|dirty_bits) begin
                        wb_way <= first_dirty[6];
                        wb_set <= first_dirty[5:0];
                        wb_for_maintenance <= 1;
                        wb_beat <= 0;
                        state <= ST_WB_PRIME;
                    end else begin
                        if (invalidate_all) begin
                            way_0_valid <= 0;
                            way_1_valid <= 0;
                        end
                        maintenance_active <= 0;
                        maintenance_done <= 1;
                    end
                end else if (!response_valid && cpu_request_valid) begin
                    pending_write <= cpu_write;
                    pending_address <= cpu_address;
                    pending_write_data <= cpu_write_data;
                    response_error <= 0;
                    state <= ST_LOOKUP;
                end
            end
            ST_LOOKUP: begin
                if (!pending_address_valid) begin
                    response_data <= 0;
                    response_error <= 1;
                    response_valid <= 1;
                    state <= ST_IDLE;
                end else if (pending_hit) begin
                    response_data <= pending_write ? 16'b0 : hit_read_data;
                    response_error <= 0;
                    response_valid <= 1;
                    state <= ST_IDLE;
                end else begin
                    pending_way <= selected_victim;
                    if (selected_victim_dirty) begin
                        wb_way <= selected_victim;
                        wb_set <= pending_set;
                        wb_for_maintenance <= 0;
                        wb_beat <= 0;
                        state <= ST_WB_PRIME;
                    end else begin
                        if (selected_victim) way_1_valid[pending_set] <= 0;
                        else way_0_valid[pending_set] <= 0;
                        refill_beat <= 0;
                        state <= ST_LINE_REQUEST;
                    end
                end
            end
            ST_WB_PRIME: begin
                wb_address <= {(wb_way ? way_1_tag : way_0_tag), wb_set, 4'b0};
                wb_beat <= 0;
                state <= ST_WB_CAPTURE;
            end
            ST_WB_CAPTURE: begin
                wb_first_data <= wb_read_data;
                state <= ST_WB_REQUEST;
            end
            ST_WB_REQUEST: if (memory_request_ready) begin
                wb_beat <= 1;
                state <= ST_WB_STREAM;
            end
            ST_WB_STREAM: begin
                if (wb_beat == 3)
                    state <= ST_WB_RESPONSE;
                else wb_beat <= wb_beat + 1'b1;
            end
            ST_WB_RESPONSE: if (memory_response_valid) begin
                if (memory_error) begin
                    way_0_valid <= 0;
                    way_1_valid <= 0;
                    if (maintenance_active) begin
                        maintenance_active <= 0;
                        maintenance_error <= 1;
                        maintenance_done <= 1;
                    end else begin
                        response_error <= 1;
                        response_data <= 0;
                        response_valid <= 1;
                    end
                    state <= ST_IDLE;
                end else begin
                    if (wb_for_maintenance) begin
                        if (|remaining_dirty_bits) begin
                            wb_way <= next_dirty[6];
                            wb_set <= next_dirty[5:0];
                            wb_beat <= 0;
                            state <= ST_WB_PRIME;
                        end else begin
                            if (maintenance_invalidate) begin
                                way_0_valid <= 0;
                                way_1_valid <= 0;
                            end
                            maintenance_active <= 0;
                            maintenance_done <= 1;
                            state <= ST_IDLE;
                        end
                    end else begin
                        if (pending_way) way_1_valid[pending_set] <= 0;
                        else way_0_valid[pending_set] <= 0;
                        refill_beat <= 0;
                        state <= ST_LINE_REQUEST;
                    end
                end
            end
            ST_LINE_REQUEST: if (memory_request_ready) begin
                refill_beat <= 0;
                state <= ST_LINE_RECEIVE;
            end
            ST_LINE_RECEIVE: if (memory_response_valid) begin
                if (memory_error) begin
                    way_0_valid <= 0;
                    way_1_valid <= 0;
                    response_data <= 0;
                    response_error <= 1;
                    response_valid <= 1;
                    state <= ST_IDLE;
                end else begin
                    if (refill_beat == pending_word[3:2])
                        case (pending_word[1:0])
                            0: refill_response_data <= memory_read_data[15:0];
                            1: refill_response_data <= memory_read_data[31:16];
                            2: refill_response_data <= memory_read_data[47:32];
                            default: refill_response_data <= memory_read_data[63:48];
                        endcase
                    if (refill_beat == 3) begin
                    if (pending_way) way_1_valid[pending_set] <= 1;
                    else way_0_valid[pending_set] <= 1;
                    victim[pending_set] <= !pending_way;
                    response_data <= pending_write ? 16'b0 :
                        (pending_word[3:2] == 3 ?
                            (pending_word[1:0] == 0 ? memory_read_data[15:0] :
                             pending_word[1:0] == 1 ? memory_read_data[31:16] :
                             pending_word[1:0] == 2 ? memory_read_data[47:32] : memory_read_data[63:48]) :
                            refill_response_data);
                    response_error <= 0;
                    response_valid <= 1;
                    state <= ST_IDLE;
                    end else refill_beat <= refill_beat + 1'b1;
                end
            end
            default: state <= ST_IDLE;
        endcase
    end
end

endmodule
