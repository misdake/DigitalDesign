module G16CpuBoardTest(
    input wire clk,
    input wire [1:0] buttons,
    output wire [5:0] leds,
    output wire uart_tx
);

reg [2:0] startup_reset = 3'd7;
reg [1:0] button_sync = 0;
always @(posedge clk) begin
    if (startup_reset != 0)
        startup_reset <= startup_reset - 1'b1;
    button_sync <= {button_sync[0], |buttons};
end
wire core_reset = startup_reset != 0 || button_sync[1];

wire instruction_request_valid;
wire [31:0] instruction_address;
reg instruction_response_valid = 0;
reg instruction_error = 0;
wire [15:0] instruction_data;
wire [15:0] unused_rw_data;

__PROGRAM_MEMORY__ u_program(
    .clk(clk),
    .read_address(instruction_address[9:0]),
    .rw_write_enable(1'b0),
    .rw_address(10'b0),
    .rw_write_data(16'b0),
    .read_data(instruction_data),
    .rw_read_data(unused_rw_data)
);

always @(posedge clk) begin
    instruction_response_valid <= instruction_request_valid;
    instruction_error <= instruction_request_valid && instruction_address[31:10] != 0;
end

wire data_request_valid;
wire data_write;
wire [31:0] data_address;
wire [15:0] data_write_data;
reg data_response_valid = 0;
reg data_error = 0;
always @(posedge clk) begin
    data_response_valid <= data_request_valid;
    data_error <= data_request_valid;
end

wire halted;
wire [15:0] halt_signal;
wire faulted;
wire [7:0] fault_code;
wire [15:0] fault_pc;
wire [15:0] pc;
wire [15:0] code_segment;
wire [15:0] data_segment;
wire [31:0] retired_words;

__G16_CORE__ u_core(
    .clk(clk),
    .reset(core_reset),
    .instruction_request_ready(1'b1),
    .instruction_response_valid(instruction_response_valid),
    .instruction_data(instruction_data),
    .instruction_error(instruction_error),
    .data_request_ready(1'b1),
    .data_response_valid(data_response_valid),
    .data_read_data(16'b0),
    .data_error(data_error),
    .instruction_request_valid(instruction_request_valid),
    .instruction_address(instruction_address),
    .instruction_response_ready(),
    .data_request_valid(data_request_valid),
    .data_write(data_write),
    .data_address(data_address),
    .data_write_data(data_write_data),
    .data_response_ready(),
    .halted(halted),
    .halt_signal(halt_signal),
    .fault(faulted),
    .fault_code(fault_code),
    .fault_pc(fault_pc),
    .pc(pc),
    .code_segment(code_segment),
    .data_segment(data_segment),
    .retired_words(retired_words)
);

wire passed = halted && halt_signal == 16'd15;
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
