module testbench;
reg clk = 0;
reg [1:0] buttons = 0;
wire [5:0] leds;
wire uart_tx;

G16CpuBoardTest dut(
    .clk(clk),
    .buttons(buttons),
    .leds(leds),
    .uart_tx(uart_tx)
);

always #1 clk = ~clk;

initial begin
    repeat (300) @(posedge clk);
    if (leds !== 6'b000001) $fatal(1, "G16 program did not pass");
    $finish;
end
endmodule
