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
    parameter INIT_CYCLES = 21600,
    parameter CAPTURE_FALLING = 0,
    parameter CHAIN = 0,
    parameter READ_CHAIN = 0,
    parameter SCALAR_TAIL_CYCLES = 2,
    parameter READ_CAPTURE_ALWAYS = 1,
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
    output            chain_accept,
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
    output reg [ 3:0] O_sdram_dqm = 15,
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
    reg dq_drive = 0;
    reg [31:0] dq_out = 0;
    assign {O_sdram_ras_n, O_sdram_cas_n, O_sdram_wen_n} = command;
    assign O_sdram_clk = sdram_clk;
    assign O_sdram_cs_n = 0;
    assign IO_sdram_dq = dq_drive ? dq_out : 32'bz;

    // Transaction state, command timers and four independent active-row records.
    reg [4:0] state = ST_POWER_WAIT, after_wait = ST_POWER_WAIT;
    assign phase = {command, state};
    reg [15:0] init_count = 0;
    reg [7:0] wait_count = 0, read_age = 0;
    reg [7:0] command_age = 0;
    reg read_active = 0;
    reg capture_window = 0;
    reg [7:0] read_limit = 0;
    reg [11:0] refresh_age = 0;
    reg [3:0] row_valid = 0;
    reg [10:0] row[0:3];

    // Latched group descriptor and speculative preparation of the next bank.
    reg [20:0] saved_address = 0;
    reg [5:0] saved_words = 0;
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
    reg [12:0] next_row_identity_q = 0;
    reg [1:0] group_extra_segments = 0;

    // Each two-bit equality fits one LUT4. Preserve these boundaries so a wide
    // equality is not rebuilt as a carry chain on the last-cycle acceptance path.
    (* syn_keep = 1 *) wire [6:0] next_identity_equal;
    genvar identity_group;
    generate
        for (
            identity_group = 0; identity_group < 6; identity_group = identity_group + 1
        ) begin : identity_luts
            assign next_identity_equal[identity_group] = next_row_identity_q[identity_group*2+:2] ==
                mapped_next_address[identity_group*2+8+:2];
        end
    endgenerate
    assign next_identity_equal[6] = next_row_identity_q[12] == mapped_next_address[20];
    wire qualified_next_match = next_matches_q && (&next_identity_equal);
    reg read_chain_slot_q = 0;
    wire read_chain_issue = READ_CHAIN && read_chain_slot_q && next_valid && qualified_next_match &&
        prep_wait == 0;
    reg [7:0] launch_age = 0;
    reg observed_chain = 0;
    assign chain_accept = CHAIN && OPEN_ROW && state == ST_WRITE_STREAM &&
        write_index == saved_last && write_chain_eligible && next_valid && qualified_next_match &&
        prep_wait == 0 && refresh_age < 1000 && group_extra_segments != 3;
    assign ready = state == ST_IDLE && initialized && refresh_age < 1100;
    // Capture validity gates ownership; free-running data may be meaningless.
    generate
        if (CAPTURE_FALLING) begin : falling
            reg valid_falling = 0;
            reg [31:0] data_falling = 0;
            always @(negedge clk) begin
                valid_falling <= capture_window;
                if (READ_CAPTURE_ALWAYS || capture_window) data_falling <= IO_sdram_dq;
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
                read_valid <= read_active && read_age >= CAPTURE_LATENCY && read_age < read_limit;
                // Free-running input capture has no enable/reset mux. Only valid is gated,
                // permitting I/O register packing and keeping the DQ route off core logic.
                if (READ_CAPTURE_ALWAYS ||
                    (read_active && read_age >= CAPTURE_LATENCY && read_age < read_limit))
                    read_data <= IO_sdram_dq;
                if (reset) begin
                    read_valid <= 0;
                    if (!READ_CAPTURE_ALWAYS) read_data <= 0;
                end
            end
        end
    endgenerate
    always @(posedge clk) begin
        command <= 7;
        done <= 0;
        next_matches_q <= next_matches;
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
            saved_words == 32 && refresh_age < 999 && group_extra_segments != 3;
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
        if (read_active) read_age <= read_age + 1'b1;
        if (read_active) command_age <= command_age + 1'b1;
        if (read_boundary) read_chain_pending <= 0;
        if (prep_wait != 0) prep_wait <= prep_wait - 1'b1;
        if (prep_rp != 0) prep_rp <= prep_rp - 1'b1;
        capture_window <= read_active && read_age >= CAPTURE_LATENCY && read_age < read_limit;
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
                refresh_age <= 0;
                state <= ST_IDLE;
            end
            ST_IDLE:
            if (refresh_age >= 1100) begin
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
                refresh_age <= 0;
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
                O_sdram_dqm <= saved_mask;
                dq_drive <= 1;
                dq_out <= write_data;
                write_index <= 1;
                state <= ST_WRITE_STREAM;
                prep_needed <= 0;
            end
            ST_WRITE_STREAM: begin
                // Other-bank preparation uses free command slots while current DQ writes.
                if (CHAIN && write_chain_eligible && write_index == 1) begin
                    prep_needed <= next_valid && next_other_bank && !next_matches;
                    prep_open <= row_valid[mapped_next_address[20:19]];
                    prep_address <= mapped_next_address;
                end
                if (CHAIN && prep_needed && prep_rp == 0 && write_index < saved_words) begin
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
                command <= 5;
                O_sdram_addr <= {3'b0, saved_address[7:0]};
                O_sdram_dqm <= 0;
                read_active <= 1;
                read_age <= 0;
                command_age <= 0;
                state <= ST_READ_STREAM;
                prep_needed <= 0;
            end
            ST_READ_STREAM: begin
                if (READ_CHAIN && saved_words == 32 && command_age == 8) begin
                    prep_needed <= next_valid && next_other_bank && !next_matches;
                    prep_open <= row_valid[mapped_next_address[20:19]];
                    prep_address <= mapped_next_address;
                end
                if (READ_CHAIN && prep_needed && prep_rp == 0) begin
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
                    command_age == saved_words + scalar_tail)
                    command <= 6;
                if (read_age == read_finish) begin
                    read_active <= 0;
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
        if (read_chain_issue) begin
            group_extra_segments <= group_extra_segments + 1'b1;
            command <= 5;
            O_sdram_ba <= mapped_next_address[20:19];
            O_sdram_addr <= {3'b0, mapped_next_address[7:0]};
            saved_address <= mapped_next_address;
            command_age <= 0;
            read_limit <= read_limit + saved_words;
            read_chain_pending <= 1;
        end
        // Reset wins over every state transition and aborts all ownership.
        if (reset) begin
            state <= ST_POWER_WAIT;
            command <= 7;
            O_sdram_cke <= 0;
            O_sdram_dqm <= 15;
            dq_drive <= 0;
            initialized <= 0;
            init_count <= 0;
            refresh_age <= 0;
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
            request_matches_q <= 0;
            write_chain_eligible <= 0;
            read_chain_slot_q <= 0;
        end
    end
endmodule
