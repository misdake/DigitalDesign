module CpuV3SdramBoardTest (
    input wire clk,
    input wire [1:0] buttons,
    input wire [31:0] sdram_read_data,
    input wire sdram_read_valid,
    input wire sdram_init_done,
    input wire sdram_command_ack,
    output wire [5:0] leds,
    output wire uart_tx,
    output wire sdram_command_valid,
    output wire [2:0] sdram_command,
    output wire sdram_precharge,
    output wire [20:0] sdram_address,
    output wire [3:0] sdram_write_mask,
    output wire [31:0] sdram_write_data,
    output wire [7:0] sdram_burst_length
);

wire reset;
wire clock_ready_synchronized;
wire external_reset_seen;
__RESET_CONTROLLER__ u_reset(
    .clk(clk),
    .external_reset(|buttons),
    .clock_ready(1'b1),
    .reset(reset),
    .clock_ready_synchronized(clock_ready_synchronized),
    .external_reset_seen(external_reset_seen)
);

wire instruction_request_valid;
wire [31:0] instruction_address;
wire instruction_response_ready;
wire instruction_response_valid;
wire [15:0] instruction_data;
wire instruction_error;

wire boot_selected = instruction_address[31:10] == 0;
wire [15:0] boot_read_data;
wire [15:0] unused_boot_rw_data;
reg boot_pending = 0;
reg boot_response_valid = 0;
wire boot_request_ready = !boot_pending && !boot_response_valid;
wire boot_accept = instruction_request_valid && boot_selected && boot_request_ready;

__BOOT_MEMORY__ u_boot (
    .clk(clk),
    .read_address(instruction_address[9:0]),
    .rw_write_enable(1'b0),
    .rw_address(10'b0),
    .rw_write_data(16'b0),
    .read_data(boot_read_data),
    .rw_read_data(unused_boot_rw_data)
);

always @(posedge clk) begin
    if (reset) begin
        boot_pending <= 0;
        boot_response_valid <= 0;
    end else if (boot_response_valid) begin
        if (instruction_response_ready)
            boot_response_valid <= 0;
    end else if (boot_pending) begin
        boot_pending <= 0;
        boot_response_valid <= 1;
    end else if (boot_accept) begin
        boot_pending <= 1;
    end
end

wire icache_cpu_request_ready;
wire icache_cpu_response_valid;
wire [15:0] icache_cpu_read_data;
wire icache_cpu_error;
wire icache_memory_request_valid;
wire icache_memory_request_ready;
wire [21:0] icache_memory_address;
wire icache_memory_response_valid;
wire [15:0] icache_memory_read_data;
wire icache_memory_error;
wire icache_memory_response_ready;

__CACHE__ u_instruction_cache (
    .clk(clk),
    .reset(reset),
    .invalidate_all(1'b0),
    .cpu_request_valid(instruction_request_valid && !boot_selected),
    .cpu_write(1'b0),
    .cpu_address(instruction_address),
    .cpu_write_data(16'b0),
    .cpu_response_ready(instruction_response_ready && !boot_selected),
    .memory_request_ready(icache_memory_request_ready),
    .memory_response_valid(icache_memory_response_valid),
    .memory_read_data(icache_memory_read_data),
    .memory_error(icache_memory_error),
    .cpu_request_ready(icache_cpu_request_ready),
    .cpu_response_valid(icache_cpu_response_valid),
    .cpu_read_data(icache_cpu_read_data),
    .cpu_error(icache_cpu_error),
    .memory_request_valid(icache_memory_request_valid),
    .memory_write(),
    .memory_address(icache_memory_address),
    .memory_write_data(),
    .memory_response_ready(icache_memory_response_ready)
);

assign instruction_response_valid = boot_selected ? boot_response_valid : icache_cpu_response_valid;
assign instruction_data = boot_selected ? boot_read_data : icache_cpu_read_data;
assign instruction_error = boot_selected ? 1'b0 : icache_cpu_error;
wire instruction_request_ready = boot_selected ? boot_request_ready : icache_cpu_request_ready;

wire core_data_request_valid;
wire core_data_write;
wire [31:0] core_data_address;
wire [15:0] core_data_write_data;
wire core_data_response_ready;
wire core_data_request_ready;
wire core_data_response_valid;
wire [15:0] core_data_read_data;
wire core_data_error;

wire [2:0] device_index;
wire [3:0] device_channel;
wire device_read_enable;
wire device_write_enable;
wire [15:0] device_write_data;
wire [15:0] device_read_data;

wire unused_dma_start;
wire [23:0] unused_flash_offset;
wire [21:0] unused_destination;
wire [31:0] unused_file_size_bytes;
wire [31:0] unused_memory_size_bytes;
__BOOT_DMA_DEVICE__ u_boot_dma_device (
    .clk(clk),
    .reset(reset),
    .device_index(device_index),
    .device_channel(device_channel),
    .device_read_enable(device_read_enable),
    .device_write_enable(device_write_enable),
    .device_write_data(device_write_data),
    .dma_busy(1'b0),
    .dma_done(1'b0),
    .dma_error(1'b0),
    .dma_error_code(8'b0),
    .dma_completed_words(32'b0),
    .device_read_data(device_read_data),
    .dma_start(unused_dma_start),
    .flash_offset(unused_flash_offset),
    .destination(unused_destination),
    .file_size_bytes(unused_file_size_bytes),
    .memory_size_bytes(unused_memory_size_bytes)
);

wire dcache_cpu_request_ready;
wire dcache_cpu_response_valid;
wire [15:0] dcache_cpu_read_data;
wire dcache_cpu_error;
wire dcache_memory_request_valid;
wire dcache_memory_write;
wire [21:0] dcache_memory_address;
wire [15:0] dcache_memory_write_data;
wire dcache_memory_request_ready;
wire dcache_memory_response_valid;
wire [15:0] dcache_memory_read_data;
wire dcache_memory_error;
wire dcache_memory_response_ready;

__CACHE__ u_data_cache (
    .clk(clk),
    .reset(reset),
    .invalidate_all(1'b0),
    .cpu_request_valid(core_data_request_valid),
    .cpu_write(core_data_write),
    .cpu_address(core_data_address),
    .cpu_write_data(core_data_write_data),
    .cpu_response_ready(core_data_response_ready),
    .memory_request_ready(dcache_memory_request_ready),
    .memory_response_valid(dcache_memory_response_valid),
    .memory_read_data(dcache_memory_read_data),
    .memory_error(dcache_memory_error),
    .cpu_request_ready(dcache_cpu_request_ready),
    .cpu_response_valid(dcache_cpu_response_valid),
    .cpu_read_data(dcache_cpu_read_data),
    .cpu_error(dcache_cpu_error),
    .memory_request_valid(dcache_memory_request_valid),
    .memory_write(dcache_memory_write),
    .memory_address(dcache_memory_address),
    .memory_write_data(dcache_memory_write_data),
    .memory_response_ready(dcache_memory_response_ready)
);

assign core_data_request_ready = dcache_cpu_request_ready;
assign core_data_response_valid = dcache_cpu_response_valid;
assign core_data_read_data = dcache_cpu_read_data;
assign core_data_error = dcache_cpu_error;

wire halted;
wire [15:0] halt_signal;
wire faulted;
wire [7:0] fault_code;
wire [15:0] fault_pc;
wire [15:0] pc;
wire [15:0] code_segment;
wire [15:0] data_segment;
wire [31:0] retired_words;

__CPU_V3_CORE__ u_core (
    .clk(clk),
    .reset(reset),
    .instruction_request_ready(instruction_request_ready),
    .instruction_response_valid(instruction_response_valid),
    .instruction_data(instruction_data),
    .instruction_error(instruction_error),
    .data_request_ready(core_data_request_ready),
    .data_response_valid(core_data_response_valid),
    .data_read_data(core_data_read_data),
    .data_error(core_data_error),
    .device_read_data(device_read_data),
    .instruction_request_valid(instruction_request_valid),
    .instruction_address(instruction_address),
    .instruction_response_ready(instruction_response_ready),
    .data_request_valid(core_data_request_valid),
    .data_write(core_data_write),
    .data_address(core_data_address),
    .data_write_data(core_data_write_data),
    .data_response_ready(core_data_response_ready),
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

wire memory_request_valid;
wire memory_write;
wire [21:0] memory_address;
wire [15:0] memory_write_data;
wire memory_request_ready;
wire memory_response_valid;
wire [15:0] memory_read_data;
wire memory_error;
wire memory_response_ready;

__ARBITER__ u_memory_arbiter (
    .clk(clk),
    .reset(reset),
    .instruction_request_valid(icache_memory_request_valid),
    .instruction_address(icache_memory_address),
    .instruction_response_ready(icache_memory_response_ready),
    .data_request_valid(dcache_memory_request_valid),
    .data_write(dcache_memory_write),
    .data_address(dcache_memory_address),
    .data_write_data(dcache_memory_write_data),
    .data_response_ready(dcache_memory_response_ready),
    .dma_request_valid(1'b0),
    .dma_write(1'b1),
    .dma_address(22'b0),
    .dma_write_data(16'b0),
    .dma_response_ready(1'b1),
    .memory_request_ready(memory_request_ready),
    .memory_response_valid(memory_response_valid),
    .memory_read_data(memory_read_data),
    .memory_error(memory_error),
    .instruction_request_ready(icache_memory_request_ready),
    .instruction_response_valid(icache_memory_response_valid),
    .instruction_read_data(icache_memory_read_data),
    .instruction_error(icache_memory_error),
    .data_request_ready(dcache_memory_request_ready),
    .data_response_valid(dcache_memory_response_valid),
    .data_read_data(dcache_memory_read_data),
    .data_error(dcache_memory_error),
    .dma_request_ready(),
    .dma_response_valid(),
    .dma_read_data(),
    .dma_error(),
    .memory_request_valid(memory_request_valid),
    .memory_write(memory_write),
    .memory_address(memory_address),
    .memory_write_data(memory_write_data),
    .memory_response_ready(memory_response_ready)
);

__SDRAM_WORD_PORT__ u_sdram_word_port (
    .clk(clk),
    .reset(reset),
    .request_valid(memory_request_valid),
    .write(memory_write),
    .address(memory_address),
    .write_data(memory_write_data),
    .response_ready(memory_response_ready),
    .controller_read_data(sdram_read_data),
    .controller_read_valid(sdram_read_valid),
    .controller_init_done(sdram_init_done),
    .controller_command_ack(sdram_command_ack),
    .request_ready(memory_request_ready),
    .response_valid(memory_response_valid),
    .read_data(memory_read_data),
    .error(memory_error),
    .controller_command_valid(sdram_command_valid),
    .controller_command(sdram_command),
    .controller_precharge(sdram_precharge),
    .controller_address(sdram_address),
    .controller_write_mask(sdram_write_mask),
    .controller_write_data(sdram_write_data),
    .controller_burst_length(sdram_burst_length)
);

wire passed = halted && halt_signal == 16'h1235;
assign leds = passed ? 6'b000001 : (faulted || halted ? 6'b100001 : 6'b001100);

wire test_done = halted || faulted;
wire [7:0] report_status = passed ? 8'h00 : (faulted ? 8'h02 : 8'h01);
wire uart_busy;
wire frame_toggle;
__DIAGNOSTIC_REPORTER__ u_reporter(
    .clk(clk),
    .report_enable(test_done),
    .status(report_status),
    .uart_tx(uart_tx),
    .uart_busy(uart_busy),
    .frame_toggle(frame_toggle)
);

endmodule
