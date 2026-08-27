// Board characterization for the CPU device path: CpuV3Core directly drives
// SystemControlDevice. The compiled program runs from the BSRAM boot memory.
module CpuV3DeviceBoardTest(
    input wire clk,
    input wire [1:0] buttons,
    output wire [5:0] leds,
    output wire uart_tx
);

wire core_reset;
wire clock_ready_synchronized;
wire external_reset_seen;
__RESET_CONTROLLER__ u_reset(
    .clk(clk),
    .external_reset(|buttons),
    .clock_ready(1'b1),
    .reset(core_reset),
    .clock_ready_synchronized(clock_ready_synchronized),
    .external_reset_seen(external_reset_seen)
);

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
wire data_response_ready;
reg data_response_valid = 0;
always @(posedge clk)
    data_response_valid <= data_request_valid;

wire [2:0] device_index;
wire [3:0] device_channel;
wire device_read_enable;
wire device_write_enable;
wire [15:0] device_write_data;
wire [15:0] device_read_data;

SystemControlDevice_CLOCKS_PER_BIT234 u_sysctl (
    .clk(clk),
    .reset(core_reset),
    .device_index(device_index),
    .device_channel(device_channel),
    .device_read_enable(device_read_enable),
    .device_write_enable(device_write_enable),
    .device_write_data(device_write_data),
    .device_read_data(device_read_data),
    .icache_invalidate(),
    .dcache_invalidate(),
    .leds(leds),
    .uart_tx(uart_tx)
);

wire halted;
wire [15:0] halt_signal;
wire faulted;
wire [7:0] fault_code;
wire [15:0] fault_pc;
wire [15:0] pc;
wire [15:0] code_segment;
wire [15:0] data_segment;
wire [31:0] retired_words;

__CPU_V3_CORE__ u_core(
    .clk(clk),
    .reset(core_reset),
    .instruction_request_ready(1'b1),
    .instruction_response_valid(instruction_response_valid),
    .instruction_data(instruction_data),
    .instruction_error(instruction_error),
    .data_request_ready(1'b1),
    .data_response_valid(data_response_valid),
    .data_read_data(16'b0),
    .data_error(data_response_valid),
    .device_read_data(device_read_data),
    .instruction_request_valid(instruction_request_valid),
    .instruction_address(instruction_address),
    .instruction_response_ready(),
    .data_request_valid(data_request_valid),
    .data_write(data_write),
    .data_address(data_address),
    .data_write_data(data_write_data),
    .data_response_ready(data_response_ready),
    .device_index(device_index),
    .device_channel(device_channel),
    .device_read_enable(device_read_enable),
    .device_write_enable(device_write_enable),
    .device_write_data(device_write_data),
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

endmodule
