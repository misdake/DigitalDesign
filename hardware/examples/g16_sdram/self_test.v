module G16SdramBoardTest (
    input wire clk,
    input wire [1:0] buttons,
    input wire [31:0] sdram_read_data,
    input wire sdram_read_valid,
    input wire sdram_init_done,
    input wire sdram_command_ack,
    output wire [5:0] leds,
    output wire uart_tx,
    output reg sdram_command_valid,
    output reg [2:0] sdram_command,
    output reg sdram_precharge,
    output reg [20:0] sdram_address,
    output reg [3:0] sdram_write_mask,
    output reg [31:0] sdram_write_data,
    output reg [7:0] sdram_burst_length
);

localparam [2:0] CMD_ACTIVE = 3'b011;
localparam [2:0] CMD_WRITE  = 3'b100;
localparam [2:0] CMD_READ   = 3'b101;

localparam [4:0] ST_INIT          = 0;
localparam [4:0] ST_BOOT_ADDRESS  = 1;
localparam [4:0] ST_BOOT_WAIT     = 2;
localparam [4:0] ST_BOOT_CAPTURE  = 3;
localparam [4:0] ST_ACT_WRITE     = 4;
localparam [4:0] ST_ACT_WRITE_ACK = 5;
localparam [4:0] ST_WRITE         = 6;
localparam [4:0] ST_WRITE_DATA    = 7;
localparam [4:0] ST_WRITE_ACK     = 8;
localparam [4:0] ST_ACT_READ      = 9;
localparam [4:0] ST_ACT_READ_ACK  = 10;
localparam [4:0] ST_READ          = 11;
localparam [4:0] ST_READ_DATA     = 12;
localparam [4:0] ST_FILL_CACHE    = 13;
localparam [4:0] ST_CPU_INIT      = 14;
localparam [4:0] ST_CPU           = 15;
localparam [4:0] ST_DONE          = 16;
localparam [4:0] ST_ERROR         = 17;
localparam [4:0] ST_DCACHE_WAIT_1 = 18;
localparam [4:0] ST_DCACHE_WAIT_2 = 19;
localparam [4:0] ST_DATA_ACT_W    = 20;
localparam [4:0] ST_DATA_ACT_W_ACK = 21;
localparam [4:0] ST_DATA_WRITE    = 22;
localparam [4:0] ST_DATA_WRITE_ACK = 23;
localparam [4:0] ST_DATA_ACT_R    = 24;
localparam [4:0] ST_DATA_ACT_R_ACK = 25;
localparam [4:0] ST_DATA_READ     = 26;
localparam [4:0] ST_DATA_READ_DATA = 27;
localparam [4:0] ST_FILL_DATA_CACHE = 28;
localparam [4:0] ST_COMPLETE_LOAD = 29;
localparam [4:0] ST_DCACHE_STORE_CHECK = 30;

reg [4:0] state = ST_INIT;
reg [3:0] boot_index = 0;
reg [5:0] boot_page = 0;
reg [9:0] boot_address = 0;
wire [15:0] boot_read_data;
wire [15:0] unused_boot_rw_data;
reg [15:0] line_words [0:15];
reg [31:0] read_beats [0:7];
reg [2:0] beat_index = 0;
reg read_ack_seen = 0;
reg [3:0] fill_word = 0;
reg [19:0] timeout_counter = 0;
reg [7:0] error_code = 0;
reg [1:0] button_meta = 0;
reg [1:0] button_sync = 0;

reg cache_write_enable = 0;
reg [9:0] cache_rw_address = 0;
reg [15:0] cache_write_data = 0;
wire [15:0] unused_cache_rw_data;
reg [9:0] fetch_address = 0;
wire [15:0] instruction;

reg data_cache_write_enable = 0;
reg [9:0] data_cache_rw_address = 0;
reg [15:0] data_cache_write_data = 0;
reg [9:0] data_cache_read_address = 0;
wire [15:0] data_cache_read_data;
wire [15:0] unused_data_cache_rw_data;
reg [5:0] data_cache_tags [0:63];
reg [63:0] data_cache_valid = 0;
reg [15:0] memory_address = 0;
reg [15:0] memory_write_data = 0;
reg [3:0] load_destination = 0;
reg [3:0] data_fill_word = 0;

__BOOT_MEMORY__ u_boot (
    .clk(clk),
    .read_address(boot_address),
    .rw_write_enable(1'b0),
    .rw_address(10'b0),
    .rw_write_data(16'b0),
    .read_data(boot_read_data),
    .rw_read_data(unused_boot_rw_data)
);

__INSTRUCTION_CACHE__ u_instruction_cache (
    .clk(clk),
    .read_address(fetch_address),
    .rw_write_enable(cache_write_enable),
    .rw_address(cache_rw_address),
    .rw_write_data(cache_write_data),
    .read_data(instruction),
    .rw_read_data(unused_cache_rw_data)
);

__DATA_CACHE__ u_data_cache (
    .clk(clk),
    .read_address(data_cache_read_address),
    .rw_write_enable(data_cache_write_enable),
    .rw_address(data_cache_rw_address),
    .rw_write_data(data_cache_write_data),
    .read_data(data_cache_read_data),
    .rw_read_data(unused_data_cache_rw_data)
);

reg [15:0] registers [0:15];
reg [15:0] pc = 0;
reg [1:0] fetch_phase = 0;
reg prefix_valid = 0;
reg [11:0] prefix_high = 0;
reg halted = 0;
reg faulted = 0;
integer register_index;
integer cache_index;

function [15:0] sign_extend4;
    input [3:0] value;
    sign_extend4 = {{12{value[3]}}, value};
endfunction

function [15:0] immediate4;
    input [15:0] inst;
    input signed_value;
    begin
        if (prefix_valid)
            immediate4 = {prefix_high, inst[3:0]};
        else if (signed_value)
            immediate4 = sign_extend4(inst[3:0]);
        else
            immediate4 = {12'b0, inst[3:0]};
    end
endfunction

wire [3:0] opcode = instruction[15:12];
wire [3:0] field_d = instruction[11:8];
wire [3:0] field_a = instruction[7:4];
wire [3:0] field_b = instruction[3:0];
wire [15:0] effective_address = registers[field_a] +
                                immediate4(instruction, 1'b1);
wire is_consumer = opcode == 4'h8 || opcode == 4'h9 || opcode == 4'hb ||
                   opcode == 4'hc ||
                   (opcode == 4'ha &&
                    ((field_d <= 4'h4) ||
                     (field_d >= 4'h8 && field_d <= 4'hb) ||
                     field_d >= 4'he));

always @(posedge clk) begin
    button_meta <= buttons;
    button_sync <= button_meta;
    sdram_command_valid <= 0;
    cache_write_enable <= 0;
    data_cache_write_enable <= 0;

    if (button_sync[1]) begin
        state <= ST_INIT;
        boot_index <= 0;
        boot_page <= 0;
        timeout_counter <= 0;
        error_code <= 0;
        halted <= 0;
        faulted <= 0;
    end else begin
        case (state)
            ST_INIT: begin
                sdram_command <= 3'b111;
                sdram_precharge <= 0;
                sdram_address <= 0;
                sdram_write_mask <= 0;
                sdram_write_data <= 0;
                sdram_burst_length <= 7;
                boot_index <= 0;
                // Button 1 is a diagnostic page selector. Its reachable
                // 6-bit value also keeps the complete boot image implemented
                // by the declared 1024-word BSRAM leaf.
                if (button_sync[0])
                    boot_page <= boot_page + 1'b1;
                else if (sdram_init_done)
                    state <= ST_BOOT_ADDRESS;
            end

            ST_BOOT_ADDRESS: begin
                boot_address <= {boot_page, boot_index};
                state <= ST_BOOT_WAIT;
            end

            ST_BOOT_WAIT: state <= ST_BOOT_CAPTURE;

            ST_BOOT_CAPTURE: begin
                line_words[boot_index] <= boot_read_data;
                if (boot_index == 15)
                    state <= ST_ACT_WRITE;
                else begin
                    boot_index <= boot_index + 1'b1;
                    state <= ST_BOOT_ADDRESS;
                end
            end

            ST_ACT_WRITE: begin
                sdram_command <= CMD_ACTIVE;
                sdram_precharge <= 0;
                sdram_address <= 0;
                sdram_command_valid <= 1;
                timeout_counter <= 0;
                state <= ST_ACT_WRITE_ACK;
            end

            ST_ACT_WRITE_ACK: begin
                if (sdram_command_ack)
                    state <= ST_WRITE;
                else if (timeout_counter == 20'hf_ffff) begin
                    error_code <= 8'h11;
                    state <= ST_ERROR;
                end else
                    timeout_counter <= timeout_counter + 1'b1;
            end

            ST_WRITE: begin
                sdram_command <= CMD_WRITE;
                sdram_precharge <= 1;
                sdram_address <= 0;
                sdram_write_mask <= 0;
                sdram_write_data <= {line_words[1], line_words[0]};
                sdram_command_valid <= 1;
                beat_index <= 0;
                state <= ST_WRITE_DATA;
            end

            ST_WRITE_DATA: begin
                if (beat_index == 7) begin
                    timeout_counter <= 0;
                    state <= ST_WRITE_ACK;
                end else begin
                    beat_index <= beat_index + 1'b1;
                    sdram_write_data <= {
                        line_words[((beat_index + 1'b1) << 1) + 1'b1],
                        line_words[(beat_index + 1'b1) << 1]
                    };
                end
            end

            ST_WRITE_ACK: begin
                if (sdram_command_ack)
                    state <= ST_ACT_READ;
                else if (timeout_counter == 20'hf_ffff) begin
                    error_code <= 8'h12;
                    state <= ST_ERROR;
                end else
                    timeout_counter <= timeout_counter + 1'b1;
            end

            ST_ACT_READ: begin
                sdram_command <= CMD_ACTIVE;
                sdram_precharge <= 0;
                sdram_address <= 0;
                sdram_command_valid <= 1;
                timeout_counter <= 0;
                state <= ST_ACT_READ_ACK;
            end

            ST_ACT_READ_ACK: begin
                if (sdram_command_ack)
                    state <= ST_READ;
                else if (timeout_counter == 20'hf_ffff) begin
                    error_code <= 8'h13;
                    state <= ST_ERROR;
                end else
                    timeout_counter <= timeout_counter + 1'b1;
            end

            ST_READ: begin
                sdram_command <= CMD_READ;
                sdram_precharge <= 1;
                sdram_address <= 0;
                sdram_command_valid <= 1;
                beat_index <= 0;
                read_ack_seen <= 0;
                timeout_counter <= 0;
                state <= ST_READ_DATA;
            end

            ST_READ_DATA: begin
                if (sdram_command_ack)
                    read_ack_seen <= 1;
                if (sdram_read_valid) begin
                    read_beats[beat_index] <= sdram_read_data;
                    if (beat_index == 7) begin
                        if (!(read_ack_seen || sdram_command_ack)) begin
                            error_code <= 8'h14;
                            state <= ST_ERROR;
                        end else begin
                            fill_word <= 0;
                            state <= ST_FILL_CACHE;
                        end
                    end else
                        beat_index <= beat_index + 1'b1;
                end else if (timeout_counter == 20'hf_ffff) begin
                    error_code <= 8'h15;
                    state <= ST_ERROR;
                end else
                    timeout_counter <= timeout_counter + 1'b1;
            end

            ST_FILL_CACHE: begin
                cache_write_enable <= 1;
                cache_rw_address <= {6'b0, fill_word};
                if (fill_word[0])
                    cache_write_data <= read_beats[fill_word[3:1]][31:16];
                else
                    cache_write_data <= read_beats[fill_word[3:1]][15:0];
                if (fill_word == 15)
                    state <= ST_CPU_INIT;
                else
                    fill_word <= fill_word + 1'b1;
            end

            ST_CPU_INIT: begin
                pc <= 0;
                fetch_address <= 0;
                fetch_phase <= 0;
                prefix_valid <= 0;
                halted <= 0;
                faulted <= 0;
                for (register_index = 0; register_index < 16;
                     register_index = register_index + 1)
                    registers[register_index] <= 0;
                for (cache_index = 0; cache_index < 64;
                     cache_index = cache_index + 1)
                    data_cache_tags[cache_index] <= 0;
                data_cache_valid <= 0;
                state <= ST_CPU;
            end

            ST_CPU: begin
                if (halted) begin
                    if (registers[0] == 16'h1235)
                        state <= ST_DONE;
                    else begin
                        error_code <= 8'h21;
                        state <= ST_ERROR;
                    end
                end else if (faulted) begin
                    error_code <= 8'h22;
                    state <= ST_ERROR;
                end else if (fetch_phase == 0) begin
                    fetch_address <= pc[9:0];
                    fetch_phase <= 1;
                end else if (fetch_phase == 1) begin
                    fetch_phase <= 2;
                end else begin
                    fetch_phase <= 0;
                    pc <= pc + 1'b1;
                    if (opcode == 4'hf) begin
                        prefix_high <= instruction[11:0];
                        prefix_valid <= 1;
                    end else begin
                        if (prefix_valid && !is_consumer)
                            prefix_valid <= 0;
                        case (opcode)
                            4'h0: registers[field_d] <=
                                registers[field_a] + registers[field_b];
                            4'h1: registers[field_d] <=
                                registers[field_a] - registers[field_b];
                            4'h8: begin
                                memory_address <= effective_address;
                                load_destination <= field_d;
                                data_cache_read_address <= effective_address[9:0];
                                prefix_valid <= 0;
                                state <= ST_DCACHE_WAIT_1;
                            end
                            4'h9: begin
                                memory_address <= effective_address;
                                memory_write_data <= registers[field_d];
                                prefix_valid <= 0;
                                state <= ST_DCACHE_STORE_CHECK;
                            end
                            4'ha: begin
                                case (field_d)
                                    4'h0: registers[field_a] <= registers[field_a] +
                                        immediate4(instruction, 1'b1);
                                    4'h9: registers[field_a] <= registers[field_a] ==
                                        immediate4(instruction, 1'b1);
                                    4'he: registers[field_a] <= prefix_valid ?
                                        immediate4(instruction, 1'b0) :
                                        sign_extend4(field_b);
                                    4'hf: registers[field_a] <=
                                        immediate4(instruction, 1'b0);
                                    default: faulted <= 1;
                                endcase
                                prefix_valid <= 0;
                            end
                            4'hb: begin
                                case (field_d)
                                    4'h0: if (registers[field_a] == 0)
                                        pc <= pc + 1'b1 +
                                              immediate4(instruction, 1'b1);
                                    4'h1: if (registers[field_a] != 0)
                                        pc <= pc + 1'b1 +
                                              immediate4(instruction, 1'b1);
                                    default: faulted <= 1;
                                endcase
                                prefix_valid <= 0;
                            end
                            4'hc: begin
                                if (field_d != 4'hf)
                                    registers[field_d] <= pc + 1'b1;
                                if (prefix_valid)
                                    pc <= pc + 1'b1 +
                                          {prefix_high[7:0], instruction[7:0]};
                                else
                                    pc <= pc + 1'b1 +
                                          {{8{instruction[7]}}, instruction[7:0]};
                                prefix_valid <= 0;
                            end
                            4'he: begin
                                case (field_d)
                                    4'h1: registers[field_a] <= registers[field_b];
                                    4'h8: if (field_a == 0 && field_b == 0)
                                        halted <= 1;
                                    default: faulted <= 1;
                                endcase
                            end
                            default: faulted <= 1;
                        endcase
                    end
                end
            end

            ST_DCACHE_WAIT_1: state <= ST_DCACHE_WAIT_2;

            ST_DCACHE_WAIT_2: begin
                if (data_cache_valid[memory_address[9:4]] &&
                    data_cache_tags[memory_address[9:4]] == memory_address[15:10]) begin
                    registers[load_destination] <= data_cache_read_data;
                    state <= ST_CPU;
                end else begin
                    state <= ST_DATA_ACT_R;
                end
            end

            ST_DCACHE_STORE_CHECK: begin
                if (data_cache_valid[memory_address[9:4]] &&
                    data_cache_tags[memory_address[9:4]] == memory_address[15:10]) begin
                    data_cache_write_enable <= 1;
                    data_cache_rw_address <= memory_address[9:0];
                    data_cache_write_data <= memory_write_data;
                end
                state <= ST_DATA_ACT_W;
            end

            ST_DATA_ACT_W: begin
                sdram_command <= CMD_ACTIVE;
                sdram_precharge <= 0;
                sdram_address <= {6'b0, memory_address[15:1]};
                sdram_command_valid <= 1;
                timeout_counter <= 0;
                state <= ST_DATA_ACT_W_ACK;
            end

            ST_DATA_ACT_W_ACK: begin
                if (sdram_command_ack)
                    state <= ST_DATA_WRITE;
                else if (timeout_counter == 20'hf_ffff) begin
                    error_code <= 8'h31;
                    state <= ST_ERROR;
                end else
                    timeout_counter <= timeout_counter + 1'b1;
            end

            ST_DATA_WRITE: begin
                sdram_command <= CMD_WRITE;
                sdram_precharge <= 1;
                sdram_address <= {6'b0, memory_address[15:1]};
                sdram_burst_length <= 0;
                if (memory_address[0]) begin
                    sdram_write_mask <= 4'b0011;
                    sdram_write_data <= {memory_write_data, 16'b0};
                end else begin
                    sdram_write_mask <= 4'b1100;
                    sdram_write_data <= {16'b0, memory_write_data};
                end
                sdram_command_valid <= 1;
                timeout_counter <= 0;
                state <= ST_DATA_WRITE_ACK;
            end

            ST_DATA_WRITE_ACK: begin
                if (sdram_command_ack) begin
                    sdram_burst_length <= 7;
                    state <= ST_CPU;
                end else if (timeout_counter == 20'hf_ffff) begin
                    error_code <= 8'h32;
                    state <= ST_ERROR;
                end else
                    timeout_counter <= timeout_counter + 1'b1;
            end

            ST_DATA_ACT_R: begin
                sdram_command <= CMD_ACTIVE;
                sdram_precharge <= 0;
                sdram_address <= {6'b0, memory_address[15:4], 3'b000};
                sdram_command_valid <= 1;
                timeout_counter <= 0;
                state <= ST_DATA_ACT_R_ACK;
            end

            ST_DATA_ACT_R_ACK: begin
                if (sdram_command_ack)
                    state <= ST_DATA_READ;
                else if (timeout_counter == 20'hf_ffff) begin
                    error_code <= 8'h33;
                    state <= ST_ERROR;
                end else
                    timeout_counter <= timeout_counter + 1'b1;
            end

            ST_DATA_READ: begin
                sdram_command <= CMD_READ;
                sdram_precharge <= 1;
                sdram_address <= {6'b0, memory_address[15:4], 3'b000};
                sdram_burst_length <= 7;
                sdram_command_valid <= 1;
                beat_index <= 0;
                read_ack_seen <= 0;
                timeout_counter <= 0;
                state <= ST_DATA_READ_DATA;
            end

            ST_DATA_READ_DATA: begin
                if (sdram_command_ack)
                    read_ack_seen <= 1;
                if (sdram_read_valid) begin
                    read_beats[beat_index] <= sdram_read_data;
                    if (beat_index == 7) begin
                        if (!(read_ack_seen || sdram_command_ack)) begin
                            error_code <= 8'h34;
                            state <= ST_ERROR;
                        end else begin
                            data_fill_word <= 0;
                            state <= ST_FILL_DATA_CACHE;
                        end
                    end else
                        beat_index <= beat_index + 1'b1;
                end else if (timeout_counter == 20'hf_ffff) begin
                    error_code <= 8'h35;
                    state <= ST_ERROR;
                end else
                    timeout_counter <= timeout_counter + 1'b1;
            end

            ST_FILL_DATA_CACHE: begin
                data_cache_write_enable <= 1;
                data_cache_rw_address <= {memory_address[9:4], data_fill_word};
                if (data_fill_word[0])
                    data_cache_write_data <= read_beats[data_fill_word[3:1]][31:16];
                else
                    data_cache_write_data <= read_beats[data_fill_word[3:1]][15:0];
                if (data_fill_word == 15) begin
                    data_cache_tags[memory_address[9:4]] <= memory_address[15:10];
                    data_cache_valid[memory_address[9:4]] <= 1;
                    state <= ST_COMPLETE_LOAD;
                end else
                    data_fill_word <= data_fill_word + 1'b1;
            end

            ST_COMPLETE_LOAD: begin
                if (memory_address[0])
                    registers[load_destination] <=
                        read_beats[memory_address[3:1]][31:16];
                else
                    registers[load_destination] <=
                        read_beats[memory_address[3:1]][15:0];
                state <= ST_CPU;
            end

            ST_DONE: state <= ST_DONE;
            ST_ERROR: state <= ST_ERROR;
            default: begin
                error_code <= 8'hff;
                state <= ST_ERROR;
            end
        endcase
    end
end

assign leds = state == ST_DONE ? 6'b000001 :
              state == ST_ERROR ? 6'b100001 : 6'b001100;

wire test_done = state == ST_DONE || state == ST_ERROR;
reg [24:0] report_delay = 0;
reg [9:0] uart_frame = 10'h3ff;
reg [3:0] uart_bit = 0;
reg [8:0] uart_divider = 0;
reg [3:0] report_byte_index = 0;
reg uart_busy = 0;

function [7:0] report_byte;
    input [3:0] index;
    reg [7:0] status;
    begin
        status = state == ST_DONE ? 8'h00 : error_code;
        case (index)
            0: report_byte = 8'h44;
            1: report_byte = 8'h44;
            2: report_byte = 8'h48;
            3: report_byte = 8'h54;
            4: report_byte = 8'h01;
            5: report_byte = 8'h05;
            6: report_byte = status;
            default: report_byte = 8'h18 ^ status;
        endcase
    end
endfunction

always @(posedge clk) begin
    if (!test_done) begin
        report_delay <= 0;
        uart_frame <= 10'h3ff;
        uart_bit <= 0;
        uart_divider <= 0;
        report_byte_index <= 0;
        uart_busy <= 0;
    end else if (!uart_busy) begin
        if (report_delay == 25'd27_000_000) begin
            uart_frame <= {1'b1, report_byte(0), 1'b0};
            uart_bit <= 0;
            uart_divider <= 0;
            report_byte_index <= 0;
            uart_busy <= 1;
            report_delay <= 0;
        end else
            report_delay <= report_delay + 1'b1;
    end else if (uart_divider == 9'd468) begin
        uart_divider <= 0;
        if (uart_bit == 9) begin
            if (report_byte_index == 7)
                uart_busy <= 0;
            else begin
                report_byte_index <= report_byte_index + 1'b1;
                uart_frame <= {1'b1, report_byte(report_byte_index + 1'b1), 1'b0};
                uart_bit <= 0;
            end
        end else
            uart_bit <= uart_bit + 1'b1;
    end else
        uart_divider <= uart_divider + 1'b1;
end

assign uart_tx = uart_busy ? uart_frame[uart_bit] : 1'b1;

endmodule
