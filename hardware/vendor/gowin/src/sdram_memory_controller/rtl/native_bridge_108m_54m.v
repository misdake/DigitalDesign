// One-outstanding, related-clock transport between 54 MHz 64-bit clients and
// the 108 MHz native 32-bit SDRAM controller. Both clocks come from one PLL.
// No SDRAM row/refresh policy or transaction-wide data buffer lives here.
module TangNano20KSdramNativeBridge108M54M #(
    // BANK_BIT=5 stripes 128-byte sectors over four banks; 19 keeps contiguous banks.
    parameter PREPARE_NEXT = 0,
    parameter CHAIN_GROUP_FOUR = 0,
    parameter BANK_BIT = 5
) (
    input wire logic_clk,
    input wire controller_clk,
    input wire sdram_clk,
    input wire reset,
    input wire request_valid,
    input wire next_valid,
    input wire [20:0] next_address,
    input wire writing,
    input wire [20:0] address,
    input wire [5:0] words,
    input wire [3:0] write_mask,
    input wire [63:0] write_data,
    input wire write_data_valid,
    output wire request_ready,
    output wire stream_active,
    output wire write_data_ready,
    output reg [63:0] read_data = 0,
    output reg read_valid = 0,
    output reg done = 0,
    output wire initialized,
    output wire O_sdram_clk,
    output wire O_sdram_cke,
    output wire O_sdram_cs_n,
    output wire O_sdram_cas_n,
    output wire O_sdram_ras_n,
    output wire O_sdram_wen_n,
    output wire [3:0] O_sdram_dqm,
    output wire [10:0] O_sdram_addr,
    output wire [1:0] O_sdram_ba,
    inout wire [31:0] IO_sdram_dq
);
    reg occupied_54 = 0;
    reg request_toggle_54 = 0;
    reg writing_54 = 0;
    reg [20:0] address_54 = 0;
    reg [7:0] words_54 = 0;
    reg [3:0] mask_54 = 0;
    reg [7:0] pairs_fed_54 = 0;
    reg [63:0] pair_head_54 = 0;
    reg [63:0] pair_next_54 = 0;
    reg [1:0] queued_54 = 0;
    reg done_seen_54 = 0;
    reg read_seen_54 = 0;
    reg init_54 = 0;
    reg stream_active_54 = 0;
    reg cpu_edge_toggle_54 = 0;

    reg request_seen_108 = 0;
    reg next_valid_108 = 0;
    reg [20:0] next_address_108 = 0;
    reg done_event_108 = 0;
    reg done_toggle_108 = 0;
    reg read_event_108 = 0;
    reg read_toggle_108 = 0;
    reg cpu_edge_seen_108 = 0;
    reg slot_detect_108 = 0;
    reg slot_pipe_108 = 0;
    reg write_slot_108 = 0;
    reg [31:0] read_low_108 = 0;
    reg [63:0] read_pair_108 = 0;
    reg read_half_108 = 0;
    // Code zero expands to 128 words only for an explicitly admitted group.
    // The source/sink covers the whole group, so a segment boundary does not
    // depend on returning an ack through the arbiter before supplying data.
    reg [1:0] segment_108 = 0;
    reg restart_108 = 0;
    reg group_next_valid_108 = 0;
    reg [4:0] read_word_108 = 0;
    // Snapshot the stable 54 MHz descriptor before letting it enter the core
    // combinational admission/chain paths. This is a related-clock header
    // pipeline, not an asynchronous CDC or a timing exception.
    reg descriptor_toggle_108 = 0, group_108 = 0, writing_108 = 0;
    reg [20:0] address_108 = 0;
    reg [5:0] words_108 = 0;
    reg [3:0] mask_108 = 0;
    wire group_active = CHAIN_GROUP_FOUR && group_108;
    wire [20:0] core_base_address = CHAIN_GROUP_FOUR ? address_108 : address_54;
    wire core_writing = CHAIN_GROUP_FOUR ? writing_108 : writing_54;
    // Group alignment makes this pure wiring, not a 21-bit carry path.
    wire [20:0] group_next_address = {core_base_address[20:7], (segment_108 + 2'd1), 5'd0};
    // synthesis translate_off
    always @(posedge logic_clk) if(!reset && request_valid && request_ready &&
        CHAIN_GROUP_FOUR && words == 0 && address[6:0] != 0)
        $fatal(1, "native four-sector group must be 512-byte aligned");
    // synthesis translate_on

    wire core_ready;
    wire core_done;
    wire core_initialized;
    wire [31:0] core_read_data;
    wire core_read_valid;
    wire [5:0] core_write_index;
    wire [7:0] core_phase;
    wire core_chain_accept, core_read_chain_accept;
    wire request_pending_108 = (CHAIN_GROUP_FOUR ? descriptor_toggle_108 : request_toggle_54) != request_seen_108;
    wire core_request = (request_pending_108 || restart_108) && (!core_writing || queued_54 != 0);
    wire [20:0] core_address = restart_108 ? group_next_address : core_base_address;
    wire [5:0] core_words = CHAIN_GROUP_FOUR ? words_108 : words_54[5:0];
    wire [31:0] core_write_data = core_write_index[0] ?
        pair_head_54[63:32] : pair_head_54[31:0];
    wire high_word_54 = core_phase[4:0] == 5'd15 && core_write_index[0];
    wire pop_pair_54 = occupied_54 && writing_54 && high_word_54 && queued_54 != 0;
    wire pair_needed_54 = occupied_54 && writing_54 &&
        pairs_fed_54 < ((words_54 + 8'd1) >> 1);
    wire push_pair_54 = write_data_valid && write_data_ready;

    assign initialized = init_54;
    assign stream_active = stream_active_54;
    assign request_ready = init_54 && !occupied_54 && !reset;
    // Keep the 108 MHz word index out of the GPU/arbiter ready path. Two
    // 64-bit entries allow the source to get ahead of WRITE_LAUNCH while the
    // head pair is consumed low/high without a bubble.
    assign write_data_ready = pair_needed_54 && queued_54 < 2;

    always @(posedge logic_clk) begin
        if (reset) begin
            occupied_54 <= 0;
            request_toggle_54 <= 0;
            pairs_fed_54 <= 0;
            queued_54 <= 0;
            done_seen_54 <= 0;
            read_seen_54 <= 0;
            init_54 <= 0;
            stream_active_54 <= 0;
            cpu_edge_toggle_54 <= 0;
            done <= 0;
            read_valid <= 0;
        end else begin
            cpu_edge_toggle_54 <= !cpu_edge_toggle_54;
            init_54 <= core_initialized;
            stream_active_54 <= occupied_54 &&
                (core_phase[4:0] == 5'd15 || core_phase[4:0] == 5'd18);
            done <= 0;
            read_valid <= 0;
            if (request_valid && request_ready) begin
                occupied_54 <= 1;
                request_toggle_54 <= !request_toggle_54;
                writing_54 <= writing;
                address_54 <= address;
                words_54 <= CHAIN_GROUP_FOUR && words == 0 ? 8'd128 : {2'b0, words};
                mask_54 <= write_mask;
                pairs_fed_54 <= 0;
                queued_54 <= 0;
            end
            if (push_pair_54)
                pairs_fed_54 <= pairs_fed_54 + 1'b1;
            if (push_pair_54 && !pop_pair_54) begin
                if (queued_54 == 0)
                    pair_head_54 <= write_data;
                else
                    pair_next_54 <= write_data;
                queued_54 <= queued_54 + 1'b1;
            end else if (pop_pair_54 && !push_pair_54) begin
                pair_head_54 <= pair_next_54;
                queued_54 <= queued_54 - 1'b1;
            end else if (pop_pair_54 && push_pair_54) begin
                // The head's high half and the incoming replacement are
                // sampled on the same CPU edge.
                if (queued_54 == 2) begin
                    pair_head_54 <= pair_next_54;
                    pair_next_54 <= write_data;
                end else pair_head_54 <= write_data;
            end
            if (read_toggle_108 != read_seen_54) begin
                read_seen_54 <= read_toggle_108;
                read_data <= read_pair_108;
                read_valid <= 1;
            end
            if (done_toggle_108 != done_seen_54) begin
                done_seen_54 <= done_toggle_108;
                occupied_54 <= 0;
                queued_54 <= 0;
                done <= 1;
            end
        end
    end

    // Detect the derived CPU edge halfway through a controller cycle. Two
    // rising-edge stages preserve the CPU-falling launch phase while moving
    // the controller's WRITE_LAUNCH command path off the half-cycle boundary.
    always @(negedge controller_clk) begin
        if (reset) begin
            cpu_edge_seen_108 <= 0;
            slot_detect_108 <= 0;
            done_toggle_108 <= 0;
            read_toggle_108 <= 0;
        end else begin
            slot_detect_108 <= cpu_edge_toggle_54 != cpu_edge_seen_108;
            cpu_edge_seen_108 <= cpu_edge_toggle_54;
            done_toggle_108 <= done_event_108;
            read_toggle_108 <= read_event_108;
        end
    end

    always @(posedge controller_clk) begin
        if (reset) begin
            request_seen_108 <= 0;
            next_valid_108 <= 0;
            slot_pipe_108 <= 0;
            write_slot_108 <= 0;
            done_event_108 <= 0;
            read_event_108 <= 0;
            read_half_108 <= 0;
            segment_108 <= 0;
            restart_108 <= 0;
            group_next_valid_108 <= 0;
            read_word_108 <= 0;
            descriptor_toggle_108 <= 0;
            group_108 <= 0;
            writing_108 <= 0;
            address_108 <= 0;
            words_108 <= 0;
            mask_108 <= 0;
        end else begin
            slot_pipe_108 <= slot_detect_108;
            next_valid_108 <= next_valid;
            next_address_108 <= next_address;
            write_slot_108 <= slot_pipe_108;
            if (CHAIN_GROUP_FOUR && request_toggle_54 != descriptor_toggle_108) begin
                descriptor_toggle_108 <= request_toggle_54;
                group_108 <= words_54 == 8'd128;
                writing_108 <= writing_54;
                address_108 <= address_54;
                words_108 <= words_54 == 8'd128 ? 6'd32 : words_54[5:0];
                mask_108 <= mask_54;
            end
            if (core_request && core_ready) begin
                request_seen_108 <= CHAIN_GROUP_FOUR ? descriptor_toggle_108 : request_toggle_54;
                read_half_108 <= 0;
                read_word_108 <= 0;
                segment_108 <= restart_108 ? segment_108 + 1'b1 : 2'd0;
                restart_108 <= 0;
                group_next_valid_108 <= group_active && (!restart_108 || segment_108 != 2);
            end
            if (core_chain_accept || core_read_chain_accept) begin
                segment_108 <= segment_108 + 1'b1;
                if (segment_108 == 2) group_next_valid_108 <= 0;
            end
            if (core_done) begin
                group_next_valid_108 <= 0;
                if (group_active && segment_108 != 3)
                    restart_108 <= 1;
                else done_event_108 <= !done_event_108;
            end
            if (core_read_valid) begin
                read_word_108 <= read_word_108 + 1'b1;
                if (words_54 == 1) begin
                    read_pair_108 <= {32'b0, core_read_data};
                    read_event_108 <= !read_event_108;
                end else if (!read_half_108) begin
                    read_low_108 <= core_read_data;
                    read_half_108 <= 1;
                end else begin
                    read_pair_108 <= {core_read_data, read_low_108};
                    read_event_108 <= !read_event_108;
                    read_half_108 <= 0;
                end
            end
        end
    end

    SdramController #(.BANK_BIT(BANK_BIT), .PREPARE_NEXT(PREPARE_NEXT),
        .CHAIN(CHAIN_GROUP_FOUR), .READ_CHAIN(CHAIN_GROUP_FOUR)) u_controller (
        .clk(controller_clk), .sdram_clk(sdram_clk), .reset(reset),
        .request(core_request), .writing(core_writing), .address(core_address),
        .words(core_words), .write_slot(write_slot_108),
        .next_valid(group_active ? group_next_valid_108 :
                    (PREPARE_NEXT && !CHAIN_GROUP_FOUR && next_valid_108)),
        .next_address(group_active ? group_next_address : next_address_108),
        .chain_accept(core_chain_accept), .read_chain_accept(core_read_chain_accept),
        .read_boundary(group_active && core_read_valid && read_word_108 == 31), .read_chain_pending(),
        .ready(core_ready), .done(core_done),
        .write_data(core_write_data), .write_mask(CHAIN_GROUP_FOUR ? mask_108 : mask_54),
        .write_index(core_write_index), .read_data(core_read_data),
        .read_valid(core_read_valid), .initialized(core_initialized),
        .phase(core_phase), .stream_stats(),
        .O_sdram_clk(O_sdram_clk), .O_sdram_cke(O_sdram_cke),
        .O_sdram_cs_n(O_sdram_cs_n), .O_sdram_cas_n(O_sdram_cas_n),
        .O_sdram_ras_n(O_sdram_ras_n), .O_sdram_wen_n(O_sdram_wen_n),
        .O_sdram_dqm(O_sdram_dqm), .O_sdram_addr(O_sdram_addr),
        .O_sdram_ba(O_sdram_ba), .IO_sdram_dq(IO_sdram_dq)
    );
endmodule
