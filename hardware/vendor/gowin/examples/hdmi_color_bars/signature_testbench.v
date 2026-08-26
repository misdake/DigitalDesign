`timescale 1ns/1ps
module tb;
reg clk = 0;
reg [1:0] buttons = 0;
wire [5:0] leds;
wire tmds_clk_p;
wire tmds_clk_n;
wire [2:0] tmds_data_p;
wire [2:0] tmds_data_n;

HdmiColorBars dut (
    .clk(clk), .buttons(buttons), .leds(leds),
    .tmds_clk_p(tmds_clk_p), .tmds_clk_n(tmds_clk_n),
    .tmds_data_p(tmds_data_p), .tmds_data_n(tmds_data_n)
);
always #5 clk = ~clk;

integer cycles = 0;
integer active_samples = 0;
integer line_starts = 0;
reg frame_seen = 0;
always @(posedge clk) begin
    cycles <= cycles + 1;
    if (dut.active)
        active_samples <= active_samples + 1;
    if (dut.h_count == 0)
        line_starts <= line_starts + 1;
    if (dut.h_count == 1649 && dut.v_count == 749)
        frame_seen <= 1;
end

initial begin
    repeat (8) @(posedge clk);
    wait (!dut.video_reset);

    // Control period starts with HS and VS asserted, so blue channel carries 11.
    repeat (2) @(posedge clk);
    if (dut.blue_symbol !== 10'b1010101011)
        $fatal(1, "blue control symbol for VS/HS=11 is %b", dut.blue_symbol);
    if (dut.green_symbol !== 10'b1101010100 ||
        dut.red_symbol !== 10'b1101010100)
        $fatal(1, "green/red blanking symbols are invalid");

    wait (dut.active && dut.active_x == 0 && dut.active_y == 0);
    @(posedge clk);
    #1;
    if (dut.rgb !== 24'hffffff)
        $fatal(1, "first color bar must be white, got %h", dut.rgb);

    wait (dut.active && dut.active_x == 160 && dut.active_y == 0);
    @(posedge clk);
    #1;
    if (dut.rgb !== 24'hffff00)
        $fatal(1, "second color bar must be yellow, got %h", dut.rgb);

    buttons = 2'b01;
    repeat (4) @(posedge clk);
    wait (dut.active && dut.active_x[4:0] == 0);
    @(posedge clk);
    #1;
    if (dut.rgb !== 24'hffffff)
        $fatal(1, "Button1 must enable the white grid overlay");

    buttons = 0;
    wait (frame_seen);
    @(posedge clk);
    if (line_starts < 750 || active_samples < 1280 * 720)
        $fatal(1, "incomplete 720p frame: lines=%0d active=%0d",
               line_starts, active_samples);
    if (tmds_clk_n !== ~tmds_clk_p || tmds_data_n !== ~tmds_data_p)
        $fatal(1, "simulation differential outputs are not complementary");

    $display("validated 1280x720 timing, color bars, controls, and differential pairs");
    $display("DIGITAL_DESIGN_PASS");
    $finish;
end

initial begin
    repeat (1300000) @(posedge clk);
    $fatal(1, "timeout h=%0d v=%0d active=%0d", dut.h_count, dut.v_count,
           active_samples);
end
endmodule
