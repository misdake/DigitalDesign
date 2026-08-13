module G16CpuBoardTest(
    input wire clk,
    input wire [1:0] buttons,
    output wire [5:0] leds,
    output wire uart_tx
);

reg [15:0] registers [0:15];
reg [15:0] pc = 0;
reg [9:0] fetch_address = 0;
wire [15:0] instruction;
wire [15:0] unused_rw_data;
reg [1:0] fetch_phase = 0;
reg prefix_valid = 0;
reg [11:0] prefix_high = 0;
reg halted = 0;
reg faulted = 0;
reg [1:0] button_sync = 0;
integer register_index;

__PROGRAM_MEMORY__ u_program(
    .clk(clk),
    .read_address(fetch_address),
    .rw_write_enable(1'b0),
    .rw_address(10'b0),
    .rw_write_data(16'b0),
    .read_data(instruction),
    .rw_read_data(unused_rw_data)
);

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
wire is_consumer = opcode == 4'h8 || opcode == 4'h9 || opcode == 4'hb ||
                   opcode == 4'hc ||
                   (opcode == 4'ha &&
                    ((field_d <= 4'h4) ||
                     (field_d >= 4'h8 && field_d <= 4'hb) ||
                     field_d >= 4'he));

always @(posedge clk) begin
    button_sync <= {button_sync[0], |buttons};
    if (button_sync[1]) begin
        pc <= 0;
        fetch_address <= 0;
        fetch_phase <= 0;
        prefix_valid <= 0;
        halted <= 0;
        faulted <= 0;
        for (register_index = 0; register_index < 16; register_index = register_index + 1)
            registers[register_index] <= 0;
    end else if (!halted && !faulted) begin
        if (fetch_phase == 0) begin
            fetch_address <= pc[9:0];
            fetch_phase <= 1;
        end else if (fetch_phase == 1) begin
            // The inferred synchronous BSRAM samples read_address on this
            // edge; execute on the following edge after read_data updates.
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
                    4'h0: registers[field_d] <= registers[field_a] + registers[field_b];
                    4'h1: registers[field_d] <= registers[field_a] - registers[field_b];
                    4'ha: begin
                        case (field_d)
                            4'h0: registers[field_a] <= registers[field_a] + immediate4(instruction, 1'b1);
                            4'h9: registers[field_a] <= registers[field_a] == immediate4(instruction, 1'b1);
                            4'he: registers[field_a] <= prefix_valid ? immediate4(instruction, 1'b0) : sign_extend4(field_b);
                            4'hf: registers[field_a] <= immediate4(instruction, 1'b0);
                            default: faulted <= 1;
                        endcase
                        prefix_valid <= 0;
                    end
                    4'hb: begin
                        case (field_d)
                            4'h0: if (registers[field_a] == 0)
                                pc <= pc + 1'b1 + immediate4(instruction, 1'b1);
                            4'h1: if (registers[field_a] != 0)
                                pc <= pc + 1'b1 + immediate4(instruction, 1'b1);
                            default: faulted <= 1;
                        endcase
                        prefix_valid <= 0;
                    end
                    4'hc: begin
                        if (field_d != 4'hf)
                            registers[field_d] <= pc + 1'b1;
                        if (prefix_valid)
                            pc <= pc + 1'b1 + {prefix_high[7:0], instruction[7:0]};
                        else
                            pc <= pc + 1'b1 + {{8{instruction[7]}}, instruction[7:0]};
                        prefix_valid <= 0;
                    end
                    4'he: begin
                        case (field_d)
                            4'h1: registers[field_a] <= registers[field_b];
                            4'h8: if (field_a == 0 && field_b == 0) halted <= 1;
                            default: faulted <= 1;
                        endcase
                    end
                    default: faulted <= 1;
                endcase
            end
        end
    end
end

wire passed = halted && registers[0] == 16'd15;
assign leds = passed ? 6'b000001 : (faulted || halted ? 6'b100001 : 6'b001100);

wire test_done = halted || faulted;
reg [24:0] report_delay = 0;
reg [9:0] uart_frame = 10'h3ff;
reg [3:0] uart_bit = 0;
reg [7:0] uart_divider = 0;
reg [3:0] report_byte_index = 0;
reg uart_busy = 0;

function [7:0] report_byte;
    input [3:0] index;
    reg [7:0] status;
    begin
        status = passed ? 8'h00 : (faulted ? 8'h02 : 8'h01);
        case (index)
            0: report_byte = 8'h44;
            1: report_byte = 8'h44;
            2: report_byte = 8'h48;
            3: report_byte = 8'h54;
            4: report_byte = 8'h01;
            5: report_byte = 8'h04;
            6: report_byte = status;
            default: report_byte = 8'h19 ^ status;
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
        if (report_delay == 25'd13_500_000) begin
            uart_frame <= {1'b1, report_byte(0), 1'b0};
            uart_bit <= 0;
            uart_divider <= 0;
            report_byte_index <= 0;
            uart_busy <= 1;
            report_delay <= 0;
        end else begin
            report_delay <= report_delay + 1'b1;
        end
    end else if (uart_divider == 8'd233) begin
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
