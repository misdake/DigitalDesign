// One-outstanding, related-clock transport between 54 MHz 64-bit clients and
// the 108 MHz native 32-bit SDRAM controller. Both clocks come from one PLL.
// No SDRAM row/refresh policy or transaction-wide data buffer lives here.
module TangNano20KSdramNativeBridge108M54M #(
    // BANK_BIT=5 stripes 128-byte sectors over four banks; 19 keeps contiguous banks.
    parameter BANK_BIT = 5
) (
    input wire logic_clk,
    input wire controller_clk,
    input wire sdram_clk,
    input wire reset,
    input wire request_valid,
    input wire writing,
    input wire [20:0] address,
    input wire [5:0] words,
    input wire [3:0] write_mask,
    input wire [63:0] write_data,
    input wire write_data_valid,
    output wire request_ready,
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
    reg [5:0] words_54 = 0;
    reg [3:0] mask_54 = 0;
    reg [5:0] pairs_fed_54 = 0;
    reg [63:0] pair_head_54 = 0;
    reg [63:0] pair_next_54 = 0;
    reg [1:0] queued_54 = 0;
    reg done_seen_54 = 0;
    reg read_seen_54 = 0;
    reg init_54 = 0;
    reg cpu_edge_toggle_54 = 0;

    reg request_seen_108 = 0;
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

    wire core_ready;
    wire core_done;
    wire core_initialized;
    wire [31:0] core_read_data;
    wire core_read_valid;
    wire [5:0] core_write_index;
    wire [7:0] core_phase;
    wire request_pending_108 = request_toggle_54 != request_seen_108;
    wire core_request = request_pending_108 && (!writing_54 || queued_54 != 0);
    wire [31:0] core_write_data = core_write_index[0] ?
        pair_head_54[63:32] : pair_head_54[31:0];
    wire high_word_54 = core_phase[4:0] == 5'd15 && core_write_index[0];
    wire pop_pair_54 = occupied_54 && writing_54 && high_word_54 && queued_54 != 0;
    wire pair_needed_54 = occupied_54 && writing_54 &&
        pairs_fed_54 < ((words_54 + 6'd1) >> 1);
    wire push_pair_54 = write_data_valid && write_data_ready;

    assign initialized = init_54;
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
            cpu_edge_toggle_54 <= 0;
            done <= 0;
            read_valid <= 0;
        end else begin
            cpu_edge_toggle_54 <= !cpu_edge_toggle_54;
            init_54 <= core_initialized;
            done <= 0;
            read_valid <= 0;
            if (request_valid && request_ready) begin
                occupied_54 <= 1;
                request_toggle_54 <= !request_toggle_54;
                writing_54 <= writing;
                address_54 <= address;
                words_54 <= words;
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
                pair_head_54 <= write_data;
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
            slot_pipe_108 <= 0;
            write_slot_108 <= 0;
            done_event_108 <= 0;
            read_event_108 <= 0;
            read_half_108 <= 0;
        end else begin
            slot_pipe_108 <= slot_detect_108;
            write_slot_108 <= slot_pipe_108;
            if (core_request && core_ready) begin
                request_seen_108 <= request_toggle_54;
                read_half_108 <= 0;
            end
            if (core_done)
                done_event_108 <= !done_event_108;
            if (core_read_valid) begin
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

    SdramController #(.BANK_BIT(BANK_BIT)) u_controller (
        .clk(controller_clk), .sdram_clk(sdram_clk), .reset(reset),
        .request(core_request), .writing(writing_54), .address(address_54),
        .words(words_54), .write_slot(write_slot_108),
        .next_valid(1'b0), .next_address(21'b0), .chain_accept(),
        .read_boundary(1'b0), .read_chain_pending(),
        .ready(core_ready), .done(core_done),
        .write_data(core_write_data), .write_mask(mask_54),
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
