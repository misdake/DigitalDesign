// GPU device 4, temporary command processor, and dummy framebuffer writer.
//
// One serial command FSM drives a fixed 32-byte command read master (`gpu_ro`)
// and a fixed 32-byte framebuffer write master (`gpu_fb_w`). `gpu_fb_r` is
// reserved for the next milestone and is tied idle here. Every framebuffer
// tile is solid RGB565 and is written as 16 consecutive 32-byte rows.
//
// The state machine mirrors `hardware::gpu::GpuCore` exactly: one combinational
// block computes the next register values, one clocked block applies them.
module CpuV3Gpu (
    input wire clk, input wire reset,
    input wire [2:0] device_index, input wire [3:0] device_channel,
    input wire device_read_enable, input wire device_write_enable,
    input wire [15:0] device_write_data,

    input wire gpu_ro_request_ready, input wire gpu_ro_write_data_ready,
    input wire gpu_ro_response_valid,
    input wire [63:0] gpu_ro_read_data, input wire gpu_ro_response_last,
    input wire gpu_ro_error,

    input wire gpu_fb_w_request_ready, input wire gpu_fb_w_write_data_ready,
    input wire gpu_fb_w_response_valid,
    input wire gpu_fb_w_response_last, input wire gpu_fb_w_error,

    input wire gpu_fb_r_request_ready, input wire gpu_fb_r_write_data_ready,
    input wire gpu_fb_r_response_valid,
    input wire [63:0] gpu_fb_r_read_data, input wire gpu_fb_r_response_last,
    input wire gpu_fb_r_error,

    output wire [15:0] device_read_data,
    output wire gpu_ro_request_valid, output wire gpu_ro_write,
    output wire [21:0] gpu_ro_address, output wire [1:0] gpu_ro_line_count_minus_1,
    output wire [63:0] gpu_ro_write_data,
    output wire gpu_fb_w_request_valid, output wire gpu_fb_w_write,
    output wire [21:0] gpu_fb_w_address, output wire [1:0] gpu_fb_w_line_count_minus_1,
    output wire [63:0] gpu_fb_w_write_data,
    output wire gpu_fb_r_request_valid, output wire gpu_fb_r_write,
    output wire [21:0] gpu_fb_r_address, output wire [1:0] gpu_fb_r_line_count_minus_1,
    output wire [63:0] gpu_fb_r_write_data
);
    // Device ABI.
    localparam [2:0] DEVICE = 3'd4;
    localparam [3:0] CMD_BASE_LOW = 4'd0, CMD_BASE_HIGH = 4'd1;
    localparam [3:0] CMD_WORDS_LOW = 4'd2, CMD_WORDS_HIGH = 4'd3;
    localparam [3:0] SUBMIT = 4'd4, CONTROL = 4'd5;
    localparam [3:0] RECEIVED_COUNT = 4'd0, EXECUTED_COUNT = 4'd1;
    localparam [3:0] STATUS = 4'd2, QUEUE_LEVEL = 4'd3;

    localparam [15:0] STATUS_BUSY = 16'h0001, STATUS_FIFO_FULL = 16'h0002,
                       STATUS_SUBMIT_REJECTED = 16'h0004, STATUS_COMMAND_ERROR = 16'h0008;

    localparam [7:0] OP_SET_TARGET = 8'he0, OP_FAKE_DRAW = 8'he1, OP_END = 8'hff;
    localparam [21:0] FRAMEBUFFER_A = 22'h200000, FRAMEBUFFER_B = 22'h218000;
    localparam [8:0] TILE_TOTAL = 9'd375;
    localparam [4:0] TILE_COLUMNS = 5'd25;

    localparam [3:0] PH_IDLE = 4'd0, PH_FETCH = 4'd1, PH_RECEIVE = 4'd2, PH_DECODE = 4'd3,
                     PH_DRAW_REQ = 4'd4, PH_DRAW_WAIT = 4'd5, PH_RETIRE = 4'd6, PH_ERROR = 4'd7;

    reg [3:0] phase = PH_IDLE;
    reg [21:0] staging_base = 0;
    reg [15:0] staging_words = 0;
    reg base_low_written = 0, base_high_written = 0, words_low_written = 0, words_high_written = 0;
    reg staging_bad = 0;
    reg [15:0] received_count = 0, executed_count = 0;
    reg submit_rejected = 0, command_error = 0;

    reg [21:0] fifo_base [0:1];
    reg [15:0] fifo_words [0:1];
    reg fifo_head = 0, fifo_tail = 0;
    reg [1:0] fifo_count = 0;

    reg active = 0;
    reg [21:0] active_base = 0;
    reg [15:0] active_words = 0;
    reg [15:0] qword_index = 0;
    reg [15:0] line_qword_base = 16'hffff;
    reg line_valid = 0;
    reg [1:0] recv_beat = 0;
    reg [63:0] line [0:3];

    reg [7:0] pending_opcode = 0;
    reg [7:0] pending_count = 0;
    reg [31:0] pending_arg0 = 0;
    reg have_pending = 0;

    reg target_set = 0;
    reg [21:0] target_base = 0;
    reg [8:0] tiles_done = 0;
    reg [15:0] draw_tiles_left = 0;
    reg [4:0] tile_x = 0;
    reg [3:0] tile_y = 0;
    reg [15:0] phase_bias = 0, r_bias = 0, g_bias = 0, b_bias = 0;
    reg [15:0] pixel = 0;
    reg [3:0] row = 0;
    reg [21:0] draw_addr = 0;

    wire write_selected = device_write_enable && device_index == DEVICE;
    wire read_selected = device_read_enable && device_index == DEVICE;
    wire [31:0] beat_pixel = {pixel, pixel};
    wire fifo_empty = fifo_count == 0;
    wire fifo_full = fifo_count == 2;
    wire busy = active || !fifo_empty;

    assign device_read_data = !read_selected ? 16'h0000 :
        (device_channel == RECEIVED_COUNT) ? received_count :
        (device_channel == EXECUTED_COUNT) ? executed_count :
        (device_channel == STATUS) ?
            ((busy ? STATUS_BUSY : 16'h0) | (fifo_full ? STATUS_FIFO_FULL : 16'h0)
             | (submit_rejected ? STATUS_SUBMIT_REJECTED : 16'h0)
             | (command_error ? STATUS_COMMAND_ERROR : 16'h0)) :
        (device_channel == QUEUE_LEVEL) ? {14'b0, fifo_count} : 16'h0000;

    // 32-byte line request address: base + (qword_index >> 2) * 16.
    wire [21:0] request_address = active_base + {qword_index[15:2], 4'b0};

    assign gpu_ro_request_valid = phase == PH_FETCH;
    assign gpu_ro_write = 1'b0;
    assign gpu_ro_address = request_address;
    assign gpu_ro_line_count_minus_1 = 2'b00;
    assign gpu_ro_write_data = 64'h0;

    assign gpu_fb_w_request_valid = phase == PH_DRAW_REQ || phase == PH_DRAW_WAIT;
    assign gpu_fb_w_write = 1'b1;
    assign gpu_fb_w_address = draw_addr;
    assign gpu_fb_w_line_count_minus_1 = 2'b00;
    assign gpu_fb_w_write_data = {beat_pixel, beat_pixel};

    assign gpu_fb_r_request_valid = 1'b0;
    assign gpu_fb_r_write = 1'b0;
    assign gpu_fb_r_address = 22'h0;
    assign gpu_fb_r_line_count_minus_1 = 2'b00;
    assign gpu_fb_r_write_data = 64'h0;

    wire [63:0] header = line[qword_index[1:0]];
    wire [7:0] header_opcode = header[7:0];
    wire [7:0] header_count_byte = header[15:8];
    wire [8:0] header_count = {1'b0, header_count_byte};
    wire header_flags_zero = header[31:16] == 16'h0000;
    wire [31:0] header_arg0 = header[63:32];
    wire [15:0] words_total = active_words >> 2;
    wire qword_in_range = qword_index < words_total;
    wire line_matches = line_qword_base == {qword_index[15:2], 2'b00};

    // Solid-tile pixel formula, combinational for the current tile.
    wire [4:0] r5 = tile_x + r_bias[4:0] + phase_bias[4:0];
    wire [5:0] g6 = {tile_y, 2'b00} + g_bias[5:0] + phase_bias[7:2];
    wire [4:0] b5 = tile_x + tile_y + b_bias[4:0] + phase_bias[4:0];
    wire [15:0] tile_pixel = {r5, g6, b5};

    // Pixel of the current tile using the payload biases being latched this
    // cycle (the registered biases still hold the previous draw's values).
    wire [15:0] draw_phase = header[15:0];
    wire [15:0] draw_r = header[31:16];
    wire [15:0] draw_g = header[47:32];
    wire [15:0] draw_b = header[63:48];
    wire [15:0] start_pixel = {
        (tile_x + draw_r[4:0] + draw_phase[4:0]),
        ({tile_y, 2'b00} + draw_g[5:0] + draw_phase[7:2]),
        (tile_x + tile_y + draw_b[4:0] + draw_phase[4:0])
    };

    wire last_tile_column = tile_x == TILE_COLUMNS - 1'b1;
    wire [4:0] next_tile_x = last_tile_column ? 5'd0 : tile_x + 1'b1;
    wire [3:0] next_tile_y = last_tile_column ? tile_y + 1'b1 : tile_y;
    wire [4:0] next_r5 = next_tile_x + r_bias[4:0] + phase_bias[4:0];
    wire [5:0] next_g6 = {next_tile_y, 2'b00} + g_bias[5:0] + phase_bias[7:2];
    wire [4:0] next_b5 = next_tile_x + next_tile_y + b_bias[4:0] + phase_bias[4:0];
    wire [15:0] next_tile_pixel = {next_r5, next_g6, next_b5};

    // ---- submit acceptance ----
    wire stage_complete = base_low_written & base_high_written & words_low_written & words_high_written;
    wire base_aligned = staging_base[3:0] == 4'h0;
    wire words_nonzero = staging_words != 16'h0;
    wire words_qword = staging_words[1:0] == 2'b00;
    wire base_in_range = staging_base[21:20] == 2'b00;
    wire submit_valid = stage_complete & !staging_bad & base_aligned & words_nonzero &
                        words_qword & base_in_range & !fifo_full;
    wire submit_accepted = write_selected && device_channel == SUBMIT && submit_valid;
    wire submit_rejected_now = write_selected && device_channel == SUBMIT && !submit_valid;

    wire control_reset = write_selected && device_channel == CONTROL && device_write_data[0] && !busy;
    wire clear_errors = write_selected && device_channel == CONTROL && device_write_data[1];

    wire pop = phase == PH_IDLE && !active && !fifo_empty;

    // ---- decode step results ----
    // Only meaningful while `phase == PH_DECODE && line_matches`.
    wire payload_valid = pending_count == 8'd2 && pending_arg0[31:16] == 16'h0000;
    wire payload_target_ok = target_set;
    wire [9:0] running_tiles = {1'b0, tiles_done} + pending_arg0[9:0];
    wire payload_tiles_ok = pending_arg0[15:10] == 6'h00 &&
                            running_tiles <= {1'b0, TILE_TOTAL};
    wire do_start_draw = have_pending && pending_opcode == OP_FAKE_DRAW &&
                         payload_valid && payload_target_ok && payload_tiles_ok;

    wire header_count_ok = header_count_byte != 8'd0;
    wire header_in_range = qword_index + header_count <= words_total;
    wire target_slot_ok = header_arg0[21:0] == FRAMEBUFFER_A || header_arg0[21:0] == FRAMEBUFFER_B;
    wire set_target_ok = header_opcode == OP_SET_TARGET && header_count_byte == 8'd1 &&
                         header_arg0[31:22] == 10'd0 && target_slot_ok;
    wire end_ok = header_opcode == OP_END && header_count_byte == 8'd1 && header_arg0 == 32'h0 &&
                  target_set && tiles_done == TILE_TOTAL;
    wire known_opcode = header_opcode == OP_SET_TARGET || header_opcode == OP_FAKE_DRAW ||
                        header_opcode == OP_END;
    wire header_ok = header_count_ok && header_flags_zero && header_in_range && known_opcode &&
                     ((header_opcode == OP_SET_TARGET && set_target_ok) ||
                      (header_opcode == OP_FAKE_DRAW && header_count_byte == 8'd2) ||
                      (header_opcode == OP_END && end_ok));

    wire decode_error = line_matches && !have_pending && !header_ok;
    wire payload_error = have_pending && !do_start_draw;

    wire draw_row_last = row == 4'd15;
    wire draw_finishes_draw = draw_row_last && draw_tiles_left == 16'd1;

    // 'Have pending' start-draw: true means a payload qword is awaited.
    wire awaiting_payload = !have_pending && header_ok && header_opcode == OP_FAKE_DRAW;

    // ---- combinational next-state ----
    reg [3:0] next_phase;
    always @(*) begin
        next_phase = phase;
        if (phase == PH_FETCH && gpu_ro_request_ready) next_phase = PH_RECEIVE;
        else if (phase == PH_RECEIVE && gpu_ro_response_valid)
            next_phase = gpu_ro_error ? PH_ERROR : (recv_beat == 2'd3 || gpu_ro_response_last) ? PH_DECODE : PH_RECEIVE;
        else if (phase == PH_DECODE) begin
            if (!line_matches) next_phase = PH_FETCH;
            else if (!qword_in_range) next_phase = PH_ERROR;
            else if (have_pending) next_phase = do_start_draw ? PH_DRAW_REQ : PH_ERROR;
            else if (decode_error) next_phase = PH_ERROR;
            else if (header_opcode == OP_END) next_phase = PH_RETIRE;
            else next_phase = PH_DECODE;
        end
        else if (phase == PH_DRAW_REQ && gpu_fb_w_request_ready) next_phase = PH_DRAW_WAIT;
        else if (phase == PH_DRAW_WAIT && gpu_fb_w_response_valid)
            next_phase = gpu_fb_w_error ? PH_ERROR : (gpu_fb_w_response_last ?
                (draw_finishes_draw ? PH_DECODE : PH_DRAW_REQ) : PH_DRAW_WAIT);
        else if (phase == PH_RETIRE || phase == PH_ERROR) next_phase = PH_IDLE;
        else if (phase == PH_IDLE && !active && !fifo_empty) next_phase = PH_FETCH;
    end

    // ---- clocked register updates ----
    always @(posedge clk) begin
        if (reset) begin
            phase <= PH_IDLE;
            staging_base <= 0; staging_words <= 0;
            base_low_written <= 0; base_high_written <= 0;
            words_low_written <= 0; words_high_written <= 0; staging_bad <= 0;
            received_count <= 0; executed_count <= 0;
            submit_rejected <= 0; command_error <= 0;
            fifo_head <= 0; fifo_tail <= 0; fifo_count <= 0;
            active <= 0; active_base <= 0; active_words <= 0;
            qword_index <= 0; line_qword_base <= 16'hffff; line_valid <= 0; recv_beat <= 0;
            pending_opcode <= 0; pending_count <= 0; pending_arg0 <= 0; have_pending <= 0;
            target_set <= 0; target_base <= 0; tiles_done <= 0; draw_tiles_left <= 0;
            tile_x <= 0; tile_y <= 0; phase_bias <= 0; r_bias <= 0; g_bias <= 0; b_bias <= 0;
            pixel <= 0; row <= 0; draw_addr <= 0;
        end else begin
            phase <= control_reset ? PH_IDLE : next_phase;

            // Device writes: staging and submit.
            if (write_selected) begin
                case (device_channel)
                    CMD_BASE_LOW: begin
                        staging_base[15:0] <= device_write_data;
                        base_low_written <= 1;
                    end
                    CMD_BASE_HIGH: begin
                        staging_base[21:16] <= device_write_data[5:0];
                        base_high_written <= 1;
                        if (device_write_data[15:6] != 0) staging_bad <= 1;
                    end
                    CMD_WORDS_LOW: begin
                        staging_words <= device_write_data;
                        words_low_written <= 1;
                    end
                    CMD_WORDS_HIGH: begin
                        words_high_written <= 1;
                        if (device_write_data != 0) staging_bad <= 1;
                    end
                    default: ;
                endcase
            end
            if (submit_accepted) begin
                fifo_base[fifo_tail] <= staging_base;
                fifo_words[fifo_tail] <= staging_words;
                fifo_tail <= ~fifo_tail;
                fifo_count <= fifo_count + 1'b1;
                received_count <= received_count + 1'b1;
                base_low_written <= 0; base_high_written <= 0;
                words_low_written <= 0; words_high_written <= 0; staging_bad <= 0;
            end
            if (submit_rejected_now) submit_rejected <= 1;
            if (clear_errors) begin
                submit_rejected <= 0; command_error <= 0;
            end
            if (control_reset) begin
                fifo_head <= 0; fifo_tail <= 0; fifo_count <= 0;
                active <= 0; line_valid <= 0; have_pending <= 0;
                base_low_written <= 0; base_high_written <= 0;
                words_low_written <= 0; words_high_written <= 0; staging_bad <= 0;
                submit_rejected <= 0; command_error <= 0;
            end

            // Pop a queued submission when idle.
            if (pop) begin
                active <= 1;
                active_base <= fifo_base[fifo_head];
                active_words <= fifo_words[fifo_head];
                fifo_head <= ~fifo_head;
                fifo_count <= fifo_count - 1'b1;
                qword_index <= 0;
                line_qword_base <= 16'hffff;
                line_valid <= 0;
                recv_beat <= 0;
                have_pending <= 0;
                target_set <= 0; target_base <= 0; tiles_done <= 0;
                tile_x <= 0; tile_y <= 0; row <= 0;
            end

            // Fetch and receive.
            if (phase == PH_FETCH && gpu_ro_request_ready) recv_beat <= 0;
            if (phase == PH_RECEIVE && gpu_ro_response_valid) begin
                if (gpu_ro_error) begin
                    // The error phase retires the submission.
                end else begin
                    line[recv_beat] <= gpu_ro_read_data;
                    if (recv_beat == 2'd3 || gpu_ro_response_last) begin
                        recv_beat <= 0;
                        line_qword_base <= {qword_index[15:2], 2'b00};
                        line_valid <= 1;
                    end else begin
                        recv_beat <= recv_beat + 1'b1;
                    end
                end
            end

            // Decode.
            if (phase == PH_DECODE) begin
                line_valid <= 0;
                if (line_matches && qword_in_range) begin
                    if (have_pending) begin
                        have_pending <= 0;
                        qword_index <= qword_index + 1'b1;
                        if (do_start_draw) begin
                            phase_bias <= draw_phase;
                            r_bias <= draw_r;
                            g_bias <= draw_g;
                            b_bias <= draw_b;
                            draw_tiles_left <= pending_arg0[15:0];
                            draw_addr <= target_base + {tiles_done, 8'b0};
                            row <= 0;
                            pixel <= start_pixel;
                        end
                    end else if (header_ok) begin
                        pending_opcode <= header_opcode;
                        pending_count <= header_count_byte;
                        pending_arg0 <= header_arg0;
                        if (header_opcode == OP_SET_TARGET) begin
                            target_set <= 1;
                            target_base <= header_arg0[21:0];
                            qword_index <= qword_index + 1'b1;
                        end else if (header_opcode == OP_END) begin
                            qword_index <= qword_index + 1'b1;
                        end else begin
                            // FAKE_DRAW: wait for the payload qword.
                            have_pending <= 1;
                            qword_index <= qword_index + 1'b1;
                        end
                    end
                end
            end

            // Draw rows.
            if (phase == PH_DRAW_WAIT && gpu_fb_w_response_valid && !gpu_fb_w_error &&
                gpu_fb_w_response_last) begin
                draw_addr <= draw_addr + 22'd16;
                row <= row + 1'b1;
                if (draw_row_last) begin
                    row <= 0;
                    tiles_done <= tiles_done + 1'b1;
                    draw_tiles_left <= draw_tiles_left - 1'b1;
                    tile_x <= next_tile_x;
                    tile_y <= next_tile_y;
                    if (draw_tiles_left != 16'd1) pixel <= next_tile_pixel;
                end
            end

            // Retire / error.
            if (phase == PH_RETIRE) begin
                executed_count <= executed_count + 1'b1;
                active <= 0;
            end
            if (phase == PH_ERROR) begin
                command_error <= 1;
                executed_count <= executed_count + 1'b1;
                active <= 0;
            end
        end
    end
endmodule
