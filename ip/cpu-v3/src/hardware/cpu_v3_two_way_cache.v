module CpuV3TwoWayCache (
    input wire clk,
    input wire reset,
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
    output wire memory_response_ready
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
// Valid and victim bits live in a RAM16 leaf (asynchronous read, synchronous
// write, one set per access), like the tag arrays. invalidate_all cannot
// clear RAM in one cycle: it starts a 64-set sweep that clears both ways in
// parallel. New lookups are blocked through cpu_request_ready while the sweep
// (or the invalidate pulse) is active, which preserves the all-invalid
// semantics of the old flip-flop clear.
reg sweep_active = 0;
reg [5:0] sweep_set = 0;
wire way_0_valid_read;
wire way_1_valid_read;
wire victim_read;

wire pending_address_valid = pending_address[31:22] == 0;

wire [5:0] pending_set = pending_address[9:4];
wire [11:0] pending_tag = pending_address[21:10];
wire [3:0] pending_word = pending_address[3:0];
wire [11:0] way_0_tag_read_data;
wire [11:0] way_1_tag_read_data;
wire invalidating = invalidate_all || sweep_active;
// The sweep blocks new requests through cpu_request_ready, so hit
// qualification does not need its own `!invalidating` term. Keeping the gate
// off this expression removes a LUT level from the tight fetch-frontend to
// I-cache way-valid path that the old instant valid clear did not pay.
wire way_0_hit = way_0_valid_read && way_0_tag_read_data == pending_tag;
wire way_1_hit = way_1_valid_read && way_1_tag_read_data == pending_tag;
wire pending_hit = way_0_hit || way_1_hit;
wire hit_way = !way_0_hit && way_1_hit;
wire selected_victim = !way_0_valid_read ? 1'b0 :
                       !way_1_valid_read ? 1'b1 : victim_read;
wire refill_commit = state == ST_LINE_RECEIVE && memory_response_valid &&
                        !memory_error && refill_beat == 3 &&
                        !refill_discard && !invalidating;
wire tag_write_enable = refill_commit;
// Single valid-array write port: the sweep has priority; otherwise the
// line-request issue clears the victim way and a refill commit installs it.
wire valid_write_enable = !sweep_active &&
    (refill_commit || (state == ST_LINE_REQUEST && memory_request_ready));

__CACHE_VALID__ u_valid (
    .clk(clk),
    .clear_enable(sweep_active),
    .clear_set(sweep_set),
    .write_enable(valid_write_enable),
    .write_way(pending_way),
    .write_set(pending_set),
    .write_value(refill_commit),
    .victim_write_enable(refill_commit),
    .victim_write_value(!pending_way),
    .read_set(pending_set),
    .way_0_valid(way_0_valid_read),
    .way_1_valid(way_1_valid_read),
    .victim(victim_read)
);

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
                 response_space && !invalidating;
wire refill_write = state == ST_LINE_RECEIVE && memory_response_valid && !memory_error &&
                    !refill_discard && !invalidating;
// Start both candidate-way reads on the request-acceptance edge. The data and
// parallel tag comparison are therefore ready when the lookup resolves on the
// following cycle.
wire lookup_read_hit = lookup_valid && pending_address_valid &&
                       !pending_write && pending_hit;
assign cpu_request_ready = !invalidate_all && !sweep_active &&
    (state == ST_IDLE && (!lookup_valid || lookup_read_hit && response_space));
wire accept_cpu_request = cpu_request_valid && cpu_request_ready;
wire [31:0] cache_lookup_address = accept_cpu_request ? cpu_address :
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
assign memory_request_valid = state == ST_WORD_REQUEST || state == ST_LINE_REQUEST;
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
        response_error <= 0;
        response_valid <= 0;
        refill_discard <= 0;
        sweep_active <= 0;
        sweep_set <= 0;
    end else begin
        if (response_valid && cpu_response_ready)
            response_valid <= 0;
        if (invalidate_all) begin
            sweep_active <= 1;
            sweep_set <= 0;
        end else if (sweep_active) begin
            sweep_set <= sweep_set + 1'b1;
            if (sweep_set == 63)
                sweep_active <= 0;
        end
        case (state)
            ST_IDLE: begin
                if (lookup_valid && response_space) begin
                    if (!pending_address_valid) begin
                        response_data <= 0;
                        response_error <= 1;
                        response_valid <= 1;
                        lookup_valid <= 0;
                    end else if (pending_write) begin
                        lookup_valid <= 0;
                        state <= ST_WORD_REQUEST;
                    end else if (pending_hit) begin
                        response_data <= cache_read_data;
                        response_error <= 0;
                        response_valid <= 1;
                        lookup_valid <= 0;
                    end else begin
                        lookup_valid <= 0;
                        pending_way <= selected_victim;
                        refill_beat <= 0;
                        // An invalidate coincident with miss detection
                        // belongs to the old fetch epoch. Complete its
                        // protocol response, but never install the line.
                        refill_discard <= invalidate_all;
                        state <= ST_LINE_REQUEST;
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
                if (memory_request_ready) begin
                    refill_beat <= 0;
                    state <= ST_LINE_RECEIVE;
                end
            end

            ST_LINE_RECEIVE: if (memory_response_valid) begin
                if (memory_error) begin
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
                    response_data <= pending_word[3:2] == 3 ?
                        (pending_word[1:0] == 0 ? memory_read_data[15:0] :
                         pending_word[1:0] == 1 ? memory_read_data[31:16] :
                         pending_word[1:0] == 2 ? memory_read_data[47:32] : memory_read_data[63:48]) :
                        refill_response_data;
                    response_error <= 0;
                    response_valid <= 1;
                    state <= ST_IDLE;
                    end else refill_beat <= refill_beat + 1'b1;
                end
            end

            default: state <= ST_IDLE;
        endcase

        if (accept_cpu_request) begin
            pending_write <= cpu_write;
            pending_address <= cpu_address;
            pending_write_data <= cpu_write_data;
            response_error <= 0;
            refill_discard <= 0;
            lookup_valid <= 1;
        end

        if (invalidate_all) begin
            if (state == ST_LINE_REQUEST || state == ST_LINE_RECEIVE)
                refill_discard <= 1;
        end
    end
end

endmodule
