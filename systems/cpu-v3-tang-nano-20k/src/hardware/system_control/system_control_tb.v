module tb;
reg clk = 0;
reg reset = 0;
reg [2:0] device_index = 0;
reg [3:0] device_channel = 0;
reg device_read_enable = 0;
reg device_write_enable = 0;
reg [15:0] device_write_data = 0;
reg dcache_maintenance_busy = 0;
reg dcache_maintenance_done = 0;
reg dcache_maintenance_error = 0;
reg [15:0] watch_read_data = 0;
wire [15:0] device_read_data;
wire icache_invalidate;
wire dcache_invalidate;
wire dcache_clean;
wire cpu_hold;
wire [2:0] watch_device_index;
wire [3:0] watch_device_channel;
wire watch_read_enable;
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
    if (!cpu_hold) fail("invalidate must hold the CPU");
    @(posedge clk);
    #1;
    if (dcache_invalidate !== 1'b0) fail("dcache_invalidate must last one clock");
    dcache_maintenance_done = 1;
    @(posedge clk); #1; dcache_maintenance_done = 0;
    if (cpu_hold) fail("successful maintenance must release hold");

    // A maintenance request made while LCOPY owns the cache is deferred. The
    // LCOPY completion coincides with release of busy, but must not release
    // the CPU before the deferred invalidate itself completes.
    dcache_maintenance_busy = 1;
    write_channel(0, 1, 16'd0);
    if (dcache_invalidate || !cpu_hold)
        fail("busy dcache must defer invalidate and hold CPU");
    @(posedge clk); #1;
    if (dcache_invalidate) fail("invalidate must remain deferred while busy");
    dcache_maintenance_busy = 0;
    dcache_maintenance_done = 1;
    @(posedge clk); #1;
    dcache_maintenance_done = 0;
    if (!dcache_invalidate || !cpu_hold)
        fail("deferred invalidate must ignore prior operation completion");
    @(posedge clk); #1;
    if (dcache_invalidate || !cpu_hold)
        fail("deferred invalidate pulse width or hold is wrong");
    dcache_maintenance_done = 1;
    @(posedge clk); #1; dcache_maintenance_done = 0;
    if (cpu_hold) fail("deferred invalidate completion must release hold");

    write_channel(0, 4, 16'd0);
    if (!dcache_clean || !cpu_hold) fail("channel 4 must start clean and hold");
    // CPU-local hold must not freeze the system-control peripheral itself.
    write_channel(0, 2, 16'h0025);
    if (!cpu_hold || leds !== 6'h25)
        fail("maintenance CPU hold must not stall other devices");
    dcache_maintenance_done = 1; dcache_maintenance_error = 1;
    @(posedge clk); #1;
    dcache_maintenance_done = 0; dcache_maintenance_error = 0;
    if (cpu_hold) fail("failed maintenance must release hold");
    device_index=0; device_channel=5; device_read_enable=1; #1;
    if(device_read_data!==16'h8000) fail("maintenance error status missing");
    device_read_enable=0;

    // Writes to another device index are ignored.
    write_channel(2, 0, 16'd1);
    if (icache_invalidate !== 1'b0) fail("device index must filter invalidate writes");
    write_channel(2, 2, 16'h003f);
    if (leds !== 6'h25) fail("device index must filter led writes");

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

    // A staged generic watch holds only the CPU and continuously probes the
    // selected device channel until its value changes.
    write_channel(0, 6, {9'd0, 4'd1, 3'd4});
    if (cpu_hold || watch_read_enable) fail("staging watch target must not hold");
    watch_read_data = 16'd7;
    write_channel(0, 7, 16'd7);
    if (!cpu_hold || !watch_read_enable) fail("watch arm must hold CPU");
    if (watch_device_index != 3'd4 || watch_device_channel != 4'd1)
        fail("watch target output mismatch");
    @(posedge clk); #1;
    if (!cpu_hold) fail("equal watched value must keep CPU held");
    watch_read_data = 16'd8;
    @(posedge clk); #1;
    if (cpu_hold || watch_read_enable) fail("changed watched value must release CPU");
    // A change that happened before arming is caught by the first probe.
    watch_read_data = 16'd10;
    write_channel(0, 7, 16'd9);
    if (!cpu_hold) fail("watch must enter hold on the arm edge");
    @(posedge clk); #1;
    if (cpu_hold) fail("pre-arm target change must not be missed");

    $display("DIGITAL_DESIGN_PASS");
    $finish;
end

initial begin
    #({{ clocks_per_bit }} * 1000 + 100000);
    $display("FAIL: timeout");
    $finish(1);
end
endmodule
