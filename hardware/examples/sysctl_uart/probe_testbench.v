// Testbench for SysctlUartProbe: decodes the UART byte stream and expects the
// DDHT frame for test ID 0x08, then checks the LED pattern write.
module tb;
reg clk = 0;
wire [5:0] leds;
wire uart_tx;

SysctlUartProbe dut (
    .clk(clk),
    .buttons(2'b00),
    .leds(leds),
    .uart_tx(uart_tx)
);

always #5 clk = ~clk;

// 234 clocks per bit, 8N1, LSB first.
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
    // The first frame starts right after the startup delay; listen from t=0.
    for (i = 0; i < 8; i = i + 1)
        read_byte(received[i]);
    if (received[0] !== 8'h44 || received[1] !== 8'h44 ||
        received[2] !== 8'h48 || received[3] !== 8'h54 ||
        received[4] !== 8'h01 || received[5] !== 8'h08 ||
        received[6] !== 8'h00 || received[7] !== 8'h15) begin
        $display("FAIL: bad frame %02x %02x %02x %02x %02x %02x %02x %02x",
            received[0], received[1], received[2], received[3],
            received[4], received[5], received[6], received[7]);
        $finish(1);
    end
    // The frame just finished; the FSM is in ST_GAP. uart_busy may still be
    // high for the final stop bit, so only the marker and state are checked.
    if (leds[4:0] !== 5'b01100) begin
        $display("FAIL: leds %b (want marker 01, state GAP=100)", leds);
        $finish(1);
    end
    $display("DIGITAL_DESIGN_PASS");
    $finish;
end

initial begin
    repeat (3_000_000) @(posedge clk);
    $display("FAIL: timeout");
    $finish(1);
end
endmodule
