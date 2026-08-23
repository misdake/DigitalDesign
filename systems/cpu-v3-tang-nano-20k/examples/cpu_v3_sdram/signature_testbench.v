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

reg [15:0] memory [0:65535];
integer read_delay = 0;
reg [20:0] pending_read_address = 0;
integer cycle;

G16SdramBoardTest dut (.*);
always #5 clk = ~clk;

always @(posedge clk) begin
    sdram_command_ack <= 0;
    sdram_read_valid <= 0;

    if (sdram_command_valid && sdram_command == 3'b011)
        sdram_command_ack <= 1;

    if (sdram_command_valid && sdram_command == 3'b100) begin
        if (!sdram_write_mask[0]) memory[{sdram_address, 1'b0}][7:0] <= sdram_write_data[7:0];
        if (!sdram_write_mask[1]) memory[{sdram_address, 1'b0}][15:8] <= sdram_write_data[15:8];
        if (!sdram_write_mask[2]) memory[{sdram_address, 1'b1}][7:0] <= sdram_write_data[23:16];
        if (!sdram_write_mask[3]) memory[{sdram_address, 1'b1}][15:8] <= sdram_write_data[31:24];
        sdram_command_ack <= 1;
    end

    if (sdram_command_valid && sdram_command == 3'b101) begin
        pending_read_address <= sdram_address;
        read_delay <= 2;
    end else if (read_delay != 0) begin
        read_delay <= read_delay - 1;
        if (read_delay == 1) begin
            sdram_read_data <= {
                memory[{pending_read_address, 1'b1}],
                memory[{pending_read_address, 1'b0}]
            };
            sdram_read_valid <= 1;
            sdram_command_ack <= 1;
        end
    end
end

initial begin
    for (cycle = 0; cycle < 65536; cycle = cycle + 1)
        memory[cycle] = 0;
    repeat (10) @(posedge clk);
    sdram_init_done = 1;
    for (cycle = 0; cycle < 5000 && leds != 6'b000001; cycle = cycle + 1)
        @(posedge clk);
    #1;
    if (leds !== 6'b000001) begin
        $display(
            "pc=0x%04x cseg=0x%04x dseg=0x%04x fault=%0d code=%0d fault_pc=0x%04x signal=0x%04x retired=%0d",
            dut.pc, dut.code_segment, dut.data_segment, dut.faulted,
            dut.fault_code, dut.fault_pc, dut.halt_signal, dut.retired_words
        );
        $fatal(1, "G16 did not execute through the reusable SDRAM hierarchy");
    end
    if (memory[16'h4000] !== 16'h1234)
        $fatal(1, "write-through data did not reach SDRAM");
    if (sdram_burst_length !== 0)
        $fatal(1, "first reusable cache revision must use word transactions");
    $display("DIGITAL_DESIGN_PASS");
    $finish;
end
endmodule
