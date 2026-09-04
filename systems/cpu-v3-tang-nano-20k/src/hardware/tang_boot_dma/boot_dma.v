module TangNano20KBootDma (
    input wire clk,
    input wire reset,
    input wire start,
    input wire [23:0] flash_offset,
    input wire [21:0] destination,
    input wire [31:0] file_size_bytes,
    input wire [31:0] memory_size_bytes,
    input wire flash_miso,
    input wire [31:0] sdram_read_data,
    input wire sdram_read_valid,
    input wire sdram_init_done,
    input wire sdram_command_ack,
    output wire busy,
    output wire done,
    output wire error,
    output wire [7:0] error_code,
    output wire [31:0] completed_words,
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
wire flash_start;
wire [23:0] flash_address;
wire [23:0] flash_length;
wire flash_data_ready;
wire flash_ready;
wire flash_data_valid;
wire [7:0] flash_data;
wire flash_done;
wire flash_error;

wire memory_request_valid;
wire memory_request_ready;
wire memory_write;
wire [21:0] memory_address;
wire [15:0] memory_write_data;
wire memory_response_ready;
wire memory_response_valid;
wire [31:0] memory_read_data;
wire memory_error;

__BOOT_DMA_ENGINE__ u_engine (
    .clk(clk),
    .reset(reset),
    .start(start),
    .flash_offset(flash_offset),
    .destination(destination),
    .file_size_bytes(file_size_bytes),
    .memory_size_bytes(memory_size_bytes),
    .flash_ready(flash_ready),
    .flash_data_valid(flash_data_valid),
    .flash_data(flash_data),
    .flash_done(flash_done),
    .flash_error(flash_error),
    .memory_request_ready(memory_request_ready),
    .memory_response_valid(memory_response_valid),
    .memory_error(memory_error),
    .busy(busy),
    .done(done),
    .error(error),
    .error_code(error_code),
    .completed_words(completed_words),
    .flash_start(flash_start),
    .flash_address(flash_address),
    .flash_length(flash_length),
    .flash_data_ready(flash_data_ready),
    .memory_request_valid(memory_request_valid),
    .memory_write(memory_write),
    .memory_address(memory_address),
    .memory_write_data(memory_write_data),
    .memory_response_ready(memory_response_ready)
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

__SDRAM_WORD_PORT__ u_memory (
    .clk(clk),
    .reset(reset),
    .request_valid(memory_request_valid),
    .write(memory_write),
    .read_line(1'b0),
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
    .response_last(),
    .error(memory_error),
    .controller_command_valid(sdram_command_valid),
    .controller_command(sdram_command),
    .controller_precharge(sdram_precharge),
    .controller_address(sdram_address),
    .controller_write_mask(sdram_write_mask),
    .controller_write_data(sdram_write_data),
    .controller_burst_length(sdram_burst_length)
);

wire [31:0] unused_memory_read_data = memory_read_data;
endmodule
