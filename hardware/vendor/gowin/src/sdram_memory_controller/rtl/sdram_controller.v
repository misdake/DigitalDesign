// Transparent full-page SDR SDRAM backend. Serial requests, optional open rows.
// Default timing profile: 108 MHz, CL2, RCD/RP2, RFC9, WR2.
// RCD/RP2 is based on the 18ns reference; choose cycles for the target grade.
// Rising-core capture is the default; falling-core capture retimes the return path.
// Falling capture defers standalone READ termination to preserve the last word.
// CAPTURE_LATENCY is an FPGA return-path parameter, separate from device CAS.
// Device tAC alone omits pad/routing delay; board tests do not prove PVT corners.
module SdramController #(
    parameter BANK_BIT = 19,
    parameter OPEN_ROW = 1,
    parameter CAS = 2,
    parameter RCD_CYCLES = 2,
    parameter RP_CYCLES = 2,
    parameter RFC_CYCLES = 9,
    parameter CAPTURE_LATENCY = 2,
    // Physical input alignment only; zero bypasses the Gowin I/O delay cells.
    parameter DQ_INPUT_DELAY_TAPS = 0,
    parameter INIT_CYCLES = 21600,
    parameter CAPTURE_FALLING = 0,
    parameter CHAIN = 0,
    parameter CONTINUE = 0,
    parameter READ_CHAIN = 0,
    parameter PREPARE_NEXT = 0,
    parameter SCALAR_TAIL_CYCLES = 2,
    parameter READ_CAPTURE_ALWAYS = 1,
    parameter NEXT_TAGGED = 0,
    parameter REQUEST_PIPELINE = 1
) (
    input             clk,
    input             sdram_clk,
    input             reset,
    input             request,
    input             writing,
    input      [20:0] address,
    input      [ 5:0] words,
    input             write_slot,
    input             next_valid,
    input      [20:0] next_address,
    input      [5:0] next_words,
    input             next_chainable,
    input             next_write_ready,
    input             next_generation,
    output            chain_accept,
    output            read_chain_accept,
    input             read_boundary,
    output reg        read_chain_pending = 0,
    output            ready,
    output reg        done = 0,
    input      [31:0] write_data,
    input      [ 3:0] write_mask,
    output reg [ 5:0] write_index = 0,
    output reg [31:0] read_data = 0,
    output reg        read_valid = 0,
    output reg        initialized = 0,
    output     [ 7:0] phase,
    output reg [23:0] stream_stats = 0,
    output            O_sdram_clk,
    output reg        O_sdram_cke = 0,
    output            O_sdram_cs_n,
    output            O_sdram_cas_n,
    output            O_sdram_ras_n,
    output            O_sdram_wen_n,
    output reg [ 3:0] O_sdram_dqm /* synthesis syn_preserve = 1 */ = 15,
    output reg [10:0] O_sdram_addr = 0,
    output reg [ 1:0] O_sdram_ba = 0,
    inout      [31:0] IO_sdram_dq
);
    // State encodings remain visible through phase[4:0].
    localparam [4:0] ST_POWER_WAIT = 5'd0;
    localparam [4:0] ST_INIT_PRECHARGE = 5'd1;
    localparam [4:0] ST_TIMING_WAIT = 5'd2;
    localparam [4:0] ST_INIT_REFRESH_1 = 5'd3;
    localparam [4:0] ST_INIT_REFRESH_2 = 5'd4;
    localparam [4:0] ST_LOAD_MODE = 5'd5;
    localparam [4:0] ST_INIT_DONE = 5'd6;
    localparam [4:0] ST_IDLE = 5'd10;
    localparam [4:0] ST_REFRESH = 5'd11;
    localparam [4:0] ST_ACTIVATE = 5'd12;
    localparam [4:0] ST_WRITE_LAUNCH = 5'd14;
    localparam [4:0] ST_WRITE_STREAM = 5'd15;
    localparam [4:0] ST_WRITE_END = 5'd16;
    localparam [4:0] ST_READ_LAUNCH = 5'd17;
    localparam [4:0] ST_READ_STREAM = 5'd18;
    localparam [4:0] ST_COMPLETE = 5'd20;
    localparam [4:0] ST_RESOLVE_ROW = 5'd21;

    // BANK_BIT is a compile-time wiring permutation on the logical word address.
    // 19 preserves contiguous 2 MiB banks; 5 stripes consecutive 128-byte sectors.
    // Legal values 5..19 preserve every supported burst's within-bank alignment.
    function [20:0] native_address;
        input [20:0] logical_address;
        reg [18:0] remaining;
        begin
            remaining = (logical_address & ((21'd1 << BANK_BIT) - 1)) |
                ((logical_address >> (BANK_BIT + 2)) << BANK_BIT);
            native_address[20:19] = logical_address >> BANK_BIT;
            native_address[18:0] = remaining;
        end
    endfunction
    // synthesis translate_off
    initial begin
        if (BANK_BIT < 5 || BANK_BIT > 19) $fatal(1, "BANK_BIT must be 5..19");
    end
    // synthesis translate_on

    wire [20:0] mapped_address = native_address(address);
    wire [20:0] mapped_next_address = native_address(next_address);

    // Registered SDRAM command and output data path.
    reg [2:0] command = 7;
    reg dq_drive /* synthesis syn_keep = 1 */ = 0;
    reg [3:0] dqm_state /* synthesis syn_keep = 1 */ = 15;
    reg [31:0] dq_oe_n_pins /* synthesis syn_preserve = 1 */ = 32'hffffffff;
    reg [31:0] dq_out = 0;
    assign {O_sdram_ras_n, O_sdram_cas_n, O_sdram_wen_n} = command;
    assign O_sdram_clk = sdram_clk;
    assign O_sdram_cs_n = 0;
    genvar dq_pin, delay_pin;
    generate for (dq_pin=0;dq_pin<32;dq_pin=dq_pin+1) begin : dq_output_pins
        assign IO_sdram_dq[dq_pin] = !dq_oe_n_pins[dq_pin] ? dq_out[dq_pin] : 1'bz;
    end endgenerate

    wire [31:0] dq_sample;
    generate if (DQ_INPUT_DELAY_TAPS == 0) begin : dq_input_bypass
        assign dq_sample = IO_sdram_dq;
    end else begin : dq_input_delay
        for (delay_pin=0;delay_pin<32;delay_pin=delay_pin+1) begin : pins
            IODELAY #(.C_STATIC_DLY(DQ_INPUT_DELAY_TAPS)) delay_cell (
                .DI(IO_sdram_dq[delay_pin]), .DO(dq_sample[delay_pin]), .DF(),
                .SDTAP(1'b0), .SETN(1'b1), .VALUE(1'b0));
        end
    end endgenerate

    // Transaction state, command timers and four independent active-row records.
    reg [4:0] state = ST_POWER_WAIT, after_wait = ST_POWER_WAIT;
    assign phase = {command, state};
    reg [15:0] init_count = 0;
    reg [7:0] wait_count = 0, read_age = 0;
    reg [7:0] command_age = 0;
    reg read_active = 0;
    reg capture_window = 0;
    reg [7:0] read_limit = 0;
    reg[6:0] stream_left=0;reg[(CAPTURE_LATENCY<2?1:$clog2(CAPTURE_LATENCY+1))-1:0] stream_delay=0;reg[1:0]stream_tail=0;
    wire stream_capture=read_active&&stream_delay==0&&stream_left!=0;
    wire capture_valid=CONTINUE?stream_capture:(read_active&&read_age>=CAPTURE_LATENCY&&read_age<read_limit);
    reg [11:0] refresh_age = 0;
    reg prep_refresh_ok = 1;
    (* syn_keep=1, syn_preserve=1 *) reg refresh_due_q = 0;
    (* syn_keep=1, syn_preserve=1 *) reg chain_refresh_ok = 1;
    reg [3:0] row_valid = 0;
    reg [10:0] row[0:3];

    // Latched group descriptor and speculative preparation of the next bank.
    reg [20:0] saved_address = 0;
    reg [5:0] saved_words = 0;
    reg [7:0] read_stop_age = 0;
    reg write_chain_eligible = 0;
    wire [7:0] scalar_tail = saved_words == 1 ? SCALAR_TAIL_CYCLES : 0;
    reg [7:0] read_finish = 0;
    reg [5:0] saved_last = 0;
    reg saved_write = 0;
    reg [3:0] saved_mask = 0;
    reg [3:0] prep_wait = 0, prep_rp = 0;
    reg prep_needed = 0, prep_open = 0;
    reg [20:0] prep_address = 0;
    wire next_other_bank = mapped_next_address[20:19] != saved_address[20:19];

    // Compare against each bank in parallel, then select a single match bit.
    // Avoid putting an eleven-bit bank mux in front of the row comparator.
    wire [3:0] request_matches, next_match_bits;
    genvar bank_index;
    generate
        for (bank_index = 0; bank_index < 4; bank_index = bank_index + 1) begin : match_rows
            assign request_matches[bank_index] = row_valid[bank_index] &&
                row[bank_index] == mapped_address[18:8];
            assign next_match_bits[bank_index] = row_valid[bank_index] &&
                row[bank_index] == mapped_next_address[18:8];
        end
    endgenerate
    wire next_matches = next_match_bits[mapped_next_address[20:19]];
    reg [3:0] request_matches_q = 0;
    reg next_matches_q = 0;
    reg next_generation_q = 0;
    reg [12:0] next_row_identity_q = 0;
    reg [1:0] group_extra_segments = 0;

    wire next_lookup_current = next_generation_q == next_generation;
    wire qualified_next_match;
    genvar identity_group;
    generate
        if (NEXT_TAGGED) begin : tagged_lookup
            assign qualified_next_match = next_matches_q && next_lookup_current;
        end else begin : address_lookup
            // Retain the independently usable controller's original interface.
            (* syn_keep = 1 *) wire [6:0] next_identity_equal;
            for (identity_group = 0; identity_group < 6; identity_group = identity_group + 1) begin : identity_luts
                assign next_identity_equal[identity_group] = next_row_identity_q[identity_group*2+:2] ==
                    mapped_next_address[identity_group*2+8+:2];
            end
            assign next_identity_equal[6] = next_row_identity_q[12] == mapped_next_address[20];
            assign qualified_next_match = next_matches_q && (&next_identity_equal);
        end
    endgenerate
    // Query/consume are separated by one native edge. Never turn an unknown
    // (changed-address) result into a miss. RCD covers the row-table ACT update.
    wire next_needs_prepare = (NEXT_TAGGED && RCD_CYCLES >= 2) ?
        (next_lookup_current && !next_matches_q && prep_wait == 0) : !next_matches;
    reg read_chain_slot_q = 0;
    (* syn_keep=1, syn_preserve=1 *) reg read_prepare_window = 0;
    (* syn_keep=1, syn_preserve=1 *) reg write_prepare_window = 0;
    wire read_chain_issue = READ_CHAIN && read_chain_slot_q && next_valid && (!CONTINUE||next_chainable) && qualified_next_match &&
        !prep_needed && prep_wait == 0;
    assign read_chain_accept = read_chain_issue;
    reg [7:0] launch_age = 0;
    reg observed_chain = 0;
    assign chain_accept = CHAIN && OPEN_ROW && state == ST_WRITE_STREAM &&
        write_index == saved_last && write_chain_eligible && next_valid && (!CONTINUE||(next_chainable&&next_write_ready)) && qualified_next_match &&
        !prep_needed && prep_wait == 0 && chain_refresh_ok && (CONTINUE||group_extra_segments != 3);
    // Address has no ownership on NOP/STOP. Stage the potential next column
    // independently of the final payload-credit decision; only command issue
    // commits it. A PRE/ACT preparation retains exclusive pin ownership.
    wire column_candidate = CONTINUE && READ_CHAIN && read_chain_slot_q &&
        next_valid && !prep_needed && prep_wait == 0;
    assign ready = state == ST_IDLE && initialized && !refresh_due_q &&
        prep_wait == 0 && prep_rp == 0;
    // Capture validity gates ownership; free-running data may be meaningless.
    generate
        if (CAPTURE_FALLING) begin : falling
            reg valid_falling = 0;
            reg [31:0] data_falling = 0;
            always @(negedge clk) begin
                valid_falling <= capture_window;
                if (READ_CAPTURE_ALWAYS || capture_window) data_falling <= dq_sample;
                if (reset) begin
                    valid_falling <= 0;
                    if (!READ_CAPTURE_ALWAYS) data_falling <= 0;
                end
            end
            // Only a register-to-register transfer crosses the half cycle. Return data
            // and all descriptor/counter control then have a full rising-core cycle.
            always @(posedge clk) begin
                read_valid <= valid_falling;
                read_data  <= data_falling;
                if (reset) begin
                    read_valid <= 0;
                    read_data  <= 0;
                end
            end
        end else begin : rising
            always @(posedge clk) begin
                read_valid <= capture_valid;
                // Free-running input capture has no enable/reset mux. Only valid is gated,
                // permitting I/O register packing and keeping the DQ route off core logic.
                if (READ_CAPTURE_ALWAYS ||
                    (capture_valid))
                    read_data <= dq_sample;
                if (reset) begin
                    read_valid <= 0;
                    if (!READ_CAPTURE_ALWAYS) read_data <= 0;
                end
            end
        end
    endgenerate
    // Owner state remains local; each physical output captures the same next
    // value on this edge. No protocol cycle is added and pad Q never feeds back.
    wire [3:0] dqm_next = reset ? 4'b1111 : state == ST_WRITE_LAUNCH ? saved_mask :
        state == ST_READ_LAUNCH ? 4'b0000 : dqm_state;
    wire dq_oe_n_next = reset ? 1'b1 : state == ST_WRITE_LAUNCH ? 1'b0 :
        state == ST_WRITE_END ? 1'b1 : !dq_drive;
    always @(posedge clk) begin
        O_sdram_dqm <= dqm_next;
        dq_oe_n_pins <= {32{dq_oe_n_next}};
    end
    always @(posedge clk) begin
        command <= 7;
        done <= 0;
        next_matches_q <= next_matches;
        next_generation_q <= next_generation;
        request_matches_q <= request_matches;
        next_row_identity_q <= mapped_next_address[20:8];
        // The limit changes at request capture or the 32-word chain boundary, well
        // before completion. Register tail arithmetic outside the state-transition
        // path; it does not delay completion or add a streaming gap.
        read_finish <= read_limit + (CAPTURE_FALLING ? 8'd1 : 8'd0) + scalar_tail;
        // Predict the fixed 32-word command boundary one clock early. Live descriptor
        // validity, identity, and bank preparation still decide acceptance at the
        // boundary itself. Reserve a refresh clock conservatively.
        read_chain_slot_q <= state == ST_READ_STREAM && command_age == saved_last - 1'b1 &&
            (CONTINUE?saved_words>=8:saved_words==32) && refresh_age < 999 && (CONTINUE||group_extra_segments!=3);
        // Predict the preparation deadline before the tail owns the command
        // port. Keep the wide age/length comparison off address-register enables.
        if (state == ST_READ_STREAM && command_age == saved_last - 1'b1)
            read_prepare_window <= 0;
        if (launch_age < 255) launch_age <= launch_age + 1'b1;
        if (chain_accept || read_chain_issue) observed_chain <= 1;
        if (command == 4 || command == 5) begin
            launch_age <= 1;
            if (observed_chain) begin
                observed_chain <= 0;
                stream_stats[23:8] <= stream_stats[23:8] + 1'b1;
                if (launch_age != saved_words) stream_stats[7] <= 1;
                stream_stats[6:0] <= launch_age[6:0];
            end
        end
        if (refresh_age < 4095) refresh_age <= refresh_age + 1'b1;
        prep_refresh_ok <= refresh_age < 989;
        // Predict the incremented counter's predicates on the preceding edge.
        // Every counter restart overrides these alongside refresh_age below.
        refresh_due_q <= refresh_age >= 1099;
        chain_refresh_ok <= refresh_age < 999;
        if(CONTINUE&&read_active)begin
            if(stream_delay!=0)stream_delay<=stream_delay-1'b1;
            else if(stream_left!=0)begin
                stream_left<=stream_left-1'b1;
                if(stream_left==1)stream_tail<=2;
            end else if(stream_tail!=0)stream_tail<=stream_tail-1'b1;
        end
        if (read_active) read_age <= read_age + 1'b1;
        if (read_active) command_age <= command_age + 1'b1;
        if (read_boundary) read_chain_pending <= 0;
        if (prep_wait != 0) prep_wait <= prep_wait - 1'b1;
        if (prep_rp != 0) prep_rp <= prep_rp - 1'b1;
        capture_window <= capture_valid;
        case (state)
            ST_POWER_WAIT:
            if (init_count == INIT_CYCLES - 1) begin
                O_sdram_cke <= 1;
                state <= ST_INIT_PRECHARGE;
            end else init_count <= init_count + 1'b1;
            ST_INIT_PRECHARGE: begin
                command <= 2;
                O_sdram_addr <= 11'h400;
                wait_count <= RP_CYCLES - 1;
                after_wait <= ST_INIT_REFRESH_1;
                state <= ST_TIMING_WAIT;
            end
            ST_TIMING_WAIT:
            if (wait_count == 1) state <= after_wait;
            else wait_count <= wait_count - 1'b1;
            ST_INIT_REFRESH_1: begin
                command <= 1;
                wait_count <= RFC_CYCLES - 1;
                after_wait <= ST_INIT_REFRESH_2;
                state <= ST_TIMING_WAIT;
            end
            ST_INIT_REFRESH_2: begin
                command <= 1;
                wait_count <= RFC_CYCLES - 1;
                after_wait <= ST_LOAD_MODE;
                state <= ST_TIMING_WAIT;
            end
            ST_LOAD_MODE: begin
                command <= 0;
                O_sdram_ba <= 0;
                O_sdram_addr <= CAS * 16 + 7;
                wait_count <= 1;
                after_wait <= ST_INIT_DONE;
                state <= ST_TIMING_WAIT;
            end
            ST_INIT_DONE: begin
                initialized <= 1;
                refresh_age <= 0; refresh_due_q<=0; chain_refresh_ok<=1;
                state <= ST_IDLE;
            end
            ST_IDLE:
            if (prep_wait != 0 || prep_rp != 0) begin
                // A prepared bank must satisfy tRCD/tRP before a new column
                // command or an all-bank refresh can consume its row state.
            end else if (refresh_due_q) begin
                if (row_valid != 0) begin
                    command <= 2;
                    O_sdram_addr <= 11'h400;
                    row_valid <= 0;
                    wait_count <= RP_CYCLES - 1;
                    after_wait <= ST_REFRESH;
                    state <= ST_TIMING_WAIT;
                end else state <= ST_REFRESH;
            end else if (request && ready) begin
                write_chain_eligible <= |words[5:3];
                group_extra_segments <= 0;
                saved_address <= mapped_address;
                saved_words <= words;
                read_stop_age <= {2'd0,words} + (words==1 ? SCALAR_TAIL_CYCLES : 0);
                saved_last <= words - 1'b1;
                saved_write <= writing;
                saved_mask <= write_mask;
                write_index <= 0;
                read_limit <= CAPTURE_LATENCY + words;
                O_sdram_ba <= mapped_address[20:19];
                if (REQUEST_PIPELINE) state <= ST_RESOLVE_ROW;
                else if (row_valid[mapped_address[20:19]]) begin
                    if (request_matches[mapped_address[20:19]])
                        state <= writing ? ST_WRITE_LAUNCH : ST_READ_LAUNCH;
                    else begin
                        command <= 2;
                        O_sdram_addr <= 0;
                        row_valid[mapped_address[20:19]] <= 0;
                        wait_count <= RP_CYCLES - 1;
                        after_wait <= ST_ACTIVATE;
                        state <= ST_TIMING_WAIT;
                    end
                end else begin
                    command <= 3;
                    O_sdram_addr <= mapped_address[18:8];
                    row_valid[mapped_address[20:19]] <= 1;
                    row[mapped_address[20:19]] <= mapped_address[18:8];
                    wait_count <= RCD_CYCLES - 1;
                    after_wait <= writing ? ST_WRITE_LAUNCH : ST_READ_LAUNCH;
                    state <= ST_TIMING_WAIT;
                end
            end
            ST_REFRESH: begin
                command <= 1;
                refresh_age <= 0; refresh_due_q<=0; chain_refresh_ok<=1;
                wait_count <= RFC_CYCLES - 1;
                after_wait <= ST_IDLE;
                state <= ST_TIMING_WAIT;
            end
            ST_ACTIVATE: begin
                command <= 3;
                O_sdram_addr <= saved_address[18:8];
                row_valid[saved_address[20:19]] <= 1;
                row[saved_address[20:19]] <= saved_address[18:8];
                wait_count <= RCD_CYCLES - 1;
                after_wait <= saved_write ? ST_WRITE_LAUNCH : ST_READ_LAUNCH;
                state <= ST_TIMING_WAIT;
            end
            // Launch samples word zero; each following edge consumes one word.
            ST_WRITE_LAUNCH:
            if (write_slot) begin
                command <= 4;
                O_sdram_ba <= saved_address[20:19];
                O_sdram_addr <= {3'b0, saved_address[7:0]};
                dqm_state <= saved_mask;
                dq_drive <= 1;
                dq_out <= write_data;
                write_index <= 1;
                write_prepare_window <= saved_words > 1;
                state <= ST_WRITE_STREAM;
                prep_needed <= 0;
            end
            ST_WRITE_STREAM: begin
                if (write_index == saved_last) write_prepare_window <= 0;
                // Other-bank preparation uses free command slots while current DQ writes.
                // A separately arbitrated successor may arrive after word 1.
                // Prepare its other bank when the stable descriptor is available,
                // including an open conflicting row, rather than missing the
                // only early sample. The current bank is never disturbed.
                if ((CHAIN || PREPARE_NEXT) && next_valid && next_other_bank &&
                    next_needs_prepare && !prep_needed && prep_refresh_ok && write_prepare_window &&
                    (CHAIN || !row_valid[mapped_next_address[20:19]])) begin
                    prep_needed <= 1;
                    prep_open <= row_valid[mapped_next_address[20:19]];
                    prep_address <= mapped_next_address;
                end
                if ((CHAIN || PREPARE_NEXT) && prep_needed && prep_rp == 0 &&
                    write_prepare_window) begin
                    O_sdram_ba <= prep_address[20:19];
                    if (prep_open) begin
                        command <= 2;
                        O_sdram_addr <= 0;
                        row_valid[prep_address[20:19]] <= 0;
                        prep_rp <= RP_CYCLES - 1;
                        prep_open <= 0;
                    end else begin
                        command <= 3;
                        O_sdram_addr <= prep_address[18:8];
                        row_valid[prep_address[20:19]] <= 1;
                        row[prep_address[20:19]] <= prep_address[18:8];
                        prep_wait <= RCD_CYCLES - 1;
                        prep_needed <= 0;
                    end
                end
                if (write_index < saved_words) begin
                    dq_out <= write_data;
                    write_index <= write_index + 1'b1;
                    if (chain_accept) begin
                        saved_address <= mapped_next_address;
                        write_index <= 0;
                        if(CONTINUE)begin saved_words<=next_words;saved_last<=next_words-1'b1;end
                        state <= ST_WRITE_LAUNCH;
                        group_extra_segments <= group_extra_segments + 1'b1;
                    end
                end else begin
                    command <= 6;
                    state   <= ST_WRITE_END;
                end
            end
            ST_WRITE_END: begin
                dq_drive <= 0;
                if (PREPARE_NEXT) prep_needed <= 0;
                if (OPEN_ROW) begin
                    done  <= 1;
                    state <= ST_IDLE;
                end else begin
                    command <= 2;
                    O_sdram_addr <= 0;
                    row_valid[saved_address[20:19]] <= 0;
                    wait_count <= RP_CYCLES - 1;
                    after_wait <= ST_COMPLETE;
                    state <= ST_TIMING_WAIT;
                end
            end
            ST_READ_LAUNCH: begin
                read_prepare_window <= saved_words > 1;
                command <= 5;
                O_sdram_addr <= {3'b0, saved_address[7:0]};
                dqm_state <= 0;
                read_active <= 1;
                read_age <= 0;
                if(CONTINUE)begin stream_left<=saved_words;stream_delay<=CAPTURE_LATENCY;stream_tail<=0;end
                command_age <= 0;
                state <= ST_READ_STREAM;
                prep_needed <= 0;
            end
            ST_READ_STREAM: begin
                if ((READ_CHAIN || PREPARE_NEXT) && next_valid && next_other_bank &&
                    next_needs_prepare && !prep_needed && prep_refresh_ok && read_prepare_window &&
                    (READ_CHAIN || !row_valid[mapped_next_address[20:19]])) begin
                    prep_needed <= 1;
                    prep_open <= row_valid[mapped_next_address[20:19]];
                    prep_address <= mapped_next_address;
                end
                // The tail owns the command port for READ continuation/STOP.
                // Never update bank state for a PRE/ACT that that tail would
                // overwrite. An unfinished preparation closes or stays pending
                // until normal request resolution; it is not a chainable row.
                if ((READ_CHAIN || PREPARE_NEXT) && prep_needed && prep_rp == 0 &&
                    read_prepare_window) begin
                    O_sdram_ba <= prep_address[20:19];
                    if (prep_open) begin
                        command <= 2;
                        O_sdram_addr <= 0;
                        row_valid[prep_address[20:19]] <= 0;
                        prep_rp <= RP_CYCLES - 1;
                        prep_open <= 0;
                    end else begin
                        command <= 3;
                        O_sdram_addr <= prep_address[18:8];
                        row_valid[prep_address[20:19]] <= 1;
                        row[prep_address[20:19]] <= prep_address[18:8];
                        prep_wait <= RCD_CYCLES - 1;
                        prep_needed <= 0;
                    end
                end
                if (command_age == saved_last) begin
                    if (!read_chain_issue && !CAPTURE_FALLING && CAPTURE_LATENCY < CAS)
                        command <= 6;
                end
                // Keep the last requested word driven through the later FPGA sampling edge.
                // One extra physical read word is discarded; the returned length stays
                // unchanged. Chained READs still issue exactly every saved_words clocks.
                if ((CAPTURE_FALLING || CAPTURE_LATENCY >= CAS) &&
                    command_age == read_stop_age)
                    command <= 6;
                if (CONTINUE?(stream_left==0&&stream_delay==0&&stream_tail==1):(read_age==read_finish)) begin
                    read_active <= 0;
                    if (PREPARE_NEXT) prep_needed <= 0;
                    if (OPEN_ROW) begin
                        done  <= 1;
                        state <= ST_IDLE;
                    end else begin
                        command <= 2;
                        O_sdram_addr <= 0;
                        row_valid[saved_address[20:19]] <= 0;
                        wait_count <= RP_CYCLES - 1;
                        after_wait <= ST_COMPLETE;
                        state <= ST_TIMING_WAIT;
                    end
                end
            end
            ST_COMPLETE: begin
                done  <= 1;
                state <= ST_IDLE;
            end
            // Capture the row comparison on the request handshake edge. This decision
            // uses only the saved descriptor and that edge's comparison; the source may
            // replace its live mapped_address immediately afterwards. Chained columns bypass it.
            ST_RESOLVE_ROW:
            if (row_valid[saved_address[20:19]]) begin
                if (request_matches_q[saved_address[20:19]])
                    state <= saved_write ? ST_WRITE_LAUNCH : ST_READ_LAUNCH;
                else begin
                    command <= 2;
                    O_sdram_addr <= 0;
                    row_valid[saved_address[20:19]] <= 0;
                    wait_count <= RP_CYCLES - 1;
                    after_wait <= ST_ACTIVATE;
                    state <= ST_TIMING_WAIT;
                end
            end else begin
                command <= 3;
                O_sdram_addr <= saved_address[18:8];
                row_valid[saved_address[20:19]] <= 1;
                row[saved_address[20:19]] <= saved_address[18:8];
                wait_count <= RCD_CYCLES - 1;
                after_wait <= saved_write ? ST_WRITE_LAUNCH : ST_READ_LAUNCH;
                state <= ST_TIMING_WAIT;
            end
            default: state <= ST_POWER_WAIT;
        endcase
        if (column_candidate) begin
            O_sdram_ba <= mapped_next_address[20:19];
            O_sdram_addr <= {3'b0, mapped_next_address[7:0]};
        end
        if (read_chain_issue) begin
            read_prepare_window <= 1;
            group_extra_segments <= group_extra_segments + 1'b1;
            command <= 5;
            if (!CONTINUE) begin
                O_sdram_ba <= mapped_next_address[20:19];
                O_sdram_addr <= {3'b0, mapped_next_address[7:0]};
            end
            saved_address <= mapped_next_address;
            command_age <= 0;
            read_limit <= read_limit + saved_words;
            if(CONTINUE)begin
                stream_left<=stream_left+next_words-(stream_capture?7'd1:7'd0);
                saved_words<=next_words;saved_last<=next_words-1'b1;read_stop_age<={2'd0,next_words};
            end
            read_chain_pending <= 1;
        end
        // Reset wins over every state transition and aborts all ownership.
        if (reset) begin
            state <= ST_POWER_WAIT;
            command <= 7;
            O_sdram_cke <= 0;
            dqm_state <= 15;
            dq_drive <= 0;
            initialized <= 0;
            init_count <= 0;
            refresh_age <= 0; refresh_due_q<=0; chain_refresh_ok<=1;
            prep_refresh_ok <= 1;
            row_valid <= 0;
            read_active <= 0;
            read_age <= 0;
            write_index <= 0;
            done <= 0;
            capture_window <= 0;
            prep_wait <= 0;
            prep_rp <= 0;
            launch_age <= 0;
            observed_chain <= 0;
            stream_stats <= 0;
            prep_needed <= 0;
            prep_open <= 0;
            command_age <= 0;
            read_chain_pending <= 0;
            group_extra_segments <= 0;
            next_row_identity_q <= 0;
            next_matches_q <= 0;
            next_generation_q <= 0;
            request_matches_q <= 0;
            write_chain_eligible <= 0;
            read_chain_slot_q <= 0;stream_left<=0;stream_delay<=0;stream_tail<=0;
            read_prepare_window <= 0;
            write_prepare_window <= 0;
            read_stop_age <= 0;
        end
    end
`ifndef SYNTHESIS
    always @(posedge clk) if (!reset && NEXT_TAGGED && next_valid && next_lookup_current &&
        next_row_identity_q != mapped_next_address[20:8])
        $fatal(1,"Native next-generation/row identity mismatch");
`endif
`ifndef SYNTHESIS
    always @(posedge clk) if(!reset && initialized) begin
        if(refresh_due_q !== (refresh_age>=1100) || chain_refresh_ok !== (refresh_age<1000))
            $fatal(1,"refresh predicate lookahead calendar");
    end
`endif
endmodule
