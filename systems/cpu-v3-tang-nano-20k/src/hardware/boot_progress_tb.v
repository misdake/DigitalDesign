module tb;
reg clk = 0;
reg reset = 1;
reg sdram_ready = 0;
reg dma_busy = 0;
reg dma_error = 0;
reg cpu_fault = 0;
reg [15:0] code_segment = 0;
reg software_led_write = 0;
wire diagnostic_active;
wire [5:0] diagnostic_leds;
wire [2:0] phase;
wire error_sticky;

BootProgressMonitor dut(.*);
always #1 clk = ~clk;

task expect_phase;
    input [2:0] expected_phase;
    input [5:0] expected_leds;
    begin
        #1;
        if (phase !== expected_phase || diagnostic_leds !== expected_leds)
            $fatal(1, "phase=%0d leds=%b expected phase=%0d leds=%b",
                phase, diagnostic_leds, expected_phase, expected_leds);
    end
endtask

initial begin
    expect_phase(0, 6'b000001);
    @(negedge clk); reset = 0;
    expect_phase(1, 6'b000010);
    sdram_ready = 1;
    expect_phase(2, 6'b000100);
    dma_busy = 1;
    expect_phase(3, 6'b001000);
    code_segment = 1;
    expect_phase(4, 6'b010000);
    dma_busy = 0;
    code_segment = 3;
    expect_phase(5, 6'b100000);

    software_led_write = 1;
    @(posedge clk); #1;
    if (diagnostic_active !== 0)
        $fatal(1, "software LED handoff was not immediate");
    software_led_write = 0;
    dma_error = 1;
    @(posedge clk); #1;
    dma_error = 0;
    if (phase !== 7 || error_sticky !== 1)
        $fatal(1, "DMA error was not retained");

    reset = 1;
    @(posedge clk); #1;
    if (diagnostic_active !== 1 || error_sticky !== 0)
        $fatal(1, "reset did not restore monitor ownership");
    $display("DIGITAL_DESIGN_PASS");
    $finish;
end

initial begin
    repeat (64) @(posedge clk);
    $fatal(1, "timeout");
end
endmodule
