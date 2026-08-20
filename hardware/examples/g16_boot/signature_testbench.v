`timescale 1ns/1ps
// Signature testbench for the two-stage flash boot. Phase 1 preloads the
// Flash model with the packed boot package and expects the demo application
// to emit the DDHT 0x07 success frame through the system control UART.
// Phase 2 corrupts the descriptor magic, resets the board, and expects the
// Stage0 boot error report: LED {stage 1, category 1} plus repeating G16B
// UART frames.
module tb;
reg clk = 0;
reg [1:0] buttons = 0;
reg flash_miso = 1;
reg [31:0] sdram_read_data = 0;
reg sdram_read_valid = 0;
reg sdram_init_done = 0;
reg sdram_command_ack = 0;
wire [5:0] leds;
wire uart_tx;
wire flash_clk;
wire flash_cs_n;
wire flash_mosi;
wire sdram_command_valid;
wire [2:0] sdram_command;
wire sdram_precharge;
wire [20:0] sdram_address;
wire [3:0] sdram_write_mask;
wire [31:0] sdram_write_data;
wire [7:0] sdram_burst_length;

G16BootSelfTest dut(.*);
always #5 clk = ~clk;

// SDRAM model: 16-bit words, two words per 32-bit controller word. The boot
// sections stay below physical word 0x50000, so 19 index bits suffice.
reg [15:0] memory [0:524287];
integer read_delay = 0;
reg [20:0] pending_read_address = 0;
integer cycle;

always @(posedge clk) begin
    sdram_command_ack <= 0;
    sdram_read_valid <= 0;

    // The word port interleaves a refresh command every 600 clocks.
    if (sdram_command_valid && (sdram_command == 3'b001 || sdram_command == 3'b011))
        sdram_command_ack <= 1;

    if (sdram_command_valid && sdram_command == 3'b100) begin
        if (!sdram_write_mask[0]) memory[{sdram_address[17:0], 1'b0}][7:0] <= sdram_write_data[7:0];
        if (!sdram_write_mask[1]) memory[{sdram_address[17:0], 1'b0}][15:8] <= sdram_write_data[15:8];
        if (!sdram_write_mask[2]) memory[{sdram_address[17:0], 1'b1}][7:0] <= sdram_write_data[23:16];
        if (!sdram_write_mask[3]) memory[{sdram_address[17:0], 1'b1}][15:8] <= sdram_write_data[31:24];
        sdram_command_ack <= 1;
    end

    if (sdram_command_valid && sdram_command == 3'b101) begin
        pending_read_address <= sdram_address;
        read_delay <= 2;
    end else if (read_delay != 0) begin
        read_delay <= read_delay - 1;
        if (read_delay == 1) begin
            sdram_read_data <= {
                memory[{pending_read_address[17:0], 1'b1}],
                memory[{pending_read_address[17:0], 1'b0}]
            };
            sdram_read_valid <= 1;
            sdram_command_ack <= 1;
        end
    end
end

// SPI Flash model: standard read command 03h plus a 24-bit byte address,
// then package bytes MSB-first. Addresses outside the packed boot package
// (placed at Flash byte 0x100000) read as erased Flash.
localparam integer FLASH_BASE = 32'h00100000;
localparam integer FLASH_PACKAGE_SIZE = __FLASH_PACKAGE_SIZE__;

reg [7:0] flash_image [0:FLASH_PACKAGE_SIZE-1];
reg [31:0] flash_command = 0;
integer flash_command_bits = 0;
reg [23:0] flash_byte_address = 0;
integer flash_data_bit = 0;
reg [7:0] flash_current_byte = 0;
reg corrupt_magic = 0;
integer flash_init_index;

initial begin
    for (flash_init_index = 0; flash_init_index < FLASH_PACKAGE_SIZE; flash_init_index = flash_init_index + 1)
        flash_image[flash_init_index] = 8'hff;
__FLASH_PACKAGE_INIT__
end

always @(posedge flash_cs_n) begin
    flash_command_bits = 0;
    flash_data_bit = 0;
end

always @(posedge flash_clk) begin
    if (!flash_cs_n && flash_command_bits < 32) begin
        flash_command = {flash_command[30:0], flash_mosi};
        flash_command_bits = flash_command_bits + 1;
        // The 24-bit address is complete after the 32nd command bit.
        if (flash_command_bits == 32)
            flash_byte_address = flash_command[23:0];
    end
end

always @(negedge flash_clk) begin
    if (!flash_cs_n && flash_command_bits >= 32) begin
        if (flash_data_bit == 0) begin
            if (flash_byte_address >= FLASH_BASE && flash_byte_address < FLASH_BASE + FLASH_PACKAGE_SIZE)
                flash_current_byte = flash_image[flash_byte_address - FLASH_BASE];
            else
                flash_current_byte = 8'hff;
            if (corrupt_magic && flash_byte_address == FLASH_BASE)
                flash_current_byte = flash_current_byte ^ 8'h01;
            flash_byte_address = flash_byte_address + 1;
        end
        flash_miso <= flash_current_byte[7 - flash_data_bit];
        flash_data_bit = (flash_data_bit + 1) % 8;
    end
end

// UART monitor: 8N1 at the system control device's 469 clocks per bit,
// keeping the last ten received bytes in a window for frame matching.
localparam integer CLOCKS_PER_BIT = 469;
integer uart_count = 0;
integer uart_bit = 0;
reg [7:0] uart_shift = 0;
reg uart_receiving = 0;
reg [7:0] uart_history [0:9];
reg ddht_frame_seen = 0;
reg g16b_frame_seen = 0;

always @(posedge clk) begin
    if (!uart_receiving) begin
        if (!uart_tx) begin
            uart_receiving <= 1;
            uart_count <= CLOCKS_PER_BIT + CLOCKS_PER_BIT / 2;
            uart_bit <= 0;
        end
    end else if (uart_count == 0) begin
        if (uart_bit == 8) begin
            uart_receiving <= 0;
            if (uart_tx) begin
                uart_history[0] = uart_history[1];
                uart_history[1] = uart_history[2];
                uart_history[2] = uart_history[3];
                uart_history[3] = uart_history[4];
                uart_history[4] = uart_history[5];
                uart_history[5] = uart_history[6];
                uart_history[6] = uart_history[7];
                uart_history[7] = uart_history[8];
                uart_history[8] = uart_history[9];
                uart_history[9] = uart_shift;
                // DDHT success frame: magic, version 1, test ID 0x07,
                // status 0, XOR checksum of bytes 0..6.
                if (uart_history[2] == 8'h44 && uart_history[3] == 8'h44 &&
                    uart_history[4] == 8'h48 && uart_history[5] == 8'h54 &&
                    uart_history[6] == 8'h01 && uart_history[7] == 8'h07 &&
                    uart_history[8] == 8'h00 &&
                    (uart_history[2] ^ uart_history[3] ^ uart_history[4] ^
                     uart_history[5] ^ uart_history[6] ^ uart_history[7] ^
                     uart_history[8] ^ uart_history[9]) == 0)
                    ddht_frame_seen = 1;
                // Boot error frame: magic G16B, stage 1, category 1, code 1,
                // detail 0, XOR checksum of bytes 0..8.
                if (uart_history[0] == 8'h47 && uart_history[1] == 8'h31 &&
                    uart_history[2] == 8'h36 && uart_history[3] == 8'h42 &&
                    uart_history[4] == 8'h01 && uart_history[5] == 8'h01 &&
                    uart_history[6] == 8'h01 && uart_history[7] == 8'h00 &&
                    uart_history[8] == 8'h00 &&
                    (uart_history[0] ^ uart_history[1] ^ uart_history[2] ^
                     uart_history[3] ^ uart_history[4] ^ uart_history[5] ^
                     uart_history[6] ^ uart_history[7] ^ uart_history[8] ^
                     uart_history[9]) == 0)
                    g16b_frame_seen = 1;
            end
        end else begin
            uart_shift[uart_bit] <= uart_tx;
            uart_bit <= uart_bit + 1;
            uart_count <= CLOCKS_PER_BIT - 1;
        end
    end else
        uart_count <= uart_count - 1;
end

initial begin
    for (cycle = 0; cycle < 524288; cycle = cycle + 1)
        memory[cycle] = 0;
    // Dirty the BSS range so the zero-fill DMA is actually observable.
    for (cycle = 0; cycle < 32; cycle = cycle + 1)
        memory[20'h40100 + cycle] = 16'hffff;
    repeat (10) @(posedge clk);
    sdram_init_done = 1;

    // Phase 1: the intact package boots Stage0 -> Stage1 -> application.
    wait (ddht_frame_seen);
    @(posedge clk);
    if (leds !== 6'b000000)
        $fatal(1, "successful boot must leave the LEDs dark, got %b", leds);
    if (dut.code_segment !== 16'd3 || dut.data_segment !== 16'd4)
        $fatal(1, "application segments not reached: cseg=0x%04x dseg=0x%04x",
            dut.code_segment, dut.data_segment);
    if (memory[20'h40000] !== 16'hbeef || memory[20'h40001] !== 16'h0055)
        $fatal(1, "data section did not reach SDRAM: %04x %04x",
            memory[20'h40000], memory[20'h40001]);
    if (memory[20'h40100] !== 16'h0000 || memory[20'h4011f] !== 16'h0000)
        $fatal(1, "bss section was not zero-filled");
    if (sdram_burst_length !== 0)
        $fatal(1, "first reusable cache revision must use word transactions");

    // Phase 2: corrupt the descriptor magic, reset through the button input,
    // and expect the Stage0 boot error report.
    corrupt_magic = 1;
    buttons = 2'b01;
    repeat (8) @(posedge clk);
    buttons = 2'b00;
    wait (g16b_frame_seen);
    @(posedge clk);
    if (leds !== 6'b010001)
        $fatal(1, "stage0 descriptor failure must light LEDs 6'b010001, got %b", leds);
    if (dut.code_segment !== 16'd0)
        $fatal(1, "failed boot must stay in the boot segment, cseg=0x%04x", dut.code_segment);
    $display("DIGITAL_DESIGN_PASS");
    $finish;
end

initial begin
    repeat (4000000) @(posedge clk);
    $display("FAIL: timeout (cseg=0x%04x dseg=0x%04x pc=0x%04x leds=%b retired=%0d)",
        dut.code_segment, dut.data_segment, dut.pc, leds, dut.retired_words);
    $finish(1);
end
endmodule
