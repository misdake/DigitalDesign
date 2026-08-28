module CpuV3Core (
    input wire clk,
    input wire reset,
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
    output wire [15:0] halt_signal,
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
localparam [4:0] ST_FPU_ROM_NORMALIZE = 9;
localparam [4:0] ST_FPU_EXECUTE = 10;
localparam [4:0] ST_FPU_WRITE_LANES = 11;
localparam [4:0] ST_FPU_GATHER_READ = 12;
localparam [4:0] ST_FPU_GATHER_WRITE = 13;
localparam [4:0] ST_FPU_SCATTER = 14;
localparam [4:0] ST_FPU_TRANSPOSE = 15;
localparam [4:0] ST_FPU_MULTIPLY_WAIT = 16;
localparam [4:0] ST_FPU_MULTIPLY_COMMIT = 17;
localparam [4:0] ST_FPU_MULTIPLY_SETTLE = 23;
localparam [4:0] ST_FPU_ROM_ADDRESS = 18;
localparam [4:0] ST_FPU_ROM_WAIT = 19;
localparam [4:0] ST_FPU_ROM_COMMIT = 20;
localparam [4:0] ST_FPU_COMMIT = 21;
localparam [4:0] ST_RESET_CLEAR = 22;
localparam [4:0] ST_FPU_ROM_LOOKUP = 25;
localparam [4:0] ST_FPU_MULTIPLY_PIPELINE = 26;
localparam [4:0] ST_FPU_UNARY_DISPATCH = 27;
localparam [4:0] ST_FPU_ROM_WRITE = 28;

localparam [7:0] FAULT_INVALID_INSTRUCTION = 1;
localparam [7:0] FAULT_FPU_DOMAIN = 2;
localparam [7:0] FAULT_INSTRUCTION_MEMORY = 3;
localparam [7:0] FAULT_DATA_MEMORY = 4;

// Pending test result encoding, set by CMP-class instructions and consumed
// by conditional branches.
localparam [1:0] TEST_LESS = 0;
localparam [1:0] TEST_EQUAL = 1;
localparam [1:0] TEST_GREATER = 2;

reg [4:0] state = ST_FETCH_REQUEST;
reg [15:0] registers [0:15];
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

reg [3:0] multiply_destination = 0;
reg [1:0] multiply_retire_words = 0;
wire signed [17:0] multiplier_left;
wire signed [17:0] multiplier_right;
wire signed [35:0] multiplier_product;

// The sixteen four-lane F registers live in a dual-asynchronous-read SSRAM
// organized as 16 x 64-bit vectors with per-lane write enables. A registered
// issue address makes the whole vector combinationally readable, so
// data-movement instructions transfer a full vec4 per cycle while the
// serialized lane ALU still consumes one lane at a time.
wire [63:0] fpu_rf_read_a_data;
wire [63:0] fpu_rf_read_b_data;
reg [3:0] fpu_rf_read_a_address;
reg [3:0] fpu_rf_read_b_address;
// The write port is driven only by FSM-registered signals, so a write fires
// one cycle after the state that computed it and no state-decode logic ever
// sits on the RAM data or address paths. Single-lane writes replicate the
// value across all four lanes and enable only the target lane.
reg [3:0] fpu_rf_write_enable = 0;
reg [3:0] fpu_rf_write_address = 0;
reg [63:0] fpu_rf_write_data = 0;

reg [4:0] fpu_step = 0;
reg signed [39:0] fpu_accumulator = 0;
reg fpu_memory_active = 0;
reg [1:0] fpu_memory_lane = 0;
reg signed [15:0] fpu_memory_value [0:3];
reg [15:0] fpu_scalar = 0;
reg [9:0] fpu_rom_address = 0;
wire [15:0] fpu_rom_read_data;
reg signed [5:0] fpu_rom_exponent = 0;
reg [7:0] fpu_rom_index = 0;
reg [16:0] fpu_normalized = 0;
reg fpu_rom_negative = 0;
reg [9:0] fpu_sine_phase = 0;
reg fpu_sine_endpoint = 0;
reg signed [15:0] fpu_rom_first = 0;
reg signed [15:0] fpu_rom_second = 0;
reg fpu_rom_step = 0;
// Whole-vector snapshots: export/scatter share one buffer, transpose keeps
// all four rows so the in-place rewrite stays snapshot-clean.
reg [63:0] fpu_vector_buffer = 0;
reg [63:0] fpu_row_0 = 0;
reg [63:0] fpu_row_1 = 0;
reg [63:0] fpu_row_2 = 0;
reg [63:0] fpu_row_3 = 0;
reg signed [15:0] fpu_operand_a = 0;
reg signed [15:0] fpu_operand_b = 0;
reg [15:0] fpu_result = 0;
reg [1:0] fpu_mul_valid = 0;
reg [1:0] fpu_mul_tag_0 = 0;
reg [1:0] fpu_mul_tag_1 = 0;
reg [1:0] fpu_retire_words = 0;
reg [15:0] fpu_fault_pc = 0;
reg [5:0] fpu_clear_index = 0;

wire [3:0] opcode = instruction[15:12];
wire [3:0] field_d = instruction[11:8];
wire [3:0] field_a = instruction[7:4];
wire [3:0] field_b = instruction[3:0];
wire fpu_sine_operation = field_d == 4'he && field_b == 4'h2;
// DSP lane operands index the wide asynchronous reads directly; the FSM parks
// both read addresses on Fa/Fb for the whole operation, so fpu_step selects
// the lane in flight.
wire [15:0] fpu_multiply_a_word = fpu_rf_read_a_data[fpu_step[1:0]*16 +: 16];
wire [15:0] fpu_multiply_b_word = fpu_rf_read_b_data[fpu_step[1:0]*16 +: 16];
wire signed [17:0] fpu_multiplier_left =
    {{2{fpu_multiply_a_word[15]}}, fpu_multiply_a_word};
wire signed [17:0] fpu_multiplier_right =
    fpu_sine_operation ? 18'sd41722 :
    (field_d == 4'hf ?
     (state == ST_FPU_EXECUTE ? {{2{fpu_multiply_b_word[15]}}, fpu_multiply_b_word} :
      {{2{fpu_scalar[15]}}, fpu_scalar}) :
     {{2{fpu_multiply_b_word[15]}}, fpu_multiply_b_word});
wire signed [35:0] fpu_multiplier_product;
wire prefix_consumer = opcode == 4'h8 || opcode == 4'h9 ||
                       (opcode == 4'ha &&
                        !(field_d >= 4'h5 && field_d <= 4'h7)) ||
                       (opcode == 4'hb &&
                        (field_d <= 4'h5 || field_d == 4'h8 || field_d == 4'h9));
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

// Products, accumulators, and lane results narrow by a constant 8 or 16
// bits, so their rounding is wires plus an increment; only ROM
// normalization and scaling need the shared variable shifter further down.
function [15:0] fix16_saturate17;
    input signed [16:0] value;
    begin
        if (value > 17'sd32767)
            fix16_saturate17 = 16'h7fff;
        else if (value < -17'sd32768)
            fix16_saturate17 = 16'h8000;
        else
            fix16_saturate17 = value[15:0];
    end
endfunction

function [15:0] fix16_from_product;
    input signed [35:0] value;
    reg [35:0] magnitude;
    reg [27:0] quotient;
    reg signed [28:0] rounded;
    begin
        magnitude = value[35] ? -value : value;
        quotient = magnitude[35:8];
        if (magnitude[7:0] > 8'h80 ||
            (magnitude[7:0] == 8'h80 && quotient[0]))
            quotient = quotient + 1'b1;
        rounded = value[35] ? -$signed({1'b0, quotient}) : $signed({1'b0, quotient});
        if (rounded > 29'sd32767)
            fix16_from_product = 16'h7fff;
        else if (rounded < -29'sd32768)
            fix16_from_product = 16'h8000;
        else
            fix16_from_product = rounded[15:0];
    end
endfunction

function [15:0] fix16_from_accumulator;
    input signed [39:0] value;
    reg [39:0] magnitude;
    reg [31:0] quotient;
    reg signed [32:0] rounded;
    begin
        magnitude = value[39] ? -value : value;
        quotient = magnitude[39:8];
        if (magnitude[7:0] > 8'h80 ||
            (magnitude[7:0] == 8'h80 && quotient[0]))
            quotient = quotient + 1'b1;
        rounded = value[39] ? -$signed({1'b0, quotient}) : $signed({1'b0, quotient});
        if (rounded > 33'sd32767)
            fix16_from_accumulator = 16'h7fff;
        else if (rounded < -33'sd32768)
            fix16_from_accumulator = 16'h8000;
        else
            fix16_from_accumulator = rounded[15:0];
    end
endfunction

function [15:0] fix16_round_integer;
    input signed [15:0] value;
    reg [15:0] magnitude;
    reg [7:0] quotient;
    reg signed [8:0] rounded;
    begin
        magnitude = value[15] ? -value : value;
        quotient = magnitude[15:8];
        if (magnitude[7:0] > 8'h80 ||
            (magnitude[7:0] == 8'h80 && quotient[0]))
            quotient = quotient + 1'b1;
        rounded = value[15] ? -$signed({1'b0, quotient}) : $signed({1'b0, quotient});
        if (rounded > 9'sd127)
            fix16_round_integer = 16'h7fff;
        else if (rounded < -9'sd128)
            fix16_round_integer = 16'h8000;
        else
            fix16_round_integer = {rounded[7:0], 8'b0};
    end
endfunction

function [9:0] fpu_phase_from_product;
    input signed [35:0] value;
    reg [35:0] magnitude;
    reg [19:0] quotient;
    reg signed [20:0] rounded;
    begin
        magnitude = value[35] ? -value : value;
        quotient = magnitude[35:16];
        if (magnitude[15:0] > 16'h8000 ||
            (magnitude[15:0] == 16'h8000 && quotient[0]))
            quotient = quotient + 1'b1;
        rounded = value[35] ? -$signed({1'b0, quotient}) : $signed({1'b0, quotient});
        fpu_phase_from_product = rounded[9:0];
    end
endfunction

// Shared variable shifter for the ROM paths: normalization (ST_FPU_ROM_NORMALIZE)
// and Q15 scaling (ST_FPU_ROM_COMMIT) are the only variable shifts, both
// over a 16-bit unsigned domain. Negative shifts widen to the left.
function [16:0] fpu_round_shift16;
    input [15:0] value;
    input signed [4:0] shift;
    reg [16:0] magnitude;
    reg [16:0] quotient;
    reg [16:0] remainder;
    reg [16:0] half;
    begin
        magnitude = {1'b0, value};
        if (shift > 0) begin
            quotient = magnitude >> shift;
            remainder = magnitude & ((17'd1 << shift) - 1'b1);
            half = 17'd1 << (shift - 1'b1);
            if (remainder > half || (remainder == half && quotient[0]))
                quotient = quotient + 1'b1;
        end else
            quotient = magnitude << -shift;
        fpu_round_shift16 = quotient;
    end
endfunction

function signed [39:0] fpu_accumulate_product;
    input signed [39:0] accumulator;
    input signed [35:0] product;
    reg signed [40:0] sum;
    begin
        sum = $signed({accumulator[39], accumulator}) +
              $signed({{5{product[35]}}, product});
        if (sum > 41'sd549755813887)
            fpu_accumulate_product = 40'sh7fffffffff;
        else if (sum < -41'sd549755813888)
            fpu_accumulate_product = 40'sh8000000000;
        else
            fpu_accumulate_product = sum[39:0];
    end
endfunction

function signed [5:0] fpu_normalize_exponent;
    input [15:0] magnitude;
    integer bit_index;
    reg found;
    begin
        fpu_normalize_exponent = -6'sd8;
        found = 0;
        for (bit_index = 15; bit_index >= 0; bit_index = bit_index - 1) begin
            if (!found && magnitude[bit_index]) begin
                fpu_normalize_exponent = bit_index - 8;
                found = 1;
            end
        end
    end
endfunction

function [7:0] fpu_sine_address;
    input [9:0] phase;
    begin
        if (!phase[8])
            fpu_sine_address = phase[7:0];
        else if (phase[7:0] == 0)
            fpu_sine_address = 0;
        else
            fpu_sine_address = 8'h00 - phase[7:0];
    end
endfunction

function fpu_sine_is_endpoint;
    input [9:0] phase;
    fpu_sine_is_endpoint = phase[8] && phase[7:0] == 0;
endfunction

// One priority encoder serves the ROM unary normalization path.
wire [15:0] fpu_unary_magnitude =
    fpu_operand_a[15] ? -fpu_operand_a : fpu_operand_a;
wire signed [5:0] fpu_unary_exponent = fpu_normalize_exponent(fpu_unary_magnitude);

// The ROM normalize (ST_FPU_ROM_NORMALIZE) and scale (ST_FPU_ROM_COMMIT) paths
// share the single variable shifter. Both shifter inputs are registered in
// earlier phases (the unary magnitude in ST_FPU_UNARY_DISPATCH) or come
// straight from the ROM output register, so no combinational cone crosses
// from the latched unary operand into the scale result.
reg [15:0] fpu_magnitude = 0;
reg [15:0] fpu_variable_input;
reg signed [4:0] fpu_variable_amount;
always @* begin
    if (state == ST_FPU_ROM_COMMIT) begin
        fpu_variable_input = fpu_rom_read_data;
        fpu_variable_amount = 5'sd7 + fpu_rom_exponent[4:0];
    end else begin
        fpu_variable_input = fpu_magnitude;
        fpu_variable_amount = fpu_rom_exponent[4:0];
    end
end
wire [16:0] fpu_variable_shifted = fpu_round_shift16(fpu_variable_input, fpu_variable_amount);
wire [15:0] fpu_rom_scaled = fpu_rom_negative ?
    (fpu_variable_shifted >= 17'd32768 ? 16'h8000 : -fpu_variable_shifted[15:0]) :
    (fpu_variable_shifted > 17'd32767 ? 16'h7fff : fpu_variable_shifted[15:0]);

// ALU lanes stage lane zero in ST_FPU_EXECUTE. Each following cycle writes
// that registered lane while the wide asynchronous reads stage the next lane,
// keeping the SSRAM read and ALU/writeback paths separated.
wire [1:0] fpu_write_lane = fpu_step[1:0];


// Single-lane add/sub operands for the serialized vector ALU.
wire signed [16:0] fpu_lane_sum =
    {fpu_operand_a[15], fpu_operand_a} + {fpu_operand_b[15], fpu_operand_b};
wire signed [16:0] fpu_lane_difference =
    {fpu_operand_a[15], fpu_operand_a} - {fpu_operand_b[15], fpu_operand_b};
wire [15:0] fpu_lane_addsub =
    fix16_saturate17(field_d == 4'h9 ? fpu_lane_difference : fpu_lane_sum);

// Single-lane unary results for the serialized vector unary operations.
wire signed [15:0] fpu_lane_left = fpu_operand_a;
reg [15:0] fpu_lane_result;
always @* begin
    case (field_b)
        4'h3: fpu_lane_result = fpu_lane_left == 16'sh8000 ? 16'h7fff :
            (fpu_lane_left[15] ? -fpu_lane_left : fpu_lane_left);
        4'h4: fpu_lane_result = fpu_lane_left == 16'sh8000 ? 16'h7fff : -fpu_lane_left;
        4'h5: fpu_lane_result = {fpu_lane_left[15:8], 8'b0};
        4'h6: fpu_lane_result = fpu_lane_left[7:0] == 0 ? fpu_lane_left :
            (fpu_lane_left > 16'sh7eff ? 16'h7fff : {fpu_lane_left[15:8] + 1'b1, 8'b0});
        4'h7: fpu_lane_result = fix16_round_integer(fpu_lane_left);
        4'h8: fpu_lane_result = fpu_lane_left < 0 ? 16'd0 :
            (fpu_lane_left > 16'sd256 ? 16'd256 : fpu_lane_left);
        4'h9: fpu_lane_result = fpu_lane_left < 0 ? -16'sd256 :
            (fpu_lane_left == 0 ? 16'd0 : 16'd256);
        default: fpu_lane_result = 16'd0;
    endcase
end

// Write data for the serial lane writer: the single shared ALU lane result.
wire [15:0] fpu_write_lanes_data =
    field_d <= 4'h9 ? fpu_lane_addsub : fpu_lane_result;

// Transpose write phase (steps 2..5): output row w gathers lane w from all
// four buffered rows, a pure wiring permutation of the snapshot. The two-bit
// subtraction wraps modulo four, landing exactly on 0..3 for steps 2..5.
wire [1:0] fpu_transpose_write_index = fpu_step[1:0] - 2'd2;
wire [15:0] fpu_transpose_lane_0 = fpu_row_0[fpu_transpose_write_index*16 +: 16];
wire [15:0] fpu_transpose_lane_1 = fpu_row_1[fpu_transpose_write_index*16 +: 16];
wire [15:0] fpu_transpose_lane_2 = fpu_row_2[fpu_transpose_write_index*16 +: 16];
wire [15:0] fpu_transpose_lane_3 = fpu_row_3[fpu_transpose_write_index*16 +: 16];
wire immediate_multiply = opcode == 4'ha && field_d == 4'h8;
wire [15:0] multiply_left_word = registers[field_a];
wire [15:0] multiply_right_word =
    immediate_multiply ? immediate_signed(instruction) : registers[field_b];
assign multiplier_left = {2'b0, multiply_left_word};
assign multiplier_right = immediate_multiply ?
    {{2{multiply_right_word[15]}}, multiply_right_word} :
    {2'b0, multiply_right_word};

__DSP_MULTIPLIER__ u_multiplier (
    .clk(clk),
    .a(multiplier_left),
    .b(multiplier_right),
    .product(multiplier_product)
);

__FPU_DSP_MULTIPLIER__ u_fpu_multiplier (
    .clk(clk),
    .a(fpu_multiplier_left),
    .b(fpu_multiplier_right),
    .product(fpu_multiplier_product)
);

__FPU_ROM__ u_fpu_rom (
    .clk(clk),
    .write_enable(1'b0),
    .address(fpu_rom_address),
    .write_data(16'b0),
    .read_data(fpu_rom_read_data)
);

__FPU_REGISTER_RAM__ u_fpu_register_ram (
    .clk(clk),
    .write_enable(fpu_rf_write_enable),
    .write_address(fpu_rf_write_address),
    .write_data(fpu_rf_write_data),
    .read_a_address(fpu_rf_read_a_address),
    .read_b_address(fpu_rf_read_b_address),
    .read_a_data(fpu_rf_read_a_data),
    .read_b_data(fpu_rf_read_b_data)
);

assign instruction_request_valid = state == ST_FETCH_REQUEST;
assign instruction_address = {code_segment_register, pc_register};
assign instruction_response_ready = state == ST_FETCH_RESPONSE;
assign data_request_valid = state == ST_DATA_REQUEST;
assign data_write = pending_write;
assign data_address = pending_address;
assign data_write_data = pending_write_data;
assign data_response_ready = state == ST_DATA_RESPONSE;
assign device_index = field_d[2:0];
assign device_channel = field_a;
assign device_read_enable = state == ST_EXECUTE && opcode == 4'hc && !field_d[3];
assign device_write_enable = state == ST_EXECUTE && opcode == 4'hc && field_d[3];
assign device_write_data = registers[field_b];
assign halted = state == ST_HALTED;
assign halt_signal = registers[0];
assign fault = state == ST_FAULT;
assign pc = pc_register;
assign code_segment = code_segment_register;
assign data_segment = data_segment_register;

integer register_index;
reg [15:0] left_value;
reg [15:0] right_value;
reg [15:0] immediate_value;
reg [15:0] logical_address;
reg branch_taken;
reg [15:0] jump_offset;
reg [15:0] jump_target;

always @(posedge clk) begin
    if (reset) begin
        fpu_rf_write_enable <= 1'b0;
        state <= ST_RESET_CLEAR;
        fpu_clear_index <= 0;
        pc_register <= 0;
        code_segment_register <= 0;
        data_segment_register <= 0;
        prefix_valid <= 0;
        pending_test_valid <= 0;
        pending_test_result <= 0;
        fpu_accumulator <= 0;
        fpu_memory_active <= 0;
        fpu_memory_lane <= 0;
        fpu_rf_read_a_address <= 0;
        fpu_rf_read_b_address <= 0;
        fpu_mul_valid <= 0;
        fpu_mul_tag_0 <= 0;
        fpu_mul_tag_1 <= 0;
        retired_words <= 0;
        fault_code <= 0;
        fault_pc <= 0;
        for (register_index = 0; register_index < 16; register_index = register_index + 1)
            registers[register_index] <= 0;
    end else begin
        fpu_rf_write_enable <= 1'b0;
        case (state)
            ST_FETCH_REQUEST: begin
                if (instruction_request_ready)
                    state <= ST_FETCH_RESPONSE;
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
                            registers[field_d] <= registers[field_a] + registers[field_b];
                            retired_words <= retired_words + success_retire_words;
                            state <= ST_FETCH_REQUEST;
                        end
                        4'h1: begin
                            registers[field_d] <= registers[field_a] - registers[field_b];
                            retired_words <= retired_words + success_retire_words;
                            state <= ST_FETCH_REQUEST;
                        end
                        4'h2: begin
                            multiply_destination <= field_d;
                            multiply_retire_words <= success_retire_words;
                            state <= ST_MULTIPLY_WAIT;
                        end
                        4'h3: begin
                            registers[field_d] <= registers[field_a] & registers[field_b];
                            retired_words <= retired_words + success_retire_words;
                            state <= ST_FETCH_REQUEST;
                        end
                        4'h4: begin
                            registers[field_d] <= registers[field_a] | registers[field_b];
                            retired_words <= retired_words + success_retire_words;
                            state <= ST_FETCH_REQUEST;
                        end
                        4'h5: begin
                            registers[field_d] <= registers[field_a] ^ registers[field_b];
                            retired_words <= retired_words + success_retire_words;
                            state <= ST_FETCH_REQUEST;
                        end
                        4'h6: begin
                            registers[field_d] <= registers[field_a] << registers[field_b][3:0];
                            retired_words <= retired_words + success_retire_words;
                            state <= ST_FETCH_REQUEST;
                        end
                        4'h7: begin
                            registers[field_d] <= $signed(registers[field_a]) >>> registers[field_b][3:0];
                            retired_words <= retired_words + success_retire_words;
                            state <= ST_FETCH_REQUEST;
                        end
                        4'h8, 4'h9: begin
                            logical_address = registers[field_a] + immediate_signed(instruction);
                            pending_write <= opcode == 4'h9;
                            pending_address <= {data_segment_register, logical_address};
                            pending_write_data <= registers[field_d];
                            pending_destination <= field_d;
                            pending_retire_words <= success_retire_words;
                            pending_fault_pc <= current_fault_pc;
                            state <= ST_DATA_REQUEST;
                        end
                        4'ha: begin
                            left_value = registers[field_a];
                            immediate_value = immediate_signed(instruction);
                            case (field_d)
                                4'h0: registers[field_a] <= left_value + immediate_value;
                                4'h1: registers[field_a] <= left_value - immediate_value;
                                4'h2: registers[field_a] <= left_value & immediate_unsigned(instruction);
                                4'h3: registers[field_a] <= left_value | immediate_unsigned(instruction);
                                4'h4: registers[field_a] <= left_value ^ immediate_unsigned(instruction);
                                4'h5: registers[field_a] <= left_value << instruction[3:0];
                                4'h6: registers[field_a] <= left_value >> instruction[3:0];
                                4'h7: registers[field_a] <= $signed(left_value) >>> instruction[3:0];
                                4'h8: begin
                                    multiply_destination <= field_a;
                                    multiply_retire_words <= success_retire_words;
                                    state <= ST_MULTIPLY_WAIT;
                                end
                                4'h9: registers[field_a] <= left_value == immediate_value;
                                4'ha: registers[field_a] <= $signed(left_value) < $signed(immediate_value);
                                4'hb: registers[field_a] <= left_value < immediate_unsigned(instruction);
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
                                4'he: registers[field_a] <= prefix_valid ?
                                    immediate_unsigned(instruction) : sign_extend4(instruction[3:0]);
                                4'hf: registers[field_a] <= immediate_unsigned(instruction);
                                default: begin
                                    fault_code <= FAULT_INVALID_INSTRUCTION;
                                    fault_pc <= current_fault_pc;
                                    state <= ST_FAULT;
                                end
                            endcase
                            if (field_d != 4'h8) begin
                                retired_words <= retired_words + success_retire_words;
                                state <= ST_FETCH_REQUEST;
                            end
                        end
                        4'hb: begin
                            jump_offset = prefix_valid ?
                                {prefix_high[7:0], instruction[7:0]} :
                                sign_extend8(instruction[7:0]);
                            if (field_d <= 4'h5) begin
                                // Conditional branches consume the pending
                                // test result left by a CMP-class instruction.
                                if (!pending_test_valid) begin
                                    fault_code <= FAULT_INVALID_INSTRUCTION;
                                    fault_pc <= current_fault_pc;
                                    state <= ST_FAULT;
                                end else begin
                                    case (field_d)
                                        0: branch_taken = pending_test_result == TEST_EQUAL;
                                        1: branch_taken = pending_test_result != TEST_EQUAL;
                                        2: branch_taken = pending_test_result == TEST_LESS;
                                        3: branch_taken = pending_test_result != TEST_LESS;
                                        4: branch_taken = pending_test_result == TEST_GREATER;
                                        default: branch_taken = pending_test_result != TEST_GREATER;
                                    endcase
                                    if (branch_taken)
                                        pc_register <= pc_register + jump_offset;
                                    retired_words <= retired_words + success_retire_words;
                                    state <= ST_FETCH_REQUEST;
                                end
                            end else if (field_d == 4'h8) begin
                                // JREL: unconditional relative jump, no link.
                                pc_register <= pc_register + jump_offset;
                                retired_words <= retired_words + success_retire_words;
                                state <= ST_FETCH_REQUEST;
                            end else if (field_d == 4'h9) begin
                                // JALREL: link the fall-through address into r14.
                                registers[4'he] <= pc_register;
                                pc_register <= pc_register + jump_offset;
                                retired_words <= retired_words + success_retire_words;
                                state <= ST_FETCH_REQUEST;
                            end else begin
                                fault_code <= FAULT_INVALID_INSTRUCTION;
                                fault_pc <= current_fault_pc;
                                state <= ST_FAULT;
                            end
                        end
                        4'hc: begin
                            if (!field_d[3])
                                registers[field_b] <= device_read_data;
                            retired_words <= retired_words + success_retire_words;
                            state <= ST_FETCH_REQUEST;
                        end
                        4'hd: begin
                            fpu_retire_words <= success_retire_words;
                            fpu_fault_pc <= current_fault_pc;
                            fpu_step <= 0;
                            fpu_rf_read_a_address <= field_a;
                            fpu_rf_read_b_address <= field_b;
                            state <= ST_FPU_EXECUTE;
                        end
                        4'he: begin
                            case (field_d)
                                0: begin
                                    registers[field_a] <= population_count(registers[field_b]);
                                    retired_words <= retired_words + success_retire_words;
                                    state <= ST_FETCH_REQUEST;
                                end
                                1: begin
                                    registers[field_a] <= registers[field_b];
                                    retired_words <= retired_words + success_retire_words;
                                    state <= ST_FETCH_REQUEST;
                                end
                                2: begin
                                    registers[field_a] <= ~registers[field_b];
                                    retired_words <= retired_words + success_retire_words;
                                    state <= ST_FETCH_REQUEST;
                                end
                                3: begin
                                    registers[field_a] <= -registers[field_b];
                                    retired_words <= retired_words + success_retire_words;
                                    state <= ST_FETCH_REQUEST;
                                end
                                4: if (field_a == 0) begin
                                    pc_register <= registers[field_b];
                                    retired_words <= retired_words + success_retire_words;
                                    state <= ST_FETCH_REQUEST;
                                end else begin
                                    fault_code <= FAULT_INVALID_INSTRUCTION;
                                    fault_pc <= current_fault_pc;
                                    state <= ST_FAULT;
                                end
                                // JALR: the link field is architecturally
                                // fixed to r14.
                                5: if (field_a == 4'he) begin
                                    jump_target = registers[field_b];
                                    registers[4'he] <= pc_register;
                                    pc_register <= jump_target;
                                    retired_words <= retired_words + success_retire_words;
                                    state <= ST_FETCH_REQUEST;
                                end else begin
                                    fault_code <= FAULT_INVALID_INSTRUCTION;
                                    fault_pc <= current_fault_pc;
                                    state <= ST_FAULT;
                                end
                                6: begin
                                    registers[field_a] <= {{8{registers[field_b][7]}}, registers[field_b][7:0]};
                                    retired_words <= retired_words + success_retire_words;
                                    state <= ST_FETCH_REQUEST;
                                end
                                7: begin
                                    registers[field_a] <= count_leading_zeros(registers[field_b]);
                                    retired_words <= retired_words + success_retire_words;
                                    state <= ST_FETCH_REQUEST;
                                end
                                8: if (field_a == 0 && field_b == 0) begin
                                    retired_words <= retired_words + success_retire_words;
                                    state <= ST_HALTED;
                                end else begin
                                    fault_code <= FAULT_INVALID_INSTRUCTION;
                                    fault_pc <= current_fault_pc;
                                    state <= ST_FAULT;
                                end
                                9: begin
                                    registers[field_a] <= $signed(registers[field_a]) < $signed(registers[field_b]);
                                    retired_words <= retired_words + success_retire_words;
                                    state <= ST_FETCH_REQUEST;
                                end
                                10: begin
                                    registers[field_a] <= registers[field_a] < registers[field_b];
                                    retired_words <= retired_words + success_retire_words;
                                    state <= ST_FETCH_REQUEST;
                                end
                                // CMPS: pending test = signed ordering of
                                // r[rd] and r[rs]; no register is written.
                                11: begin
                                    pending_test_valid <= 1;
                                    pending_test_result <=
                                        registers[field_a] == registers[field_b] ? TEST_EQUAL :
                                        $signed(registers[field_a]) < $signed(registers[field_b]) ? TEST_LESS :
                                        TEST_GREATER;
                                    retired_words <= retired_words + success_retire_words;
                                    state <= ST_FETCH_REQUEST;
                                end
                                // CMPU: pending test = unsigned ordering of
                                // r[rd] and r[rs]; no register is written.
                                12: begin
                                    pending_test_valid <= 1;
                                    pending_test_result <=
                                        registers[field_a] == registers[field_b] ? TEST_EQUAL :
                                        registers[field_a] < registers[field_b] ? TEST_LESS :
                                        TEST_GREATER;
                                    retired_words <= retired_words + success_retire_words;
                                    state <= ST_FETCH_REQUEST;
                                end
                                13: begin
                                    if (field_b == 0)
                                        registers[field_a] <= code_segment_register;
                                    else if (field_b == 1)
                                        registers[field_a] <= data_segment_register;
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
                                        data_segment_register <= registers[field_b];
                                        retired_words <= retired_words + success_retire_words;
                                        state <= ST_FETCH_REQUEST;
                                    end else begin
                                        fault_code <= FAULT_INVALID_INSTRUCTION;
                                        fault_pc <= current_fault_pc;
                                        state <= ST_FAULT;
                                    end
                                end
                                15: begin
                                    code_segment_register <= registers[field_a];
                                    pc_register <= registers[field_b];
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
                        default: begin
                            fault_code <= FAULT_INVALID_INSTRUCTION;
                            fault_pc <= current_fault_pc;
                            state <= ST_FAULT;
                        end
                    endcase
                end
            end
            ST_DATA_REQUEST: begin
                if (data_request_ready)
                    state <= ST_DATA_RESPONSE;
            end
            ST_DATA_RESPONSE: begin
                if (data_response_valid) begin
                    if (data_error) begin
                        fpu_memory_active <= 0;
                        fault_code <= FAULT_DATA_MEMORY;
                        fault_pc <= pending_fault_pc;
                        state <= ST_FAULT;
                    end else if (fpu_memory_active) begin
                        if (!pending_write)
                            fpu_memory_value[fpu_memory_lane] <= data_read_data;
                        if (fpu_memory_lane == 3) begin
                            fpu_memory_active <= 0;
                            if (!pending_write) begin
                                // Imported beats land in the register file as
                                // one wide vector once the fourth transfer
                                // confirmed.
                                fpu_rf_write_enable <= 4'b1111;
                                fpu_rf_write_address <= pending_destination;
                                fpu_rf_write_data <= {data_read_data,
                                    fpu_memory_value[2], fpu_memory_value[1],
                                    fpu_memory_value[0]};
                                state <= ST_FPU_COMMIT;
                            end else begin
                                retired_words <= retired_words + pending_retire_words;
                                state <= ST_FETCH_REQUEST;
                            end
                        end else begin
                            // Exported beats stream from the dispatch-time
                            // vector snapshot, so no FPR reads remain here.
                            fpu_memory_lane <= fpu_memory_lane + 1'b1;
                            pending_address <= pending_address + 1'b1;
                            pending_write_data <=
                                fpu_vector_buffer[(fpu_memory_lane + 2'd1)*16 +: 16];
                            state <= ST_DATA_REQUEST;
                        end
                    end else begin
                        if (!pending_write)
                            registers[pending_destination] <= data_read_data;
                        retired_words <= retired_words + pending_retire_words;
                        state <= ST_FETCH_REQUEST;
                    end
                end
            end
            ST_MULTIPLY_WAIT: state <= ST_MULTIPLY_COMMIT;
            ST_MULTIPLY_COMMIT: begin
                registers[multiply_destination] <= multiplier_product[15:0];
                retired_words <= retired_words + multiply_retire_words;
                state <= ST_FETCH_REQUEST;
            end
            ST_FPU_EXECUTE: begin
                case (field_d)
                    0: begin
                        // FLOAD: one wide write, lane zero plus cleared lanes.
                        fpu_rf_write_enable <= 4'b1111;
                        fpu_rf_write_address <= field_a;
                        fpu_rf_write_data <= {48'd0, registers[field_b]};
                        state <= ST_FPU_COMMIT;
                    end
                    1: begin
                        registers[field_a] <= fpu_rf_read_b_data[15:0];
                        state <= ST_FPU_COMMIT;
                    end
                    2, 3: begin
                        if (registers[field_b][1:0] != 0) begin
                            fault_code <= FAULT_DATA_MEMORY;
                            fault_pc <= fpu_fault_pc;
                            state <= ST_FAULT;
                        end else begin
                            fpu_memory_active <= 1;
                            fpu_memory_lane <= 0;
                            pending_write <= field_d == 3;
                            pending_address <= {data_segment_register, registers[field_b]};
                            pending_write_data <= fpu_rf_read_a_data[15:0];
                            fpu_vector_buffer <= fpu_rf_read_a_data;
                            pending_destination <= field_a;
                            pending_retire_words <= fpu_retire_words;
                            pending_fault_pc <= fpu_fault_pc;
                            state <= ST_DATA_REQUEST;
                        end
                    end
                    4: begin
                        // FMOV: one wide vector copy.
                        fpu_rf_write_enable <= 4'b1111;
                        fpu_rf_write_address <= field_a;
                        fpu_rf_write_data <= fpu_rf_read_b_data;
                        state <= ST_FPU_COMMIT;
                    end
                    5: begin
                        if (field_b <= 12) begin
                            // Pack4: port B currently reads Fb; the remaining
                            // snapshot reads run two vectors per cycle.
                            fpu_memory_value[0] <= fpu_rf_read_b_data[15:0];
                            fpu_rf_read_a_address <= field_b + 4'd1;
                            fpu_rf_read_b_address <= field_b + 4'd2;
                            fpu_step <= 0;
                            state <= ST_FPU_GATHER_READ;
                        end else begin
                            fault_code <= FAULT_INVALID_INSTRUCTION;
                            fault_pc <= fpu_fault_pc;
                            state <= ST_FAULT;
                        end
                    end
                    6: begin
                        if (field_a <= 12) begin
                            // Unpack4 snapshots the source vector so a
                            // destination range overlapping Fb stays clean.
                            fpu_vector_buffer <= fpu_rf_read_b_data;
                            fpu_step <= 0;
                            state <= ST_FPU_SCATTER;
                        end else begin
                            fault_code <= FAULT_INVALID_INSTRUCTION;
                            fault_pc <= fpu_fault_pc;
                            state <= ST_FAULT;
                        end
                    end
                    7: begin
                        if (field_a <= 12 && field_b == 0) begin
                            // Transpose snapshots all four rows before any
                            // write; port A currently reads row Fa.
                            fpu_row_0 <= fpu_rf_read_a_data;
                            fpu_rf_read_a_address <= field_a + 4'd1;
                            fpu_rf_read_b_address <= field_a + 4'd2;
                            fpu_step <= 0;
                            state <= ST_FPU_TRANSPOSE;
                        end else begin
                            fault_code <= FAULT_INVALID_INSTRUCTION;
                            fault_pc <= fpu_fault_pc;
                            state <= ST_FAULT;
                        end
                    end
                    8, 9: begin
                        fpu_operand_a <= fpu_rf_read_a_data[15:0];
                        fpu_operand_b <= fpu_rf_read_b_data[15:0];
                        fpu_step <= 0;
                        state <= ST_FPU_WRITE_LANES;
                    end
                    10, 11, 15: begin
                        // Latch the broadcast scalar: earlier lane commits
                        // may overwrite Fb.x when Fa and Fb alias.
                        fpu_scalar <= fpu_rf_read_b_data[15:0];
                        fpu_step <= 1;
                        fpu_mul_valid <= 2'b01;
                        fpu_mul_tag_0 <= 0;
                        state <= ST_FPU_MULTIPLY_PIPELINE;
                    end
                    12: begin
                        if (field_b <= 3) begin
                            fpu_rf_write_enable <= 4'b0001 << field_b[1:0];
                            fpu_rf_write_address <= field_a;
                            fpu_rf_write_data <=
                                {4{fix16_from_accumulator(fpu_accumulator)}};
                            fpu_accumulator <= 0;
                            state <= ST_FPU_COMMIT;
                        end else begin
                            fault_code <= FAULT_INVALID_INSTRUCTION;
                            fault_pc <= fpu_fault_pc;
                            state <= ST_FAULT;
                        end
                    end
                    13: begin
                        pending_test_valid <= 1;
                        pending_test_result <=
                            fpu_rf_read_a_data[15:0] == fpu_rf_read_b_data[15:0] ? TEST_EQUAL :
                            $signed(fpu_rf_read_a_data[15:0]) < $signed(fpu_rf_read_b_data[15:0]) ? TEST_LESS :
                            TEST_GREATER;
                        state <= ST_FPU_COMMIT;
                    end
                    14: begin
                        case (field_b)
                            0, 1: begin
                                fpu_operand_a <= fpu_rf_read_a_data[15:0];
                                state <= ST_FPU_UNARY_DISPATCH;
                            end
                            2: begin
                                fpu_rom_step <= 0;
                                fpu_step <= 0;
                                state <= ST_FPU_MULTIPLY_WAIT;
                            end
                            3, 4, 5, 6, 7, 8, 9, 10: begin
                                fpu_operand_a <= fpu_rf_read_a_data[15:0];
                                fpu_operand_b <= fpu_rf_read_b_data[15:0];
                                fpu_step <= 0;
                                state <= ST_FPU_WRITE_LANES;
                            end
                            default: begin
                                fault_code <= FAULT_INVALID_INSTRUCTION;
                                fault_pc <= fpu_fault_pc;
                                state <= ST_FAULT;
                            end
                        endcase
                    end
                    default: begin
                        fault_code <= FAULT_INVALID_INSTRUCTION;
                        fault_pc <= fpu_fault_pc;
                        state <= ST_FAULT;
                    end
                endcase
            end
            ST_FPU_UNARY_DISPATCH: begin
                if ((field_b == 0 && fpu_operand_a == 0) ||
                    (field_b == 1 && $signed(fpu_operand_a) <= 0)) begin
                    fault_code <= FAULT_FPU_DOMAIN;
                    fault_pc <= fpu_fault_pc;
                    state <= ST_FAULT;
                end else begin
                    fpu_rom_negative <= field_b == 0 && fpu_operand_a[15];
                    fpu_magnitude <= fpu_unary_magnitude;
                    fpu_rom_exponent <= fpu_unary_exponent;
                    state <= ST_FPU_ROM_NORMALIZE;
                end
            end
            ST_FPU_WRITE_LANES: begin
                if (fpu_step < 3) begin
                    fpu_operand_a <= fpu_rf_read_a_data[(fpu_step[1:0] + 2'd1)*16 +: 16];
                    fpu_operand_b <= fpu_rf_read_b_data[(fpu_step[1:0] + 2'd1)*16 +: 16];
                end
                fpu_rf_write_enable <= 4'b0001 << fpu_write_lane;
                fpu_rf_write_address <= field_a;
                fpu_rf_write_data <= {4{fpu_write_lanes_data}};
                if (fpu_step == 3) begin
                    fpu_step <= 0;
                    state <= ST_FPU_COMMIT;
                end else begin
                    fpu_step <= fpu_step + 1'b1;
                end
            end
            ST_FPU_MULTIPLY_PIPELINE: begin
                // The DSP has two registered stages. Tags follow its products
                // so one lane can be issued every cycle and committed in order.
                fpu_mul_valid <= {fpu_mul_valid[0], fpu_step < 4};
                fpu_mul_tag_1 <= fpu_mul_tag_0;
                if (fpu_step < 4) begin
                    fpu_mul_tag_0 <= fpu_step[1:0];
                    fpu_step <= fpu_step + 1'b1;
                end
                if (fpu_mul_valid[1]) begin
                    if (field_d == 11) begin
                        fpu_accumulator <=
                            fpu_accumulate_product(fpu_accumulator, fpu_multiplier_product);
                    end else begin
                        fpu_rf_write_enable <= 4'b0001 << fpu_mul_tag_1;
                        fpu_rf_write_address <= field_a;
                        fpu_rf_write_data <= {4{fix16_from_product(fpu_multiplier_product)}};
                    end
                    if (fpu_mul_tag_1 == 3) begin
                        fpu_step <= 0;
                        fpu_mul_valid <= 0;
                        state <= ST_FPU_COMMIT;
                    end
                end
            end
            ST_FPU_GATHER_READ: begin
                // Pack4 snapshots the four lane-x sources two vectors per
                // cycle so overlapping source and destination ranges stay
                // snapshot-clean.
                if (fpu_step == 0) begin
                    fpu_memory_value[1] <= fpu_rf_read_a_data[15:0];
                    fpu_memory_value[2] <= fpu_rf_read_b_data[15:0];
                    fpu_rf_read_a_address <= field_b + 4'd3;
                    fpu_step <= 1;
                end else begin
                    fpu_memory_value[3] <= fpu_rf_read_a_data[15:0];
                    fpu_step <= 0;
                    state <= ST_FPU_GATHER_WRITE;
                end
            end
            ST_FPU_GATHER_WRITE: begin
                fpu_rf_write_enable <= 4'b1111;
                fpu_rf_write_address <= field_a;
                fpu_rf_write_data <= {fpu_memory_value[3], fpu_memory_value[2],
                    fpu_memory_value[1], fpu_memory_value[0]};
                state <= ST_FPU_COMMIT;
            end
            ST_FPU_SCATTER: begin
                // One wide write per destination vector: lane zero carries the
                // selected source lane, the other lanes clear.
                fpu_rf_write_enable <= 4'b1111;
                fpu_rf_write_address <= field_a + {2'b00, fpu_step[1:0]};
                fpu_rf_write_data <=
                    {48'd0, fpu_vector_buffer[fpu_step[1:0]*16 +: 16]};
                if (fpu_step == 3) begin
                    fpu_step <= 0;
                    state <= ST_FPU_COMMIT;
                end else begin
                    fpu_step <= fpu_step + 1'b1;
                end
            end
            ST_FPU_TRANSPOSE: begin
                // Steps 0-1 snapshot the remaining rows two per cycle; steps
                // 2-5 write one transposed row per cycle from the snapshot.
                if (fpu_step == 0) begin
                    fpu_row_1 <= fpu_rf_read_a_data;
                    fpu_row_2 <= fpu_rf_read_b_data;
                    fpu_rf_read_a_address <= field_a + 4'd3;
                end
                if (fpu_step == 1)
                    fpu_row_3 <= fpu_rf_read_a_data;
                if (fpu_step >= 2) begin
                    fpu_rf_write_enable <= 4'b1111;
                    fpu_rf_write_address <=
                        field_a + {2'b00, fpu_transpose_write_index};
                    fpu_rf_write_data <= {fpu_transpose_lane_3,
                        fpu_transpose_lane_2, fpu_transpose_lane_1,
                        fpu_transpose_lane_0};
                end
                if (fpu_step == 5) begin
                    fpu_step <= 0;
                    state <= ST_FPU_COMMIT;
                end else begin
                    fpu_step <= fpu_step + 1'b1;
                end
            end
            // SINCOS keeps the non-streaming DSP path because it issues one
            // range-reduction multiply followed by two ROM reads.
            ST_FPU_MULTIPLY_WAIT: state <= ST_FPU_MULTIPLY_SETTLE;
            ST_FPU_MULTIPLY_SETTLE: state <= ST_FPU_MULTIPLY_COMMIT;
            ST_FPU_MULTIPLY_COMMIT: begin
                fpu_sine_phase <= fpu_phase_from_product(fpu_multiplier_product);
                state <= ST_FPU_ROM_LOOKUP;
            end
            ST_FPU_ROM_NORMALIZE: begin
                // Register the barrel-shifter output before endpoint handling
                // so normalization and exponent adjustment are separate hops.
                fpu_normalized <= fpu_variable_shifted;
                state <= ST_FPU_ROM_ADDRESS;
            end
            ST_FPU_ROM_ADDRESS: begin
                fpu_rom_index <=
                    fpu_normalized == 17'd512 ? 8'd0 : fpu_normalized[7:0];
                fpu_rom_exponent <= fpu_rom_exponent +
                    (fpu_normalized == 17'd512 ? 6'sd1 : 6'sd0);
                state <= ST_FPU_ROM_LOOKUP;
            end
            ST_FPU_ROM_LOOKUP: begin
                // Short hop: every long path was registered in earlier states.
                if (field_b == 0) begin
                    fpu_rom_address <= 10'd256 + {2'b00, fpu_rom_index};
                end else if (field_b == 1) begin
                    fpu_rom_address <=
                        (fpu_rom_exponent[0] ? 10'd768 : 10'd512) + {2'b00, fpu_rom_index};
                    fpu_rom_exponent <= fpu_rom_exponent >>> 1;
                end else begin
                    fpu_rom_address <= {2'b00, fpu_sine_address(fpu_sine_phase)};
                    fpu_rom_negative <= fpu_sine_phase >= 10'd512;
                    fpu_sine_endpoint <= fpu_sine_is_endpoint(fpu_sine_phase);
                end
                state <= ST_FPU_ROM_WAIT;
            end
            ST_FPU_ROM_WAIT: state <= ST_FPU_ROM_COMMIT;
            ST_FPU_ROM_COMMIT: begin
                if (field_b <= 1) begin
                    fpu_result <= fpu_rom_scaled;
                    state <= ST_FPU_ROM_WRITE;
                end else if (!fpu_rom_step) begin
                    fpu_rom_first <= fpu_rom_negative ?
                        -(fpu_sine_endpoint ? 16'sd256 : $signed(fpu_rom_read_data)) :
                        (fpu_sine_endpoint ? 16'sd256 : $signed(fpu_rom_read_data));
                    fpu_sine_phase <= fpu_sine_phase + 10'd256;
                    fpu_rom_step <= 1;
                    state <= ST_FPU_ROM_LOOKUP;
                end else begin
                    fpu_rom_second <= fpu_rom_negative ?
                        -(fpu_sine_endpoint ? 16'sd256 : $signed(fpu_rom_read_data)) :
                        (fpu_sine_endpoint ? 16'sd256 : $signed(fpu_rom_read_data));
                    fpu_rom_step <= 0;
                    state <= ST_FPU_ROM_WRITE;
                end
            end
            ST_FPU_ROM_WRITE: begin
                if (field_b <= 1) begin
                    // RCP/RSQRT write only lane zero.
                    fpu_rf_write_enable <= 4'b0001;
                    fpu_rf_write_data <= {4{fpu_result}};
                end else begin
                    // SINCOS lands the whole vector in one wide write.
                    fpu_rf_write_enable <= 4'b1111;
                    fpu_rf_write_data <= {32'd0, fpu_rom_second, fpu_rom_first};
                end
                fpu_rf_write_address <= field_a;
                state <= ST_FPU_COMMIT;
            end
            ST_RESET_CLEAR: begin
                // Reset walks the register file back to zero, one vector per
                // cycle through the wide write port.
                fpu_rf_write_enable <= 4'b1111;
                fpu_rf_write_address <= fpu_clear_index[3:0];
                fpu_rf_write_data <= 64'd0;
                if (fpu_clear_index == 15)
                    state <= ST_FETCH_REQUEST;
                else
                    fpu_clear_index <= fpu_clear_index + 1'b1;
            end
            ST_FPU_COMMIT: begin
                retired_words <= retired_words + fpu_retire_words;
                state <= ST_FETCH_REQUEST;
            end
            default: state <= state;
        endcase
    end
end
endmodule
