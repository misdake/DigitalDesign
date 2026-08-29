// Board harness for the complete CPU V3 two-stage flash boot: the Stage0 BSRAM
// boot ROM loads Stage1 from SPI Flash through the boot DMA engine, Stage1
// loads the demo application, and the application reports through the
// device-0 system control UART. Reporting is entirely the software's job;
// the harness only wires devices, caches, and memories together.
module CpuV3BootSelfTest (
    input wire clk,
    input wire [1:0] buttons,
    input wire flash_miso,
    input wire [31:0] sdram_read_data,
    input wire sdram_read_valid,
    input wire sdram_init_done,
    input wire sdram_command_ack,
    output wire [5:0] leds,
    output wire uart_tx,
    output wire flash_clk,
    output wire flash_cs_n,
    output wire flash_mosi,
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

// Boot window: physical instruction words 0x0000..0x03ff fetch the Stage0
// BSRAM image while every data access reaches SDRAM through the data cache.
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

wire sysctl_icache_invalidate;
wire sysctl_dcache_invalidate;

__CACHE__ u_instruction_cache (
    .clk(clk),
    .reset(reset),
    .invalidate_all(sysctl_icache_invalidate),
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
wire [15:0] sysctl_read_data;
wire [15:0] boot_select_read_data;
wire [15:0] dma_device_read_data;
wire [5:0] software_leds;

// Unselected devices read back zero, so the core sees the OR of all buses.
assign device_read_data = sysctl_read_data | boot_select_read_data | dma_device_read_data;

// Buttons are reset inputs, so their live value is 00 by the time Stage1 can
// run. Synchronize and remember only the two valid one-hot selections while a
// button is held; 00 retains the last selection and 11 is deliberately ignored.
reg [1:0] buttons_meta = 0;
reg [1:0] buttons_synchronized = 0;
reg [1:0] boot_select = 0;
always @(posedge clk) begin
    buttons_meta <= buttons;
    buttons_synchronized <= buttons_meta;
    case (buttons_synchronized)
        2'b01, 2'b10: boot_select <= buttons_synchronized;
        default: boot_select <= boot_select;
    endcase
end
assign boot_select_read_data =
    device_read_enable && device_index == 3'd1 && device_channel == 4'd0
        ? {14'b0, boot_select}
        : 16'b0;

// Device 0: cache invalidate pulses, LEDs, and the reporting UART.
__SYSTEM_CONTROL__ u_sysctl (
    .clk(clk),
    .reset(reset),
    .device_index(device_index),
    .device_channel(device_channel),
    .device_read_enable(device_read_enable),
    .device_write_enable(device_write_enable),
    .device_write_data(device_write_data),
    .device_read_data(sysctl_read_data),
    .icache_invalidate(sysctl_icache_invalidate),
    .dcache_invalidate(sysctl_dcache_invalidate),
    .leds(software_leds),
    .uart_tx(uart_tx)
);

wire dma_start;
wire [23:0] dma_flash_offset;
wire [21:0] dma_destination;
wire [31:0] dma_file_size_bytes;
wire [31:0] dma_memory_size_bytes;
wire dma_busy;
wire dma_done;
wire dma_error;
wire [7:0] dma_error_code;
wire [31:0] dma_completed_words;

// Device 2: boot DMA command/status register bank.
__BOOT_DMA_DEVICE__ u_boot_dma_device (
    .clk(clk),
    .reset(reset),
    .device_index(device_index),
    .device_channel(device_channel),
    .device_read_enable(device_read_enable),
    .device_write_enable(device_write_enable),
    .device_write_data(device_write_data),
    .dma_busy(dma_busy),
    .dma_done(dma_done),
    .dma_error(dma_error),
    .dma_error_code(dma_error_code),
    .dma_completed_words(dma_completed_words),
    .device_read_data(dma_device_read_data),
    .dma_start(dma_start),
    .flash_offset(dma_flash_offset),
    .destination(dma_destination),
    .file_size_bytes(dma_file_size_bytes),
    .memory_size_bytes(dma_memory_size_bytes)
);

wire flash_start;
wire [23:0] flash_address;
wire [23:0] flash_length;
wire flash_data_ready;
wire flash_ready;
wire flash_data_valid;
wire [7:0] flash_data;
wire flash_done;
wire flash_error;

wire dma_memory_request_valid;
wire dma_memory_request_ready;
wire dma_memory_write;
wire [21:0] dma_memory_address;
wire [15:0] dma_memory_write_data;
wire dma_memory_response_ready;
wire dma_memory_response_valid;
wire dma_memory_error;

// The DMA engine only writes memory; its word port shares the SDRAM through
// the arbiter's DMA client instead of owning a private controller port.
__BOOT_DMA_ENGINE__ u_boot_dma_engine (
    .clk(clk),
    .reset(reset),
    .start(dma_start),
    .flash_offset(dma_flash_offset),
    .destination(dma_destination),
    .file_size_bytes(dma_file_size_bytes),
    .memory_size_bytes(dma_memory_size_bytes),
    .flash_ready(flash_ready),
    .flash_data_valid(flash_data_valid),
    .flash_data(flash_data),
    .flash_done(flash_done),
    .flash_error(flash_error),
    .memory_request_ready(dma_memory_request_ready),
    .memory_response_valid(dma_memory_response_valid),
    .memory_error(dma_memory_error),
    .busy(dma_busy),
    .done(dma_done),
    .error(dma_error),
    .error_code(dma_error_code),
    .completed_words(dma_completed_words),
    .flash_start(flash_start),
    .flash_address(flash_address),
    .flash_length(flash_length),
    .flash_data_ready(flash_data_ready),
    .memory_request_valid(dma_memory_request_valid),
    .memory_write(dma_memory_write),
    .memory_address(dma_memory_address),
    .memory_write_data(dma_memory_write_data),
    .memory_response_ready(dma_memory_response_ready)
);

__FLASH_READER__ u_flash (
    .clk(clk),
    .start(flash_start),
    .address(flash_address),
    .length(flash_length),
    .data_ready(flash_data_ready),
    .flash_miso(flash_miso),
    .ready(flash_ready),
    .data_valid(flash_data_valid),
    .data(flash_data),
    .done(flash_done),
    .error(flash_error),
    .flash_clk(flash_clk),
    .flash_cs_n(flash_cs_n),
    .flash_mosi(flash_mosi)
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
    .invalidate_all(sysctl_dcache_invalidate),
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

wire diagnostic_active;
wire [5:0] diagnostic_leds;
wire [2:0] boot_phase;
wire boot_error_sticky;
wire software_led_write = device_write_enable && device_index == 0 && device_channel == 2;

// This observer never controls boot. It only makes pre-software progress
// visible, then permanently hands the LEDs to the first software LED write.
__BOOT_PROGRESS_MONITOR__ u_boot_progress (
    .clk(clk),
    .reset(reset),
    .sdram_ready(sdram_init_done),
    .dma_busy(dma_busy),
    .dma_error(dma_error),
    .cpu_fault(faulted),
    .code_segment(code_segment),
    .software_led_write(software_led_write),
    .diagnostic_active(diagnostic_active),
    .diagnostic_leds(diagnostic_leds),
    .phase(boot_phase),
    .error_sticky(boot_error_sticky)
);

assign leds = diagnostic_active ? diagnostic_leds : software_leds;

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
    .dma_request_valid(dma_memory_request_valid),
    .dma_write(dma_memory_write),
    .dma_address(dma_memory_address),
    .dma_write_data(dma_memory_write_data),
    .dma_response_ready(dma_memory_response_ready),
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
    .dma_request_ready(dma_memory_request_ready),
    .dma_response_valid(dma_memory_response_valid),
    .dma_read_data(),
    .dma_error(dma_memory_error),
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

endmodule
