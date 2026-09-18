module tb;
reg clk = 0;
reg reset = 1;
reg hold = 0;
reg instruction_request_ready = 1;
reg instruction_response_valid = 0;
reg [15:0] instruction_data = 0;
reg instruction_error = 0;
reg data_request_ready = 1;
reg data_response_valid = 0;
reg [15:0] data_read_data = 0;
reg data_error = 0;
wire [15:0] device_read_data;
wire instruction_request_valid;
wire [31:0] instruction_address;
wire instruction_response_ready;
wire data_request_valid;
wire data_write;
wire [31:0] data_address;
wire [15:0] data_write_data;
wire data_response_ready;
wire [2:0] device_index;
wire [3:0] device_channel;
wire device_read_enable;
wire device_write_enable;
wire [15:0] device_write_data;
wire halted;
wire [15:0] halt_signal;
wire fault;
wire [7:0] fault_code;
wire [15:0] fault_pc;
wire [15:0] pc;
wire [15:0] code_segment;
wire [15:0] data_segment;
wire [31:0] retired_words;

CpuV3Core dut(.*);
always #5 clk = ~clk;

reg [15:0] memory [0:65535];
reg [15:0] devices [0:127];
assign device_read_data = devices[{device_index, device_channel}];
integer index;
integer errors = 0;
integer scenario = 0;
integer last_run_cycles = 0;
integer cond;
reg check_high_data_address = 0;
integer data_beat = 0;
integer fail_data_beat = -1;
reg pending_tb_data_write = 0;
reg [31:0] pending_tb_data_address = 0;
reg [15:0] pending_tb_data_write_data = 0;
reg delay_data_response = 0;
integer data_response_delay = 0;
integer observed_data_requests = 0;
reg observed_alu_during_store = 0;

function [15:0] gpr_word;
    input [3:0] index;
    gpr_word = dut.u_gpr_ram.words[index];
endfunction

always @(posedge clk) begin
    instruction_response_valid <= instruction_request_valid;
    if (instruction_request_valid)
        instruction_data <= memory[instruction_address[15:0]];
    if (delay_data_response) begin
        data_response_valid <= 0;
        if (data_request_valid)
            data_response_delay <= 6;
        else if (data_response_delay > 0) begin
            data_response_delay <= data_response_delay - 1;
            if (data_response_delay == 1)
                data_response_valid <= 1;
        end
    end else begin
        data_response_valid <= data_request_valid;
    end
    if (data_response_valid && pending_tb_data_write && !data_error)
        memory[pending_tb_data_address[15:0]] <= pending_tb_data_write_data;
    if (data_request_valid) begin
        if (scenario == 37)
            observed_data_requests <= observed_data_requests + 1;
        if (scenario == 14) begin
            $display("FAIL: scenario 14 DEV instruction used the data port");
            errors = errors + 1;
        end
        if (check_high_data_address && data_address !== 32'h0003_ff00) begin
            $display("FAIL: scenario %0d high-offset data address %h", scenario, data_address);
            errors = errors + 1;
        end
        pending_tb_data_write <= data_write;
        pending_tb_data_address <= data_address;
        pending_tb_data_write_data <= data_write_data;
        data_error <= data_beat == fail_data_beat;
        data_beat <= data_beat + 1;
        if (!data_write)
            data_read_data <= memory[data_address[15:0]];
    end else
        data_error <= 0;
    if (scenario == 37 && dut.async_store_valid && dut.async_store_issued &&
        dut.state == 2 && dut.opcode == 0)
        observed_alu_during_store <= 1;
    if (device_write_enable)
        devices[{device_index, device_channel}] <= device_write_data;
end

task clear_memory;
    begin
        for (index = 0; index < 65536; index = index + 1)
            memory[index] = 0;
        for (index = 0; index < 128; index = index + 1)
            devices[index] = 0;
    end
endtask

task run_core;
    input integer max_cycles;
    integer cycles;
    begin
        reset = 1;
        instruction_response_valid = 0;
        data_response_valid = 0;
        data_error = 0;
        pending_tb_data_write = 0;
        repeat (3) @(posedge clk);
        #1 reset = 0;
        cycles = 0;
        while (!halted && !fault && cycles < max_cycles) begin
            @(posedge clk);
            #1;
            cycles = cycles + 1;
        end
        last_run_cycles = cycles;
        if (!halted && !fault) begin
            $display("FAIL: scenario %0d exceeded %0d cycles", scenario, max_cycles);
            errors = errors + 1;
        end
    end
endtask

task expect_halt;
    input [15:0] signal;
    input integer max_cycles;
    begin
        run_core(max_cycles);
        if (fault || halt_signal !== signal) begin
            $display("FAIL: scenario %0d expected halt %h, got fault=%d code=%d signal=%h",
                     scenario, signal, fault, fault_code, halt_signal);
            errors = errors + 1;
        end
    end
endtask

task expect_fault;
    input [7:0] code;
    input [15:0] expected_pc;
    input integer max_cycles;
    begin
        run_core(max_cycles);
        if (!fault || fault_code !== code || fault_pc !== expected_pc) begin
            $display("FAIL: scenario %0d expected fault code=%d pc=%h, got fault=%d code=%d pc=%h",
                     scenario, code, expected_pc, fault, fault_code, fault_pc);
            errors = errors + 1;
        end
    end
endtask

initial begin
    // Scenario 1: baseline load/store, prefix load-immediate, and multiply.
    clear_memory;
    memory[0] = 16'ha323; // LDUI r2, 3
    memory[1] = 16'ha334; // LDUI r3, 4
    // ISA 0.8 multiply is destructive: MUL0 rd, rs computes rd = rd * rs.
    memory[2] = 16'h6042; // MOV r4, r2
    memory[3] = 16'h2843; // MUL0 r4, r3 -> r4 = 12
    memory[4] = 16'hf020; // PFX12 0x020
    memory[5] = 16'ha350; // LDUI r5, 0 -> r5 = 0x200
    memory[6] = 16'h9450; // STORE r4, [r5+0]
    memory[7] = 16'h8050; // LOAD r0, [r5+0]
    memory[8] = 16'ha003; // ADDI r0, 3
    memory[9] = 16'h6c00; // HALT (SIGNAL r0, 0)
    scenario = 1;
    expect_halt(16'd15, 200);

    // Scenario 2: all six predicates, taken and not-taken, on pending Less
    // (r1 = 3 < 5 = r2, signed).
    for (cond = 0; cond < 6; cond = cond + 1) begin
        clear_memory;
        memory[0] = 16'hf000; // PFX12 0
        memory[1] = 16'ha313; // LDUI r1, 3
        memory[2] = 16'hf000; // PFX12 0
        memory[3] = 16'ha325; // LDUI r2, 5
        memory[4] = 16'h6a12; // CMPS r1, r2 -> Less
        memory[5] = 16'hb001 | (cond << 8); // B cond, +1
        memory[6] = 16'ha309; // LDUI r0, 9 (not-taken marker)
        memory[7] = 16'h6c00; // HALT (SIGNAL r0, 0)
        scenario = scenario + 1;
        // Taken for NE/LT/LE (cond 1, 2, 5): r0 stays 0.
        expect_halt((cond == 0 || cond == 3 || cond == 4) ? 16'd9 : 16'd0, 200);
    end

    // Scenario 8: conditional branch without a pending test faults.
    clear_memory;
    memory[0] = 16'hb000; // BEQ +0
    scenario = 8;
    expect_fault(8'd1, 16'd0, 100);

    // Scenario 9: a prefixed conditional branch with no pending test faults
    // at the prefix address and retires nothing.
    clear_memory;
    memory[0] = 16'hf000; // PFX12 0
    memory[1] = 16'hb100; // BNE +0 (consumes the prefix)
    scenario = 9;
    expect_fault(8'd1, 16'd0, 100);
    if (retired_words !== 0) begin
        $display("FAIL: scenario 9 retired %0d words before the fault", retired_words);
        errors = errors + 1;
    end

    // Scenario 10: prefix transparency - CMPSI, then PFX12, then BLT with
    // a wide 16-bit offset {prefix[7:0], imm8}.
    clear_memory;
    memory[0] = 16'ha310; // LDUI r1, 0
    memory[1] = 16'hac15; // CMPSI r1, 5 -> Less
    memory[2] = 16'hf001; // PFX12 0x001
    memory[3] = 16'hb203; // BLT offset 0x0103 -> target 0x107
    memory[4] = 16'ha309; // LDUI r0, 9 (fall-through marker)
    memory[5] = 16'h6c00; // HALT (SIGNAL r0, 0)
    for (index = 6; index < 16'h107; index = index + 1)
        memory[index] = 16'ha321; // filler: LDU r2, 1 (must not run)
    memory[16'h107] = 16'ha302; // LDUI r0, 2
    memory[16'h108] = 16'h6c00; // HALT (SIGNAL r0, 0)
    scenario = 10;
    expect_halt(16'd2, 1000);
    if (retired_words !== 6) begin
        $display("FAIL: scenario 10 retired %0d words, expected 6", retired_words);
        errors = errors + 1;
    end

    // Scenario 11: JREL skips, JALREL links the fall-through address into r14.
    clear_memory;
    memory[0] = 16'hb602; // JREL +2 -> 3
    memory[1] = 16'ha309; // LDUI r0, 9 (skipped)
    memory[2] = 16'h6c00; // HALT (SIGNAL r0, 0) (skipped)
    memory[3] = 16'hb702; // JALREL +2 -> 6, r14 = 4
    memory[4] = 16'ha309; // LDUI r0, 9 (skipped)
    memory[5] = 16'h6c00; // HALT (SIGNAL r0, 0) (skipped)
    memory[6] = 16'h600e; // MOV r0, r14
    memory[7] = 16'h6c00; // HALT (SIGNAL r0, 0)
    scenario = 11;
    expect_halt(16'd4, 100);

    // Scenario 12: JALR with a link field other than r14 faults.
    clear_memory;
    memory[0] = 16'hbfd1; // JALR with link field 13 (!= the fixed 14)
    scenario = 12;
    expect_fault(8'd1, 16'd0, 100);

    // Scenario 13: CMPS/CMPU at the 0x7fff/0x8000 sign boundary, prefixed
    // CMPSI/CMPUI, and CMP-class instructions write no register.
    clear_memory;
    memory[0] = 16'hf7ff; // PFX12 0x7ff
    memory[1] = 16'ha31f; // LDUI r1, 0xf -> r1 = 0x7fff
    memory[2] = 16'hf800; // PFX12 0x800
    memory[3] = 16'ha320; // LDUI r2, 0 -> r2 = 0x8000
    memory[4] = 16'ha331; // LDUI r3, 1
    memory[5] = 16'h6a12; // CMPS r1, r2: 32767 > -32768 -> Greater
    memory[6] = 16'hb401; // BGT +1
    memory[7] = 16'ha330; // LDUI r3, 0
    memory[8] = 16'ha341; // LDUI r4, 1
    memory[9] = 16'h6b12; // CMPU r1, r2: 0x7fff < 0x8000 -> Less
    memory[10] = 16'hb201; // BLT +1
    memory[11] = 16'ha340; // LDUI r4, 0
    memory[12] = 16'ha351; // LDUI r5, 1
    memory[13] = 16'hf800; // PFX12 0x800
    memory[14] = 16'hac10; // CMPSI r1, 0x8000 (i16 -32768) -> Greater
    memory[15] = 16'hb401; // BGT +1
    memory[16] = 16'ha350; // LDUI r5, 0
    memory[17] = 16'ha361; // LDUI r6, 1
    memory[18] = 16'hf800; // PFX12 0x800
    memory[19] = 16'had10; // CMPUI r1, 0x8000 (u16) -> Less
    memory[20] = 16'hb201; // BLT +1
    memory[21] = 16'ha360; // LDUI r6, 0
    memory[22] = 16'h2441; // SHLI r4, 1
    memory[23] = 16'h2452; // SHLI r5, 2
    memory[24] = 16'h2463; // SHLI r6, 3
    memory[25] = 16'h0034; // ADD r0, r3, r4
    memory[26] = 16'h0005; // ADD r0, r0, r5
    memory[27] = 16'h0006; // ADD r0, r0, r6
    memory[28] = 16'hf030; // PFX12 0x030
    memory[29] = 16'ha370; // LDUI r7, 0 -> r7 = 0x300
    memory[30] = 16'h9170; // STORE r1, [r7+0] (unchanged by CMPS/CMPU)
    memory[31] = 16'h9271; // STORE r2, [r7+1]
    memory[32] = 16'h6c00; // HALT (SIGNAL r0, 0)
    scenario = 13;
    expect_halt(16'd15, 500);
    if (memory[16'h0300] !== 16'h7fff || memory[16'h0301] !== 16'h8000) begin
        $display("FAIL: scenario 13 CMP wrote registers: r1=%h r2=%h",
                 memory[16'h0300], memory[16'h0301]);
        errors = errors + 1;
    end

    // Scenario 14: single-cycle DEVSEND/DEVRECV on device 2, channel 3.
    clear_memory;
    memory[0] = 16'hf123; // PFX12 0x123
    memory[1] = 16'ha314; // LDUI r1, 4 -> r1 = 0x1234
    memory[2] = 16'h7a31; // DEVSEND r1, dev 2, ch 3
    memory[3] = 16'h7230; // DEVRECV r0, dev 2, ch 3
    memory[4] = 16'h6c00; // HALT (SIGNAL r0, 0)
    scenario = 14;
    expect_halt(16'h1234, 100);
    if (devices[7'h23] !== 16'h1234) begin
        $display("FAIL: scenario 14 device write data %h", devices[7'h23]);
        errors = errors + 1;
    end

    // Scenario 15: offsets 0xff00..0xffff retain DSEG like every other load/store.
    clear_memory;
    memory[0] = 16'hf000; // PFX12 0
    memory[1] = 16'ha313; // LDUI r1, 3
    memory[2] = 16'h6e11; // MTSR DSEG, r1
    memory[3] = 16'hfff0; // PFX12 0xfff
    memory[4] = 16'ha320; // LDUI r2, 0 -> 0xff00
    memory[5] = 16'hf55a; // PFX12 0x55a
    memory[6] = 16'ha33a; // LDUI r3, 0xa -> 0x55aa
    memory[7] = 16'h9320; // STORE r3, [r2]
    memory[8] = 16'h8020; // LOAD r0, [r2]
    memory[9] = 16'h6c00; // HALT (SIGNAL r0, 0)
    scenario = 15;
    check_high_data_address = 1;
    expect_halt(16'h55aa, 150);
    check_high_data_address = 0;

    // Scenario 16: device instructions do not consume a prefix; both physical
    // words retire and the device read still uses the dedicated port.
    clear_memory;
    memory[0] = 16'hf000; // PFX12 0
    memory[1] = 16'h7232; // DEVRECV r2, dev 2, ch 3
    memory[2] = 16'h6002; // MOV r0, r2
    memory[3] = 16'h6c00; // HALT (SIGNAL r0, 0)
    devices[7'h23] = 16'h4567;
    scenario = 16;
    expect_halt(16'h4567, 100);
    if (retired_words !== 4) begin
        $display("FAIL: scenario 16 retired %0d words, expected 4", retired_words);
        errors = errors + 1;
    end

    // Scenario 17: an ordinary retired instruction expires the pending test.
    clear_memory;
    memory[0] = 16'ha310; // LDUI r1, 0
    memory[1] = 16'hac10; // CMPSI r1, 0 -> Equal
    memory[2] = 16'h6011; // MOV r1, r1 (expires the pending test)
    memory[3] = 16'hb000; // BEQ +0 -> fault: no pending test
    scenario = 17;
    expect_fault(8'd1, 16'd3, 100);

    // Scenario 37: one scalar store runs in the background. An ALU operation
    // overlaps it, but the following load cannot request the data port until
    // the store response arrives.
    clear_memory;
    memory[0] = 16'hf010;
    memory[1] = 16'ha310; // r1 = 0x0100
    memory[2] = 16'ha32a; // r2 = 10
    memory[3] = 16'h9210; // STORE r2, [r1]
    memory[4] = 16'h0322; // ADD r3, r2, r2
    memory[5] = 16'h8010; // LOAD r0, [r1]
    memory[6] = 16'h6c00; // HALT (SIGNAL r0, 0)
    scenario = 37;
    delay_data_response = 1;
    data_response_delay = 0;
    observed_data_requests = 0;
    observed_alu_during_store = 0;
    expect_halt(16'd10, 200);
    delay_data_response = 0;
    if (!observed_alu_during_store) begin
        $display("FAIL: scenario 37 did not overlap ALU with async store");
        errors = errors + 1;
    end
    if (observed_data_requests != 2) begin
        $display("FAIL: scenario 37 observed %0d data requests, expected 2",
                 observed_data_requests);
        errors = errors + 1;
    end
    if (memory[16'h0100] !== 16'd10) begin
        $display("FAIL: scenario 37 store value %h", memory[16'h0100]);
        errors = errors + 1;
    end


    // Scenario 40: major D (the old fix16 FPU space) is reserved until
    // FPU v2 integration and faults as an invalid instruction.
    clear_memory;
    memory[0] = 16'hdf00; // reserved FPU fn 15
    scenario = 40;
    expect_fault(8'd1, 16'd0, 100);

    // Scenario 42: majors C and E are fully reserved in ISA 0.8; the
    // revision 0.7 HALT word 0xe800 faults as an invalid instruction.
    clear_memory;
    memory[0] = 16'hc000; // reserved major C
    scenario = 42;
    expect_fault(8'd1, 16'd0, 100);
    clear_memory;
    memory[0] = 16'he800; // reserved major E (the 0.7 HALT word)
    scenario = 42;
    expect_fault(8'd1, 16'd0, 100);

    // Scenario 43: unsigned multiply windows, a masked destructive register
    // shift, conditional moves, and a non-halting SIGNAL retiring as a NOP.
    clear_memory;
    memory[0] = 16'hf00f; // PFX12 0x00f
    memory[1] = 16'ha31f; // LDUI r1, 0xf -> r1 = 0x00ff
    memory[2] = 16'h6021; // MOV r2, r1
    // 0xff * 0xff = 0xfe01: MUL8 keeps [23:8] = 0xfe.
    memory[3] = 16'h2921; // MUL8 r2, r1
    memory[4] = 16'hffff; // PFX12 0xfff
    memory[5] = 16'ha33f; // LDUI r3, 0xf -> r3 = 0xffff
    // 0xffff * 0x00ff = 0xfeff01: MUL16 keeps [31:16] = 0xfe.
    memory[6] = 16'h2a31; // MUL16 r3, r1
    memory[7] = 16'hf800; // PFX12 0x800
    memory[8] = 16'ha340; // LDUI r4, 0 -> r4 = 0x8000
    memory[9] = 16'hf001; // PFX12 0x001
    memory[10] = 16'ha351; // LDUI r5, 1 -> r5 = 0x11
    // Register-count shifts mask rs to four bits: 0x11 & 15 = 1.
    memory[11] = 16'h2145; // SHR r4, r5 -> r4 = 0x4000
    memory[12] = 16'h6a12; // CMPS r1, r2: 0xff > 0xfe -> Greater
    memory[13] = 16'hbc61; // MOVGT r6, r1 -> r6 = 0x00ff
    memory[14] = 16'h6a12; // CMPS r1, r2 -> Greater again
    // Not taken, but still consumes the pending test.
    memory[15] = 16'hba63; // MOVLT r6, r3
    memory[16] = 16'h6a11; // CMPS r1, r1 -> Equal
    memory[17] = 16'hb871; // MOVEQ r7, r1 -> r7 = 0x00ff
    memory[18] = 16'h6c61; // SIGNAL r6, 1 -> retires as a NOP
    memory[19] = 16'h6002; // MOV r0, r2
    memory[20] = 16'h0003; // ADD r0, r0, r3
    memory[21] = 16'h0004; // ADD r0, r0, r4
    memory[22] = 16'h0006; // ADD r0, r0, r6
    memory[23] = 16'h0007; // ADD r0, r0, r7
    memory[24] = 16'h6c00; // HALT (SIGNAL r0, 0)
    scenario = 43;
    expect_halt(16'h43fa, 300);

    // Scenario 44: LDC/ADDC index the shared symmetric constant table; a
    // pending prefix expires unused before the non-consuming ADDC and retires
    // separately. ADDI/SUBI read the unprefixed immediate as an unsigned u4
    // (15 was -1 under the old signed reading).
    clear_memory;
    memory[0] = 16'ha709; // LDC r0, 9 -> r0 = 0xfff0 (-16)
    memory[1] = 16'ha71f; // LDC r1, 15 -> r1 = 0xfe00 (-512)
    memory[2] = 16'hab14; // ADDC r1, 4 -> r1 = 0xfe00 + 64 = 0xfe40
    memory[3] = 16'hfabc; // PFX12 0xabc (expires unused)
    memory[4] = 16'hab06; // ADDC r0, 6 -> r0 = -16 + 256 = 0x00f0
    memory[5] = 16'ha00f; // ADDI r0, 15 -> r0 = 0x00ff
    memory[6] = 16'ha10f; // SUBI r0, 15 -> r0 = 0x00f0
    memory[7] = 16'ha000; // ADDI r0, 0 -> r0 = 0x00f0
    memory[8] = 16'h6c00; // HALT (SIGNAL r0, 0)
    scenario = 44;
    expect_halt(16'h00f0, 200);
    if (gpr_word(1) !== 16'hfe40) begin
        $display("FAIL: scenario 44 r1 %h, expected fe40", gpr_word(1));
        errors = errors + 1;
    end
    if (retired_words !== 9) begin
        $display("FAIL: scenario 44 retired %0d words, expected 9", retired_words);
        errors = errors + 1;
    end

    if (errors != 0) begin
        $display("FAIL: %0d error(s)", errors);
        $finish(1);
    end
    $display("DIGITAL_DESIGN_PASS");
    $finish;
end

initial begin
    #500000;
    $display("FAIL: global timeout");
    $finish(1);
end
endmodule
