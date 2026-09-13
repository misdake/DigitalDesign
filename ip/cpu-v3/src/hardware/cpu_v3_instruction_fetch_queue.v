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

reg stream_valid = 0;
reg [31:0] expected_core_address = 0;
reg [31:0] next_memory_address = 0;

// These small FIFOs and the BTC intentionally use FFs, not scarce RAM16 cells.
(* syn_ramstyle = "registers" *) reg [15:0] queue_data [0:3];
(* syn_ramstyle = "registers" *) reg queue_error [0:3];
(* syn_ramstyle = "registers" *) reg [31:0] queue_address [0:3];
reg [1:0] queue_head = 0;
reg [1:0] queue_tail = 0;
reg [2:0] queue_count = 0;

// Clear live ownership on every restart. Unlike a toggled epoch, these bits
// cannot alias an old request after a sequence of fast BTC redirects.
reg [3:0] metadata_current = 0;
(* syn_ramstyle = "registers" *) reg [31:0] metadata_address [0:3];
reg [1:0] metadata_head = 0;
reg [1:0] metadata_tail = 0;
reg [2:0] metadata_count = 0;

reg [BTC_STORAGE-1:0] btc_valid = 0;
(* syn_ramstyle = "registers" *) reg [21:0] btc_tag [0:BTC_STORAGE-1];
(* syn_ramstyle = "registers" *) reg [15:0] btc_word0 [0:BTC_STORAGE-1];
(* syn_ramstyle = "registers" *) reg [15:0] btc_word1 [0:BTC_STORAGE-1];
(* syn_ramstyle = "registers" *) reg [BTC_INDEX_BITS-1:0] btc_rank [0:BTC_STORAGE-1];
reg [BTC_INDEX_BITS-1:0] replay_entry = 0;
reg [1:0] replay_remaining = 0;
reg [1:0] fill_phase = 0;
reg [21:0] fill_tag = 0;
reg [15:0] fill_word = 0;

function [31:0] next_word;
    input [31:0] address;
    input [1:0] count;
    begin next_word = {address[31:16], (address[15:0] + {14'b0, count})}; end
endfunction

wire core_address_matches = stream_valid && core_address == expected_core_address;
wire queue_head_matches = queue_count != 0 && queue_address[queue_head] == core_address;
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
        if (btc_rank[lookup_index] == BTC_ENTRIES - 1)
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
    memory_response_valid && response_is_current && metadata_address[metadata_head] == core_address;
assign core_response_valid = !reset && !flush && core_request_valid &&
    (btc_response || (!restart && core_address_matches && (queue_head_matches || response_bypass)));
wire core_pop = core_response_valid && core_response_ready;
wire queue_pop = core_pop && !btc_response && !response_bypass;
wire bypass_pop = core_pop && response_bypass;
wire [BTC_INDEX_BITS-1:0] response_entry = btc_hit ? hit_entry : replay_entry;
wire first_btc_word = btc_hit || replay_remaining == 2;
assign core_request_ready = core_pop;
assign core_read_data = btc_response ?
    (first_btc_word ? btc_word0[response_entry] : btc_word1[response_entry]) :
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
wire touch = !reset && !flush && core_pop && btc_response && first_btc_word;
wire [BTC_INDEX_BITS-1:0] touch_entry = install ? victim_entry : response_entry;
integer update_index;
always @(posedge clk) begin
    if (reset) begin
        stream_valid <= 0;
        expected_core_address <= 0;
        next_memory_address <= 0;
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
        fill_word <= 0;
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
                    fill_word <= core_read_data;
                    fill_phase <= 2;
                end else if (install) fill_phase <= 0;
            end
            if (install) begin
                btc_valid[victim_entry] <= 1;
                btc_tag[victim_entry] <= fill_tag;
                btc_word0[victim_entry] <= fill_word;
                btc_word1[victim_entry] <= core_read_data;
            end
            if (install || touch) begin
                for (update_index = 0; update_index < BTC_ENTRIES; update_index = update_index + 1) begin
                    if (update_index == touch_entry) btc_rank[update_index] <= 0;
                    else if (btc_valid[update_index] && (install || btc_rank[update_index] < btc_rank[touch_entry])) begin
                        if (btc_rank[update_index] != BTC_ENTRIES - 1)
                            btc_rank[update_index] <= btc_rank[update_index] + 1'b1;
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
            if (core_request_valid) begin
                expected_core_address <= next_word(core_address, core_pop ? 2'd1 : 2'd0);
                if (flush) next_memory_address <= core_address;
                else next_memory_address <= next_word(memory_address, memory_request_fire ? 2'd1 : 2'd0);
            end
        end else begin
            if (core_pop) expected_core_address <= next_word(expected_core_address, 2'd1);
            if (queue_pop) queue_head <= queue_head + 1'b1;
            if (enqueue_response) begin
                queue_data[queue_tail] <= memory_read_data;
                queue_error[queue_tail] <= memory_error;
                queue_address[queue_tail] <= metadata_address[metadata_head];
                queue_tail <= queue_tail + 1'b1;
            end
            case ({enqueue_response, queue_pop})
                2'b10: queue_count <= queue_count + 1'b1;
                2'b01: queue_count <= queue_count - 1'b1;
                default: queue_count <= queue_count;
            endcase
            if (memory_request_fire) next_memory_address <= next_word(memory_address, 2'd1);
        end
        if (memory_response_fire) begin
            metadata_current[metadata_head] <= 0;
            metadata_head <= metadata_head + 1'b1;
        end
        if (memory_request_fire) begin
            metadata_current[metadata_tail] <= 1;
            metadata_address[metadata_tail] <= memory_address;
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
