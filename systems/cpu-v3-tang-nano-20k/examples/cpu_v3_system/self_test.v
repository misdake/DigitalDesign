// CPU V3 cold-start system. A fixed hardware loader copies the single
// application from SPI Flash and clears the static arena before CPU release; the
// application reports through the device-0 system control UART. The 400x240
// RGB565 framebuffer scanout is written to SDRAM by the CPU and scanned out
// through the SharedSdramPort adapter and the fitted HDMI datapath (the
// compiled-in mode from `ACTIVE_DISPLAY_CONFIG`). Reporting is
// entirely the software's job; the harness only wires devices, caches,
// memories, and the display together.
module CpuV3System (
    input wire clk,
    input wire [1:0] buttons,
    input wire flash_miso,
    input wire [63:0] sdram_read_data,
    input wire sdram_read_valid,
    input wire sdram_init_done,
    input wire sdram_request_ready,
    input wire sdram_stream_active,
    input wire sdram_clock_ready,
    input wire sdram_done,
    input wire sdram_write_data_ready,
    input wire pixel_clock,
    input wire serial_clock,
    input wire video_locked,
    output wire [5:0] leds,
    output wire uart_tx,
    output wire flash_clk,
    output wire flash_cs_n,
    output wire flash_mosi,
    output wire sdram_request_valid,
    output wire sdram_next_valid,
    output wire [20:0] sdram_next_address,
    output wire sdram_write,
    output wire [20:0] sdram_address,
    output wire [3:0] sdram_write_mask,
    output wire [63:0] sdram_write_data,
    output wire sdram_write_data_valid,
    output wire [5:0] sdram_words,
    output wire tmds_clk_p,
    output wire tmds_clk_n,
    output wire [2:0] tmds_data_p,
    output wire [2:0] tmds_data_n
);

// No global runtime reset tree. Module declaration initial values are the
// FPGA configuration state. This monotonic gate only releases cold-start work.
wire reset=1'b0;
reg [1:0] cold_ready_sync=0;
reg cold_ready=0;
always @(posedge clk) if(!cold_ready) begin
    cold_ready_sync<={cold_ready_sync[0],sdram_clock_ready && sdram_init_done};
    if(cold_ready_sync[1])cold_ready<=1;
end
// The existing video-domain startup guard sees a monotonic ready flag;
// established lock loss cannot become a hidden runtime pipeline reset.
reg [2:0] video_cold_ready_sync=0;
always @(posedge pixel_clock) if(!video_cold_ready_sync[2])
    video_cold_ready_sync<={video_cold_ready_sync[1:0],video_locked};
wire boot_done,boot_error,boot_load_done;
wire display_started_ack,display_startup_error;
wire [3:0] boot_error_code;

wire core_instruction_request_valid;
wire [31:0] core_instruction_address;
wire core_instruction_response_ready;
wire core_instruction_request_ready;
wire core_instruction_response_valid;
wire [15:0] core_instruction_data;
wire core_instruction_error;

wire instruction_request_valid;
wire [31:0] instruction_address;
wire instruction_response_ready;
wire instruction_response_valid;
wire [15:0] instruction_data;
wire instruction_error;
wire instruction_request_ready;
wire sysctl_icache_invalidate;
wire sysctl_dcache_invalidate;
wire sysctl_dcache_clean;
wire sysctl_cpu_hold;
wire dcache_maintenance_busy;
wire dcache_maintenance_done;
wire dcache_maintenance_error;
wire dcache_valid_sweep;
wire halted;
wire faulted;
wire [15:0] data_segment;

__FETCH_QUEUE__ u_instruction_fetch_queue (
    .clk(clk),
    .reset(reset),
    .flush(sysctl_icache_invalidate || halted || faulted),
    .core_request_valid(core_instruction_request_valid),
    .core_address(core_instruction_address),
    .core_response_ready(core_instruction_response_ready),
    .memory_request_ready(instruction_request_ready),
    .memory_response_valid(instruction_response_valid),
    .memory_read_data(instruction_data),
    .memory_error(instruction_error),
    .core_request_ready(core_instruction_request_ready),
    .core_response_valid(core_instruction_response_valid),
    .core_read_data(core_instruction_data),
    .core_error(core_instruction_error),
    .memory_request_valid(instruction_request_valid),
    .memory_address(instruction_address),
    .memory_response_ready(instruction_response_ready)
);

// There is no instruction overlay or source-selection FIFO.
wire icache_cpu_request_ready;
assign instruction_request_ready=icache_cpu_request_ready;

wire icache_cpu_response_valid;
wire [15:0] icache_cpu_read_data;
wire icache_cpu_error;
wire icache_memory_request_valid;
wire icache_memory_request_ready;
wire [21:0] icache_memory_address;
wire icache_memory_response_valid;
wire [63:0] icache_memory_read_data;
wire icache_memory_error;
wire icache_memory_response_ready;

__CACHE__ u_instruction_cache (
    .clk(clk),
    .reset(reset),
    .invalidate_all(sysctl_icache_invalidate),
    .cpu_request_valid(instruction_request_valid),
    .cpu_address(instruction_address),
    .cpu_response_ready(instruction_response_ready),
    .memory_request_ready(icache_memory_request_ready),
    .memory_response_valid(icache_memory_response_valid),
    .memory_read_data(icache_memory_read_data),
    .memory_error(icache_memory_error),
    .cpu_request_ready(icache_cpu_request_ready),
    .cpu_response_valid(icache_cpu_response_valid),
    .cpu_read_data(icache_cpu_read_data),
    .cpu_error(icache_cpu_error),
    .memory_request_valid(icache_memory_request_valid),
    .memory_address(icache_memory_address),
    .memory_response_ready(icache_memory_response_ready)
);

assign instruction_response_valid=icache_cpu_response_valid;
assign instruction_data=icache_cpu_read_data;
assign instruction_error=icache_cpu_error;

wire core_data_request_valid;
wire core_data_write;
wire [31:0] core_data_address;
wire [15:0] core_data_write_data;
wire core_data_response_ready;
wire core_data_request_ready;
wire core_data_response_valid;
wire [15:0] core_data_read_data;
wire core_data_error;
wire core_data_line_copy_valid;
wire [21:0] core_data_line_copy_source;
wire [7:0] core_data_line_copy_destination_page;
wire core_data_line_clean_valid;
wire [21:0] core_data_line_clean_address;
wire dcache_line_copy_ready;

wire [2:0] device_index;
wire [3:0] device_channel;
wire device_read_enable;
wire device_write_enable;
wire [15:0] device_write_data;
wire [15:0] device_read_data;
wire [15:0] sysctl_read_data;
wire [15:0] display_read_data;
wire [15:0] gpu_read_data;
wire [5:0] software_leds;
wire [2:0] watch_device_index;
wire [3:0] watch_device_channel;
wire watch_read_enable;
wire [2:0] device_query_index = watch_read_enable ? watch_device_index : device_index;
wire [3:0] device_query_channel = watch_read_enable ? watch_device_channel : device_channel;
wire device_query_read_enable = device_read_enable || watch_read_enable;
wire device_query_write_enable = device_write_enable && !watch_read_enable;

// Unselected devices read back zero, so the core sees the OR of all buses.
assign device_read_data =
    sysctl_read_data | display_read_data |
    gpu_read_data;

// Device 0: cache invalidate pulses, LEDs, and the reporting UART.
__SYSTEM_CONTROL__ u_sysctl (
    .clk(clk),
    .reset(reset),
    .device_index(device_query_index),
    .device_channel(device_query_channel),
    .device_read_enable(device_query_read_enable),
    .device_write_enable(device_query_write_enable),
    .device_write_data(device_write_data),
    .dcache_maintenance_busy(dcache_maintenance_busy),
    .dcache_maintenance_done(dcache_maintenance_done),
    .dcache_maintenance_error(dcache_maintenance_error),
    .watch_read_data(device_read_data),
    .device_read_data(sysctl_read_data),
    .icache_invalidate(sysctl_icache_invalidate),
    .dcache_invalidate(sysctl_dcache_invalidate),
    .dcache_clean(sysctl_dcache_clean),
    .cpu_hold(sysctl_cpu_hold),
    .watch_device_index(watch_device_index),
    .watch_device_channel(watch_device_channel),
    .watch_read_enable(watch_read_enable),
    .leds(software_leds),
    .uart_tx(uart_tx)
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

// Fixed boot owner; every write waits for the real arbiter response.
assign dma_memory_write=1'b1;
__SINGLE_BOOT_LOADER__ u_boot_loader (
    .clk(clk),.ready_to_start(cold_ready),
    .flash_ready(flash_ready),.flash_data_valid(flash_data_valid),.flash_data(flash_data),
    .flash_done(flash_done),.flash_error(flash_error),
    .memory_request_ready(dma_memory_request_ready),
    .memory_response_valid(dma_memory_response_valid),.memory_error(dma_memory_error),
    .display_started(display_started_ack),.display_error(display_startup_error),
    .cache_ready(!dcache_valid_sweep),.load_done(boot_load_done),
    .flash_start(flash_start),.flash_address(flash_address),.flash_length(flash_length),
    .flash_data_ready(flash_data_ready),.memory_request_valid(dma_memory_request_valid),
    .memory_address(dma_memory_address),.memory_write_data(dma_memory_write_data),
    .memory_response_ready(dma_memory_response_ready),
    .done(boot_done),.error(boot_error),.error_code(boot_error_code)
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
wire [63:0] dcache_memory_write_data;
wire dcache_memory_line;
wire dcache_memory_request_ready;
wire dcache_memory_write_data_ready;
wire dcache_memory_response_valid;
wire [63:0] dcache_memory_read_data;
wire dcache_memory_error;
wire dcache_memory_response_ready;

__DATA_CACHE__ u_data_cache (
    .clk(clk),
    .reset(reset),
    .clean_all(sysctl_dcache_clean),
    .invalidate_all(sysctl_dcache_invalidate),
    .line_copy_start(core_data_line_copy_valid),
    .line_copy_source(core_data_line_copy_source),
    .line_copy_destination_page(core_data_line_copy_destination_page),
    .line_clean_start(core_data_line_clean_valid),
    .line_clean_address(core_data_line_clean_address),
    .line_copy_ready(dcache_line_copy_ready),
    .cpu_request_valid(core_data_request_valid),
    .cpu_write(core_data_write),
    .cpu_address(core_data_address),
    .cpu_write_data(core_data_write_data),
    .cpu_response_ready(core_data_response_ready),
    .memory_request_ready(dcache_memory_request_ready),
    .memory_write_data_ready(dcache_memory_write_data_ready),
    .memory_response_valid(dcache_memory_response_valid),
    .memory_read_data(dcache_memory_read_data),
    .memory_error(dcache_memory_error),
    .cpu_request_ready(dcache_cpu_request_ready),
    .cpu_response_valid(dcache_cpu_response_valid),
    .cpu_read_data(dcache_cpu_read_data),
    .cpu_error(dcache_cpu_error),
    .memory_request_valid(dcache_memory_request_valid),
    .memory_write(dcache_memory_write),
    .memory_line(dcache_memory_line),
    .memory_address(dcache_memory_address),
    .memory_write_data(dcache_memory_write_data),
    .memory_response_ready(dcache_memory_response_ready),
    .maintenance_busy(dcache_maintenance_busy),
    .maintenance_done(dcache_maintenance_done),
    .maintenance_error(dcache_maintenance_error),
    .valid_sweep(dcache_valid_sweep)
);

assign core_data_request_ready = dcache_cpu_request_ready;
assign core_data_response_valid = dcache_cpu_response_valid;
assign core_data_read_data = dcache_cpu_read_data;
assign core_data_error = dcache_cpu_error;

wire [15:0] halt_signal;
wire [7:0] fault_code;
wire [15:0] fault_pc;
wire [15:0] pc;
wire [15:0] code_segment;
wire [31:0] retired_words;

__CPU_V3_CORE__ #(.INITIAL_CODE_SEGMENT(16'd7)) u_core (
    .clk(clk),
    .reset(reset),
    // Maintenance blocks architectural CPU progress only. The D-cache,
    // arbiter, DMA, display, and SDRAM adapter keep using clk normally. The
    // core is also held while the RAM16 valid arrays sweep-clear, because the
    // D-cache cannot accept requests until every stale line is invalid.
    .hold(!boot_done || sysctl_cpu_hold || dcache_valid_sweep),
    .instruction_request_ready(core_instruction_request_ready),
    .instruction_response_valid(core_instruction_response_valid),
    .instruction_data(core_instruction_data),
    .instruction_error(core_instruction_error),
    .data_request_ready(core_data_request_ready),
    .data_response_valid(core_data_response_valid),
    .data_read_data(core_data_read_data),
    .data_error(core_data_error),
    .data_line_copy_ready(dcache_line_copy_ready),
    .data_cache_command_error(dcache_maintenance_error),
    .device_read_data(device_read_data),
    .instruction_request_valid(core_instruction_request_valid),
    .instruction_address(core_instruction_address),
    .instruction_response_ready(core_instruction_response_ready),
    .data_request_valid(core_data_request_valid),
    .data_write(core_data_write),
    .data_address(core_data_address),
    .data_write_data(core_data_write_data),
    .data_response_ready(core_data_response_ready),
    .data_line_copy_valid(core_data_line_copy_valid),
    .data_line_copy_source(core_data_line_copy_source),
    .data_line_copy_destination_page(core_data_line_copy_destination_page),
    .data_line_clean_valid(core_data_line_clean_valid),
    .data_line_clean_address(core_data_line_clean_address),
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

// Boot status is observable without firmware. Software owns LEDs after release.
assign leds=boot_error ? {2'b11,boot_error_code} : !boot_done ? 6'b000001 : software_leds;

wire memory_request_valid;
wire memory_write;
wire memory_line;
wire [21:0] memory_address;
wire [1:0] memory_line_count_minus_1;
wire [63:0] memory_write_data;
wire memory_request_ready;
wire memory_write_data_ready;
wire memory_response_valid;
wire [63:0] memory_read_data;
wire memory_response_last;
wire memory_error;
wire memory_response_ready;

// GPU: device 4 with a command/tile-list read master plus independent
// framebuffer read and write masters.
wire gpu_ro_memory_request_valid;
wire gpu_ro_memory_write;
wire [21:0] gpu_ro_memory_address;
wire [1:0] gpu_ro_memory_line_count_minus_1;
wire [63:0] gpu_ro_memory_write_data;
wire gpu_ro_memory_request_ready;
wire gpu_ro_memory_write_data_ready;
wire gpu_ro_memory_response_valid;
wire [63:0] gpu_ro_memory_read_data;
wire gpu_ro_memory_response_last;
wire gpu_ro_memory_error;
wire gpu_fb_r_memory_request_valid;
wire gpu_fb_r_memory_write;
wire [21:0] gpu_fb_r_memory_address;
wire [1:0] gpu_fb_r_memory_line_count_minus_1;
wire [63:0] gpu_fb_r_memory_write_data;
wire gpu_fb_r_memory_request_ready;
wire gpu_fb_r_memory_write_data_ready;
wire gpu_fb_r_memory_response_valid;
wire [63:0] gpu_fb_r_memory_read_data;
wire gpu_fb_r_memory_response_last;
wire gpu_fb_r_memory_error;
wire gpu_fb_w_memory_request_valid;
wire gpu_fb_w_memory_write;
wire [21:0] gpu_fb_w_memory_address;
wire [1:0] gpu_fb_w_memory_line_count_minus_1;
wire [63:0] gpu_fb_w_memory_write_data;
wire gpu_fb_w_memory_request_ready;
wire gpu_fb_w_memory_write_data_ready;
wire gpu_fb_w_memory_response_valid;
wire [63:0] gpu_fb_w_memory_read_data;
wire gpu_fb_w_memory_response_last;
wire gpu_fb_w_memory_error;

// Display scanout client of the shared SDRAM port. The arbiter gives it strict
// priority; the scanout captures each 64-bit line beat and drains it locally.
// Declared here because the arbiter below already references it.
wire display_memory_request_valid;
wire display_memory_urgent;
wire [21:0] display_memory_address;
wire display_memory_request_ready;
wire display_memory_data_valid;
wire [63:0] display_memory_read_data;
wire display_memory_last;
wire display_memory_error;

__GPU__ u_gpu (
    .clk(clk),
    .reset(reset),
    .device_index(device_query_index),
    .device_channel(device_query_channel),
    .device_read_enable(device_query_read_enable),
    .device_write_enable(device_query_write_enable),
    .device_write_data(device_write_data),
    .gpu_ro_request_ready(gpu_ro_memory_request_ready),
    .gpu_ro_write_data_ready(gpu_ro_memory_write_data_ready),
    .gpu_ro_response_valid(gpu_ro_memory_response_valid),
    .gpu_ro_read_data(gpu_ro_memory_read_data),
    .gpu_ro_response_last(gpu_ro_memory_response_last),
    .gpu_ro_error(gpu_ro_memory_error),
    .gpu_fb_w_request_ready(gpu_fb_w_memory_request_ready),
    .gpu_fb_w_write_data_ready(gpu_fb_w_memory_write_data_ready),
    .gpu_fb_w_response_valid(gpu_fb_w_memory_response_valid),
    .gpu_fb_w_response_last(gpu_fb_w_memory_response_last),
    .gpu_fb_w_error(gpu_fb_w_memory_error),
    .gpu_fb_r_request_ready(gpu_fb_r_memory_request_ready),
    .gpu_fb_r_write_data_ready(gpu_fb_r_memory_write_data_ready),
    .gpu_fb_r_response_valid(gpu_fb_r_memory_response_valid),
    .gpu_fb_r_read_data(gpu_fb_r_memory_read_data),
    .gpu_fb_r_response_last(gpu_fb_r_memory_response_last),
    .gpu_fb_r_error(gpu_fb_r_memory_error),
    .device_read_data(gpu_read_data),
    .gpu_ro_request_valid(gpu_ro_memory_request_valid),
    .gpu_ro_write(gpu_ro_memory_write),
    .gpu_ro_address(gpu_ro_memory_address),
    .gpu_ro_line_count_minus_1(gpu_ro_memory_line_count_minus_1),
    .gpu_ro_write_data(gpu_ro_memory_write_data),
    .gpu_fb_w_request_valid(gpu_fb_w_memory_request_valid),
    .gpu_fb_w_write(gpu_fb_w_memory_write),
    .gpu_fb_w_address(gpu_fb_w_memory_address),
    .gpu_fb_w_line_count_minus_1(gpu_fb_w_memory_line_count_minus_1),
    .gpu_fb_w_write_data(gpu_fb_w_memory_write_data),
    .gpu_fb_r_request_valid(gpu_fb_r_memory_request_valid),
    .gpu_fb_r_write(gpu_fb_r_memory_write),
    .gpu_fb_r_address(gpu_fb_r_memory_address),
    .gpu_fb_r_line_count_minus_1(gpu_fb_r_memory_line_count_minus_1),
    .gpu_fb_r_write_data(gpu_fb_r_memory_write_data)
);

wire sdram_lookahead_window;

__ARBITER__ u_memory_arbiter (
    .lookahead_enable(sdram_lookahead_window),
    .clk(clk),
    .reset(reset),
    .instruction_request_valid(icache_memory_request_valid),
    .instruction_address(icache_memory_address),
    .instruction_response_ready(icache_memory_response_ready),
    .data_request_valid(dcache_memory_request_valid),
    .data_write(dcache_memory_write),
    .data_line(dcache_memory_line),
    .data_address(dcache_memory_address),
    .data_write_data(dcache_memory_write_data),
    .data_response_ready(dcache_memory_response_ready),
    .dma_request_valid(dma_memory_request_valid),
    .dma_write(dma_memory_write),
    .dma_address(dma_memory_address),
    .dma_write_data(dma_memory_write_data),
    .dma_response_ready(dma_memory_response_ready),
    .display_request_valid(display_memory_request_valid),
    .display_address(display_memory_address),
    .display_response_ready(1'b1),
    .gpu_ro_request_valid(gpu_ro_memory_request_valid && boot_done),
    .gpu_ro_write(gpu_ro_memory_write),
    .gpu_ro_address(gpu_ro_memory_address),
    .gpu_ro_line_count_minus_1(gpu_ro_memory_line_count_minus_1),
    .gpu_ro_write_data(gpu_ro_memory_write_data),
    .gpu_fb_r_request_valid(gpu_fb_r_memory_request_valid && boot_done),
    .gpu_fb_r_write(gpu_fb_r_memory_write),
    .gpu_fb_r_address(gpu_fb_r_memory_address),
    .gpu_fb_r_line_count_minus_1(gpu_fb_r_memory_line_count_minus_1),
    .gpu_fb_r_write_data(gpu_fb_r_memory_write_data),
    .gpu_fb_w_request_valid(gpu_fb_w_memory_request_valid && boot_done),
    .gpu_fb_w_write(gpu_fb_w_memory_write),
    .gpu_fb_w_address(gpu_fb_w_memory_address),
    .gpu_fb_w_line_count_minus_1(gpu_fb_w_memory_line_count_minus_1),
    .gpu_fb_w_write_data(gpu_fb_w_memory_write_data),
    .memory_request_ready(memory_request_ready),
    .memory_write_data_ready(memory_write_data_ready),
    .memory_response_valid(memory_response_valid),
    .memory_read_data(memory_read_data),
    .memory_response_last(memory_response_last),
    .memory_error(memory_error),
    .instruction_request_ready(icache_memory_request_ready),
    .instruction_response_valid(icache_memory_response_valid),
    .instruction_read_data(icache_memory_read_data),
    .instruction_error(icache_memory_error),
    .data_request_ready(dcache_memory_request_ready),
    .data_write_data_ready(dcache_memory_write_data_ready),
    .data_response_valid(dcache_memory_response_valid),
    .data_read_data(dcache_memory_read_data),
    .data_error(dcache_memory_error),
    .dma_request_ready(dma_memory_request_ready),
    .dma_response_valid(dma_memory_response_valid),
    .dma_read_data(),
    .dma_error(dma_memory_error),
    .display_request_ready(display_memory_request_ready),
    .display_response_valid(display_memory_data_valid),
    .display_read_data(display_memory_read_data),
    .display_response_last(display_memory_last),
    .display_error(display_memory_error),
    .gpu_ro_request_ready(gpu_ro_memory_request_ready),
    .gpu_ro_write_data_ready(gpu_ro_memory_write_data_ready),
    .gpu_ro_response_valid(gpu_ro_memory_response_valid),
    .gpu_ro_read_data(gpu_ro_memory_read_data),
    .gpu_ro_response_last(gpu_ro_memory_response_last),
    .gpu_ro_error(gpu_ro_memory_error),
    .gpu_fb_r_request_ready(gpu_fb_r_memory_request_ready),
    .gpu_fb_r_write_data_ready(gpu_fb_r_memory_write_data_ready),
    .gpu_fb_r_response_valid(gpu_fb_r_memory_response_valid),
    .gpu_fb_r_read_data(gpu_fb_r_memory_read_data),
    .gpu_fb_r_response_last(gpu_fb_r_memory_response_last),
    .gpu_fb_r_error(gpu_fb_r_memory_error),
    .gpu_fb_w_request_ready(gpu_fb_w_memory_request_ready),
    .gpu_fb_w_write_data_ready(gpu_fb_w_memory_write_data_ready),
    .gpu_fb_w_response_valid(gpu_fb_w_memory_response_valid),
    .gpu_fb_w_read_data(gpu_fb_w_memory_read_data),
    .gpu_fb_w_response_last(gpu_fb_w_memory_response_last),
    .gpu_fb_w_error(gpu_fb_w_memory_error),
    .memory_request_valid(memory_request_valid),
    .memory_write(memory_write),
    .memory_line(memory_line),
    .memory_address(memory_address),
    .memory_line_count_minus_1(memory_line_count_minus_1),
    .memory_write_data(memory_write_data),
    .memory_response_ready(memory_response_ready)
);

__SHARED_SDRAM_PORT__ #(.EARLY_GRANT(1)) u_shared_sdram_port (
    .clk(clk),
    .reset(reset),
    .cpu_request_valid(memory_request_valid),
    .cpu_write(memory_write),
    .cpu_line(memory_line),
    .cpu_address(memory_address),
    .cpu_line_count_minus_1(memory_line_count_minus_1),
    .cpu_write_data(memory_write_data),
    .cpu_response_ready(memory_response_ready),
    .controller_read_data(sdram_read_data),
    .controller_read_valid(sdram_read_valid),
    .controller_init_done(sdram_init_done),
    .controller_request_ready(sdram_request_ready),
    .controller_stream_active(sdram_stream_active),
    .controller_done(sdram_done),
    .controller_write_data_ready(sdram_write_data_ready),
    .cpu_request_ready(memory_request_ready),
    .cpu_lookahead_window(sdram_lookahead_window),
    .cpu_write_data_ready(memory_write_data_ready),
    .cpu_response_valid(memory_response_valid),
    .cpu_read_data(memory_read_data),
    .cpu_response_last(memory_response_last),
    .cpu_error(memory_error),
    .controller_request_valid(sdram_request_valid),
    .controller_next_valid(sdram_next_valid),
    .controller_next_address(sdram_next_address),
    .controller_write(sdram_write),
    .controller_address(sdram_address),
    .controller_write_mask(sdram_write_mask),
    .controller_write_data(sdram_write_data),
    .controller_write_data_valid(sdram_write_data_valid),
    .controller_words(sdram_words)
);

// Device 3: framebuffer scanout and HDMI TMDS output.
__FRAMEBUFFER_HDMI__ u_display (
    .clk(clk),
    .reset(reset),
    .pixel_clock(pixel_clock),
    .serial_clock(serial_clock),
    .video_locked(video_cold_ready_sync[2]),
    .boot_enable(boot_load_done),.boot_failed(boot_error),
    .started_ack(display_started_ack),.startup_error(display_startup_error),
    .memory_request_ready(display_memory_request_ready),
    .memory_data_valid(display_memory_data_valid),
    .memory_read_data(display_memory_read_data),
    .memory_last(display_memory_last),
    .memory_error(display_memory_error),
    .device_index(device_query_index),
    .device_channel(device_query_channel),
    .device_read_enable(device_query_read_enable),
    .device_write_enable(device_query_write_enable),
    .device_write_data(device_write_data),
    .memory_request_valid(display_memory_request_valid),
    .memory_urgent(display_memory_urgent),
    .memory_address(display_memory_address),
    .underflow(),
    .device_read_data(display_read_data),
    .tmds_clk_p(tmds_clk_p),
    .tmds_clk_n(tmds_clk_n),
    .tmds_data_p(tmds_data_p),
    .tmds_data_n(tmds_data_n)
);

endmodule
