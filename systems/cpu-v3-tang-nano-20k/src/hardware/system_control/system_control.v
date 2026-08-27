module {{ module_name }} (
    input wire clk,
    input wire reset,
    input wire [2:0] device_index,
    input wire [3:0] device_channel,
    input wire device_read_enable,
    input wire device_write_enable,
    input wire [15:0] device_write_data,
    output reg [15:0] device_read_data,
    output reg icache_invalidate = 0,
    output reg dcache_invalidate = 0,
    output reg [5:0] leds = 0,
    output wire uart_tx
);

// 8N1 transmitter, adapted from the proven self-test shift logic: one start
// bit, eight data bits (LSB first), one stop bit, CLOCKS_PER_BIT clocks per
// bit. A channel-3 write while busy is dropped; software polls the busy
// readback before enqueueing the next byte.
reg uart_busy = 0;
reg [9:0] uart_frame = 10'h3ff;
reg [3:0] uart_bit = 0;
reg [15:0] uart_divider = 0;

always @* begin
    device_read_data = 0;
    if (device_read_enable && device_index == 3'd0) begin
        case (device_channel)
            3: device_read_data = {15'b0, uart_busy};
            default: device_read_data = 0;
        endcase
    end
end

always @(posedge clk) begin
    if (reset) begin
        icache_invalidate <= 0;
        dcache_invalidate <= 0;
        leds <= 0;
        uart_busy <= 0;
        uart_frame <= 10'h3ff;
        uart_bit <= 0;
        uart_divider <= 0;
    end else begin
        icache_invalidate <= 0;
        dcache_invalidate <= 0;
        if (uart_busy) begin
            if (uart_divider == 16'd{{ clocks_per_bit_minus_one }}) begin
                uart_divider <= 0;
                if (uart_bit == 4'd9) begin
                    uart_busy <= 0;
                end else begin
                    uart_bit <= uart_bit + 1'b1;
                end
            end else begin
                uart_divider <= uart_divider + 1'b1;
            end
        end
        if (device_write_enable && device_index == 3'd0) begin
            case (device_channel)
                0: icache_invalidate <= 1;
                1: dcache_invalidate <= 1;
                2: leds <= device_write_data[5:0];
                3: if (!uart_busy) begin
                    uart_frame <= {1'b1, device_write_data[7:0], 1'b0};
                    uart_bit <= 0;
                    uart_divider <= 0;
                    uart_busy <= 1;
                end
                default: begin end
            endcase
        end
    end
end

assign uart_tx = uart_busy ? uart_frame[uart_bit] : 1'b1;

endmodule
