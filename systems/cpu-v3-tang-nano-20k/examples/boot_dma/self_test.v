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

localparam [2:0] ST_WAIT_INIT = 0;
localparam [2:0] ST_WAIT_DMA = 1;
localparam [2:0] ST_READ_REQUEST = 2;
localparam [2:0] ST_READ_RESPONSE = 3;
localparam [2:0] ST_DONE = 4;
localparam [2:0] ST_ERROR = 5;

reg [2:0] state = ST_WAIT_INIT;
reg dma_start = 0;
reg write_prefix_mismatch = 0;
reg [5:0] issued_write_words = 0;
reg [5:0] read_word = 0;
reg [7:0] diagnostic_status = 8'hff;

wire dma_busy;
wire dma_done;
wire dma_error;
wire [7:0] dma_error_code;
wire [31:0] dma_completed_words;
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

__BOOT_DMA_ENGINE__ u_dma (
    .clk(clk), .reset(buttons[1]), .start(dma_start),
    .flash_offset(24'h100000), .destination(22'h000040),
    .file_size_bytes(32'd64), .memory_size_bytes(32'd64),
    .flash_ready(flash_ready), .flash_data_valid(flash_data_valid),
    .flash_data(flash_data), .flash_done(flash_done), .flash_error(flash_error),
    .memory_request_ready(dma_memory_request_ready),
    .memory_response_valid(dma_memory_response_valid),
    .memory_error(dma_memory_error), .busy(dma_busy), .done(dma_done),
    .error(dma_error), .error_code(dma_error_code),
    .completed_words(dma_completed_words), .flash_start(flash_start),
    .flash_address(flash_address), .flash_length(flash_length),
    .flash_data_ready(flash_data_ready),
    .memory_request_valid(dma_memory_request_valid),
    .memory_write(dma_memory_write), .memory_address(dma_memory_address),
    .memory_write_data(dma_memory_write_data),
    .memory_response_ready(dma_memory_response_ready)
);

__FLASH_READER__ u_flash (
    .clk(clk), .start(flash_start), .address(flash_address),
    .length(flash_length), .data_ready(flash_data_ready),
    .flash_miso(flash_miso), .ready(flash_ready),
    .data_valid(flash_data_valid), .data(flash_data), .done(flash_done),
    .error(flash_error), .flash_clk(flash_clk), .flash_cs_n(flash_cs_n),
    .flash_mosi(flash_mosi)
);

wire probe_read_active = state == ST_READ_REQUEST || state == ST_READ_RESPONSE;
wire memory_request_valid = state == ST_READ_REQUEST ? 1'b1 :
                            probe_read_active ? 1'b0 : dma_memory_request_valid;
wire memory_write = probe_read_active ? 1'b0 : dma_memory_write;
wire [21:0] memory_address = probe_read_active ? 22'h000040 + read_word :
                             dma_memory_address;
wire [15:0] memory_write_data = probe_read_active ? 16'b0 : dma_memory_write_data;
wire memory_response_ready = state == ST_READ_RESPONSE ? 1'b1 :
                             probe_read_active ? 1'b0 : dma_memory_response_ready;
wire memory_request_ready;
wire memory_response_valid;
wire [15:0] memory_read_data;
wire memory_error;

assign dma_memory_request_ready = !probe_read_active && memory_request_ready;
assign dma_memory_response_valid = !probe_read_active && memory_response_valid;
assign dma_memory_error = !probe_read_active && memory_error;

__SDRAM_WORD_PORT__ u_memory (
    .clk(clk), .reset(buttons[1]), .request_valid(memory_request_valid),
    .write(memory_write), .address(memory_address), .write_data(memory_write_data),
    .response_ready(memory_response_ready), .controller_read_data(sdram_read_data),
    .controller_read_valid(sdram_read_valid), .controller_init_done(sdram_init_done),
    .controller_command_ack(sdram_command_ack), .request_ready(memory_request_ready),
    .response_valid(memory_response_valid), .read_data(memory_read_data),
    .error(memory_error), .controller_command_valid(sdram_command_valid),
    .controller_command(sdram_command), .controller_precharge(sdram_precharge),
    .controller_address(sdram_address), .controller_write_mask(sdram_write_mask),
    .controller_write_data(sdram_write_data),
    .controller_burst_length(sdram_burst_length)
);

function [15:0] expected_magic_word;
    input [1:0] index;
    begin
        case (index)
            0: expected_magic_word = 16'h5043;
            1: expected_magic_word = 16'h3355;
            2: expected_magic_word = 16'h4f42;
            default: expected_magic_word = 16'h544f;
        endcase
    end
endfunction

always @(posedge clk) begin
    dma_start <= 0;
    if (buttons[1]) begin
        state <= ST_WAIT_INIT;
        write_prefix_mismatch <= 0;
        issued_write_words <= 0;
        read_word <= 0;
        diagnostic_status <= 8'hff;
    end else begin
        if (sdram_command_valid && sdram_command == 3'b100) begin
            issued_write_words <= issued_write_words + 1'b1;
            case (issued_write_words)
                0: if (sdram_address != 21'h000020 || sdram_write_mask != 4'b1100 ||
                       sdram_write_data[15:0] != 16'h5043)
                    write_prefix_mismatch <= 1;
                1: if (sdram_address != 21'h000020 || sdram_write_mask != 4'b0011 ||
                       sdram_write_data[31:16] != 16'h3355)
                    write_prefix_mismatch <= 1;
                2: if (sdram_address != 21'h000021 || sdram_write_mask != 4'b1100 ||
                       sdram_write_data[15:0] != 16'h4f42)
                    write_prefix_mismatch <= 1;
                3: if (sdram_address != 21'h000021 || sdram_write_mask != 4'b0011 ||
                       sdram_write_data[31:16] != 16'h544f)
                    write_prefix_mismatch <= 1;
                default: begin end
            endcase
        end

        case (state)
            ST_WAIT_INIT: if (sdram_init_done) begin
                dma_start <= 1;
                state <= ST_WAIT_DMA;
            end
            ST_WAIT_DMA: begin
                if (dma_error) begin
                    diagnostic_status <= 8'h10 | dma_error_code;
                    state <= ST_ERROR;
                end else if (dma_done) begin
                    if (write_prefix_mismatch) begin
                        diagnostic_status <= 8'h02;
                        state <= ST_ERROR;
                    end else if (dma_completed_words != 32) begin
                        diagnostic_status <= 8'h03;
                        state <= ST_ERROR;
                    end else if (issued_write_words != 32) begin
                        diagnostic_status <= 8'h04;
                        state <= ST_ERROR;
                    end else begin
                        read_word <= 0;
                        state <= ST_READ_REQUEST;
                    end
                end
            end
            ST_READ_REQUEST: if (memory_request_ready)
                state <= ST_READ_RESPONSE;
            ST_READ_RESPONSE: if (memory_response_valid) begin
                if (memory_error) begin
                    diagnostic_status <= 8'h30;
                    state <= ST_ERROR;
                end else if (read_word < 4 && memory_read_data != expected_magic_word(read_word[1:0])) begin
                    if (read_word == 1 && memory_read_data == 16'h5043)
                        diagnostic_status <= 8'h41; // preceding word repeated
                    else if (read_word == 1 && memory_read_data == 16'h0000)
                        diagnostic_status <= 8'h42;
                    else if (read_word == 1 && memory_read_data == 16'hffff)
                        diagnostic_status <= 8'h43;
                    else if (read_word == 1 && memory_read_data == 16'h5533)
                        diagnostic_status <= 8'h44; // expected bytes swapped
                    else
                        diagnostic_status <= 8'h20 | read_word;
                    state <= ST_ERROR;
                end else if (read_word == 31) begin
                    diagnostic_status <= 0;
                    state <= ST_DONE;
                end else begin
                    read_word <= read_word + 1'b1;
                    state <= ST_READ_REQUEST;
                end
            end
            default: begin end
        endcase
    end
end

wire finished = state == ST_DONE || state == ST_ERROR;
assign leds = state == ST_DONE ? 6'b111111 :
              state == ST_ERROR ? diagnostic_status[5:0] :
              dma_busy ? 6'b000010 : 6'b000001;

wire uart_busy;
wire frame_toggle;
__DIAGNOSTIC_REPORTER__ u_reporter(
    .clk(clk), .report_enable(finished), .status(diagnostic_status),
    .uart_tx(uart_tx), .uart_busy(uart_busy), .frame_toggle(frame_toggle)
);

endmodule
