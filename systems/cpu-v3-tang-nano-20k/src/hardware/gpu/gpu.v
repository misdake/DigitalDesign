// GPU device 4, temporary command processor, and 8-entry tile framebuffer
// cache.
//
// One serial command FSM drives a fixed 32-byte command/tile-list read master
// (`gpu_ro`), a 128-byte framebuffer read master (`gpu_fb_r`) and a 128-byte
// framebuffer write master (`gpu_fb_w`). A draw fetches `u16` tile indices,
// brings each tile into the cache (LOAD refills from `gpu_fb_r`, CLEAR
// initializes locally), writes the selected rows and marks the entry dirty. A
// dirty victim is cleaned with four 128-byte `gpu_fb_w` transactions before its
// entry is reused, and END drains every dirty entry before retiring.
//
// The state machine, register set and cycle behavior mirror
// `hardware::gpu::GpuCore` exactly: one clocked block applies the state
// transitions, and a combinational block computes every output. The cache data
// is two synchronous 512x32 banks forming a 512x64 beat array with one shared
// synchronous write port and one synchronous read port. Because the read data
// is registered, a one-cycle CLEAN_PRIME state prefetches beat zero of a line
// before it is streamed to `gpu_fb_w`.
//
// Each physical half lives in its own leaf module. This keeps the 1R1W memory
// contract explicit and makes Gowin infer both halves as block RAM.
module CpuV3GpuFramebufferCacheBank (
    input  wire        clk,
    input  wire        write_enable,
    input  wire [8:0]  write_address,
    input  wire [31:0] write_data,
    input  wire [8:0]  read_address,
    output reg  [31:0] read_data = 32'h0
);
    reg [31:0] memory [0:511];

    always @(posedge clk) begin
        if (write_enable)
            memory[write_address] <= write_data;
        read_data <= memory[read_address];
    end
endmodule

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

    output reg [15:0] device_read_data,
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
    localparam [31:0] FRAMEBUFFER_A_WORD = 32'h00200000, FRAMEBUFFER_B_WORD = 32'h00218000;
    localparam [8:0] TILE_TOTAL = 9'd375;
    // The whole command buffer must stay inside 22-bit word memory.
    localparam [22:0] MEMORY_END = 23'h400000;

    localparam [4:0] PH_IDLE = 5'd0, PH_FETCH = 5'd1, PH_RECEIVE = 5'd2, PH_DECODE = 5'd3,
                     PH_LIST_FETCH = 5'd4, PH_LIST_RECEIVE = 5'd5, PH_TILE_STEP = 5'd6,
                     PH_CLEAN_PRIME = 5'd7, PH_CLEAN_REQ = 5'd8, PH_CLEAN_WAIT = 5'd9,
                     PH_REFILL_REQ = 5'd10, PH_REFILL_WAIT = 5'd11,
                     PH_CLEAR_FILL = 5'd12, PH_DRAW_APPLY = 5'd13,
                     PH_END_SCAN = 5'd14, PH_RETIRE = 5'd15, PH_ERROR = 5'd16,
                     PH_EXECUTE = 5'd17;

    reg [4:0] phase = PH_IDLE;

    // ---- device register file ----
    reg [21:0] staging_base = 0;
    reg [15:0] staging_words = 0;
    reg base_low_written = 0, base_high_written = 0;
    reg words_low_written = 0, words_high_written = 0, staging_bad = 0;
    reg [15:0] received_count = 0, executed_count = 0;
    reg submit_rejected = 0, command_error = 0;
    // Exactly two queued submissions, plus the active one.
    reg [21:0] fifo_base [0:1];
    reg [15:0] fifo_words [0:1];
    reg fifo_head = 0, fifo_tail = 0;
    reg [1:0] fifo_count = 0;

    // ---- active submission ----
    reg active = 0;
    reg [21:0] active_base = 0;
    reg [15:0] active_words = 0;
    reg [15:0] qword_index = 0;
    reg [15:0] line_qword_base = 16'hffff;
    reg [63:0] line_buffer [0:3];
    reg [1:0] recv_beat = 0;

    // Header held between the header qword and its payload qwords.
    reg [7:0] pending_opcode = 0;
    reg [7:0] pending_count = 0;
    reg [31:0] pending_arg0 = 0;
    reg [63:0] pending_payload [0:1];
    reg [7:0] pending_remaining = 0;
    reg have_pending = 0;

    // ---- per-submission target ----
    reg target_set = 0;
    reg [21:0] target_base = 0;

    // ---- framebuffer cache metadata ----
    reg cache_valid [0:7];
    reg [5:0] cache_tag [0:7];
    reg cache_dirty [0:7];

    // ---- current draw ----
    reg [63:0] list_buffer [0:3];
    reg [1:0] list_beat = 0;
    reg [15:0] list_fetch_start = 0;
    reg [15:0] list_chunk_start = 0;
    reg list_chunk_valid = 0;
    reg [15:0] draw_tile_count = 0;
    reg [15:0] draw_tile_pos = 0;
    reg [21:0] draw_list_addr = 0;
    reg [15:0] draw_clear_color = 0, draw_color = 0, draw_row_mask = 0;
    reg draw_is_clear = 0, draw_gradient = 0;

    // ---- current tile access ----
    reg [15:0] cur_tile_index = 0;
    reg [2:0] cur_entry = 0;
    reg [5:0] cur_tag = 0;

    // ---- blocking clean/refill progress ----
    reg [2:0] transfer_entry = 0;
    reg [1:0] transfer_line = 0;
    reg [5:0] transfer_beat = 0;
    reg clean_for_end = 0;

    // ---- END drain cursor ----
    reg [3:0] end_scan_index = 0;

    // ---- cache data: two synchronous 512x32 banks ----
    wire [31:0] cache_rd_lo, cache_rd_hi;

    // Device port selection.
    wire write_selected = device_write_enable && (device_index == DEVICE);
    wire read_selected = device_read_enable && (device_index == DEVICE);

    // ---- status ----
    wire busy = active || (fifo_count != 2'd0);
    wire fifo_full = (fifo_count >= 2'd2);
    wire [15:0] status_word =
        (busy ? STATUS_BUSY : 16'h0000)
        | (fifo_full ? STATUS_FIFO_FULL : 16'h0000)
        | (submit_rejected ? STATUS_SUBMIT_REJECTED : 16'h0000)
        | (command_error ? STATUS_COMMAND_ERROR : 16'h0000);

    // Keep the readback mux structurally separate from the write-side staging
    // decoder. Besides being clearer, this avoids a large shared decode cone
    // between DEV_SEND data and DEV_RECV writeback in Gowin synthesis.
    always @* begin
        device_read_data = 0;
        if (read_selected) begin
            case (device_channel)
                RECEIVED_COUNT: device_read_data = received_count;
                EXECUTED_COUNT: device_read_data = executed_count;
                STATUS: device_read_data = status_word;
                QUEUE_LEVEL: device_read_data = {14'b0, fifo_count};
                default: device_read_data = 0;
            endcase
        end
    end

    // ---- submit acceptance ----
    wire stage_complete = base_low_written & base_high_written & words_low_written & words_high_written;
    wire [22:0] submit_end = {1'b0, staging_base} + {7'b0, staging_words};
    wire submit_valid = stage_complete & ~staging_bad & (staging_base[3:0] == 4'h0)
                        & (staging_words != 16'h0) & (staging_words[1:0] == 2'b00)
                        & (submit_end <= MEMORY_END) & (fifo_count < 2'd2);
    wire submit_accepted = write_selected && (device_channel == SUBMIT) && submit_valid;
    wire submit_rejected_now = write_selected && (device_channel == SUBMIT) && !submit_valid;

    wire control_reset = write_selected && (device_channel == CONTROL) && device_write_data[0] && !busy;
    wire clear_errors = write_selected && (device_channel == CONTROL) && device_write_data[1];

    // A submission accepted in the same cycle it reaches the head starts
    // immediately, so the pop must read the just-staged entry when the FIFO was
    // empty. The count uses the post-submit occupancy.
    wire [1:0] fifo_count_after_submit = fifo_count + {1'b0, submit_accepted};
    wire pop_cycle = (phase == PH_IDLE) && !active && (fifo_count_after_submit != 2'd0);
    wire [21:0] pop_base = (submit_accepted && (fifo_count == 2'd0)) ? staging_base : fifo_base[fifo_head];
    wire [15:0] pop_words = (submit_accepted && (fifo_count == 2'd0)) ? staging_words : fifo_words[fifo_head];

    // ---- command fetch/decode ----
    wire [15:0] total_qwords = active_words >> 2;
    wire line_matches = (line_qword_base == {qword_index[15:2], 2'b00});
    wire [63:0] decode_word = line_buffer[qword_index[1:0]];
    wire [7:0] d_opcode = decode_word[7:0];
    wire [7:0] d_count = decode_word[15:8];
    wire [15:0] d_flags = decode_word[31:16];
    wire [31:0] d_arg0 = decode_word[63:32];
    wire header_qword_ok = (d_count != 8'd0) && (d_flags == 16'h0000)
                           && ({1'b0, qword_index} + {9'b0, d_count} <= {1'b0, total_qwords});
    wire do_execute = (phase == PH_EXECUTE);
    wire [7:0] payload_slot = pending_count - 8'd1 - pending_remaining;

    // Commands execute only after the complete header/payload has been
    // registered. This intentionally cuts the command-line selector out of
    // the draw-state write path.
    wire [7:0] exec_opcode = pending_opcode;
    wire [7:0] exec_count = pending_count;
    wire [31:0] exec_arg0 = pending_arg0;
    wire [63:0] exec_payload0 = pending_payload[0];
    wire [63:0] exec_payload1 = pending_payload[1];
    wire [32:0] fake_list_end = {1'b0, exec_payload0[31:0]} + {17'b0, exec_arg0[15:0]};

    wire any_cache_valid = cache_valid[0] | cache_valid[1] | cache_valid[2] | cache_valid[3]
                           | cache_valid[4] | cache_valid[5] | cache_valid[6] | cache_valid[7];

    // ---- tile list indexing ----
    wire [15:0] tile_chunk = {draw_tile_pos[15:4], 4'b0};
    wire [15:0] list_offset = draw_tile_pos - list_chunk_start;
    wire [1:0] list_beat_sel = list_offset[3:2];
    wire [63:0] selected_list_beat = list_buffer[list_beat_sel];
    reg [15:0] list_index;
    always @(*) begin
        case (list_offset[1:0])
            2'd0: list_index = selected_list_beat[15:0];
            2'd1: list_index = selected_list_beat[31:16];
            2'd2: list_index = selected_list_beat[47:32];
            default: list_index = selected_list_beat[63:48];
        endcase
    end
    wire [2:0] tile_entry_sel = list_index[2:0];
    wire [5:0] tile_tag_sel = list_index[8:3];

    // ---- memory master addresses ----
    // Command line: active_base + (qword_index/4)*16 words.
    // Tile list: draw_list_addr + chunk, one 32-byte line at a time.
    assign gpu_ro_request_valid = (phase == PH_FETCH) || (phase == PH_LIST_FETCH);
    assign gpu_ro_write = 1'b0;
    assign gpu_ro_address = (phase == PH_FETCH)
        ? (active_base + {qword_index[15:2], 4'b0})
        : ((phase == PH_LIST_FETCH) ? (draw_list_addr + {6'b0, list_fetch_start}) : 22'h0);
    // `gpu_ro` stays at the fixed 32-byte (one-line) command/list size.
    assign gpu_ro_line_count_minus_1 = 2'b00;
    assign gpu_ro_write_data = 64'h0;

    // Clean address: target_base + {tag, entry}*256 + line*64.
    wire [16:0] clean_tile_offset = {cache_tag[transfer_entry], transfer_entry, 8'b0};
    wire [7:0] clean_line_offset = {transfer_line, 6'b0};
    assign gpu_fb_w_request_valid = (phase == PH_CLEAN_REQ);
    assign gpu_fb_w_write = 1'b1;
    assign gpu_fb_w_address = target_base + {5'b0, clean_tile_offset} + {14'b0, clean_line_offset};
    assign gpu_fb_w_line_count_minus_1 = 2'b11;
    assign gpu_fb_w_write_data = {cache_rd_hi, cache_rd_lo};

    // Refill address: target_base + tile_index*256 + line*64.
    wire [16:0] refill_tile_offset = {cur_tile_index[8:0], 8'b0};
    wire [7:0] refill_line_offset = {transfer_line, 6'b0};
    assign gpu_fb_r_request_valid = (phase == PH_REFILL_REQ);
    assign gpu_fb_r_write = 1'b0;
    assign gpu_fb_r_address = target_base + {5'b0, refill_tile_offset} + {14'b0, refill_line_offset};
    assign gpu_fb_r_line_count_minus_1 = 2'b11;
    assign gpu_fb_r_write_data = 64'h0;

    // Cache read address. CLEAN_PRIME prefetches beat zero; CLEAN_REQ and
    // CLEAN_WAIT keep the read one beat ahead of the beat being presented.
    wire [8:0] clean_prefetch_addr = (transfer_beat >= 6'd15)
        ? {transfer_entry, transfer_line, 4'd15}
        : {transfer_entry, transfer_line, transfer_beat[3:0] + 4'd1};
    wire [8:0] cache_rd_addr;
    assign cache_rd_addr = (phase == PH_CLEAN_PRIME)
        ? {transfer_entry, transfer_line, 4'b0}
        : ((phase == PH_CLEAN_REQ)
            ? (gpu_fb_w_request_ready ? {transfer_entry, transfer_line, 4'b0001}
                                      : {transfer_entry, transfer_line, 4'b0000})
            : ((phase == PH_CLEAN_WAIT)
                ? (gpu_fb_w_write_data_ready
                    ? clean_prefetch_addr
                    : {transfer_entry, transfer_line, transfer_beat[3:0]})
                : 9'd0));

    // Shared synchronous write port; address 0 when idle.
    wire cache_wr_en = ((phase == PH_REFILL_WAIT) && gpu_fb_r_response_valid && !gpu_fb_r_error)
                       || (phase == PH_CLEAR_FILL)
                       || ((phase == PH_DRAW_APPLY) && draw_row_mask[transfer_beat[5:2]]);
    wire [8:0] cache_wr_addr = (phase == PH_REFILL_WAIT)
        ? {transfer_entry, transfer_line, transfer_beat[3:0]}
        : ((phase == PH_CLEAR_FILL) ? {transfer_entry, transfer_beat[5:0]}
                                    : ((phase == PH_DRAW_APPLY) ? {cur_entry, transfer_beat[5:0]}
                                                                : 9'd0));
    // Temporary fake-draw gradient. The base color supplies the high channel
    // bits while tile-local x/y provide the low bits. Four pixels are formed
    // per beat without multipliers or wide adders.
    wire [3:0] gradient_y = transfer_beat[5:2];
    wire [3:0] gradient_x0 = {transfer_beat[1:0], 2'b00};
    wire [3:0] gradient_phase = draw_color[3:0];
    wire [3:0] gradient_shift_x0 = gradient_x0 + gradient_phase;
    wire [3:0] gradient_x1 = gradient_shift_x0 + 4'd1;
    wire [3:0] gradient_x2 = gradient_shift_x0 + 4'd2;
    wire [3:0] gradient_x3 = gradient_shift_x0 + 4'd3;
    wire [3:0] gradient_shift_y = gradient_y + gradient_phase;
    wire [3:0] gradient_sum0 = gradient_x0 + gradient_y + gradient_phase;
    wire [3:0] gradient_sum1 = gradient_sum0 + 4'd1;
    wire [3:0] gradient_sum2 = gradient_sum0 + 4'd2;
    wire [3:0] gradient_sum3 = gradient_sum0 + 4'd3;
    wire [15:0] gradient_pixel0 = {draw_color[15], gradient_shift_x0,
                                   draw_color[10:9], gradient_shift_y,
                                   draw_color[4], gradient_sum0};
    wire [15:0] gradient_pixel1 = {draw_color[15], gradient_x1,
                                   draw_color[10:9], gradient_shift_y,
                                   draw_color[4], gradient_sum1};
    wire [15:0] gradient_pixel2 = {draw_color[15], gradient_x2,
                                   draw_color[10:9], gradient_shift_y,
                                   draw_color[4], gradient_sum2};
    wire [15:0] gradient_pixel3 = {draw_color[15], gradient_x3,
                                   draw_color[10:9], gradient_shift_y,
                                   draw_color[4], gradient_sum3};
    wire [63:0] draw_write_beat = draw_gradient
        ? {gradient_pixel3, gradient_pixel2, gradient_pixel1, gradient_pixel0}
        : {draw_color, draw_color, draw_color, draw_color};

    wire [31:0] cache_wr_lo = (phase == PH_REFILL_WAIT)
        ? gpu_fb_r_read_data[31:0]
        : ((phase == PH_CLEAR_FILL) ? {draw_clear_color, draw_clear_color}
                                    : ((phase == PH_DRAW_APPLY) ? draw_write_beat[31:0]
                                                                : 32'h0));
    wire [31:0] cache_wr_hi = (phase == PH_REFILL_WAIT)
        ? gpu_fb_r_read_data[63:32]
        : ((phase == PH_CLEAR_FILL) ? {draw_clear_color, draw_clear_color}
                                    : ((phase == PH_DRAW_APPLY) ? draw_write_beat[63:32]
                                                                : 32'h0));

    CpuV3GpuFramebufferCacheBank cache_lo_bank (
        .clk(clk),
        .write_enable(cache_wr_en),
        .write_address(cache_wr_addr),
        .write_data(cache_wr_lo),
        .read_address(cache_rd_addr),
        .read_data(cache_rd_lo)
    );

    CpuV3GpuFramebufferCacheBank cache_hi_bank (
        .clk(clk),
        .write_enable(cache_wr_en),
        .write_address(cache_wr_addr),
        .write_data(cache_wr_hi),
        .read_address(cache_rd_addr),
        .read_data(cache_rd_hi)
    );

    // ---- combinational execute results used by the clocked block ----
    // Written as a task-like inline block so the phase transition and the
    // command state update commit together.
    reg [4:0] exec_phase;
    reg exec_error;
    always @(*) begin
        exec_phase = phase;
        exec_error = 1'b0;
        if (do_execute) begin
            case (exec_opcode)
                OP_SET_TARGET: begin
                    if (exec_count != 8'd1) exec_error = 1'b1;
                    else if ((exec_arg0 == FRAMEBUFFER_A_WORD) || (exec_arg0 == FRAMEBUFFER_B_WORD)) begin
                        // A cache tag is relative to one target. Switching with
                        // resident entries would make a later clean use the
                        // wrong base, so reject it explicitly.
                        if (target_set && (target_base != exec_arg0[21:0]) && any_cache_valid)
                            exec_error = 1'b1;
                        else
                            exec_phase = PH_DECODE;
                    end else
                        exec_error = 1'b1;
                end
                OP_FAKE_DRAW: begin
                    if (exec_count != 8'd3) exec_error = 1'b1;
                    else if ((exec_arg0[31:18] != 0) || (exec_arg0[17:16] > 2'd1)) exec_error = 1'b1;
                    else if (!target_set) exec_error = 1'b1;
                    else if ((exec_payload0[63:32] != 0) || (exec_payload1[63:49] != 0)) exec_error = 1'b1;
                    else if ((exec_arg0[15:0] != 0)
                             && ((exec_payload0[3:0] != 0) || (fake_list_end > MEMORY_END)))
                        exec_error = 1'b1;
                    else
                        exec_phase = (exec_arg0[15:0] == 16'h0) ? PH_DECODE : PH_TILE_STEP;
                end
                OP_END: begin
                    if ((exec_count != 8'd1) || (exec_arg0 != 32'h0)) exec_error = 1'b1;
                    else if (!target_set) exec_error = 1'b1;
                    else
                        exec_phase = PH_END_SCAN;
                end
                default: exec_error = 1'b1;
            endcase
        end
    end

    integer i;
    always @(posedge clk) begin
        if (reset) begin
            phase <= PH_IDLE;
            staging_base <= 0; staging_words <= 0;
            base_low_written <= 0; base_high_written <= 0;
            words_low_written <= 0; words_high_written <= 0; staging_bad <= 0;
            received_count <= 0; executed_count <= 0;
            submit_rejected <= 0; command_error <= 0;
            fifo_head <= 0; fifo_tail <= 0; fifo_count <= 0;
            for (i = 0; i < 2; i = i + 1) begin
                fifo_base[i] <= 0; fifo_words[i] <= 0;
            end
            active <= 0; active_base <= 0; active_words <= 0;
            qword_index <= 0; line_qword_base <= 16'hffff; recv_beat <= 0;
            pending_opcode <= 0; pending_count <= 0; pending_arg0 <= 0;
            pending_remaining <= 0; have_pending <= 0;
            target_set <= 0; target_base <= 0;
            for (i = 0; i < 8; i = i + 1) begin
                cache_valid[i] <= 0; cache_tag[i] <= 0; cache_dirty[i] <= 0;
            end
            list_fetch_start <= 0; list_chunk_start <= 0; list_chunk_valid <= 0;
            list_beat <= 0; draw_tile_count <= 0; draw_tile_pos <= 0;
            draw_list_addr <= 0; draw_clear_color <= 0; draw_color <= 0;
            draw_row_mask <= 0; draw_is_clear <= 0; draw_gradient <= 0;
            cur_tile_index <= 0; cur_entry <= 0; cur_tag <= 0;
            transfer_entry <= 0; transfer_line <= 0; transfer_beat <= 0; clean_for_end <= 0;
            end_scan_index <= 0;
        end else begin
            // Device writes are independent of the running state machine.
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
                received_count <= received_count + 16'd1;
                base_low_written <= 0; base_high_written <= 0;
                words_low_written <= 0; words_high_written <= 0; staging_bad <= 0;
            end
            if (submit_rejected_now) submit_rejected <= 1;
            if (clear_errors) begin
                submit_rejected <= 0; command_error <= 0;
            end
            if (control_reset) begin
                fifo_head <= 0; fifo_tail <= 0; fifo_count <= 0;
                active <= 0;
                base_low_written <= 0; base_high_written <= 0;
                words_low_written <= 0; words_high_written <= 0; staging_bad <= 0;
                submit_rejected <= 0; command_error <= 0;
                qword_index <= 0; line_qword_base <= 16'hffff; recv_beat <= 0;
                have_pending <= 0; pending_remaining <= 0;
                target_set <= 0; target_base <= 0;
                draw_tile_count <= 0; draw_tile_pos <= 0;
                list_chunk_valid <= 0; list_chunk_start <= 0;
                end_scan_index <= 0;
                for (i = 0; i < 8; i = i + 1) begin
                    cache_valid[i] <= 0; cache_dirty[i] <= 0;
                end
                phase <= PH_IDLE;
            end else begin
                fifo_count <= fifo_count + {1'b0, submit_accepted} - {1'b0, pop_cycle};
            end

            // ---- state machine ----
            case (phase)
                PH_IDLE: begin
                    if (!active && (fifo_count_after_submit != 2'd0)) begin
                        active <= 1;
                        active_base <= pop_base;
                        active_words <= pop_words;
                        fifo_head <= ~fifo_head;
                        qword_index <= 0;
                        line_qword_base <= 16'hffff;
                        recv_beat <= 0;
                        have_pending <= 0;
                        pending_remaining <= 0;
                        target_set <= 0; target_base <= 0;
                        draw_tile_count <= 0; draw_tile_pos <= 0;
                        list_chunk_valid <= 0; list_chunk_start <= 0;
                        end_scan_index <= 0;
                        for (i = 0; i < 8; i = i + 1) begin
                            cache_valid[i] <= 0; cache_dirty[i] <= 0;
                        end
                        phase <= PH_FETCH;
                    end
                end
                PH_FETCH: begin
                    if (gpu_ro_request_ready) begin
                        recv_beat <= 2'd0;
                        phase <= PH_RECEIVE;
                    end
                end
                PH_RECEIVE: begin
                    if (gpu_ro_response_valid) begin
                        if (gpu_ro_error) phase <= PH_ERROR;
                        else begin
                            line_buffer[recv_beat] <= gpu_ro_read_data;
                            if ((recv_beat == 2'd3) || gpu_ro_response_last) begin
                                recv_beat <= 2'd0;
                                line_qword_base <= {qword_index[15:2], 2'b00};
                                phase <= PH_DECODE;
                            end else
                                recv_beat <= recv_beat + 2'd1;
                        end
                    end
                end
                PH_DECODE: begin
                    if (!line_matches) phase <= PH_FETCH;
                    else if (qword_index >= total_qwords) phase <= PH_ERROR;
                    else if (have_pending) begin
                        if (payload_slot < 8'd2) pending_payload[payload_slot[0]] <= decode_word;
                        pending_remaining <= pending_remaining - 8'd1;
                        qword_index <= qword_index + 16'd1;
                        if (pending_remaining == 8'd1) begin
                            have_pending <= 1'b0;
                            phase <= PH_EXECUTE;
                        end
                    end
                    else if (!header_qword_ok) phase <= PH_ERROR;
                    else begin
                        pending_opcode <= d_opcode;
                        pending_count <= d_count;
                        pending_arg0 <= d_arg0;
                        qword_index <= qword_index + 16'd1;
                        if (d_count == 8'd1)
                            phase <= PH_EXECUTE;
                        else begin
                            pending_remaining <= d_count - 8'd1;
                            have_pending <= 1'b1;
                        end
                    end
                end
                PH_EXECUTE: begin
                    // The registered command is applied by the execute block
                    // below, after this case statement.
                end
                PH_LIST_FETCH: begin
                    if (gpu_ro_request_ready) begin
                        list_beat <= 2'd0;
                        phase <= PH_LIST_RECEIVE;
                    end
                end
                PH_LIST_RECEIVE: begin
                    if (gpu_ro_response_valid) begin
                        if (gpu_ro_error) phase <= PH_ERROR;
                        else begin
                            list_buffer[list_beat] <= gpu_ro_read_data;
                            if ((list_beat == 2'd3) || gpu_ro_response_last) begin
                                list_beat <= 2'd0;
                                list_chunk_start <= list_fetch_start;
                                list_chunk_valid <= 1'b1;
                                phase <= PH_TILE_STEP;
                            end else
                                list_beat <= list_beat + 2'd1;
                        end
                    end
                end
                PH_TILE_STEP: begin
                    if (draw_tile_pos >= draw_tile_count) phase <= PH_DECODE;
                    else if (!list_chunk_valid || (list_chunk_start != tile_chunk)) begin
                        list_fetch_start <= tile_chunk;
                        phase <= PH_LIST_FETCH;
                    end
                    else if (list_index >= {7'b0, TILE_TOTAL}) phase <= PH_ERROR;
                    else begin
                        cur_tile_index <= list_index;
                        cur_entry <= tile_entry_sel;
                        cur_tag <= tile_tag_sel;
                        if (cache_valid[tile_entry_sel]
                            && (cache_tag[tile_entry_sel] == tile_tag_sel)) begin
                            // Hit: LOAD keeps the line, CLEAR re-initializes it.
                            if (draw_is_clear) begin
                                transfer_entry <= tile_entry_sel;
                                transfer_beat <= 6'd0;
                                phase <= PH_CLEAR_FILL;
                            end else begin
                                transfer_beat <= 6'd0;
                                phase <= PH_DRAW_APPLY;
                            end
                        end
                        else if (cache_valid[tile_entry_sel] && cache_dirty[tile_entry_sel]) begin
                            // Clean the dirty victim first; TILE_STEP re-runs.
                            transfer_entry <= tile_entry_sel;
                            transfer_line <= 2'd0;
                            transfer_beat <= 6'd0;
                            clean_for_end <= 1'b0;
                            phase <= PH_CLEAN_PRIME;
                        end
                        else begin
                            cache_valid[tile_entry_sel] <= 1'b0;
                            cache_dirty[tile_entry_sel] <= 1'b0;
                            if (draw_is_clear) begin
                                transfer_entry <= tile_entry_sel;
                                transfer_beat <= 6'd0;
                                phase <= PH_CLEAR_FILL;
                            end else begin
                                transfer_entry <= tile_entry_sel;
                                transfer_line <= 2'd0;
                                transfer_beat <= 6'd0;
                                phase <= PH_REFILL_REQ;
                            end
                        end
                    end
                end
                PH_CLEAN_PRIME: begin
                    // The read port latches {entry, line, beat 0} this cycle.
                    phase <= PH_CLEAN_REQ;
                end
                PH_CLEAN_REQ: begin
                    if (gpu_fb_w_request_ready) begin
                        // Beat zero is captured on the accepting edge.
                        transfer_beat <= 6'd1;
                        phase <= PH_CLEAN_WAIT;
                    end
                end
                PH_CLEAN_WAIT: begin
                    if (gpu_fb_w_write_data_ready && (transfer_beat < 6'd16))
                        transfer_beat <= transfer_beat + 6'd1;
                    if (gpu_fb_w_response_valid) begin
                        if (gpu_fb_w_error) phase <= PH_ERROR;
                        else if (gpu_fb_w_response_last) begin
                            if (transfer_line == 2'd3) begin
                                cache_dirty[transfer_entry] <= 1'b0;
                                if (clean_for_end) begin
                                    end_scan_index <= end_scan_index + 4'd1;
                                    phase <= PH_END_SCAN;
                                end else begin
                                    cache_valid[transfer_entry] <= 1'b0;
                                    phase <= PH_TILE_STEP;
                                end
                            end else begin
                                transfer_line <= transfer_line + 2'd1;
                                transfer_beat <= 6'd0;
                                phase <= PH_CLEAN_PRIME;
                            end
                        end
                    end
                end
                PH_REFILL_REQ: begin
                    if (gpu_fb_r_request_ready) begin
                        transfer_beat <= 6'd0;
                        phase <= PH_REFILL_WAIT;
                    end
                end
                PH_REFILL_WAIT: begin
                    if (gpu_fb_r_response_valid) begin
                        if (gpu_fb_r_error) phase <= PH_ERROR;
                        else if ((transfer_beat == 6'd15) || gpu_fb_r_response_last) begin
                            transfer_beat <= 6'd0;
                            if (transfer_line == 2'd3) begin
                                cache_valid[transfer_entry] <= 1'b1;
                                cache_tag[transfer_entry] <= cur_tag;
                                cache_dirty[transfer_entry] <= 1'b0;
                                phase <= PH_DRAW_APPLY;
                            end else begin
                                transfer_line <= transfer_line + 2'd1;
                                phase <= PH_REFILL_REQ;
                            end
                        end else
                            transfer_beat <= transfer_beat + 6'd1;
                    end
                end
                PH_CLEAR_FILL: begin
                    if (transfer_beat == 6'd63) begin
                        cache_valid[transfer_entry] <= 1'b1;
                        cache_tag[transfer_entry] <= cur_tag;
                        cache_dirty[transfer_entry] <= 1'b0;
                        transfer_beat <= 6'd0;
                        phase <= PH_DRAW_APPLY;
                    end else
                        transfer_beat <= transfer_beat + 6'd1;
                end
                PH_DRAW_APPLY: begin
                    if (transfer_beat == 6'd63) begin
                        cache_dirty[cur_entry] <= 1'b1;
                        transfer_beat <= 6'd0;
                        draw_tile_pos <= draw_tile_pos + 16'd1;
                        phase <= PH_TILE_STEP;
                    end else
                        transfer_beat <= transfer_beat + 6'd1;
                end
                PH_END_SCAN: begin
                    if (end_scan_index >= 4'd8) phase <= PH_RETIRE;
                    else if (cache_valid[end_scan_index] && cache_dirty[end_scan_index]) begin
                        transfer_entry <= end_scan_index[2:0];
                        transfer_line <= 2'd0;
                        transfer_beat <= 6'd0;
                        clean_for_end <= 1'b1;
                        phase <= PH_CLEAN_PRIME;
                    end else
                        end_scan_index <= end_scan_index + 4'd1;
                end
                PH_RETIRE: begin
                    executed_count <= executed_count + 16'd1;
                    active <= 1'b0;
                    phase <= PH_IDLE;
                end
                default: begin
                    // PH_ERROR: sticky error, retire once.
                    command_error <= 1'b1;
                    executed_count <= executed_count + 16'd1;
                    active <= 1'b0;
                    phase <= PH_IDLE;
                end
            endcase

            // The execute result overrides the decode branch's phase.
            if (do_execute) begin
                if (exec_error) phase <= PH_ERROR;
                else begin
                    phase <= exec_phase;
                    case (exec_opcode)
                        OP_SET_TARGET: begin
                            target_set <= 1'b1;
                            target_base <= exec_arg0[21:0];
                        end
                        OP_FAKE_DRAW: begin
                            draw_tile_count <= exec_arg0[15:0];
                            draw_tile_pos <= 16'd0;
                            draw_list_addr <= exec_payload0[21:0];
                            draw_clear_color <= exec_payload1[15:0];
                            draw_color <= exec_payload1[31:16];
                            draw_row_mask <= exec_payload1[47:32];
                            draw_gradient <= exec_payload1[48];
                            draw_is_clear <= (exec_arg0[17:16] == 2'd1);
                            list_chunk_valid <= 1'b0;
                            list_chunk_start <= 16'd0;
                        end
                        OP_END: begin
                            end_scan_index <= 4'd0;
                        end
                        default: ;
                    endcase
                end
            end
        end
    end
endmodule
