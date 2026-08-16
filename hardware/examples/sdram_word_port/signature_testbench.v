`timescale 1ns/1ps
module tb;
reg clk = 0;
reg [1:0] buttons = 0;
reg [31:0] sdram_read_data = 0;
reg sdram_read_valid = 0;
reg sdram_init_done = 0;
reg sdram_command_ack = 0;
wire [5:0] leds;
wire uart_tx;
wire sdram_command_valid;
wire [2:0] sdram_command;
wire sdram_precharge;
wire [20:0] sdram_address;
wire [3:0] sdram_write_mask;
wire [31:0] sdram_write_data;
wire [7:0] sdram_burst_length;

SdramWordPortSelfTest dut (.*);
always #5 clk = ~clk;

initial begin
    repeat (3) @(posedge clk);
    #1;
    if (sdram_burst_length !== 8'd0 || uart_tx !== 1'b1 || leds !== 6'b0)
        $fatal(1, "SDRAM word-port harness defaults failed");
    $finish;
end
endmodule
