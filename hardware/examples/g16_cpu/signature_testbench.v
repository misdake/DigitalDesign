module tb;
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
    if (leds !== 6'b000001) begin
        $display(
            "pc=0x%04x instruction=0x%04x halted=%0d faulted=%0d result=0x%04x",
            dut.pc, dut.instruction, dut.halted, dut.faulted,
            dut.registers[0]
        );
        $fatal(1, "G16 program did not pass");
    end
    $display("DIGITAL_DESIGN_PASS");
    $finish;
end
endmodule
