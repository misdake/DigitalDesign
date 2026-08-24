module tb;
reg clk = 0;
reg [1:0] buttons = 0;
wire [5:0] leds;
wire uart_tx;

BoardHealthProbe dut (
    .clk(clk),
    .buttons(buttons),
    .leds(leds),
    .uart_tx(uart_tx)
);

always #5 clk = ~clk;

task read_byte;
    output [7:0] value;
    integer bit_index;
    begin
        @(negedge uart_tx);
        repeat (234 + 117) @(posedge clk);
        for (bit_index = 0; bit_index < 8; bit_index = bit_index + 1) begin
            value[bit_index] = uart_tx;
            repeat (234) @(posedge clk);
        end
    end
endtask

reg [7:0] received [0:7];
integer i;
initial begin
    for (i = 0; i < 8; i = i + 1)
        read_byte(received[i]);
    if (received[0] !== 8'h44 || received[1] !== 8'h44 ||
        received[2] !== 8'h48 || received[3] !== 8'h54 ||
        received[4] !== 8'h01 || received[5] !== 8'h0a ||
        received[6] !== 8'h00 || received[7] !== 8'h17) begin
        $display("FAIL: bad health frame");
        $finish(1);
    end

    // Accelerate the second report and verify that a high reset-button input
    // is observable as a deterministic status instead of silencing the probe.
    buttons = 2'b01;
    repeat (4) @(posedge clk);
    dut.u_reporter.delay_counter = 23'd4_999_999;
    for (i = 0; i < 8; i = i + 1)
        read_byte(received[i]);
    if (received[6] !== 8'h01 || received[7] !== 8'h16) begin
        $display("FAIL: button diagnostic status=%02x checksum=%02x",
            received[6], received[7]);
        $finish(1);
    end
    $display("DIGITAL_DESIGN_PASS");
    $finish;
end

initial begin
    repeat (3_500_000) @(posedge clk);
    $display("FAIL: timeout");
    $finish(1);
end
endmodule
