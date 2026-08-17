module G16Core (
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
    output wire instruction_request_valid,
    output wire [31:0] instruction_address,
    output wire instruction_response_ready,
    output wire data_request_valid,
    output wire data_write,
    output wire [31:0] data_address,
    output wire [15:0] data_write_data,
    output wire data_response_ready,
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
localparam [3:0] ST_FETCH_REQUEST = 0;
localparam [3:0] ST_FETCH_RESPONSE = 1;
localparam [3:0] ST_EXECUTE = 2;
localparam [3:0] ST_DATA_REQUEST = 3;
localparam [3:0] ST_DATA_RESPONSE = 4;
localparam [3:0] ST_MULTIPLY_WAIT = 5;
localparam [3:0] ST_MULTIPLY_COMMIT = 6;
localparam [3:0] ST_HALTED = 7;
localparam [3:0] ST_FAULT = 8;

localparam [7:0] FAULT_INVALID_INSTRUCTION = 1;
localparam [7:0] FAULT_UNSUPPORTED_FPU = 2;
localparam [7:0] FAULT_INSTRUCTION_MEMORY = 3;
localparam [7:0] FAULT_DATA_MEMORY = 4;

reg [3:0] state = ST_FETCH_REQUEST;
reg [15:0] registers [0:15];
reg [15:0] pc_register = 0;
reg [15:0] code_segment_register = 0;
reg [15:0] data_segment_register = 0;
reg prefix_valid = 0;
reg [11:0] prefix_high = 0;
reg [15:0] prefix_address = 0;
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

wire [3:0] opcode = instruction[15:12];
wire [3:0] field_d = instruction[11:8];
wire [3:0] field_a = instruction[7:4];
wire [3:0] field_b = instruction[3:0];
wire prefix_consumer = opcode == 4'h8 || opcode == 4'h9 || opcode == 4'hc ||
                       (opcode == 4'ha &&
                        (field_d <= 4'h4 ||
                         (field_d >= 4'h8 && field_d <= 4'hb) ||
                         field_d >= 4'he)) ||
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

assign instruction_request_valid = state == ST_FETCH_REQUEST;
assign instruction_address = {code_segment_register, pc_register};
assign instruction_response_ready = state == ST_FETCH_RESPONSE;
assign data_request_valid = state == ST_DATA_REQUEST;
assign data_write = pending_write;
assign data_address = pending_address;
assign data_write_data = pending_write_data;
assign data_response_ready = state == ST_DATA_RESPONSE;
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
        state <= ST_FETCH_REQUEST;
        pc_register <= 0;
        code_segment_register <= 0;
        data_segment_register <= 0;
        prefix_valid <= 0;
        retired_words <= 0;
        fault_code <= 0;
        fault_pc <= 0;
        for (register_index = 0; register_index < 16; register_index = register_index + 1)
            registers[register_index] <= 0;
    end else begin
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
                            pending_address <= {
                                logical_address >= 16'hff00 ? 16'b0 : data_segment_register,
                                logical_address
                            };
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
                                4'he: registers[field_a] <= prefix_valid ?
                                    immediate_unsigned(instruction) : sign_extend4(instruction[3:0]);
                                4'hf: registers[field_a] <= immediate_unsigned(instruction);
                                default: begin
                                    fault_code <= FAULT_INVALID_INSTRUCTION;
                                    fault_pc <= current_fault_pc;
                                    state <= ST_FAULT;
                                end
                            endcase
                            if (field_d != 4'h8 &&
                                (field_d <= 4'h7 ||
                                 (field_d >= 4'h9 && field_d <= 4'hb) ||
                                 field_d >= 4'he)) begin
                                retired_words <= retired_words + success_retire_words;
                                state <= ST_FETCH_REQUEST;
                            end
                        end
                        4'hb: begin
                            branch_taken = 0;
                            case (field_d)
                                0: branch_taken = registers[field_a] == 0;
                                1: branch_taken = registers[field_a] != 0;
                                2: branch_taken = $signed(registers[field_a]) < 0;
                                3: branch_taken = $signed(registers[field_a]) >= 0;
                                4: branch_taken = $signed(registers[field_a]) > 0;
                                5: branch_taken = $signed(registers[field_a]) <= 0;
                                6: branch_taken = registers[field_a][0];
                                7: branch_taken = !registers[field_a][0];
                                default: begin
                                    fault_code <= FAULT_INVALID_INSTRUCTION;
                                    fault_pc <= current_fault_pc;
                                    state <= ST_FAULT;
                                end
                            endcase
                            if (field_d <= 7) begin
                                if (branch_taken)
                                    pc_register <= pc_register + immediate_signed(instruction);
                                retired_words <= retired_words + success_retire_words;
                                state <= ST_FETCH_REQUEST;
                            end
                        end
                        4'hc: begin
                            jump_offset = prefix_valid ?
                                {prefix_high[7:0], instruction[7:0]} :
                                sign_extend8(instruction[7:0]);
                            if (field_d != 15)
                                registers[field_d] <= pc_register;
                            pc_register <= pc_register + jump_offset;
                            retired_words <= retired_words + success_retire_words;
                            state <= ST_FETCH_REQUEST;
                        end
                        4'hd: begin
                            fault_code <= FAULT_UNSUPPORTED_FPU;
                            fault_pc <= current_fault_pc;
                            state <= ST_FAULT;
                        end
                        4'he: begin
                            case (field_d)
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
                                5: begin
                                    jump_target = registers[field_b];
                                    registers[field_a] <= pc_register;
                                    pc_register <= jump_target;
                                    retired_words <= retired_words + success_retire_words;
                                    state <= ST_FETCH_REQUEST;
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
                                11: begin
                                    registers[field_a] <= population_count(registers[field_b]);
                                    retired_words <= retired_words + success_retire_words;
                                    state <= ST_FETCH_REQUEST;
                                end
                                12: begin
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
                                13: begin
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
                                14: begin
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
                        fault_code <= FAULT_DATA_MEMORY;
                        fault_pc <= pending_fault_pc;
                        state <= ST_FAULT;
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
            default: state <= state;
        endcase
    end
end
endmodule
