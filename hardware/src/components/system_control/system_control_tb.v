module tb;
reg clk = 0;
reg reset = 0;
reg [3:0] device_index = 0;
reg [3:0] device_channel = 0;
reg device_read_enable = 0;
reg device_write_enable = 0;
reg [15:0] device_write_data = 0;
wire [15:0] device_read_data;
wire icache_invalidate;
wire dcache_invalidate;
wire [5:0] leds;
wire uart_tx;

{{ module_name }} dut(.*);
always #5 clk = ~clk;

task fail;
    input [8*64:1] message;
    begin
        $display("FAIL: %0s", message);
        $finish(1);
    end
endtask

task write_channel;
    input [3:0] index;
    input [3:0] channel;
    input [15:0] value;
    begin
        device_index = index;
        device_channel = channel;
        device_write_data = value;
        device_write_enable = 1;
        @(posedge clk);
        #1;
        device_write_enable = 0;
    end
endtask

task check_busy;
    input expected;
    begin
        device_index = 0;
        device_channel = 3;
        device_read_enable = 1;
        #1;
        if (device_read_data !== {15'b0, expected}) fail("uart busy readback");
        device_read_enable = 0;
    end
endtask

task advance_bit;
    begin
        repeat ({{ clocks_per_bit }}) begin
            @(posedge clk);
            #1;
        end
    end
endtask

initial begin
    // Reset clears the LEDs and leaves the UART idle-high.
    reset = 1;
    @(posedge clk);
    #1;
    reset = 0;
    if (uart_tx !== 1'b1) fail("uart must idle high");
    if (leds !== 6'd0) fail("reset must clear leds");

    // Channel 0/1 pulse the cache invalidate outputs for one clock.
    write_channel(0, 0, 16'hffff);
    if (icache_invalidate !== 1'b1 || dcache_invalidate !== 1'b0)
        fail("channel 0 must pulse icache_invalidate");
    @(posedge clk);
    #1;
    if (icache_invalidate !== 1'b0) fail("icache_invalidate must last one clock");

    write_channel(0, 1, 16'd0);
    if (dcache_invalidate !== 1'b1 || icache_invalidate !== 1'b0)
        fail("channel 1 must pulse dcache_invalidate");
    @(posedge clk);
    #1;
    if (dcache_invalidate !== 1'b0) fail("dcache_invalidate must last one clock");

    // Writes to another device index are ignored.
    write_channel(2, 0, 16'd1);
    if (icache_invalidate !== 1'b0) fail("device index must filter invalidate writes");
    write_channel(2, 2, 16'h003f);
    if (leds !== 6'd0) fail("device index must filter led writes");

    // Channel 2 drives the LEDs from the low six write-data bits.
    write_channel(0, 2, 16'hffea);
    if (leds !== 6'h2a) fail("channel 2 must drive leds[5:0]");

    // The UART reports not busy before the first byte.
    check_busy(0);

    // Enqueue 0xa5: the start bit and the busy flag appear together.
    write_channel(0, 3, 16'h00a5);
    if (uart_tx !== 1'b0) fail("write must start the frame with a low start bit");
    check_busy(1);

    // A second write while busy is dropped.
    write_channel(0, 3, 16'h00ff);

    // The start bit completes, then 0xa5 shifts out LSB first.
    repeat ({{ clocks_per_bit_minus_one }}) begin
        @(posedge clk);
        #1;
    end
    if (uart_tx !== 1'b1) fail("data bit 0 of 0xa5");
    advance_bit;
    if (uart_tx !== 1'b0) fail("data bit 1 of 0xa5; the busy write must be dropped");
    advance_bit;
    if (uart_tx !== 1'b1) fail("data bit 2 of 0xa5");
    advance_bit;
    if (uart_tx !== 1'b0) fail("data bit 3 of 0xa5");
    advance_bit;
    if (uart_tx !== 1'b0) fail("data bit 4 of 0xa5");
    advance_bit;
    if (uart_tx !== 1'b1) fail("data bit 5 of 0xa5");
    advance_bit;
    if (uart_tx !== 1'b0) fail("data bit 6 of 0xa5");
    advance_bit;
    if (uart_tx !== 1'b1) fail("data bit 7 of 0xa5");
    advance_bit;
    if (uart_tx !== 1'b1) fail("stop bit must be high");
    check_busy(1);
    advance_bit;
    if (uart_tx !== 1'b1) fail("uart must return to idle high");
    check_busy(0);

    // Reset aborts a frame in flight and clears the LEDs.
    write_channel(0, 3, 16'h0055);
    if (uart_tx !== 1'b0) fail("second frame start bit");
    reset = 1;
    @(posedge clk);
    #1;
    reset = 0;
    if (uart_tx !== 1'b1) fail("reset must abort the frame");
    if (leds !== 6'd0) fail("reset must clear leds again");
    check_busy(0);

    $display("DIGITAL_DESIGN_PASS");
    $finish;
end

initial begin
    #({{ clocks_per_bit }} * 1000 + 100000);
    $display("FAIL: timeout");
    $finish(1);
end
endmodule
