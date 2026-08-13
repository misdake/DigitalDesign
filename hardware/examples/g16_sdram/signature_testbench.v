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

reg [31:0] line [0:7];
reg writing = 0;
reg reading = 0;
integer write_beat = 0;
integer read_beat = 0;
integer read_delay = 0;
integer cycle;

G16SdramBoardTest dut (
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

always @(posedge clk) begin
    sdram_command_ack <= 0;
    sdram_read_valid <= 0;

    if (sdram_command_valid && sdram_command == 3'b011)
        sdram_command_ack <= 1;

    if (sdram_command_valid && sdram_command == 3'b100) begin
        line[0] <= sdram_write_data;
        write_beat <= 1;
        writing <= 1;
    end else if (writing) begin
        line[write_beat] <= sdram_write_data;
        if (write_beat == 7) begin
            writing <= 0;
            sdram_command_ack <= 1;
        end else
            write_beat <= write_beat + 1;
    end

    if (sdram_command_valid && sdram_command == 3'b101) begin
        read_delay <= 3;
        read_beat <= 0;
    end else if (read_delay != 0) begin
        if (read_delay == 1)
            reading <= 1;
        read_delay <= read_delay - 1;
    end else if (reading) begin
        sdram_read_valid <= 1;
        sdram_read_data <= line[read_beat];
        if (read_beat == 6)
            sdram_command_ack <= 1;
        if (read_beat == 7)
            reading <= 0;
        else
            read_beat <= read_beat + 1;
    end
end

initial begin
    repeat (4) @(posedge clk);
    sdram_init_done = 1;
    for (cycle = 0; cycle < 1000 && leds != 6'b000001; cycle = cycle + 1)
        @(posedge clk);
    #1;
    if (sdram_burst_length !== 8'd7)
        $fatal(1, "G16 SDRAM harness selected the wrong burst length");
    if (leds !== 6'b000001)
        $fatal(1, "G16 did not execute successfully from the refilled cache");
    $finish;
end
endmodule
