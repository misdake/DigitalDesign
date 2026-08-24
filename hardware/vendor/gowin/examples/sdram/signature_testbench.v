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

SdramBoardSelfTest dut (
    .clk(clk), .buttons(buttons),
    .sdram_read_data(sdram_read_data),
    .sdram_read_valid(sdram_read_valid),
    .sdram_init_done(sdram_init_done),
    .sdram_command_ack(sdram_command_ack),
    .leds(leds), .uart_tx(uart_tx),
    .sdram_command_valid(sdram_command_valid),
    .sdram_command(sdram_command),
    .sdram_precharge(sdram_precharge),
    .sdram_address(sdram_address),
    .sdram_write_mask(sdram_write_mask),
    .sdram_write_data(sdram_write_data),
    .sdram_burst_length(sdram_burst_length)
);

always #5 clk = ~clk;

initial begin
    repeat (3) @(posedge clk);
    #1;
    if (sdram_burst_length !== 8'd7 || uart_tx !== 1'b1) begin
        $display("SDRAM harness signature/default check failed");
        $fatal(1);
    end
    $display("DIGITAL_DESIGN_PASS");
    $finish;
end
endmodule
