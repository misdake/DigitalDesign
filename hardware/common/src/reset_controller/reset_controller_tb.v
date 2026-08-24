module tb;
reg clk = 0;
reg external_reset = 0;
reg clock_ready = 0;
wire reset;
wire clock_ready_synchronized;
wire external_reset_seen;

{{ module_name }} dut(
    .clk(clk),
    .external_reset(external_reset),
    .clock_ready(clock_ready),
    .reset(reset),
    .clock_ready_synchronized(clock_ready_synchronized),
    .external_reset_seen(external_reset_seen)
);

always #1 clk = ~clk;

initial begin
    clock_ready = 1;
    repeat (2 + {{ hold_cycles }}) @(posedge clk);
    #1;
    if (reset !== 0 || clock_ready_synchronized !== 1)
        $fatal(1, "reset did not deassert after clock-ready hold");

    external_reset = 1;
    repeat (2) @(posedge clk);
    #1;
    if (reset !== 1 || external_reset_seen !== 1)
        $fatal(1, "synchronized external reset was not observed");

    external_reset = 0;
    repeat (2 + {{ hold_cycles }}) @(posedge clk);
    #1;
    if (reset !== 0 || external_reset_seen !== 1)
        $fatal(1, "reset did not recover or sticky diagnostic was lost");

    $display("DIGITAL_DESIGN_PASS");
    $finish;
end

initial begin
    repeat (64) @(posedge clk);
    $fatal(1, "timeout");
end
endmodule
