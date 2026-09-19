module CpuV3Core (
    input wire clk,
    input wire reset,
    input wire hold,
    input wire instruction_request_ready,
    input wire instruction_response_valid,
    input wire [15:0] instruction_data,
    input wire instruction_error,
    input wire data_request_ready,
    input wire data_response_valid,
    input wire [15:0] data_read_data,
    input wire data_error,
    input wire [15:0] device_read_data,
    output wire instruction_request_valid,
    output wire [31:0] instruction_address,
    output wire instruction_response_ready,
    output wire data_request_valid,
    output wire data_write,
    output wire [31:0] data_address,
    output wire [15:0] data_write_data,
    output wire data_response_ready,
    output wire [2:0] device_index,
    output wire [3:0] device_channel,
    output wire device_read_enable,
    output wire device_write_enable,
    output wire [15:0] device_write_data,
    output wire halted,
    output reg [15:0] halt_signal = 0,
    output wire fault,
    output reg [7:0] fault_code = 0,
    output reg [15:0] fault_pc = 0,
    output wire [15:0] pc,
    output wire [15:0] code_segment,
    output wire [15:0] data_segment,
    output reg [31:0] retired_words = 0
);
localparam [4:0] ST_FETCH_REQUEST = 0;
localparam [4:0] ST_FETCH_RESPONSE = 1;
localparam [4:0] ST_EXECUTE = 2;
localparam [4:0] ST_DATA_REQUEST = 3;
localparam [4:0] ST_DATA_RESPONSE = 4;
localparam [4:0] ST_MULTIPLY_WAIT = 5;
localparam [4:0] ST_MULTIPLY_COMMIT = 6;
localparam [4:0] ST_HALTED = 7;
localparam [4:0] ST_FAULT = 8;
localparam [4:0] ST_FPU2_WORD1 = 9;
localparam [4:0] ST_FPU2_EXEC = 10;
localparam [4:0] ST_FPU2_MEM_REQUEST = 11;
localparam [4:0] ST_FPU2_MEM_RESPONSE = 12;
localparam [4:0] ST_FPU2_CAPTURE = 13;
localparam [4:0] ST_RESET_CLEAR = 22;
localparam [4:0] ST_ASYNC_STORE_WAIT = 24;

localparam [7:0] FAULT_INVALID_INSTRUCTION = 1;
localparam [7:0] FAULT_INSTRUCTION_MEMORY = 3;
localparam [7:0] FAULT_DATA_MEMORY = 4;

// Pending test result encoding, set by CMP-class instructions and consumed
// by conditional branches.
localparam [1:0] TEST_LESS = 0;
localparam [1:0] TEST_EQUAL = 1;
localparam [1:0] TEST_GREATER = 2;

reg [4:0] state = ST_FETCH_REQUEST;
reg [15:0] pc_register = 0;
reg [15:0] code_segment_register = 0;
reg [15:0] data_segment_register = 0;
reg prefix_valid = 0;
reg [11:0] prefix_high = 0;
reg [15:0] prefix_address = 0;
reg pending_test_valid = 0;
reg [1:0] pending_test_result = 0;
reg [15:0] instruction = 0;
reg [15:0] instruction_pc = 0;

reg pending_write = 0;
reg [31:0] pending_address = 0;
reg [15:0] pending_write_data = 0;
reg [3:0] pending_destination = 0;
reg [1:0] pending_retire_words = 0;
reg [15:0] pending_fault_pc = 0;

// One scalar store may continue in the background. Its request and response
// own the data port until completion; later memory operations wait while
// non-memory instructions continue to execute.
reg async_store_valid = 0;
reg async_store_issued = 0;
reg [31:0] async_store_address = 0;
reg [15:0] async_store_data = 0;
reg [15:0] async_store_fault_pc = 0;

// The scalar register file has two asynchronous read ports and one
// synchronous write port. Back-to-back Execute cycles forward the pending
// synchronous write so a dependent instruction never observes the old word.
wire [15:0] gpr_read_a_ram_data;
wire [15:0] gpr_read_b_ram_data;
reg gpr_write_enable = 0;
reg [3:0] gpr_write_address = 0;
reg [15:0] gpr_write_data = 0;

reg [3:0] multiply_destination = 0;
reg [1:0] multiply_retire_words = 0;
// Post-multiply window: keep product bits [15:0], [23:8], or [31:16].
reg [4:0] multiply_shift = 0;
wire signed [17:0] multiplier_left;
wire signed [17:0] multiplier_right;
wire signed [35:0] multiplier_product;

// Reset walks the scalar register file back to zero one word per cycle
// through its synchronous write port.
reg [3:0] clear_index = 0;

wire [3:0] opcode = instruction[15:12];
wire [3:0] field_d = instruction[11:8];
wire [3:0] field_a = instruction[7:4];
wire [3:0] field_b = instruction[3:0];
// Word0 latch (AUX fields X = [11:8], Fa = [7:2], kind = [1:0]) and the
// word1 fields captured at the word1 response beat.
reg [15:0] fpu2_word0 = 0;
reg [5:0] fpu2_subop = 0;
reg [5:0] fpu2_fd = 0;
reg [5:0] fpu2_fa = 0;
reg fpu2_is_memory = 0;
reg fpu2_write_back = 0;
// Memory beats: FLD/FST stream two 16-bit halves per architectural register.
// `fpu2_beat` counts halves across the whole vector (2*len beats); bit 0
// selects low/high and the upper bits select which F register. `fpu2_len` is
// the register count (1 for scalar, 2..4 for the vector forms).
reg [3:0] fpu2_beat = 0;
reg [2:0] fpu2_len = 1;
wire [2:0] fpu2_beat_index = fpu2_beat[3:1];
wire [3:0] fpu2_double_len = {fpu2_len, 1'b0};
wire [3:0] fpu2_last_beat = fpu2_double_len - 1'b1;
reg [31:0] fpu2_address = 0;
reg [15:0] fpu2_low = 0;
reg fpu2_seen_complete = 0;
reg [15:0] fpu2_fault_pc = 0;

// FST(V) early-release store buffer (fpu-design-v2 section 18). Fpu2Exec
// captures the `len` source F registers through the FPU ext read port (the
// synchronous RF read is pipelined: address i+1 is presented while register i
// is captured) and retires; the 2*len 16-bit words then drain through the
// existing async store channel, low half first, while the core keeps
// executing. The address/fault pc are latched here so a following FPU memory
// instruction cannot disturb an in-flight drain.
reg [31:0] fpu2_buf0 = 0;
reg [31:0] fpu2_buf1 = 0;
reg [31:0] fpu2_buf2 = 0;
reg [31:0] fpu2_buf3 = 0;
reg [3:0] fpu2_pending = 0;
reg [3:0] fpu2_pending_index = 0;
reg [31:0] fpu2_store_address = 0;
reg [15:0] fpu2_store_fault_pc = 0;

wire [3:0] gpr_read_a_address =
    state == ST_HALTED ? 4'd0 :
    state == ST_FPU2_WORD1 ? fpu2_word0[11:8] :
    field_a;
wire [3:0] gpr_read_b_address =
    state == ST_EXECUTE && (opcode == 4'h8 || opcode == 4'h9) ? field_d : field_b;
wire [15:0] gpr_read_a_data =
    gpr_write_enable && gpr_write_address == gpr_read_a_address ?
    gpr_write_data : gpr_read_a_ram_data;
wire [15:0] gpr_read_b_data =
    gpr_write_enable && gpr_write_address == gpr_read_b_address ?
    gpr_write_data : gpr_read_b_ram_data;

// Conservative two-stage frontend: only instructions that retire in one
// Execute cycle and keep sequential control flow may overlap the next queue
// pop. Loads, branches/jumps, devices, and multiply operations remain
// barriers and continue to use the existing blocking FSM paths.
wire shift_pipelineable = opcode == 4'h2 &&
    (field_d <= 4'h2 || (field_d >= 4'h4 && field_d <= 4'h6));
wire immediate_pipelineable = opcode == 4'ha && field_d <= 4'hd;
// Major 6: MOV..SEQ, SLT..CMPU, non-halting SIGNAL, valid MFSR, and MTSR DSEG
// retire in one cycle; reserved fn 7, halting SIGNAL, and JSEG do not.
wire control_alu_pipelineable = opcode == 4'h6 &&
    (field_d <= 4'h6 || (field_d >= 4'h8 && field_d <= 4'hb) ||
     (field_d == 4'hc && field_b != 4'h0) ||
     (field_d == 4'hd && field_b <= 4'h1) ||
     (field_d == 4'he && field_a == 4'h1));
wire execute_pipelineable = state == ST_EXECUTE &&
    (opcode == 4'h0 || opcode == 4'h1 ||
     (opcode >= 4'h3 && opcode <= 4'h5) ||
     (opcode == 4'h9 && !async_store_valid) ||
     shift_pipelineable || immediate_pipelineable || control_alu_pipelineable ||
     opcode == 4'hf);
wire execute_fetch_accepted = execute_pipelineable && instruction_request_ready;
// PFX12 consumer closed set: LOAD/STORE, MULI, every defined major-A
// operation except LDC/ADDC, and the major-B relative forms 0..7.
wire prefix_consumer = opcode == 4'h8 || opcode == 4'h9 ||
                       (opcode == 4'h2 && field_d == 4'hc) ||
                       (opcode == 4'ha &&
                        (field_d <= 4'h6 || (field_d >= 4'h8 && field_d <= 4'ha) ||
                         field_d == 4'hc || field_d == 4'hd)) ||
                       (opcode == 4'hb && field_d <= 4'h7);
wire [1:0] success_retire_words = prefix_valid ? 2 : 1;
wire [15:0] current_fault_pc =
    prefix_valid && prefix_consumer ? prefix_address : instruction_pc;

function [15:0] sign_extend4;
    input [3:0] value;
    sign_extend4 = {{12{value[3]}}, value};
endfunction

function [15:0] sign_extend8;
    input [7:0] value;
    sign_extend8 = {{8{value[7]}}, value};
endfunction

function [15:0] immediate_signed;
    input [15:0] value;
    begin
        immediate_signed = prefix_valid ? {prefix_high, value[3:0]} :
                                           sign_extend4(value[3:0]);
    end
endfunction

function [15:0] immediate_unsigned;
    input [15:0] value;
    begin
        immediate_unsigned = prefix_valid ? {prefix_high, value[3:0]} :
                                             {12'b0, value[3:0]};
    end
endfunction

// The 16-entry constant table shared by LDC (fn 7) and ADDC (fn B), indexed
// by the immediate nibble. Symmetric around the sign bit: indices 0..7 hold
// the magnitudes 8, 16, 24, 32, 64, 128, 256, 512 and indices 8..15 their
// negations. A plain unsigned constant select.
function [15:0] constant_table;
    input [3:0] index;
    begin
        case (index)
            4'h0: constant_table = 16'h0008;
            4'h1: constant_table = 16'h0010;
            4'h2: constant_table = 16'h0018;
            4'h3: constant_table = 16'h0020;
            4'h4: constant_table = 16'h0040;
            4'h5: constant_table = 16'h0080;
            4'h6: constant_table = 16'h0100;
            4'h7: constant_table = 16'h0200;
            4'h8: constant_table = 16'hfff8;
            4'h9: constant_table = 16'hfff0;
            4'ha: constant_table = 16'hffe8;
            4'hb: constant_table = 16'hffe0;
            4'hc: constant_table = 16'hffc0;
            4'hd: constant_table = 16'hff80;
            4'he: constant_table = 16'hff00;
            4'hf: constant_table = 16'hfe00;
        endcase
    end
endfunction

function [15:0] count_leading_zeros;
    input [15:0] value;
    integer index;
    reg found;
    begin
        count_leading_zeros = 16;
        found = 0;
        for (index = 15; index >= 0; index = index - 1) begin
            if (!found && value[index]) begin
                count_leading_zeros = 15 - index;
                found = 1;
            end
        end
    end
endfunction

function [15:0] population_count;
    input [15:0] value;
    integer index;
    begin
        population_count = 0;
        for (index = 0; index < 16; index = index + 1)
            population_count = population_count + value[index];
    end
endfunction

// Destructive shifts compute the result in a statement-based case so the
// arithmetic `>>>` is never inside a conditional expression. In Verilog a `?:`
// is unsigned if any branch is unsigned, which silently turns `>>>` into a
// logical shift.
reg [15:0] shift_result;
always @* begin
    case (field_d)
        4'h0: shift_result = gpr_read_a_data << gpr_read_b_data[3:0];
        4'h1: shift_result = gpr_read_a_data >> gpr_read_b_data[3:0];
        4'h2: shift_result = $signed(gpr_read_a_data) >>> gpr_read_b_data[3:0];
        4'h4: shift_result = gpr_read_a_data << instruction[3:0];
        4'h5: shift_result = gpr_read_a_data >> instruction[3:0];
        4'h6: shift_result = $signed(gpr_read_a_data) >>> instruction[3:0];
        default: shift_result = 16'd0;
    endcase
end

// Integer multiply: both DSP inputs are zero-extended, so the 36-bit signed
// product carries the full unsigned 32-bit product in its low bits. MULI
// (major 2, fn C) sources the unsigned immediate bit pattern.
wire immediate_multiply = opcode == 4'h2 && field_d == 4'hc;
wire [15:0] multiply_left_word = gpr_read_a_data;
wire [15:0] multiply_right_word =
    immediate_multiply ? immediate_unsigned(instruction) : gpr_read_b_data;
assign multiplier_left = {2'b0, multiply_left_word};
assign multiplier_right = {2'b0, multiply_right_word};

__GPR_RAM__ u_gpr_ram (
    .clk(clk),
    .write_enable(gpr_write_enable),
    .write_address(gpr_write_address),
    .write_data(gpr_write_data),
    .read_a_address(gpr_read_a_address),
    .read_b_address(gpr_read_b_address),
    .read_a_data(gpr_read_a_ram_data),
    .read_b_data(gpr_read_b_ram_data)
);

__DSP_MULTIPLIER__ u_multiplier (
    .clk(clk),
    .a(multiplier_left),
    .b(multiplier_right),
    .product(multiplier_product)
);

// FPU v2 unit (fpu-design-v2 section 26). The core streams both instruction
// words in, and implements FLD/FST itself through the ext_* channel while
// the unit is idle.
wire fpu2_busy;
wire fpu2_flag_lt;
wire fpu2_flag_eq;
wire fpu2_flag_gt;
wire fpu2_instr_complete;
wire [31:0] fpu2_ext_read_data;
reg fpu2_word_valid = 0;
reg [15:0] fpu2_word = 0;
reg fpu2_abort = 0;

// The external channel is owned by the core while it captures an FST source
// vector and while it runs the FLD/FST memory beats, and is released as soon
// as the transaction retires. For FLD the RF write fires combinationally on
// the second response beat (a synchronous write that commits at the edge
// leaving ST_FPU2_MEM_RESPONSE).
wire fpu2_ext_access = state == ST_FPU2_EXEC && fpu2_is_memory ||
                       state == ST_FPU2_CAPTURE ||
                       state == ST_FPU2_MEM_REQUEST ||
                       state == ST_FPU2_MEM_RESPONSE;
wire fpu2_ext_write_enable = state == ST_FPU2_MEM_RESPONSE &&
    !fpu2_write_back && fpu2_beat[0] &&
    data_response_valid && !data_error;
// Capture pipelining: the register indexed by the current capture beat was
// requested on the previous beat, so present beat+1 while capturing beat. The
// FLD memory beats keep the two-half-per-register index.
wire [3:0] fpu2_ext_read_index =
    state == ST_FPU2_CAPTURE ? fpu2_beat + 1'b1 : fpu2_beat_index;

CpuV3FpuV2 u_fpu (
    .clk(clk),
    .abort(fpu2_abort),
    .word_valid(fpu2_word_valid),
    .word(fpu2_word),
    .busy(fpu2_busy),
    .flag_lt(fpu2_flag_lt),
    .flag_eq(fpu2_flag_eq),
    .flag_gt(fpu2_flag_gt),
    .instr_complete(fpu2_instr_complete),
    .ext_access(fpu2_ext_access),
    .ext_write_enable(fpu2_ext_write_enable),
    .ext_write_address({3'b000, fpu2_fd} + {6'b0, fpu2_beat_index}),
    .ext_write_data({data_read_data, fpu2_low}),
    .ext_read_address({3'b000, fpu2_fa} + {6'b0, fpu2_ext_read_index}),
    .ext_read_data(fpu2_ext_read_data)
);

assign instruction_request_valid = !hold &&
    (state == ST_FETCH_REQUEST || state == ST_FPU2_WORD1 || execute_pipelineable);
assign instruction_address = {code_segment_register, pc_register};
// A queued instruction may be returned in the same cycle that its request is
// accepted. The legacy split request/response path remains valid for slower
// instruction memories.
assign instruction_response_ready = !hold && (state == ST_FETCH_REQUEST ||
                                    state == ST_FETCH_RESPONSE ||
                                    state == ST_FPU2_WORD1 ||
                                    execute_pipelineable);
assign data_request_valid = !hold && ((async_store_valid && !async_store_issued) ||
                            state == ST_DATA_REQUEST ||
                            state == ST_FPU2_MEM_REQUEST);
// FPU memory beats never overlap the async store buffer: the core only enters
// the FPU memory states while the buffer is idle, and no new store is issued
// from inside them.
wire fpu2_mem_active = state == ST_FPU2_MEM_REQUEST ||
                       state == ST_FPU2_MEM_RESPONSE;

// Early-release store drain (section 18). The next pending word is selected
// low half first; the refill moves it into the single-entry async store
// channel whenever that channel is free (or is completing this cycle, so the
// drain streams without an empty-buffer gap). It pauses for any architectural
// memory state that owns the data port, and a CPU store enqueuing in
// ST_EXECUTE wins a free channel over the refill.
wire cpu_mem_active = state == ST_DATA_REQUEST || state == ST_DATA_RESPONSE;
wire cpu_store_enqueue = state == ST_EXECUTE && opcode == 4'h9 &&
                         !async_store_valid;
wire async_store_completing = async_store_valid && async_store_issued &&
                              data_response_valid && !data_error;
wire async_store_free = !async_store_valid || async_store_completing;
wire fpu2_refill = fpu2_pending != 0 && async_store_free &&
                   !cpu_store_enqueue && !cpu_mem_active &&
                   !fpu2_mem_active;
wire [1:0] fpu2_word_reg = fpu2_pending_index[2:1];
wire [31:0] fpu2_word_data =
    fpu2_word_reg == 2'd0 ? fpu2_buf0 :
    fpu2_word_reg == 2'd1 ? fpu2_buf1 :
    fpu2_word_reg == 2'd2 ? fpu2_buf2 : fpu2_buf3;
wire [15:0] fpu2_word_half =
    fpu2_pending_index[0] ? fpu2_word_data[31:16] : fpu2_word_data[15:0];
assign data_write = async_store_valid ? 1'b1 :
                    fpu2_mem_active ? fpu2_write_back : pending_write;
assign data_address = async_store_valid ? async_store_address :
                      fpu2_mem_active ? fpu2_address + {31'b0, fpu2_beat} :
                      pending_address;
assign data_write_data = async_store_valid ? async_store_data :
                         fpu2_mem_active ?
                             (fpu2_beat[0] == 0 ? fpu2_ext_read_data[15:0] :
                                                  fpu2_ext_read_data[31:16]) :
                         pending_write_data;
assign data_response_ready = !hold && ((async_store_valid && async_store_issued) ||
                             state == ST_DATA_RESPONSE ||
                             state == ST_FPU2_MEM_RESPONSE);
assign device_index = field_d[2:0];
assign device_channel = field_a;
assign device_read_enable = !hold && state == ST_EXECUTE && opcode == 4'h7 && !field_d[3];
assign device_write_enable = !hold && state == ST_EXECUTE && opcode == 4'h7 && field_d[3];
assign device_write_data = gpr_read_b_data;
// Do not expose HALT until the last buffered store is globally observed and
// the FST early-release drain has fully left the core.
assign halted = state == ST_HALTED && !async_store_valid && fpu2_pending == 0;
assign fault = state == ST_FAULT;
assign pc = pc_register;
assign code_segment = code_segment_register;
assign data_segment = data_segment_register;

reg [15:0] left_value;
reg [15:0] right_value;
reg [15:0] immediate_value;
reg [15:0] logical_address;
reg branch_taken;
reg [15:0] jump_offset;
reg [15:0] jump_target;

always @(posedge clk) begin
    if (reset) begin
        gpr_write_enable <= 0;
        state <= ST_RESET_CLEAR;
        clear_index <= 0;
        pc_register <= 0;
        code_segment_register <= 0;
        data_segment_register <= 0;
        prefix_valid <= 0;
        pending_test_valid <= 0;
        pending_test_result <= 0;
        retired_words <= 0;
        fault_code <= 0;
        fault_pc <= 0;
        async_store_valid <= 0;
        async_store_issued <= 0;
        fpu2_word_valid <= 0;
        fpu2_abort <= 1;
        fpu2_seen_complete <= 0;
        fpu2_is_memory <= 0;
        fpu2_pending <= 0;
        fpu2_pending_index <= 0;
    end else if (!hold) begin
        gpr_write_enable <= 0;
        fpu2_word_valid <= 0;
        fpu2_abort <= 0;
        if (async_store_valid && async_store_issued &&
            data_response_valid && data_error) begin
            async_store_valid <= 0;
            async_store_issued <= 0;
            fault_code <= FAULT_DATA_MEMORY;
            fault_pc <= async_store_fault_pc;
            state <= ST_FAULT;
        end else begin
        if (async_store_valid && !async_store_issued && data_request_ready)
            async_store_issued <= 1;
        if (async_store_valid && async_store_issued && data_response_valid) begin
            async_store_valid <= 0;
            async_store_issued <= 0;
        end
        case (state)
            ST_FETCH_REQUEST: begin
                if (instruction_request_ready) begin
                    if (instruction_response_valid) begin
                        if (instruction_error) begin
                            fault_code <= FAULT_INSTRUCTION_MEMORY;
                            fault_pc <= pc_register;
                            state <= ST_FAULT;
                        end else begin
                            instruction <= instruction_data;
                            instruction_pc <= pc_register;
                            pc_register <= pc_register + 1'b1;
                            state <= ST_EXECUTE;
                        end
                    end else begin
                        state <= ST_FETCH_RESPONSE;
                    end
                end
            end
            ST_FETCH_RESPONSE: begin
                if (instruction_response_valid) begin
                    if (instruction_error) begin
                        fault_code <= FAULT_INSTRUCTION_MEMORY;
                        fault_pc <= pc_register;
                        state <= ST_FAULT;
                    end else begin
                        instruction <= instruction_data;
                        instruction_pc <= pc_register;
                        pc_register <= pc_register + 1'b1;
                        state <= ST_EXECUTE;
                    end
                end
            end
            ST_EXECUTE: begin
                if (opcode == 4'hf) begin
                    if (prefix_valid)
                        retired_words <= retired_words + 1'b1;
                    prefix_valid <= 1;
                    prefix_high <= instruction[11:0];
                    prefix_address <= instruction_pc;
                    state <= ST_FETCH_REQUEST;
                end else begin
                    if (prefix_valid && !prefix_consumer)
                        retired_words <= retired_words + 1'b1;
                    prefix_valid <= 0;
                    // Every retired non-prefix instruction expires the pending
                    // test; CMP-class instructions below set it again.
                    pending_test_valid <= 0;
                    case (opcode)
                        4'h0: begin
                            gpr_write_enable <= 1;
                            gpr_write_address <= field_d;
                            gpr_write_data <= gpr_read_a_data + gpr_read_b_data;
                            retired_words <= retired_words + success_retire_words;
                            state <= ST_FETCH_REQUEST;
                        end
                        4'h1: begin
                            gpr_write_enable <= 1;
                            gpr_write_address <= field_d;
                            gpr_write_data <= gpr_read_a_data - gpr_read_b_data;
                            retired_words <= retired_words + success_retire_words;
                            state <= ST_FETCH_REQUEST;
                        end
                        4'h2: begin
                            case (field_d)
                                // Destructive register-count shifts.
                                4'h0, 4'h1, 4'h2: begin
                                    gpr_write_enable <= 1;
                                    gpr_write_address <= field_a;
                                    gpr_write_data <= shift_result;
                                    retired_words <= retired_words + success_retire_words;
                                    state <= ST_FETCH_REQUEST;
                                end
                                // Destructive immediate shifts.
                                4'h4, 4'h5, 4'h6: begin
                                    gpr_write_enable <= 1;
                                    gpr_write_address <= field_a;
                                    gpr_write_data <= shift_result;
                                    retired_words <= retired_words + success_retire_words;
                                    state <= ST_FETCH_REQUEST;
                                end
                                // MUL0/MUL8/MUL16 and MULI share the single
                                // integer DSP and the two-state wait/commit.
                                4'h8, 4'h9, 4'ha, 4'hc: begin
                                    multiply_destination <= field_a;
                                    multiply_shift <= field_d == 4'h9 ? 5'd8 :
                                                      field_d == 4'ha ? 5'd16 : 5'd0;
                                    multiply_retire_words <= success_retire_words;
                                    state <= ST_MULTIPLY_WAIT;
                                end
                                default: begin
                                    fault_code <= FAULT_INVALID_INSTRUCTION;
                                    fault_pc <= current_fault_pc;
                                    state <= ST_FAULT;
                                end
                            endcase
                        end
                        4'h3: begin
                            gpr_write_enable <= 1;
                            gpr_write_address <= field_d;
                            gpr_write_data <= gpr_read_a_data & gpr_read_b_data;
                            retired_words <= retired_words + success_retire_words;
                            state <= ST_FETCH_REQUEST;
                        end
                        4'h4: begin
                            gpr_write_enable <= 1;
                            gpr_write_address <= field_d;
                            gpr_write_data <= gpr_read_a_data | gpr_read_b_data;
                            retired_words <= retired_words + success_retire_words;
                            state <= ST_FETCH_REQUEST;
                        end
                        4'h5: begin
                            gpr_write_enable <= 1;
                            gpr_write_address <= field_d;
                            gpr_write_data <= gpr_read_a_data ^ gpr_read_b_data;
                            retired_words <= retired_words + success_retire_words;
                            state <= ST_FETCH_REQUEST;
                        end
                        4'h6: begin
                            case (field_d)
                                0: begin
                                    gpr_write_enable <= 1;
                                    gpr_write_address <= field_a;
                                    gpr_write_data <= gpr_read_b_data;
                                    retired_words <= retired_words + success_retire_words;
                                    state <= ST_FETCH_REQUEST;
                                end
                                1: begin
                                    gpr_write_enable <= 1;
                                    gpr_write_address <= field_a;
                                    gpr_write_data <= ~gpr_read_b_data;
                                    retired_words <= retired_words + success_retire_words;
                                    state <= ST_FETCH_REQUEST;
                                end
                                2: begin
                                    gpr_write_enable <= 1;
                                    gpr_write_address <= field_a;
                                    gpr_write_data <= -gpr_read_b_data;
                                    retired_words <= retired_words + success_retire_words;
                                    state <= ST_FETCH_REQUEST;
                                end
                                3: begin
                                    gpr_write_enable <= 1;
                                    gpr_write_address <= field_a;
                                    gpr_write_data <= {{8{gpr_read_b_data[7]}}, gpr_read_b_data[7:0]};
                                    retired_words <= retired_words + success_retire_words;
                                    state <= ST_FETCH_REQUEST;
                                end
                                4: begin
                                    gpr_write_enable <= 1;
                                    gpr_write_address <= field_a;
                                    gpr_write_data <= count_leading_zeros(gpr_read_b_data);
                                    retired_words <= retired_words + success_retire_words;
                                    state <= ST_FETCH_REQUEST;
                                end
                                5: begin
                                    gpr_write_enable <= 1;
                                    gpr_write_address <= field_a;
                                    gpr_write_data <= population_count(gpr_read_b_data);
                                    retired_words <= retired_words + success_retire_words;
                                    state <= ST_FETCH_REQUEST;
                                end
                                6: begin
                                    gpr_write_enable <= 1;
                                    gpr_write_address <= field_a;
                                    gpr_write_data <= gpr_read_a_data == gpr_read_b_data;
                                    retired_words <= retired_words + success_retire_words;
                                    state <= ST_FETCH_REQUEST;
                                end
                                8: begin
                                    gpr_write_enable <= 1;
                                    gpr_write_address <= field_a;
                                    gpr_write_data <= $signed(gpr_read_a_data) < $signed(gpr_read_b_data);
                                    retired_words <= retired_words + success_retire_words;
                                    state <= ST_FETCH_REQUEST;
                                end
                                9: begin
                                    gpr_write_enable <= 1;
                                    gpr_write_address <= field_a;
                                    gpr_write_data <= gpr_read_a_data < gpr_read_b_data;
                                    retired_words <= retired_words + success_retire_words;
                                    state <= ST_FETCH_REQUEST;
                                end
                                // CMPS: pending test = signed ordering of
                                // r[ra] and r[rb]; no register is written.
                                10: begin
                                    pending_test_valid <= 1;
                                    pending_test_result <=
                                        gpr_read_a_data == gpr_read_b_data ? TEST_EQUAL :
                                        $signed(gpr_read_a_data) < $signed(gpr_read_b_data) ? TEST_LESS :
                                        TEST_GREATER;
                                    retired_words <= retired_words + success_retire_words;
                                    state <= ST_FETCH_REQUEST;
                                end
                                // CMPU: pending test = unsigned ordering of
                                // r[ra] and r[rb]; no register is written.
                                11: begin
                                    pending_test_valid <= 1;
                                    pending_test_result <=
                                        gpr_read_a_data == gpr_read_b_data ? TEST_EQUAL :
                                        gpr_read_a_data < gpr_read_b_data ? TEST_LESS :
                                        TEST_GREATER;
                                    retired_words <= retired_words + success_retire_words;
                                    state <= ST_FETCH_REQUEST;
                                end
                                // SIGNAL r[rs], type4. Type 0 latches rs at
                                // the retire edge like a register-read and
                                // halts; nonzero types retire as a NOP.
                                12: begin
                                    if (field_b == 0) begin
                                        halt_signal <= gpr_read_a_data;
                                        state <= ST_HALTED;
                                    end else begin
                                        state <= ST_FETCH_REQUEST;
                                    end
                                    retired_words <= retired_words + success_retire_words;
                                end
                                13: begin
                                    if (field_b == 0) begin
                                        gpr_write_enable <= 1;
                                        gpr_write_address <= field_a;
                                        gpr_write_data <= code_segment_register;
                                    end else if (field_b == 1) begin
                                        gpr_write_enable <= 1;
                                        gpr_write_address <= field_a;
                                        gpr_write_data <= data_segment_register;
                                    end
                                    else begin
                                        fault_code <= FAULT_INVALID_INSTRUCTION;
                                        fault_pc <= current_fault_pc;
                                        state <= ST_FAULT;
                                    end
                                    if (field_b <= 1) begin
                                        retired_words <= retired_words + success_retire_words;
                                        state <= ST_FETCH_REQUEST;
                                    end
                                end
                                14: begin
                                    if (field_a == 1) begin
                                        data_segment_register <= gpr_read_b_data;
                                        retired_words <= retired_words + success_retire_words;
                                        state <= ST_FETCH_REQUEST;
                                    end else begin
                                        fault_code <= FAULT_INVALID_INSTRUCTION;
                                        fault_pc <= current_fault_pc;
                                        state <= ST_FAULT;
                                    end
                                end
                                15: begin
                                    code_segment_register <= gpr_read_a_data;
                                    pc_register <= gpr_read_b_data;
                                    retired_words <= retired_words + success_retire_words;
                                    state <= ST_FETCH_REQUEST;
                                end
                                default: begin
                                    fault_code <= FAULT_INVALID_INSTRUCTION;
                                    fault_pc <= current_fault_pc;
                                    state <= ST_FAULT;
                                end
                            endcase
                        end
                        4'h7: begin
                            if (!field_d[3]) begin
                                gpr_write_enable <= 1;
                                gpr_write_address <= field_b;
                                gpr_write_data <= device_read_data;
                            end
                            retired_words <= retired_words + success_retire_words;
                            state <= ST_FETCH_REQUEST;
                        end
                        4'h8, 4'h9: begin
                            logical_address = gpr_read_a_data + immediate_signed(instruction);
                            if (opcode == 4'h9 && !async_store_valid) begin
                                async_store_valid <= 1;
                                async_store_issued <= 0;
                                async_store_address <= {data_segment_register, logical_address};
                                async_store_data <= gpr_read_b_data;
                                async_store_fault_pc <= current_fault_pc;
                                retired_words <= retired_words + success_retire_words;
                                state <= ST_FETCH_REQUEST;
                            end else begin
                                pending_write <= opcode == 4'h9;
                                pending_address <= {data_segment_register, logical_address};
                                pending_write_data <= gpr_read_b_data;
                                pending_destination <= field_d;
                                pending_retire_words <= success_retire_words;
                                pending_fault_pc <= current_fault_pc;
                                state <= async_store_valid ? ST_ASYNC_STORE_WAIT :
                                         ST_DATA_REQUEST;
                            end
                        end
                        4'ha: begin
                            left_value = gpr_read_a_data;
                            immediate_value = immediate_signed(instruction);
                            case (field_d)
                                // ADDI/SUBI read the unprefixed immediate as
                                // an unsigned u4; the prefixed form uses the
                                // full 16-bit pattern.
                                4'h0: gpr_write_data <= left_value + immediate_unsigned(instruction);
                                4'h1: gpr_write_data <= left_value - immediate_unsigned(instruction);
                                4'h2: gpr_write_data <= prefix_valid ?
                                    immediate_unsigned(instruction) : sign_extend4(instruction[3:0]);
                                4'h3: gpr_write_data <= immediate_unsigned(instruction);
                                4'h4: gpr_write_data <= left_value & immediate_unsigned(instruction);
                                4'h5: gpr_write_data <= left_value | immediate_unsigned(instruction);
                                4'h6: gpr_write_data <= left_value ^ immediate_unsigned(instruction);
                                // LDC/ADDC index the shared constant table; a
                                // pending prefix expires unused (these never
                                // consume it).
                                4'h7: gpr_write_data <= constant_table(instruction[3:0]);
                                4'h8: gpr_write_data <= left_value == immediate_value;
                                4'h9: gpr_write_data <= $signed(left_value) < $signed(immediate_value);
                                4'ha: gpr_write_data <= left_value < immediate_unsigned(instruction);
                                4'hb: gpr_write_data <= left_value + constant_table(instruction[3:0]);
                                // CMPSI/CMPUI set the pending test result and
                                // write no register.
                                4'hc: begin
                                    pending_test_valid <= 1;
                                    pending_test_result <=
                                        left_value == immediate_value ? TEST_EQUAL :
                                        $signed(left_value) < $signed(immediate_value) ? TEST_LESS :
                                        TEST_GREATER;
                                end
                                4'hd: begin
                                    pending_test_valid <= 1;
                                    pending_test_result <=
                                        left_value == immediate_unsigned(instruction) ? TEST_EQUAL :
                                        left_value < immediate_unsigned(instruction) ? TEST_LESS :
                                        TEST_GREATER;
                                end
                                default: begin
                                    fault_code <= FAULT_INVALID_INSTRUCTION;
                                    fault_pc <= current_fault_pc;
                                    state <= ST_FAULT;
                                end
                            endcase
                            if (field_d <= 4'hb) begin
                                gpr_write_enable <= 1;
                                gpr_write_address <= field_a;
                            end
                            if (field_d <= 4'hd) begin
                                retired_words <= retired_words + success_retire_words;
                                state <= ST_FETCH_REQUEST;
                            end
                        end
                        4'hb: begin
                            jump_offset = prefix_valid ?
                                {prefix_high[7:0], instruction[7:0]} :
                                sign_extend8(instruction[7:0]);
                            if (field_d <= 4'h5 || (field_d >= 4'h8 && field_d <= 4'hd)) begin
                                // Conditional branches and conditional moves
                                // consume the pending test result, whether or
                                // not the condition holds.
                                if (!pending_test_valid) begin
                                    fault_code <= FAULT_INVALID_INSTRUCTION;
                                    fault_pc <= current_fault_pc;
                                    state <= ST_FAULT;
                                end else begin
                                    case (field_d[2:0])
                                        0: branch_taken = pending_test_result == TEST_EQUAL;
                                        1: branch_taken = pending_test_result != TEST_EQUAL;
                                        2: branch_taken = pending_test_result == TEST_LESS;
                                        3: branch_taken = pending_test_result != TEST_LESS;
                                        4: branch_taken = pending_test_result == TEST_GREATER;
                                        default: branch_taken = pending_test_result != TEST_GREATER;
                                    endcase
                                    if (field_d <= 4'h5) begin
                                        if (branch_taken)
                                            pc_register <= pc_register + jump_offset;
                                    end else if (branch_taken) begin
                                        // MOVcc rd, rs
                                        gpr_write_enable <= 1;
                                        gpr_write_address <= field_a;
                                        gpr_write_data <= gpr_read_b_data;
                                    end
                                    retired_words <= retired_words + success_retire_words;
                                    state <= ST_FETCH_REQUEST;
                                end
                            // JREL: unconditional relative jump, no link.
                            end else if (field_d == 4'h6) begin
                                pc_register <= pc_register + jump_offset;
                                retired_words <= retired_words + success_retire_words;
                                state <= ST_FETCH_REQUEST;
                            // JALREL: link the fall-through address into r14.
                            end else if (field_d == 4'h7) begin
                                gpr_write_enable <= 1;
                                gpr_write_address <= 4'he;
                                gpr_write_data <= pc_register;
                                pc_register <= pc_register + jump_offset;
                                retired_words <= retired_words + success_retire_words;
                                state <= ST_FETCH_REQUEST;
                            // JREG: canonical `B E 0 target`.
                            end else if (field_d == 4'he) begin
                                if (field_a == 0) begin
                                    pc_register <= gpr_read_b_data;
                                    retired_words <= retired_words + success_retire_words;
                                    state <= ST_FETCH_REQUEST;
                                end else begin
                                    fault_code <= FAULT_INVALID_INSTRUCTION;
                                    fault_pc <= current_fault_pc;
                                    state <= ST_FAULT;
                                end
                            // JALR: canonical `B F E target`, link fixed to r14.
                            end else begin
                                if (field_a == 4'he) begin
                                    jump_target = gpr_read_b_data;
                                    gpr_write_enable <= 1;
                                    gpr_write_address <= 4'he;
                                    gpr_write_data <= pc_register;
                                    pc_register <= jump_target;
                                    retired_words <= retired_words + success_retire_words;
                                    state <= ST_FETCH_REQUEST;
                                end else begin
                                    fault_code <= FAULT_INVALID_INSTRUCTION;
                                    fault_pc <= current_fault_pc;
                                    state <= ST_FAULT;
                                end
                            end
                        end
                        // FPU v2 two-word instruction (VECTOR / SCALAR /
                        // AUX). Word0 goes to the unit now; word1 is fetched
                        // through the normal instruction port. FPU v2
                        // instructions never consume a pending PFX12 prefix.
                        4'hc, 4'hd, 4'he: begin
                            fpu2_word <= instruction;
                            fpu2_word_valid <= 1;
                            fpu2_word0 <= instruction;
                            fpu2_fault_pc <= instruction_pc;
                            fpu2_seen_complete <= 0;
                            fpu2_is_memory <= 0;
                            fpu2_write_back <= 0;
                            state <= ST_FPU2_WORD1;
                        end
                        default: begin
                            fault_code <= FAULT_INVALID_INSTRUCTION;
                            fault_pc <= current_fault_pc;
                            state <= ST_FAULT;
                        end
                    endcase
                end
                // A queue hit can provide the next sequential instruction in
                // the same cycle that the current one retires. These
                // assignments intentionally follow the opcode case so they
                // replace its ST_FETCH_REQUEST transition only for the
                // explicitly pipelineable subset above.
                if (execute_fetch_accepted) begin
                    if (instruction_response_valid) begin
                        if (instruction_error) begin
                            fault_code <= FAULT_INSTRUCTION_MEMORY;
                            fault_pc <= pc_register;
                            state <= ST_FAULT;
                        end else begin
                            instruction <= instruction_data;
                            instruction_pc <= pc_register;
                            pc_register <= pc_register + 1'b1;
                            state <= ST_EXECUTE;
                        end
                    end else begin
                        state <= ST_FETCH_RESPONSE;
                    end
                end
            end
            ST_DATA_REQUEST: begin
                if (data_request_ready)
                    state <= ST_DATA_RESPONSE;
            end
            ST_ASYNC_STORE_WAIT: begin
                // Wait until the channel is free and, when an FST drain is in
                // flight, until every pending FST word has been issued: a CPU
                // store or load must never overtake an earlier FPU store.
                if (!async_store_valid && fpu2_pending == 0) begin
                    if (pending_write) begin
                        async_store_valid <= 1;
                        async_store_issued <= 0;
                        async_store_address <= pending_address;
                        async_store_data <= pending_write_data;
                        async_store_fault_pc <= pending_fault_pc;
                        retired_words <= retired_words + pending_retire_words;
                        state <= ST_FETCH_REQUEST;
                    end else begin
                        state <= ST_DATA_REQUEST;
                    end
                end
            end
            ST_DATA_RESPONSE: begin
                if (data_response_valid) begin
                    if (data_error) begin
                        fault_code <= FAULT_DATA_MEMORY;
                        fault_pc <= pending_fault_pc;
                        state <= ST_FAULT;
                    end else begin
                        if (!pending_write) begin
                            gpr_write_enable <= 1;
                            gpr_write_address <= pending_destination;
                            gpr_write_data <= data_read_data;
                        end
                        retired_words <= retired_words + pending_retire_words;
                        state <= ST_FETCH_REQUEST;
                    end
                end
            end
            ST_MULTIPLY_WAIT: state <= ST_MULTIPLY_COMMIT;
            ST_MULTIPLY_COMMIT: begin
                gpr_write_enable <= 1;
                gpr_write_address <= multiply_destination;
                // The indexed part-select picks the [15:0]/[23:8]/[31:16] window
                // of the full unsigned 32-bit product.
                gpr_write_data <= multiplier_product[multiply_shift +: 16];
                retired_words <= retired_words + multiply_retire_words;
                state <= ST_FETCH_REQUEST;
            end
            ST_FPU2_WORD1: begin
                // Fetch the second instruction word through the instruction
                // port; the unit's front-end accepts it on the beat after
                // the response arrives.
                if (instruction_response_valid) begin
                    if (instruction_error) begin
                        fault_code <= FAULT_INSTRUCTION_MEMORY;
                        fault_pc <= pc_register;
                        fpu2_abort <= 1;
                        state <= ST_FAULT;
                    end else begin
                        fpu2_word <= instruction_data;
                        fpu2_word_valid <= 1;
                        pc_register <= pc_register + 1'b1;
                        // AUX kind 00, subops FLD (0x00) / FST (0x01).
                        fpu2_subop <= instruction_data[9:4];
                        fpu2_fd <= instruction_data[15:10];
                        fpu2_fa <= fpu2_word0[7:2];
                        fpu2_is_memory <= instruction[15:12] == 4'he &&
                            fpu2_word0[1:0] == 2'b00 &&
                            instruction_data[9:4] <= 6'h01;
                        fpu2_write_back <= instruction_data[9:4] == 6'h01;
                        // mode[1:0]: 00 scalar, 01 vec2, 10 vec3, 11 vec4, so
                        // len = mode[1:0] + 1. Reserved mode[3:2] != 00 is
                        // defined to behave as the scalar form (len 1):
                        // software must not emit it, so no vector semantics
                        // are attached to the reserved bits.
                        fpu2_len <= instruction_data[3:2] != 2'b00 ? 3'd1 :
                                    {1'b0, instruction_data[1:0]} + 3'd1;
                        fpu2_address <= {data_segment_register,
                                         gpr_read_a_data};
                        fpu2_beat <= 0;
                        state <= ST_FPU2_EXEC;
                    end
                end
            end
            ST_FPU2_EXEC: begin
                if (fpu2_instr_complete)
                    fpu2_seen_complete <= 1;
                if (fpu2_is_memory) begin
                    // No FPU memory op may start while an earlier FST drain is
                    // in flight: a load must not overtake it, and a new FST
                    // capture must not overwrite the buffer payload. The async
                    // channel must also be idle, since the FPU memory states
                    // own the same data port. FST then captures its source
                    // vector (early release) while FLD takes the blocking
                    // data-port path.
                    if (fpu2_pending == 0 && !async_store_valid) begin
                        fpu2_beat <= 0;
                        state <= fpu2_write_back ? ST_FPU2_CAPTURE :
                                                   ST_FPU2_MEM_REQUEST;
                    end
                end else if (fpu2_seen_complete && !fpu2_busy) begin
                    retired_words <= retired_words + 2;
                    state <= ST_FETCH_REQUEST;
                end
            end
            ST_FPU2_CAPTURE: begin
                // The F value indexed by `fpu2_beat` was requested through the
                // ext read port on the previous beat (address fa+beat); capture
                // it now while presenting fa+beat+1 for the next beat. The last
                // capture retires the instruction with the whole vector pending
                // for the early-release drain.
                case (fpu2_beat)
                    4'd0: fpu2_buf0 <= fpu2_ext_read_data;
                    4'd1: fpu2_buf1 <= fpu2_ext_read_data;
                    4'd2: fpu2_buf2 <= fpu2_ext_read_data;
                    4'd3: fpu2_buf3 <= fpu2_ext_read_data;
                endcase
                if (fpu2_beat == fpu2_len - 1'b1) begin
                    fpu2_store_address <= fpu2_address;
                    fpu2_store_fault_pc <= fpu2_fault_pc;
                    fpu2_pending <= fpu2_double_len;
                    fpu2_pending_index <= 0;
                    retired_words <= retired_words + 2;
                    state <= ST_FETCH_REQUEST;
                end else begin
                    fpu2_beat <= fpu2_beat + 1'b1;
                end
            end
            ST_FPU2_MEM_REQUEST: begin
                if (data_request_ready)
                    state <= ST_FPU2_MEM_RESPONSE;
            end
            ST_FPU2_MEM_RESPONSE: begin
                if (data_response_valid) begin
                    if (data_error) begin
                        fault_code <= FAULT_DATA_MEMORY;
                        fault_pc <= fpu2_fault_pc;
                        fpu2_abort <= 1;
                        state <= ST_FAULT;
                    end else if (!fpu2_beat[0]) begin
                        // Low half: FLD keeps it for the register write on the
                        // following (high) beat; FST has already sent it.
                        fpu2_low <= data_read_data;
                        fpu2_beat <= fpu2_beat + 1'b1;
                        state <= ST_FPU2_MEM_REQUEST;
                    end else if (fpu2_beat == fpu2_last_beat) begin
                        // Last high half retires the whole vector. FLD's
                        // register write commits at this edge
                        // (fpu2_ext_write_enable, addressed by beat_index).
                        retired_words <= retired_words + 2;
                        state <= ST_FETCH_REQUEST;
                    end else begin
                        // High half of a non-final register: FLD's write
                        // commits at this edge, then the next register's low
                        // half begins.
                        fpu2_beat <= fpu2_beat + 1'b1;
                        state <= ST_FPU2_MEM_REQUEST;
                    end
                end
            end
            ST_RESET_CLEAR: begin
                // Reset walks the scalar register file back to zero one word
                // per cycle through its synchronous write port.
                gpr_write_enable <= 1;
                gpr_write_address <= clear_index;
                gpr_write_data <= 0;
                if (clear_index == 15)
                    state <= ST_FETCH_REQUEST;
                else
                    clear_index <= clear_index + 1'b1;
            end
            default: state <= state;
        endcase
        // FST early-release drain: move the next pending word into the async
        // store channel when it is free. This assignment deliberately follows
        // the phase case (and the async-store completion above), so a drain
        // that completes this cycle hands the channel straight to the next
        // word, while a CPU store enqueued in ST_EXECUTE still wins the free
        // channel (fpu2_refill is gated on cpu_store_enqueue).
        if (fpu2_refill) begin
            async_store_valid <= 1;
            async_store_issued <= 0;
            async_store_address <= fpu2_store_address + fpu2_pending_index;
            async_store_data <= fpu2_word_half;
            async_store_fault_pc <= fpu2_store_fault_pc;
            fpu2_pending <= fpu2_pending - 1'b1;
            fpu2_pending_index <= fpu2_pending_index + 1'b1;
        end
        end
    end
end
endmodule
