`timescale 1ns/1ps
// Signature testbench for the single-stage flash boot. Phase 1 preloads the
// Flash model with the packed boot package and verifies the default S2
// application's rejected legacy GPU submit, followed by the button-01 S1 application. Later phases
// corrupt the descriptor and manifest metadata and check the boot stage's
// failure reports.
module tb;
reg clk = 0;
reg [1:0] buttons = 0;
reg flash_miso = 1;
reg [63:0] sdram_read_data = 0;
reg sdram_read_valid = 0;
reg sdram_init_done = 0;
wire sdram_request_ready;
reg sdram_stream_active = 0;
reg sdram_clock_ready = 1;
reg sdram_done = 0;
reg sdram_write_data_ready = 0;
wire [5:0] leds;
wire uart_tx;
wire flash_clk;
wire flash_cs_n;
wire flash_mosi;
wire sdram_request_valid;
wire sdram_next_valid;
wire [20:0] sdram_next_address;
wire sdram_write;
wire [20:0] sdram_address;
wire [5:0] sdram_words;
wire [3:0] sdram_write_mask;
wire [63:0] sdram_write_data;
wire sdram_write_data_valid;
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

// Abstract native 32-bit controller port. Address is a 32-bit word address.
// The board bridge groups two native words into each 64-bit system beat.
reg [15:0] memory [0:4194303];
reg write_pending = 0;
reg [20:0] transfer_address = 0;
reg [5:0] transfer_words = 0;
reg [3:0] transfer_mask = 0;
integer transfer_pairs = 0;
integer transfer_pair = 0;
integer read_delay = 0;
integer read_pairs = 0;
reg word_read_seen = 0;
reg line_burst_seen = 0;
reg [20:0] idx;
reg [20:0] idx2;
integer cycle;

assign sdram_request_ready = !(|buttons) && !write_pending &&
    read_delay == 0 && read_pairs == 0;
always @(*) sdram_write_data_ready = !(|buttons) && write_pending;

always @(posedge clk) begin
    sdram_done <= 0;
    sdram_read_valid <= 0;
    if (|buttons) begin
        write_pending <= 0;
        read_delay <= 0;
        read_pairs <= 0;
        transfer_pair <= 0;
    end else begin
    if (sdram_request_valid && sdram_request_ready) begin
        if (sdram_words != 1 && sdram_words != 8 &&
            sdram_words != 16 && sdram_words != 32)
            $fatal(1, "illegal native descriptor length %0d", sdram_words);
        if (write_pending || read_delay != 0 || read_pairs != 0)
            $fatal(1, "overlapping native descriptors write=%0d delay=%0d pairs=%0d new_write=%0d new_words=%0d new_addr=%h old_words=%0d old_addr=%h port_state=%0d",
                write_pending, read_delay, read_pairs, sdram_write,
                sdram_words, sdram_address, transfer_words,
                transfer_address, dut.u_shared_sdram_port.state);
        transfer_address <= sdram_address;
        transfer_words <= sdram_words;
        transfer_mask <= sdram_write_mask;
        transfer_pair <= 0;
        transfer_pairs <= sdram_words == 1 ? 1 : sdram_words / 2;
        if (sdram_write) begin
            write_pending <= 1;
        end else begin
            read_delay <= 2;
            read_pairs <= sdram_words == 1 ? 1 : sdram_words / 2;
            if (sdram_write_mask != 0) $fatal(1, "read DQM mask not zero");
            if (sdram_words == 1) word_read_seen <= 1;
            else line_burst_seen <= 1;
        end
    end

    if (write_pending && sdram_write_data_valid && sdram_write_data_ready) begin
        idx = transfer_address + 2*transfer_pair;
        if (transfer_words == 1) begin
            if (!transfer_mask[0]) memory[{idx,1'b0}] <= sdram_write_data[15:0];
            if (!transfer_mask[2]) memory[{idx,1'b1}] <= sdram_write_data[31:16];
        end else begin
            idx2 = idx + 1;
            memory[{idx,1'b0}] <= sdram_write_data[15:0];
            memory[{idx,1'b1}] <= sdram_write_data[31:16];
            memory[{idx2,1'b0}] <= sdram_write_data[47:32];
            memory[{idx2,1'b1}] <= sdram_write_data[63:48];
        end
        transfer_pair <= transfer_pair + 1;
        if (transfer_pair == transfer_pairs - 1) begin
            write_pending <= 0;
            sdram_done <= 1;
        end
    end

    if (read_delay != 0) read_delay <= read_delay - 1;
    else if (read_pairs != 0) begin
        idx = transfer_address + 2*transfer_pair;
        idx2 = idx + 1;
        sdram_read_data <= transfer_words == 1 ?
            {32'b0, memory[{idx,1'b1}], memory[{idx,1'b0}]} :
            {memory[{idx2,1'b1}], memory[{idx2,1'b0}],
             memory[{idx,1'b1}], memory[{idx,1'b0}]};
        sdram_read_valid <= 1;
        transfer_pair <= transfer_pair + 1;
        read_pairs <= read_pairs - 1;
        if (read_pairs == 1) sdram_done <= 1;
    end
    end
end

always @(posedge clk) begin
    if (dut.code_segment == 16'd7 && dut.memory_response_valid && dut.memory_error)
        $fatal(1, "SDRAM native adapter error: state=%0d fed=%0d/%0d words=%0d address=%h",
            dut.u_shared_sdram_port.state, dut.u_shared_sdram_port.line_fed,
            dut.u_shared_sdram_port.line_total, transfer_words, transfer_address);
    if (dut.icache_memory_request_ready &&
        (!dut.memory_request_valid || dut.memory_write || !dut.memory_line ||
         dut.memory_address != dut.icache_memory_address)) begin
        $display("FAIL: instruction acceptance routed mismatched request");
        $finish(1);
    end
end

// SPI Flash model: standard read command 03h plus a 24-bit byte address,
// then package bytes MSB-first. Addresses outside the packed boot package
// (placed at Flash byte 0x100000) read as erased Flash.
localparam integer FLASH_BASE = 32'h00100000;
localparam integer FLASH_PACKAGE_SIZE = __FLASH_PACKAGE_SIZE__;
localparam integer S1_BASE = __S1_BASE__;
localparam integer S2_BASE = __S2_BASE__;
localparam integer S1_IMAGE_WORDS = __S1_IMAGE_WORDS__;
localparam integer S2_IMAGE_WORDS = __S2_IMAGE_WORDS__;

reg [7:0] flash_image [0:FLASH_PACKAGE_SIZE-1];
reg [15:0] expected_s1 [0:S1_IMAGE_WORDS-1];
reg [15:0] expected_s2 [0:S2_IMAGE_WORDS-1];
reg [31:0] flash_command = 0;
integer flash_command_bits = 0;
reg [23:0] flash_byte_address = 0;
integer flash_data_bit = 0;
reg [7:0] flash_current_byte = 0;
reg [1:0] corrupt_metadata = 0;
integer flash_init_index;
integer image_word;

integer framebuffer_word;

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
reg display_frame_seen = 0;
reg descriptor_error_frame_seen = 0;
reg manifest_error_frame_seen = 0;
reg wait_sdram_phase_seen = 0;
reg boot_phase_seen = 0;
reg dma_phase_seen = 0;
reg application_phase_seen = 0;
integer pre_submit_stall_cycles = 0;
reg [31:0] pre_submit_last_retired = 0;
always @(posedge clk) begin
    if ({dut.gpu_ro_memory_request_valid, dut.gpu_fb_r_memory_request_valid,
         dut.gpu_fb_w_memory_request_valid} !== 3'b000)
        $fatal(1,"retired GPU issued a memory request");

    case (dut.boot_phase)
        1: wait_sdram_phase_seen <= 1;
        2: boot_phase_seen <= 1;
        3: dma_phase_seen <= 1;
        5: application_phase_seen <= 1;
        default: begin end
    endcase
end

// Before the first GPU submission there is no intentional long CPU sleep.
// Catch a cache/CPU deadlock substantially earlier than the global scenario
// timeout while leaving the later GPU and vblank waits unconstrained here.
always @(posedge clk) begin
    if (dut.code_segment != 16'd7 || dut.halted ||
        dut.retired_words != pre_submit_last_retired) begin
        pre_submit_last_retired <= dut.retired_words;
        pre_submit_stall_cycles <= 0;
    end else begin
        pre_submit_stall_cycles <= pre_submit_stall_cycles + 1;
        if (pre_submit_stall_cycles == 1000000) begin
            $display("FAIL: pre-submit CPU stall (pc=0x%04x retired=%0d core_state=%0d halted=%0d fault=%0d fault_code=0x%02x hold=%0d if_req=%0d if_ready=%0d icache_state=%0d refill_beat=%0d pending_addr=0x%08x ic_mem_req=%0d ic_mem_ready=%0d ic_mem_resp=%0d mem_resp=%0d/%0d port_state=%0d port_pending=%0d/%0d/0x%06x beats=%0d/%0d sdram_cmd=%0d/%0d sdram_read=%0d clean_valid=%0d clean_addr=0x%06x line_ready=%0d dcache_state=%0d maint_busy=%0d maint_done=%0d)",
                dut.pc, dut.retired_words, dut.u_core.state, dut.halted,
                dut.faulted, dut.fault_code, dut.sysctl_cpu_hold,
                dut.core_instruction_request_valid, dut.core_instruction_request_ready,
                dut.u_instruction_cache.u_cache.state,
                dut.u_instruction_cache.u_cache.refill_beat,
                dut.u_instruction_cache.u_cache.pending_address,
                dut.icache_memory_request_valid, dut.icache_memory_request_ready,
                dut.icache_memory_response_valid, dut.memory_response_valid,
                dut.memory_response_last, dut.u_shared_sdram_port.state,
                dut.u_shared_sdram_port.pending_write,
                dut.u_shared_sdram_port.pending_line,
                dut.memory_address,
                dut.u_shared_sdram_port.read_beats, dut.u_shared_sdram_port.line_total,
                dut.sdram_request_valid, dut.sdram_done, dut.sdram_read_valid,
                dut.core_data_line_clean_valid, dut.core_data_line_clean_address,
                dut.dcache_line_copy_ready, dut.u_data_cache.state,
                dut.dcache_maintenance_busy, dut.dcache_maintenance_done);
            $finish(1);
        end
    end
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
                // Display success frame: same layout with test ID 0x0b.
                if (uart_history[2] == 8'h44 && uart_history[3] == 8'h44 &&
                    uart_history[4] == 8'h48 && uart_history[5] == 8'h54 &&
                    uart_history[6] == 8'h01 && uart_history[7] == 8'h0b &&
                    uart_history[8] == 8'h00 &&
                    (uart_history[2] ^ uart_history[3] ^ uart_history[4] ^
                     uart_history[5] ^ uart_history[6] ^ uart_history[7] ^
                     uart_history[8] ^ uart_history[9]) == 0)
                    display_frame_seen = 1;
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
    for (cycle = 0; cycle < 96000; cycle = cycle + 1) begin
        memory[22'h200000 + cycle] = 16'h5a5a;
        memory[22'h218000 + cycle] = 16'h5a5a;
    end
    memory[22'h217700] = 16'hbeef;
    memory[22'h22f700] = 16'hbeef;
    // Seed each application slot with the complement of its generated image.
    // The comparisons below follow image length and contents automatically.
__S1_IMAGE_INIT__
__S2_IMAGE_INIT__
    for (image_word = 0; image_word < S1_IMAGE_WORDS; image_word = image_word + 1)
        memory[S1_BASE + image_word] = ~expected_s1[image_word];
    for (image_word = 0; image_word < S2_IMAGE_WORDS; image_word = image_word + 1)
        memory[S2_BASE + image_word] = ~expected_s2[image_word];
    repeat (16) @(posedge clk);
    sdram_init_done = 1;

    // Phase 1: the intact package boots the default S2 display application
    // (no button held). The display application never writes the LEDs, so the
    // boot monitor keeps ownership and shows the application phase. Its first
    // legacy GPU submit must halt with 0x0b01 and have no rendering effects.
    wait (dut.code_segment == 16'd7);
    wait (dut.halted);
    if (dut.halt_signal !== 16'h0b01 || dut.faulted)
        $fatal(1,"retired S2 GPU must halt on submit rejection: signal=%h fault=%b",dut.halt_signal,dut.faulted);
    if (display_frame_seen || dut.u_display.next_pending)
        $fatal(1,"rejected S2 draw reported success or requested a swap");
    repeat (4) @(posedge clk);
    if (dut.data_segment !== 16'h0000 && dut.data_segment !== 16'h0020 &&
        dut.data_segment !== 16'h0021)
        $fatal(1, "S2 display application data segment is not a framebuffer store: dseg=0x%04x",
            dut.data_segment);
    for (image_word = 0; image_word < S2_IMAGE_WORDS; image_word = image_word + 1)
        if (memory[S2_BASE + image_word] !== expected_s2[image_word])
            $fatal(1, "selected S2 application word %0d: expected=%04x actual=%04x",
                image_word, expected_s2[image_word], memory[S2_BASE + image_word]);
    for (image_word = 0; image_word < S1_IMAGE_WORDS; image_word = image_word + 1)
        if (memory[S1_BASE + image_word] !== ~expected_s1[image_word])
            $fatal(1, "unselected S1 application word %0d changed", image_word);
    if (word_read_seen)
        $fatal(1, "a word read reached the SDRAM adapter; line refills must burst");
    if (!line_burst_seen)
        $fatal(1, "no line burst reached the SDRAM adapter");
    // The boot stage runs from BSRAM and never touches the I-cache; only the
    // application fetches through it. I-cache refill behavior is covered by
    // the system co-simulation's `icache_loop` program.
    if (!wait_sdram_phase_seen || !boot_phase_seen || !dma_phase_seen ||
        !application_phase_seen)
        $fatal(1, "boot observer missed phases: wait=%0d boot=%0d dma=%0d app=%0d",
            wait_sdram_phase_seen, boot_phase_seen, dma_phase_seen,
            application_phase_seen);
    if (dut.diagnostic_active !== 1 || leds !== 6'b100000)
        $fatal(1, "display application must leave diagnostic ownership at phase 5: active=%0d leds=%b",
            dut.diagnostic_active, leds);
    // Every seeded framebuffer word and guard must survive the rejected
    // submission. Boot DMA and CPU command-buffer stores use other regions.
    for (framebuffer_word=0; framebuffer_word<96000; framebuffer_word=framebuffer_word+1)
        if (memory[22'h200000+framebuffer_word] !== 16'h5a5a ||
            memory[22'h218000+framebuffer_word] !== 16'h5a5a)
            $fatal(1,"rejected GPU modified framebuffer offset=%0d",framebuffer_word);
    if (memory[22'h217700] !== 16'hbeef || memory[22'h22f700] !== 16'hbeef)
        $fatal(1, "rejected GPU draw changed framebuffer guards");
`ifdef CPU_V3_S2_REJECTION_ONLY
    $display("DIGITAL_DESIGN_PASS");
    $finish;
`endif

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
    for (image_word = 0; image_word < S1_IMAGE_WORDS; image_word = image_word + 1)
        if (memory[S1_BASE + image_word] !== expected_s1[image_word])
            $fatal(1, "selected S1 application word %0d: expected=%04x actual=%04x",
                image_word, expected_s1[image_word], memory[S1_BASE + image_word]);
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
    $display("FAIL: timeout (cseg=0x%04x dseg=0x%04x pc=0x%04x leds=%b retired=%0d halted=%0d halt_signal=%h display_seen=%0d)",
        dut.code_segment, dut.data_segment, dut.pc, leds, dut.retired_words,
        dut.halted, dut.halt_signal, display_frame_seen);
    $finish(1);
end
endmodule
