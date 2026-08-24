module BootDmaSelfTest (
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
reg start = 0;
reg started = 0;
wire busy;
wire done;
wire error;
wire [7:0] error_code;
wire [31:0] completed_words;

__TANG_NANO_20K_BOOT_DMA__ u_dma (
    .clk(clk),
    .reset(buttons[1]),
    .start(start),
    .flash_offset(24'h000000),
    .destination(22'h100007),
    .file_size_bytes(32'd3),
    .memory_size_bytes(32'd6),
    .flash_miso(flash_miso),
    .sdram_read_data(sdram_read_data),
    .sdram_read_valid(sdram_read_valid),
    .sdram_init_done(sdram_init_done),
    .sdram_command_ack(sdram_command_ack),
    .busy(busy),
    .done(done),
    .error(error),
    .error_code(error_code),
    .completed_words(completed_words),
    .flash_clk(flash_clk),
    .flash_cs_n(flash_cs_n),
    .flash_mosi(flash_mosi),
    .sdram_command_valid(sdram_command_valid),
    .sdram_command(sdram_command),
    .sdram_precharge(sdram_precharge),
    .sdram_address(sdram_address),
    .sdram_write_mask(sdram_write_mask),
    .sdram_write_data(sdram_write_data),
    .sdram_burst_length(sdram_burst_length)
);

always @(posedge clk) begin
    start <= 0;
    if (buttons[1]) begin
        started <= 0;
    end else if (sdram_init_done && !started) begin
        start <= 1;
        started <= 1;
    end
end

wire success = done && !error && completed_words == 3;
assign leds = error ? {error_code[0], error_code[1], error_code[2], error_code[3], 2'b00} :
              success ? 6'b111111 :
              busy ? 6'b000010 : 6'b000001;

wire uart_busy;
wire frame_toggle;
__DIAGNOSTIC_REPORTER__ u_reporter(
    .clk(clk),
    .report_enable(done || error),
    .status(success ? 8'h00 : 8'h01),
    .uart_tx(uart_tx),
    .uart_busy(uart_busy),
    .frame_toggle(frame_toggle)
);
endmodule
