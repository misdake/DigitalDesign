`timescale 1ns/1ps
// Signature testbench for the single-stage flash boot. Phase 1 preloads the
// Flash model with the packed boot package and verifies the default/01 primary
// application followed by the button-10 alternate application. Later phases
// corrupt the descriptor and manifest metadata and check the boot stage's
// failure reports.
module tb;
reg clk = 0;
reg [1:0] buttons = 0;
reg flash_miso = 1;
reg [63:0] sdram_read_data = 0;
reg sdram_read_valid = 0;
reg sdram_init_done = 0;
reg sdram_command_ack = 0;
reg sdram_write_data_ready = 1;
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
wire [63:0] sdram_write_data;
wire sdram_write_data_valid;
wire [7:0] sdram_burst_length;
reg pixel_clock = 0;
reg serial_clock = 0;
reg video_locked = 1;
wire tmds_clk_p;
wire tmds_clk_n;
wire [2:0] tmds_data_p;
wire [2:0] tmds_data_n;

CpuV3System dut(.*);
always #5 clk = ~clk;
always #2 pixel_clock = ~pixel_clock;
always #1 serial_clock = ~serial_clock;

// SDRAM model: 16-bit words, two words per 32-bit controller word. The boot
// sections stay below physical word 0x80000, so 19 index bits suffice.
//
// The SharedSdramPort is a 64-bit gearbox: a cache line is eight 32-bit words
// (four 64-bit beats), and it streams line-write beats through
// sdram_write_data_valid BEFORE the ACTIVE/WRITE command pair. The model
// therefore buffers the four 64-bit beats and commits them to memory when the
// WRITE command acknowledges a burst of seven, and it returns line reads as
// four ordered 64-bit beats. A 32-bit word W occupies memory[2*W] (low half)
// and memory[2*W+1] (high half), matching the port's packing.
reg [15:0] memory [0:524287];
integer read_delay = 0;
integer read_beats = 0;
reg [20:0] pending_read_address = 0;
reg read_is_line = 0;
reg [2:0] read_beat = 0;
reg word_read_seen = 0;
reg line_burst_seen = 0;
integer write_beats = 0;
reg [20:0] pending_write_address = 0;
reg [7:0] pending_write_length = 0;
reg [63:0] write_capture [0:3];
reg [2:0] write_capture_beat = 0;
reg [17:0] idx;
reg [17:0] idx2;
integer cycle;

always @(posedge clk) begin
    sdram_command_ack <= 0;
    sdram_read_valid <= 0;

    // The word port streams the four line-write beats while it is in its
    // ST_WRITE_STAGE, which runs before ACTIVE/WRITE. Capture each beat here.
    if (sdram_write_data_valid) begin
        write_capture[write_capture_beat] <= sdram_write_data;
        write_capture_beat <= write_capture_beat + 1'b1;
    end

    // The word port interleaves a refresh command every 600 clocks.
    if (sdram_command_valid && (sdram_command == 3'b001 || sdram_command == 3'b011))
        sdram_command_ack <= 1;

    if (sdram_command_valid && sdram_command == 3'b100) begin
        sdram_command_ack <= 1;
        write_capture_beat <= 0;
        pending_write_address <= sdram_address;
        pending_write_length <= sdram_burst_length;
        if (sdram_burst_length != 0 && sdram_burst_length != 7)
            $fatal(1, "unexpected write burst length %0d", sdram_burst_length);
        if (sdram_burst_length == 7) begin
            // Commit a cache line: beat j carries 32-bit words (base+2*j) and
            // (base+2*j+1) in sdram_write_data[31:0] and [63:32].
            begin : line_write_commit
                integer j;
                for (j = 0; j < 4; j = j + 1) begin
                    idx = sdram_address[17:0] + 2*j;
                    memory[{idx, 1'b0}] <= write_capture[j][15:0];
                    memory[{idx, 1'b1}] <= write_capture[j][31:16];
                    idx = idx + 1;
                    memory[{idx, 1'b0}] <= write_capture[j][47:32];
                    memory[{idx, 1'b1}] <= write_capture[j][63:48];
                end
            end
        end else begin
            if (!sdram_write_mask[0]) memory[{sdram_address[17:0], 1'b0}][7:0] <= sdram_write_data[7:0];
            if (!sdram_write_mask[1]) memory[{sdram_address[17:0], 1'b0}][15:8] <= sdram_write_data[15:8];
            if (!sdram_write_mask[2]) memory[{sdram_address[17:0], 1'b1}][7:0] <= sdram_write_data[23:16];
            if (!sdram_write_mask[3]) memory[{sdram_address[17:0], 1'b1}][15:8] <= sdram_write_data[31:24];
        end
    end

    // One READ command returns burst_length+1 ordered 64-bit beats for a line
    // (four beats) or one 32-bit word for a burst of zero.
    if (sdram_command_valid && sdram_command == 3'b101) begin
        if (sdram_burst_length == 0) word_read_seen <= 1;
        else if (sdram_burst_length == 7) line_burst_seen <= 1;
        else $fatal(1, "unexpected burst length %0d", sdram_burst_length);
        pending_read_address <= sdram_address;
        read_is_line <= sdram_burst_length == 7;
        read_delay <= 2;
        read_beat <= 0;
        read_beats <= sdram_burst_length == 7 ? 4 : 1;
        sdram_command_ack <= 1;
    end else if (read_delay != 0) begin
        read_delay <= read_delay - 1;
    end else if (read_beats != 0) begin
        sdram_read_valid <= 1;
        if (read_is_line) begin
            idx = pending_read_address[17:0] + 2*read_beat;
            idx2 = idx + 1;
            sdram_read_data <= {
                memory[{idx2, 1'b1}],
                memory[{idx2, 1'b0}],
                memory[{idx, 1'b1}],
                memory[{idx, 1'b0}]
            };
        end else begin
            idx = pending_read_address[17:0];
            sdram_read_data <= {
                32'b0,
                memory[{idx, 1'b1}],
                memory[{idx, 1'b0}]
            };
        end
        read_beat <= read_beat + 1'b1;
        read_beats <= read_beats - 1;
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
reg [1:0] corrupt_metadata = 0;
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
            if (corrupt_metadata == 1 && flash_byte_address == FLASH_BASE)
                flash_current_byte = flash_current_byte ^ 8'h01;
            if (corrupt_metadata == 2 && flash_byte_address == FLASH_BASE + 64)
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
reg descriptor_error_frame_seen = 0;
reg manifest_error_frame_seen = 0;
reg wait_sdram_phase_seen = 0;
reg boot_phase_seen = 0;
reg dma_phase_seen = 0;
reg application_phase_seen = 0;

always @(posedge clk) begin
    case (dut.boot_phase)
        1: wait_sdram_phase_seen <= 1;
        2: boot_phase_seen <= 1;
        3: dma_phase_seen <= 1;
        5: application_phase_seen <= 1;
        default: begin end
    endcase
end

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
                // Boot error frame: magic CV3B, stage 1, category 1, code 1,
                // detail 0, XOR checksum of bytes 0..8.
                if (uart_history[0] == 8'h43 && uart_history[1] == 8'h56 &&
                    uart_history[2] == 8'h33 && uart_history[3] == 8'h42 &&
                    uart_history[4] == 8'h01 && uart_history[5] == 8'h01 &&
                    uart_history[6] == 8'h01 && uart_history[7] == 8'h00 &&
                    uart_history[8] == 8'h00 &&
                    (uart_history[0] ^ uart_history[1] ^ uart_history[2] ^
                     uart_history[3] ^ uart_history[4] ^ uart_history[5] ^
                     uart_history[6] ^ uart_history[7] ^ uart_history[8] ^
                     uart_history[9]) == 0)
                    descriptor_error_frame_seen = 1;
                // Manifest error: stage 1, category 2, code 6, detail 0,
                // followed by the XOR checksum.
                if (uart_history[0] == 8'h43 && uart_history[1] == 8'h56 &&
                    uart_history[2] == 8'h33 && uart_history[3] == 8'h42 &&
                    uart_history[4] == 8'h01 && uart_history[5] == 8'h02 &&
                    uart_history[6] == 8'h06 && uart_history[7] == 8'h00 &&
                    uart_history[8] == 8'h00 &&
                    (uart_history[0] ^ uart_history[1] ^ uart_history[2] ^
                     uart_history[3] ^ uart_history[4] ^ uart_history[5] ^
                     uart_history[6] ^ uart_history[7] ^ uart_history[8] ^
                     uart_history[9]) == 0)
                    manifest_error_frame_seen = 1;
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
    // Sentinels prove that the boot stage loads only the selected application
    // slot.
    memory[20'h30200] = 16'hdead;
    memory[20'h70200] = 16'hdead;
    repeat (16) @(posedge clk);
    sdram_init_done = 1;

    // Phase 1: the intact package boots the default S2 display application
    // (no button held). The display application never writes the LEDs, so the
    // boot monitor keeps ownership and shows the application phase.
    wait (dut.code_segment == 16'd7);
    repeat (4) @(posedge clk);
    if (dut.data_segment !== 16'h0000 && dut.data_segment !== 16'h0020 &&
        dut.data_segment !== 16'h0021)
        $fatal(1, "S2 display application data segment is not a framebuffer store: dseg=0x%04x",
            dut.data_segment);
    if (memory[20'h70200] === 16'hdead)
        $fatal(1, "selected S2 application was not loaded");
    if (memory[20'h30200] !== 16'hdead)
        $fatal(1, "unselected S1 application was loaded");
    if (word_read_seen)
        $fatal(1, "a word read reached the SDRAM adapter; line refills must burst");
    if (!line_burst_seen)
        $fatal(1, "no line burst reached the SDRAM adapter");
    // The boot stage runs from BSRAM and never touches the I-cache; only the
    // application fetches through it. I-cache prefetching is covered by the
    // system co-simulation's `icache_loop` program.
    if (!wait_sdram_phase_seen || !boot_phase_seen || !dma_phase_seen ||
        !application_phase_seen)
        $fatal(1, "boot observer missed phases: wait=%0d boot=%0d dma=%0d app=%0d",
            wait_sdram_phase_seen, boot_phase_seen, dma_phase_seen,
            application_phase_seen);
    if (dut.diagnostic_active !== 1 || leds !== 6'b100000)
        $fatal(1, "display application must leave diagnostic ownership at phase 5: active=%0d leds=%b",
            dut.diagnostic_active, leds);

    // Phase 2: holding the S1 button (01) resets the CPU and latches the
    // slider boot. The live pins are 00 after release, so reaching CSEG 3 /
    // DSEG 4 proves the reset-time selection survived into the boot stage.
    ddht_frame_seen = 0;
    buttons = 2'b01;
    repeat (8) @(posedge clk);
    buttons = 2'b00;
    wait (ddht_frame_seen);
    @(posedge clk);
    if (dut.code_segment !== 16'd3 || dut.data_segment !== 16'd4)
        $fatal(1, "S1 slider application segments not reached: cseg=0x%04x dseg=0x%04x",
            dut.code_segment, dut.data_segment);
    if (memory[20'h30200] === 16'hdead)
        $fatal(1, "selected S1 application was not loaded");
    if (leds !== 6'b000001)
        $fatal(1, "slider application must light logical LED 000001, got %b", leds);

    // Phase 3: corrupt the descriptor magic, reset through button 01,
    // and expect the descriptor boot error report.
    corrupt_metadata = 1;
    buttons = 2'b01;
    repeat (8) @(posedge clk);
    buttons = 2'b00;
    wait (descriptor_error_frame_seen);
    @(posedge clk);
    if (leds !== 6'b010001)
        $fatal(1, "descriptor failure must light LEDs 6'b010001, got %b", leds);
    if (dut.code_segment !== 16'd0)
        $fatal(1, "failed boot must stay in the boot segment, cseg=0x%04x", dut.code_segment);

    // Phase 4: allow Stage0 to run, corrupt the manifest magic, and prove the
    // boot stage reports the failure without entering the application.
    corrupt_metadata = 2;
    buttons = 2'b01;
    repeat (8) @(posedge clk);
    buttons = 2'b00;
    wait (manifest_error_frame_seen);
    @(posedge clk);
    if (leds !== 6'b010010)
        $fatal(1, "manifest failure must light LEDs 6'b010010, got %b", leds);
    if (dut.code_segment !== 16'd0)
        $fatal(1, "manifest failure must stay in the boot segment, cseg=0x%04x", dut.code_segment);
    $display("DIGITAL_DESIGN_PASS");
    $finish;
end

initial begin
    repeat (8000000) @(posedge clk);
    $display("FAIL: timeout (cseg=0x%04x dseg=0x%04x pc=0x%04x leds=%b retired=%0d)",
        dut.code_segment, dut.data_segment, dut.pc, leds, dut.retired_words);
    $finish(1);
end
endmodule
