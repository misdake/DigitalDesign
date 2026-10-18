module CpuV3InstructionFetchQueue (
    input wire clk,
    input wire reset,
    input wire flush,
    input wire core_request_valid,
    input wire [31:0] core_address,
    input wire core_response_ready,
    input wire memory_request_ready,
    input wire memory_response_valid,
    input wire [15:0] memory_read_data,
    input wire memory_error,
    output wire core_request_ready,
    output wire core_response_valid,
    output wire [15:0] core_read_data,
    output wire core_error,
    output wire memory_request_valid,
    output wire [31:0] memory_address,
    output wire memory_response_ready
);

localparam [2:0] QUEUE_DEPTH = 4;
localparam integer BTC_ENTRIES = __BTC_ENTRIES__;
localparam integer BTC_STORAGE = BTC_ENTRIES == 0 ? 1 : BTC_ENTRIES;
localparam integer BTC_INDEX_BITS = BTC_ENTRIES == 8 ? 3 : 2;
localparam integer PAIR_BITS = BTC_ENTRIES == 8 ? 4 : 3;
localparam integer PAYLOAD_WORDS = BTC_ENTRIES == 8 ? 32 : 16;

reg stream_valid = 0;
reg [31:0] expected_core_address = 0;
reg [31:0] next_memory_address = 0;

// Full stream ownership is checked against expected_core_address. Redirects
// invalidate old request slots before responses can enter the new stream.
// BTC tags remain full physical addresses because entries survive redirects.
(* syn_ramstyle = "registers" *) reg [15:0] queue_data [0:3];
(* syn_ramstyle = "registers" *) reg queue_error [0:3];
// Current requests and responses are ordered, so one stream cursor identifies
// the next downstream word. Old-path requests are drained using current bits.
reg [15:0] downstream_head_offset = 0;
reg [1:0] queue_head = 0;
reg [1:0] queue_tail = 0;
reg [2:0] queue_count = 0;

// Clear live ownership on every restart. Unlike a toggled epoch, these bits
// cannot alias an old request after a sequence of fast BTC redirects.
reg [3:0] metadata_current = 0;

reg [1:0] metadata_head = 0;
reg [1:0] metadata_tail = 0;
reg [2:0] metadata_count = 0;

reg [BTC_STORAGE-1:0] btc_valid = 0;
(* syn_ramstyle = "registers" *) reg [21:0] btc_tag [0:BTC_STORAGE-1];
// One spare pair is not referenced by any complete BTC entry. First/second
// words write its two addresses separately, and installation swaps ownership
// only after the error-free second word. Partial fills cannot damage old pairs.
(* syn_ramstyle = "distributed_ram" *) reg [15:0] btc_payload [0:PAYLOAD_WORDS-1];
(* syn_ramstyle = "registers" *) reg [PAIR_BITS-1:0] entry_pair [0:BTC_STORAGE-1];
reg [PAIR_BITS-1:0] spare_pair = BTC_ENTRIES;
// Exact LRU stores one relative-order bit for each unordered entry pair.
// Touching an entry updates only its incident bits; no rank mux/add/saturation.
localparam integer AGE_BITS = BTC_ENTRIES > 1 ? BTC_ENTRIES*(BTC_ENTRIES-1)/2 : 1;
reg [AGE_BITS-1:0] btc_newer = 0;
wire [BTC_STORAGE-1:0] btc_oldest;
function integer relation_index;
    input integer left_entry, right_entry;
    begin
        relation_index = left_entry*(2*BTC_ENTRIES-left_entry-1)/2 + right_entry-left_entry-1;
    end
endfunction
genvar age_row, age_column;
generate for (age_row=0;age_row<BTC_STORAGE;age_row=age_row+1) begin: age_rows
    wire [BTC_STORAGE-1:0] others_newer;
    for (age_column=0;age_column<BTC_STORAGE;age_column=age_column+1) begin: age_columns
        if (age_row==age_column) assign others_newer[age_column]=1'b1;
        else if (age_row<age_column)
            assign others_newer[age_column]=!btc_newer[relation_index(age_row,age_column)];
        else assign others_newer[age_column]=btc_newer[relation_index(age_column,age_row)];
    end
    assign btc_oldest[age_row]=&others_newer;
end endgenerate
reg [BTC_INDEX_BITS-1:0] replay_entry = 0;
reg [1:0] replay_remaining = 0;
reg [1:0] fill_phase = 0;
reg [21:0] fill_tag = 0;


function [31:0] next_word;
    input [31:0] address;
    input [1:0] count;
    begin next_word = {address[31:16], (address[15:0] + {14'b0, count})}; end
endfunction

wire core_address_matches = stream_valid && core_address == expected_core_address;
wire queue_head_matches = queue_count != 0 && downstream_head_offset == core_address[15:0];
wire restart = core_request_valid && (!core_address_matches ||
               (replay_remaining == 0 && queue_count != 0 && !queue_head_matches));
reg btc_hit;
reg [BTC_INDEX_BITS-1:0] hit_entry;
reg [BTC_INDEX_BITS-1:0] victim_entry;
reg victim_found;
integer lookup_index;
always @* begin
    btc_hit = 0;
    hit_entry = 0;
    victim_entry = 0;
    victim_found = 0;
    for (lookup_index = 0; lookup_index < BTC_ENTRIES; lookup_index = lookup_index + 1) begin
        if (restart && !flush && !reset && core_address[31:22] == 0 &&
            btc_valid[lookup_index] && btc_tag[lookup_index] == core_address[21:0]) begin
            btc_hit = 1;
            hit_entry = lookup_index[BTC_INDEX_BITS-1:0];
        end
        if (btc_oldest[lookup_index])
            victim_entry = lookup_index[BTC_INDEX_BITS-1:0];
    end
    // Lowest-numbered invalid slot first; do not evict on a partial fill.
    for (lookup_index = 0; lookup_index < BTC_ENTRIES; lookup_index = lookup_index + 1) begin
        if (!btc_valid[lookup_index] && !victim_found) begin
            victim_entry = lookup_index[BTC_INDEX_BITS-1:0];
            victim_found = 1;
        end
    end
end

wire btc_response = !reset && !flush && core_request_valid &&
    (btc_hit || (!restart && core_address_matches && replay_remaining != 0));
wire response_is_current = metadata_count != 0 && metadata_current[metadata_head];
wire response_bypass = !reset && !flush && !restart && !btc_response &&
    core_request_valid && core_address_matches && queue_count == 0 &&
    memory_response_valid && response_is_current && downstream_head_offset == core_address[15:0];
assign core_response_valid = !reset && !flush && core_request_valid &&
    (btc_response || (!restart && core_address_matches && (queue_head_matches || response_bypass)));
wire core_pop = core_response_valid && core_response_ready;
wire queue_pop = core_pop && !btc_response && !response_bypass;
wire bypass_pop = core_pop && response_bypass;
wire [BTC_INDEX_BITS-1:0] response_entry = btc_hit ? hit_entry : replay_entry;
wire first_btc_word = btc_hit || replay_remaining == 2;
wire [PAIR_BITS:0] payload_read_address = {entry_pair[response_entry], !first_btc_word};
assign core_request_ready = core_pop;
assign core_read_data = btc_response ?
    btc_payload[payload_read_address] :
    (response_bypass ? memory_read_data : queue_data[queue_head]);
assign core_error = btc_response ? 1'b0 :
    (response_bypass ? memory_error : queue_error[queue_head]);

wire [3:0] reserved_words = {1'b0, queue_count} + {1'b0, metadata_count};
assign memory_response_ready = !reset && metadata_count != 0 &&
    (flush || restart || !response_is_current || queue_count < QUEUE_DEPTH || queue_pop);
wire memory_response_fire = memory_response_valid && memory_response_ready;
wire redirect_slot_available = metadata_count < QUEUE_DEPTH || memory_response_fire;
assign memory_request_valid = !reset && !flush &&
    ((restart && redirect_slot_available) ||
     (!restart && stream_valid && reserved_words < QUEUE_DEPTH));
assign memory_address = restart ? next_word(core_address, btc_hit ? 2'd2 : 2'd0) : next_memory_address;
wire memory_request_fire = memory_request_valid && memory_request_ready;
wire enqueue_response = memory_response_fire && response_is_current && !flush && !restart && !bypass_pop;

wire install = !reset && !flush && !restart && core_pop && !core_error && fill_phase == 2 &&
    core_address == next_word({10'b0, fill_tag}, 2'd1);
// Read and write use separate physical ports. The write address always names
// the spare pair, so no visible entry can collide with an in-progress fill.
wire payload_first_write = !reset && !flush && !restart && core_pop && !core_error &&
    fill_phase == 1 && core_address == {10'b0, fill_tag};
wire [PAIR_BITS:0] payload_write_address = {spare_pair, install};
always @(posedge clk) begin
    if (payload_first_write || install)
        btc_payload[payload_write_address] <= core_read_data;
end
wire touch = !reset && !flush && core_pop && btc_response && first_btc_word;
wire [BTC_INDEX_BITS-1:0] touch_entry = install ? victim_entry : response_entry;
// Every ordinary core pop has matched both stream ownership and the
// downstream head. One successor therefore advances both cursors; restart
// uses that same increment only when it actually consumes the first word.
wire [31:0] core_successor = next_word(core_address, 2'd1);
wire [31:0] memory_successor = next_word(memory_address, 2'd1);
integer update_index;
integer newer_index;
integer reset_pair;
always @(posedge clk) begin
    if (reset) begin
        stream_valid <= 0;
        expected_core_address <= 0;
        next_memory_address <= 0;
        downstream_head_offset <= 0;
        queue_head <= 0;
        queue_tail <= 0;
        queue_count <= 0;
        metadata_current <= 0;
        metadata_head <= 0;
        metadata_tail <= 0;
        metadata_count <= 0;
        btc_valid <= 0;
        replay_entry <= 0;
        replay_remaining <= 0;
        fill_phase <= 0;
        fill_tag <= 0;
        spare_pair <= BTC_ENTRIES;
        for (reset_pair = 0; reset_pair < BTC_STORAGE; reset_pair = reset_pair + 1)
            entry_pair[reset_pair] <= reset_pair;
    end else begin
        if (flush) begin
            btc_valid <= 0;
            replay_remaining <= 0;
            fill_phase <= 0;
        end else begin
            if (restart) begin
                replay_remaining <= btc_hit ? (core_pop ? 2'd1 : 2'd2) : 2'd0;
                if (btc_hit) replay_entry <= hit_entry;
                fill_phase <= BTC_ENTRIES != 0 && core_address[31:22] == 0 && !btc_hit ? 2'd1 : 2'd0;
                fill_tag <= core_address[21:0];
            end else if (core_pop) begin
                if (btc_response) replay_remaining <= replay_remaining - 1'b1;
                if (fill_phase != 0 && core_error) fill_phase <= 0;
                else if (fill_phase == 1 && core_address == {10'b0, fill_tag}) begin
                    fill_phase <= 2;
                end else if (install) fill_phase <= 0;
            end
            if (install) begin
                btc_valid[victim_entry] <= 1;
                btc_tag[victim_entry] <= fill_tag;
                entry_pair[victim_entry] <= spare_pair;
                spare_pair <= entry_pair[victim_entry];
            end
            if (install || touch) begin
                for (update_index = 0; update_index < BTC_ENTRIES; update_index = update_index + 1) begin
                    for (newer_index=update_index+1;newer_index<BTC_ENTRIES;newer_index=newer_index+1) begin
                        if (update_index==touch_entry)
                            btc_newer[relation_index(update_index,newer_index)]<=1'b1;
                        else if (newer_index==touch_entry)
                            btc_newer[relation_index(update_index,newer_index)]<=1'b0;
                    end
                end
            end
        end

        if (flush || restart) begin
            metadata_current <= 0;
            queue_head <= 0;
            queue_tail <= 0;
            queue_count <= 0;
            stream_valid <= core_request_valid;
            if (core_request_valid)
                downstream_head_offset <= flush ? core_address[15:0] : memory_address[15:0];
            if (core_request_valid) begin
                expected_core_address <= core_pop ? core_successor : core_address;
                if (flush) next_memory_address <= core_address;
                else next_memory_address <= memory_request_fire ? memory_successor : memory_address;
            end
        end else begin
            if (core_pop) expected_core_address <= core_successor;
            if (queue_pop || bypass_pop) downstream_head_offset <= core_successor[15:0];
            if (queue_pop) queue_head <= queue_head + 1'b1;
            if (enqueue_response) begin
                queue_data[queue_tail] <= memory_read_data;
                queue_error[queue_tail] <= memory_error;
                queue_tail <= queue_tail + 1'b1;
            end
            case ({enqueue_response, queue_pop})
                2'b10: queue_count <= queue_count + 1'b1;
                2'b01: queue_count <= queue_count - 1'b1;
                default: queue_count <= queue_count;
            endcase
            if (memory_request_fire) next_memory_address <= memory_successor;
        end
        if (memory_response_fire) begin
            metadata_current[metadata_head] <= 0;
            metadata_head <= metadata_head + 1'b1;
        end
        if (memory_request_fire) begin
            metadata_current[metadata_tail] <= 1;
            metadata_tail <= metadata_tail + 1'b1;
        end
        case ({memory_request_fire, memory_response_fire})
            2'b10: metadata_count <= metadata_count + 1'b1;
            2'b01: metadata_count <= metadata_count - 1'b1;
            default: metadata_count <= metadata_count;
        endcase
    end
end
endmodule
