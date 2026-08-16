module SdramWordPortSelfTest (
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
localparam [3:0] ST_WRITE_0 = 0;
localparam [3:0] ST_WRITE_0_WAIT = 1;
localparam [3:0] ST_WRITE_1 = 2;
localparam [3:0] ST_WRITE_1_WAIT = 3;
localparam [3:0] ST_READ_0 = 4;
localparam [3:0] ST_READ_0_WAIT = 5;
localparam [3:0] ST_READ_1 = 6;
localparam [3:0] ST_READ_1_WAIT = 7;
localparam [3:0] ST_DONE = 8;
localparam [3:0] ST_ERROR = 9;

reg [3:0] state = ST_WRITE_0;
reg request_valid = 0;
reg request_write = 0;
reg [21:0] request_address = 0;
reg [15:0] request_write_data = 0;
reg response_ready = 0;
wire request_ready;
wire response_valid;
wire [15:0] response_read_data;
wire memory_error;

__SDRAM_WORD_PORT__ u_memory (
    .clk(clk),
    .reset(buttons[1]),
    .request_valid(request_valid),
    .write(request_write),
    .address(request_address),
    .write_data(request_write_data),
    .response_ready(response_ready),
    .controller_read_data(sdram_read_data),
    .controller_read_valid(sdram_read_valid),
    .controller_init_done(sdram_init_done),
    .controller_command_ack(sdram_command_ack),
    .request_ready(request_ready),
    .response_valid(response_valid),
    .read_data(response_read_data),
    .error(memory_error),
    .controller_command_valid(sdram_command_valid),
    .controller_command(sdram_command),
    .controller_precharge(sdram_precharge),
    .controller_address(sdram_address),
    .controller_write_mask(sdram_write_mask),
    .controller_write_data(sdram_write_data),
    .controller_burst_length(sdram_burst_length)
);

always @(posedge clk) begin
    request_valid <= 0;
    response_ready <= 0;
    if (buttons[1]) begin
        state <= ST_WRITE_0;
    end else if (memory_error) begin
        state <= ST_ERROR;
    end else begin
        case (state)
            ST_WRITE_0: if (request_ready) begin
                request_valid <= 1;
                request_write <= 1;
                request_address <= 22'h000007;
                request_write_data <= 16'h1234;
                state <= ST_WRITE_0_WAIT;
            end
            ST_WRITE_0_WAIT: if (response_valid) begin
                response_ready <= 1;
                state <= ST_WRITE_1;
            end
            ST_WRITE_1: if (request_ready) begin
                request_valid <= 1;
                request_write <= 1;
                request_address <= 22'h100007;
                request_write_data <= 16'habcd;
                state <= ST_WRITE_1_WAIT;
            end
            ST_WRITE_1_WAIT: if (response_valid) begin
                response_ready <= 1;
                state <= ST_READ_0;
            end
            ST_READ_0: if (request_ready) begin
                request_valid <= 1;
                request_write <= 0;
                request_address <= 22'h000007;
                state <= ST_READ_0_WAIT;
            end
            ST_READ_0_WAIT: if (response_valid) begin
                response_ready <= 1;
                state <= response_read_data == 16'h1234 ? ST_READ_1 : ST_ERROR;
            end
            ST_READ_1: if (request_ready) begin
                request_valid <= 1;
                request_write <= 0;
                request_address <= 22'h100007;
                state <= ST_READ_1_WAIT;
            end
            ST_READ_1_WAIT: if (response_valid) begin
                response_ready <= 1;
                state <= response_read_data == 16'habcd ? ST_DONE : ST_ERROR;
            end
            default: state <= state;
        endcase
    end
end

assign leds = state == ST_DONE ? 6'b111111 :
              state == ST_ERROR ? 6'b000001 : 6'b000000;

function [7:0] report_byte;
    input [2:0] index;
    begin
        case (index)
            0: report_byte = 8'h53;
            1: report_byte = 8'h44;
            2: report_byte = 8'h57;
            3: report_byte = 8'h50;
            4: report_byte = state == ST_DONE ? 8'h00 : 8'hff;
            5: report_byte = 8'h12;
            6: report_byte = 8'h34;
            default: report_byte = 8'hcd;
        endcase
    end
endfunction

reg [9:0] uart_frame = 10'h3ff;
reg [3:0] uart_bit = 0;
reg [8:0] uart_divider = 0;
reg [2:0] uart_byte_index = 0;
reg uart_busy = 0;

always @(posedge clk) begin
    if (!uart_busy) begin
        if (state == ST_DONE || state == ST_ERROR) begin
            uart_frame <= {1'b1, report_byte(0), 1'b0};
            uart_bit <= 0;
            uart_divider <= 0;
            uart_byte_index <= 0;
            uart_busy <= 1;
        end
    end else if (uart_divider == 9'd468) begin
        uart_divider <= 0;
        if (uart_bit == 9) begin
            if (uart_byte_index == 7) begin
                uart_busy <= 0;
            end else begin
                uart_byte_index <= uart_byte_index + 1'b1;
                uart_frame <= {1'b1, report_byte(uart_byte_index + 1'b1), 1'b0};
                uart_bit <= 0;
            end
        end else begin
            uart_bit <= uart_bit + 1'b1;
        end
    end else begin
        uart_divider <= uart_divider + 1'b1;
    end
end

assign uart_tx = uart_busy ? uart_frame[uart_bit] : 1'b1;
endmodule
