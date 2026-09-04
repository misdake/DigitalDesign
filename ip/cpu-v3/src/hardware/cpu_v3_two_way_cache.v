module CpuV3TwoWayCache (
    input wire clk,
    input wire reset,
    input wire invalidate_all,
    input wire prefetch_request_valid,
    input wire [31:0] prefetch_address,
    input wire prefetch_cancel,
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
    output wire [31:0] prefetch_issued,
    output wire [31:0] prefetch_useful,
    output wire [31:0] prefetch_useless,
    output wire [31:0] prefetch_dropped
);

// A read miss issues one aligned line request and receives exactly four
// ordered 64-bit beats; beat n carries words 4*n through 4*n+3. A write
// issues one word request and receives one
// completion response. An error beat terminates a line response early; no
// further beats follow it. Each line beat writes four words directly through
// the two ports of each parity bank. The victim remains invalid until the
// final error-free beat commits its tag.

localparam [3:0] ST_IDLE = 0;
localparam [3:0] ST_WORD_REQUEST = 3;
localparam [3:0] ST_WORD_RESPONSE = 4;
localparam [3:0] ST_LINE_REQUEST = 5;
localparam [3:0] ST_LINE_RECEIVE = 6;

reg [3:0] state = ST_IDLE;
reg lookup_valid = 0;
reg pending_is_prefetch = 0;
reg pending_write = 0;
reg [31:0] pending_address = 0;
reg [15:0] pending_write_data = 0;
reg [2:0] refill_beat = 0;
reg pending_way = 0;
reg [15:0] refill_response_data = 0;
reg [15:0] response_data = 0;
reg response_error = 0;
reg response_valid = 0;
reg refill_discard = 0;
// A prefetch that has committed its line request to the arbiter is "armed".
// The unarmed first cycle of ST_LINE_REQUEST remains stealable by a demand
// request, which is how demand wins priority without routing the demand or
// cancel signals through the combinational memory_request_valid output.
reg prefetch_armed = 0;
reg prefetch_pending = 0;
reg [31:0] prefetch_pending_address = 0;
reg [63:0] way_0_valid = __INITIAL_VALID__;
reg [63:0] way_1_valid = 0;
reg [63:0] victim = 0;
reg [63:0] way_0_prefetched = 0;
reg [63:0] way_1_prefetched = 0;

// These counters have no output fanout and are consequently swept from a
// production fit. They remain visible to RTL simulation and debug probes.
reg [31:0] prefetch_issued_count = 0;
reg [31:0] prefetch_useful_count = 0;
reg [31:0] prefetch_useless_count = 0;
reg [31:0] prefetch_dropped_count = 0;
assign prefetch_issued = prefetch_issued_count;
assign prefetch_useful = prefetch_useful_count;
assign prefetch_useless = prefetch_useless_count;
assign prefetch_dropped = prefetch_dropped_count;

function [7:0] count_prefetched;
    input [127:0] bits;
    integer index;
    begin
        count_prefetched = 0;
        for (index = 0; index < 128; index = index + 1)
            count_prefetched = count_prefetched + bits[index];
    end
endfunction

wire pending_address_valid = pending_address[31:22] == 0;

wire [5:0] pending_set = pending_address[9:4];
wire [11:0] pending_tag = pending_address[21:10];
wire [3:0] pending_word = pending_address[3:0];
wire [11:0] way_0_tag_read_data;
wire [11:0] way_1_tag_read_data;
wire way_0_hit = way_0_valid[pending_set] && way_0_tag_read_data == pending_tag;
wire way_1_hit = way_1_valid[pending_set] && way_1_tag_read_data == pending_tag;
wire pending_hit = way_0_hit || way_1_hit;
wire hit_way = !way_0_hit && way_1_hit;
wire selected_victim = !way_0_valid[pending_set] ? 1'b0 :
                       !way_1_valid[pending_set] ? 1'b1 : victim[pending_set];
wire prefetch_refill_cancelled = pending_is_prefetch && prefetch_cancel;
wire refill_commit = state == ST_LINE_RECEIVE && memory_response_valid &&
                        !memory_error && refill_beat == 3 &&
                        !refill_discard && !invalidate_all &&
                        !prefetch_refill_cancelled;
wire tag_write_enable = refill_commit;

__CACHE_TAGS__ u_tags (
    .clk(clk),
    .write_enable(tag_write_enable),
    .write_way(pending_way),
    .address(pending_set),
    .write_data(pending_tag),
    .way_0_read_data(way_0_tag_read_data),
    .way_1_read_data(way_1_tag_read_data)
);

wire response_space = !response_valid || cpu_response_ready;
wire hit_write = state == ST_IDLE && lookup_valid && pending_write && pending_hit &&
                 response_space && !invalidate_all;
wire refill_write = state == ST_LINE_RECEIVE && memory_response_valid && !memory_error &&
                    !refill_discard && !invalidate_all && !prefetch_refill_cancelled;
// Start both candidate-way reads on the request-acceptance edge. The data and
// parallel tag comparison are therefore ready when the lookup resolves on the
// following cycle.
wire lookup_read_hit = lookup_valid && pending_address_valid &&
                       !pending_write && pending_hit;
wire cancel_prefetch = prefetch_cancel || invalidate_all;
wire steal_unissued_prefetch = state == ST_LINE_REQUEST &&
                               pending_is_prefetch && !prefetch_armed;
assign cpu_request_ready = !invalidate_all &&
    ((state == ST_IDLE &&
      (!lookup_valid || (pending_is_prefetch || lookup_read_hit) && response_space)) ||
     steal_unissued_prefetch);
wire accept_cpu_request = cpu_request_valid && cpu_request_ready;
wire accept_prefetch = state == ST_IDLE && !invalidate_all && !cancel_prefetch &&
                       !lookup_valid && !response_valid && prefetch_pending &&
                       !cpu_request_valid;
wire [31:0] cache_lookup_address = accept_cpu_request ? cpu_address :
                                   accept_prefetch ? prefetch_pending_address :
                                   pending_address;
wire [5:0] cache_lookup_set = cache_lookup_address[9:4];
wire [3:0] cache_lookup_word = cache_lookup_address[3:0];
wire [9:0] lookup_way_0_address = {1'b0, cache_lookup_set, cache_lookup_word[3:1]};
wire [9:0] lookup_way_1_address = {1'b1, cache_lookup_set, cache_lookup_word[3:1]};
wire [9:0] refill_address_a = {pending_way, pending_set, refill_beat[1:0], 1'b0};
wire [9:0] refill_address_b = {pending_way, pending_set, refill_beat[1:0], 1'b1};
wire [9:0] hit_write_address = {hit_way, pending_set, pending_word[3:1]};
wire [15:0] bank_0_a_read_data, bank_0_b_read_data;
wire [15:0] bank_1_a_read_data, bank_1_b_read_data;
wire [15:0] way_0_cache_read_data = pending_word[0] ?
    bank_1_a_read_data : bank_0_a_read_data;
wire [15:0] way_1_cache_read_data = pending_word[0] ?
    bank_1_b_read_data : bank_0_b_read_data;
wire [15:0] cache_read_data = hit_way ? way_1_cache_read_data : way_0_cache_read_data;

__CACHE_DATA_BANKS__ u_data_banks (
    .clk(clk),
    .bank_0_a_write_enable(refill_write || (hit_write && !pending_word[0])),
    .bank_0_a_address(refill_write ? refill_address_a :
        (hit_write && !pending_word[0] ? hit_write_address : lookup_way_0_address)),
    .bank_0_a_write_data(refill_write ? memory_read_data[15:0] : pending_write_data),
    .bank_0_a_read_data(bank_0_a_read_data),
    .bank_0_b_write_enable(refill_write),
    .bank_0_b_address(refill_write ? refill_address_b : lookup_way_1_address),
    .bank_0_b_write_data(memory_read_data[47:32]),
    .bank_0_b_read_data(bank_0_b_read_data),
    .bank_1_a_write_enable(refill_write || (hit_write && pending_word[0])),
    .bank_1_a_address(refill_write ? refill_address_a :
        (hit_write && pending_word[0] ? hit_write_address : lookup_way_0_address)),
    .bank_1_a_write_data(refill_write ? memory_read_data[31:16] : pending_write_data),
    .bank_1_a_read_data(bank_1_a_read_data),
    .bank_1_b_write_enable(refill_write),
    .bank_1_b_address(refill_write ? refill_address_b : lookup_way_1_address),
    .bank_1_b_write_data(memory_read_data[63:48]),
    .bank_1_b_read_data(bank_1_b_read_data)
);

assign cpu_response_valid = response_valid;
assign cpu_read_data = response_data;
assign cpu_error = response_valid && response_error;
assign memory_request_valid = state == ST_WORD_REQUEST ||
    (state == ST_LINE_REQUEST && (!pending_is_prefetch || prefetch_armed));
assign memory_write = pending_write;
assign memory_line = !pending_write;
assign memory_address = pending_write ? pending_address[21:0] :
                        {pending_address[21:4], 4'b0};
assign memory_write_data = {48'b0, pending_write_data};
assign memory_response_ready = state == ST_WORD_RESPONSE || state == ST_LINE_RECEIVE;

always @(posedge clk) begin
    if (reset) begin
        state <= ST_IDLE;
        lookup_valid <= 0;
        pending_is_prefetch <= 0;
        way_0_valid <= __INITIAL_VALID__;
        way_1_valid <= 0;
        victim <= 0;
        way_0_prefetched <= 0;
        way_1_prefetched <= 0;
        response_error <= 0;
        response_valid <= 0;
        refill_discard <= 0;
        prefetch_armed <= 0;
        prefetch_pending <= 0;
        prefetch_pending_address <= 0;
        prefetch_issued_count <= 0;
        prefetch_useful_count <= 0;
        prefetch_useless_count <= 0;
        prefetch_dropped_count <= 0;
    end else begin
        if (response_valid && cpu_response_ready)
            response_valid <= 0;
        if (!cancel_prefetch && prefetch_request_valid) begin
            if (prefetch_pending && prefetch_pending_address != prefetch_address)
                prefetch_dropped_count <= prefetch_dropped_count + 1'b1;
            prefetch_pending <= 1;
            prefetch_pending_address <= prefetch_address;
        end
        case (state)
            ST_IDLE: begin
                if (lookup_valid && (pending_is_prefetch || response_space) &&
                    !(cancel_prefetch && pending_is_prefetch)) begin
                    if (!pending_address_valid) begin
                        if (pending_is_prefetch)
                            prefetch_dropped_count <= prefetch_dropped_count + 1'b1;
                        else begin
                            response_data <= 0;
                            response_error <= 1;
                            response_valid <= 1;
                        end
                        lookup_valid <= 0;
                        if (pending_is_prefetch)
                            pending_is_prefetch <= 0;
                    end else if (pending_write) begin
                        lookup_valid <= 0;
                        state <= ST_WORD_REQUEST;
                    end else if (pending_hit) begin
                        if (!pending_is_prefetch) begin
                            response_data <= cache_read_data;
                            response_error <= 0;
                            response_valid <= 1;
                            if (hit_way && way_1_prefetched[pending_set]) begin
                                way_1_prefetched[pending_set] <= 0;
                                prefetch_useful_count <= prefetch_useful_count + 1'b1;
                            end else if (!hit_way && way_0_prefetched[pending_set]) begin
                                way_0_prefetched[pending_set] <= 0;
                                prefetch_useful_count <= prefetch_useful_count + 1'b1;
                            end
                        end
                        lookup_valid <= 0;
                        if (pending_is_prefetch)
                            pending_is_prefetch <= 0;
                    end else begin
                        lookup_valid <= 0;
                        if (pending_is_prefetch && accept_cpu_request) begin
                            pending_is_prefetch <= 0;
                            prefetch_dropped_count <= prefetch_dropped_count + 1'b1;
                        end else begin
                            pending_way <= selected_victim;
                            refill_beat <= 0;
                            // An invalidate coincident with miss detection
                            // belongs to the old fetch epoch. Complete its
                            // protocol response, but never install the line.
                            refill_discard <= invalidate_all;
                            prefetch_armed <= 0;
                            state <= ST_LINE_REQUEST;
                        end
                    end
                end
            end

            ST_WORD_REQUEST: if (memory_request_ready)
                state <= ST_WORD_RESPONSE;

            ST_WORD_RESPONSE: if (memory_response_valid) begin
                response_data <= 0;
                response_error <= memory_error;
                response_valid <= 1;
                state <= ST_IDLE;
            end

            ST_LINE_REQUEST: begin
                if (pending_is_prefetch && !prefetch_armed) begin
                    if (cpu_request_valid || cancel_prefetch) begin
                        if (cpu_request_valid && !cancel_prefetch)
                            prefetch_dropped_count <= prefetch_dropped_count + 1'b1;
                        pending_is_prefetch <= 0;
                        prefetch_armed <= 0;
                        refill_discard <= 0;
                        state <= ST_IDLE;
                    end else begin
                        prefetch_armed <= 1;
                    end
                end else if (memory_request_ready) begin
                    if (pending_way) way_1_valid[pending_set] <= 0;
                    else way_0_valid[pending_set] <= 0;
                    refill_beat <= 0;
                    if (pending_is_prefetch)
                        prefetch_issued_count <= prefetch_issued_count + 1'b1;
                    prefetch_armed <= 0;
                    state <= ST_LINE_RECEIVE;
                end
            end

            ST_LINE_RECEIVE: if (memory_response_valid) begin
                if (memory_error) begin
                    if (pending_is_prefetch) begin
                        if (!refill_discard)
                            prefetch_dropped_count <= prefetch_dropped_count + 1'b1;
                        pending_is_prefetch <= 0;
                    end else begin
                        response_data <= 0;
                        response_error <= 1;
                        response_valid <= 1;
                    end
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
                    if (!refill_discard && !invalidate_all &&
                        !prefetch_refill_cancelled) begin
                        if (pending_way) begin
                            if (way_1_prefetched[pending_set])
                                prefetch_useless_count <= prefetch_useless_count + 1'b1;
                            way_1_valid[pending_set] <= 1;
                            way_1_prefetched[pending_set] <= pending_is_prefetch;
                        end else begin
                            if (way_0_prefetched[pending_set])
                                prefetch_useless_count <= prefetch_useless_count + 1'b1;
                            way_0_valid[pending_set] <= 1;
                            way_0_prefetched[pending_set] <= pending_is_prefetch;
                        end
                        victim[pending_set] <= !pending_way;
                    end
                    if (!pending_is_prefetch) begin
                        response_data <= pending_word[3:2] == 3 ?
                            (pending_word[1:0] == 0 ? memory_read_data[15:0] :
                             pending_word[1:0] == 1 ? memory_read_data[31:16] :
                             pending_word[1:0] == 2 ? memory_read_data[47:32] : memory_read_data[63:48]) :
                            refill_response_data;
                        response_error <= 0;
                        response_valid <= 1;
                    end
                    pending_is_prefetch <= 0;
                    state <= ST_IDLE;
                    end else refill_beat <= refill_beat + 1'b1;
                end
            end

            default: state <= ST_IDLE;
        endcase

        if (accept_prefetch) begin
            pending_is_prefetch <= 1;
            pending_write <= 0;
            pending_address <= prefetch_pending_address;
            pending_write_data <= 0;
            refill_discard <= 0;
            lookup_valid <= 1;
            prefetch_pending <= 0;
        end
        if (accept_cpu_request) begin
            pending_is_prefetch <= 0;
            pending_write <= cpu_write;
            pending_address <= cpu_address;
            pending_write_data <= cpu_write_data;
            response_error <= 0;
            refill_discard <= 0;
            lookup_valid <= 1;
        end

        if (cancel_prefetch) begin
            if (prefetch_pending ||
                (pending_is_prefetch &&
                 ((state == ST_IDLE && lookup_valid) ||
                  (state == ST_LINE_REQUEST && !prefetch_armed) ||
                  ((state == ST_LINE_RECEIVE ||
                    (state == ST_LINE_REQUEST && prefetch_armed)) &&
                   !refill_discard))))
                prefetch_dropped_count <= prefetch_dropped_count + 1'b1;
            prefetch_pending <= 0;
            if (pending_is_prefetch && state == ST_IDLE && lookup_valid &&
                !accept_cpu_request) begin
                lookup_valid <= 0;
                pending_is_prefetch <= 0;
            end
            if (pending_is_prefetch && state == ST_LINE_REQUEST && !prefetch_armed) begin
                state <= ST_IDLE;
                pending_is_prefetch <= 0;
                prefetch_armed <= 0;
                refill_discard <= 0;
            end else if (pending_is_prefetch &&
                         (state == ST_LINE_RECEIVE ||
                          (state == ST_LINE_REQUEST && prefetch_armed))) begin
                refill_discard <= 1;
            end
        end

        if (invalidate_all) begin
            prefetch_useless_count <= prefetch_useless_count +
                count_prefetched({way_1_prefetched, way_0_prefetched});
            way_0_valid <= 0;
            way_1_valid <= 0;
            way_0_prefetched <= 0;
            way_1_prefetched <= 0;
            if (state == ST_LINE_REQUEST || state == ST_LINE_RECEIVE)
                refill_discard <= 1;
        end
    end
end

endmodule
