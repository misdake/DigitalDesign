module {{ module_name }} (
    input wire clk,
    input wire reset,
    input wire [2:0] device_index,
    input wire [3:0] device_channel,
    input wire device_read_enable,
    input wire device_write_enable,
    input wire [15:0] device_write_data,
    input wire dcache_maintenance_ready,
    input wire dcache_maintenance_done,
    input wire dcache_maintenance_error,
    input wire [15:0] watch_read_data,
    output reg [15:0] device_read_data,
    output reg icache_invalidate = 0,
    output wire dcache_invalidate,
    output wire dcache_clean,
    // This hold is deliberately CPU-local. Cache write-back traffic and all
    // non-CPU clients of the shared SDRAM continue to run during maintenance.
    output reg cpu_hold = 0,
    output wire [2:0] watch_device_index,
    output wire [3:0] watch_device_channel,
    output wire watch_read_enable,
    output reg [5:0] leds = 0,
    output wire uart_tx
);

// 8N1 transmitter, adapted from the proven self-test shift logic: one start
// bit, eight data bits (LSB first), one stop bit, CLOCKS_PER_BIT clocks per
// bit. A channel-3 write while busy is dropped; software polls the busy
// readback before enqueueing the next byte.
// The stop bit is the final nonzero bit: shifting it out ends the frame.
// Keep the public busy bit registered so status/watch queries do not reduce
// the entire frame on the CPU device-read timing path. No bit index is needed.
reg [9:0] uart_frame = 0;
reg uart_busy = 0;
reg [{{ uart_divider_msb }}:0] uart_divider = 0;
// Always main-clocked: never use CPU hold or a device enable as its CE.
reg [15:0] millisecond_divider = 0;
reg [15:0] milliseconds = 0;
// Bounded interval timer: one word, explicit clear, no multiword snapshot.
reg [{{ microsecond_divider_msb }}:0] microsecond_divider = 0;
reg [15:0] microseconds = 0;
reg [15:0] cache_maintenance_status = 0;
reg [1:0] pending_dcache_maintenance = 0;
// Pending owns the request until the cache's actual acceptance edge. After
// acceptance cpu_hold with pending==0 owns precisely the subsequent done/error.
assign dcache_invalidate = pending_dcache_maintenance == 1;
assign dcache_clean = pending_dcache_maintenance == 2;
reg watch_active = 0;
reg [2:0] watch_index = 0;
reg [3:0] watch_channel = 0;
reg [15:0] watch_expected = 0;

assign watch_device_index = watch_index;
assign watch_device_channel = watch_channel;
assign watch_read_enable = watch_active;

always @* begin
    device_read_data = 0;
    if (device_read_enable && device_index == 3'd0) begin
        case (device_channel)
            3: device_read_data = {15'b0, uart_busy};
            5: device_read_data = cache_maintenance_status;
            10: device_read_data = milliseconds;
            11: device_read_data = microseconds;
            default: device_read_data = 0;
        endcase
    end
end

always @(posedge clk) begin
    if (reset) begin
        icache_invalidate <= 0;
        cpu_hold <= 0;
        watch_active <= 0;
        watch_index <= 0;
        watch_channel <= 0;
        watch_expected <= 0;
        cache_maintenance_status <= 0;
        pending_dcache_maintenance <= 0;
        leds <= 0;
        uart_frame <= 0;
        uart_busy <= 0;
        uart_divider <= 0;
        millisecond_divider <= 0;
        milliseconds <= 0;
        microsecond_divider <= 0;
        microseconds <= 0;
    end else begin
        if (microsecond_divider == {{ clocks_per_us_minus_one }}) begin
            microsecond_divider <= 0;
            if (microseconds != 16'hffff) microseconds <= microseconds + 1'b1;
        end else begin
            microsecond_divider <= microsecond_divider + 1'b1;
        end
        if (millisecond_divider == 16'd{{ clocks_per_ms_minus_one }}) begin
            millisecond_divider <= 0;
            milliseconds <= milliseconds + 16'd1;
        end else begin
            millisecond_divider <= millisecond_divider + 1'b1;
        end
        icache_invalidate <= 0;
        if (pending_dcache_maintenance != 0 && dcache_maintenance_ready)
            pending_dcache_maintenance <= 0;
        // A completion on the accepting edge still belongs to the previous
        // cache owner. Fast clean can complete without ever asserting busy.
        if (cpu_hold && !watch_active && pending_dcache_maintenance == 0 &&
            dcache_maintenance_done) begin
            cpu_hold <= 0;
            cache_maintenance_status <= dcache_maintenance_error ? 16'h8000 : 16'h0000;
        end
        if (watch_active && watch_read_data != watch_expected) begin
            watch_active <= 0;
            cpu_hold <= 0;
        end
        if (uart_busy) begin
            if (uart_divider == 16'd{{ clocks_per_bit_minus_one }}) begin
                uart_divider <= 0;
                uart_frame <= {1'b0, uart_frame[9:1]};
                if (uart_frame == 10'd1) uart_busy <= 0;
            end else begin
                uart_divider <= uart_divider + 1'b1;
            end
        end
        if (device_write_enable && device_index == 3'd0) begin
            case (device_channel)
                0: icache_invalidate <= 1;
                1: if (!cpu_hold) begin
                    cpu_hold <= 1;
                    pending_dcache_maintenance <= 1;
                end
                2: leds <= device_write_data[5:0];
                3: if (!uart_busy) begin
                    uart_frame <= {1'b1, device_write_data[7:0], 1'b0};
                    uart_busy <= 1;
                    uart_divider <= 0;
                end
                4: if (!cpu_hold) begin
                    cpu_hold <= 1;
                    pending_dcache_maintenance <= 2;
                end
                6: begin
                    watch_index <= device_write_data[2:0];
                    watch_channel <= device_write_data[6:3];
                end
                11: if (device_write_data[0]) begin
                    microsecond_divider <= 0;
                    microseconds <= 0;
                end
                7: if (!cpu_hold) begin
                    watch_expected <= device_write_data;
                    watch_active <= 1;
                    cpu_hold <= 1;
                end
                default: begin end
            endcase
        end
    end
end

assign uart_tx = uart_busy ? uart_frame[0] : 1'b1;

endmodule
