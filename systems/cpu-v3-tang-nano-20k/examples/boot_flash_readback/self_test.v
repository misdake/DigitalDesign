// Read-only Flash readback probe. Each byte is sent as an eight-byte UART
// record: "FBR1", offset_lo, offset_hi, data, xor_checksum. The complete
// package is repeated so a host can require multiple matching observations.
module FlashReadbackProbe #(
    parameter integer READ_LENGTH = __FLASH_PACKAGE_SIZE__
) (
    input wire clk,
    input wire [1:0] buttons,
    input wire flash_miso,
    output wire [5:0] leds,
    output wire uart_tx,
    output wire flash_clk,
    output wire flash_cs_n,
    output wire flash_mosi
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

reg start = 0;
wire flash_ready;
wire data_valid;
wire [7:0] data;
wire done;
wire error;
reg uart_busy = 0;

__FLASH_READER__ u_flash (
    .clk(clk),
    .start(start),
    .address(24'h100000),
    .length(READ_LENGTH[23:0]),
    .data_ready(!uart_busy),
    .flash_miso(flash_miso),
    .ready(flash_ready),
    .data_valid(data_valid),
    .data(data),
    .done(done),
    .error(error),
    .flash_clk(flash_clk),
    .flash_cs_n(flash_cs_n),
    .flash_mosi(flash_mosi)
);

function [7:0] record_byte;
    input [2:0] index;
    input [15:0] record_offset;
    input [7:0] record_data;
    begin
        case (index)
            0: record_byte = 8'h46; // F
            1: record_byte = 8'h42; // B
            2: record_byte = 8'h52; // R
            3: record_byte = 8'h31; // 1
            4: record_byte = record_offset[7:0];
            5: record_byte = record_offset[15:8];
            6: record_byte = record_data;
            default: record_byte = 8'h67 ^ record_offset[7:0]
                ^ record_offset[15:8] ^ record_data;
        endcase
    end
endfunction

reg started = 0;
reg [15:0] stream_offset = 0;
reg [19:0] repeat_delay = 0;
reg completion_toggle = 0;
reg seen_error = 0;

reg [15:0] record_offset = 0;
reg [7:0] record_data = 0;
reg [2:0] uart_byte = 0;
reg [9:0] uart_frame = 10'h3ff;
reg [3:0] uart_bit = 0;
reg [7:0] uart_divider = 0;

always @(posedge clk) begin
    if (reset) begin
        start <= 0;
        started <= 0;
        stream_offset <= 0;
        repeat_delay <= 0;
        completion_toggle <= 0;
        seen_error <= 0;
        record_offset <= 0;
        record_data <= 0;
        uart_byte <= 0;
        uart_frame <= 10'h3ff;
        uart_bit <= 0;
        uart_divider <= 0;
        uart_busy <= 0;
    end else begin
        start <= 0;
        if (!started && flash_ready && repeat_delay == 20'd999999) begin
            start <= 1;
            started <= 1;
            stream_offset <= 0;
            repeat_delay <= 0;
        end else if (!started) begin
            repeat_delay <= repeat_delay + 1'b1;
        end

        if (data_valid && !uart_busy) begin
            record_offset <= stream_offset;
            record_data <= data;
            stream_offset <= stream_offset + 1'b1;
            uart_byte <= 0;
            uart_frame <= {1'b1, record_byte(0, stream_offset, data), 1'b0};
            uart_bit <= 0;
            uart_divider <= 0;
            uart_busy <= 1;
        end else if (uart_busy && uart_divider == 8'd233) begin
            uart_divider <= 0;
            if (uart_bit == 9) begin
                if (uart_byte == 7) begin
                    uart_busy <= 0;
                end else begin
                    uart_byte <= uart_byte + 1'b1;
                    uart_frame <= {1'b1,
                        record_byte(uart_byte + 1'b1, record_offset, record_data), 1'b0};
                    uart_bit <= 0;
                end
            end else begin
                uart_bit <= uart_bit + 1'b1;
            end
        end else if (uart_busy) begin
            uart_divider <= uart_divider + 1'b1;
        end

        if (done) begin
            started <= 0;
            repeat_delay <= 0;
            completion_toggle <= !completion_toggle;
        end
        if (error)
            seen_error <= 1;
    end
end

assign leds = {seen_error, completion_toggle, uart_busy, started,
    stream_offset[9], stream_offset[8]};
assign uart_tx = uart_busy ? uart_frame[uart_bit] : 1'b1;

endmodule
