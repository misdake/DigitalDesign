module tb;
reg clk = 0;
reg reset = 1;
reg instruction_request_ready = 1;
reg instruction_response_valid = 0;
reg [15:0] instruction_data = 0;
reg instruction_error = 0;
reg data_request_ready = 1;
reg data_response_valid = 0;
reg [15:0] data_read_data = 0;
reg data_error = 0;
wire instruction_request_valid;
wire [31:0] instruction_address;
wire instruction_response_ready;
wire data_request_valid;
wire data_write;
wire [31:0] data_address;
wire [15:0] data_write_data;
wire data_response_ready;
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
integer index;
integer errors = 0;
integer scenario = 0;
integer cond;

// When armed, every data access must target the device word 0xff23.
reg check_device_address = 0;

always @(posedge clk) begin
    instruction_response_valid <= instruction_request_valid;
    if (instruction_request_valid)
        instruction_data <= memory[instruction_address[15:0]];
    data_response_valid <= data_request_valid;
    if (data_request_valid) begin
        if (check_device_address && data_address !== 32'h0000_ff23) begin
            $display("FAIL: scenario %0d device access address %h", scenario, data_address);
            errors = errors + 1;
        end
        if (data_write)
            memory[data_address[15:0]] <= data_write_data;
        else
            data_read_data <= memory[data_address[15:0]];
    end
end

task clear_memory;
    begin
        for (index = 0; index < 65536; index = index + 1)
            memory[index] = 0;
    end
endtask

task run_core;
    input integer max_cycles;
    integer cycles;
    begin
        reset = 1;
        repeat (3) @(posedge clk);
        #1 reset = 0;
        cycles = 0;
        while (!halted && !fault && cycles < max_cycles) begin
            @(posedge clk);
            #1;
            cycles = cycles + 1;
        end
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
    memory[0] = 16'haf23; // LDU r2, 3
    memory[1] = 16'haf34; // LDU r3, 4
    memory[2] = 16'h2432; // MUL r4, r2, r3
    memory[3] = 16'hf020; // IMMHI12 0x020
    memory[4] = 16'haf50; // LDU r5, 0 -> r5 = 0x200
    memory[5] = 16'h9450; // STORE r4, [r5+0]
    memory[6] = 16'h8050; // LOAD r0, [r5+0]
    memory[7] = 16'ha003; // ADDI r0, 3
    memory[8] = 16'he800; // HALT
    scenario = 1;
    expect_halt(16'd15, 200);

    // Scenario 2: all six predicates, taken and not-taken, on pending Less
    // (r1 = 3 < 5 = r2, signed).
    for (cond = 0; cond < 6; cond = cond + 1) begin
        clear_memory;
        memory[0] = 16'hf000; // IMMHI12 0
        memory[1] = 16'haf13; // LDU r1, 3
        memory[2] = 16'hf000; // IMMHI12 0
        memory[3] = 16'haf25; // LDU r2, 5
        memory[4] = 16'he012; // CMPS r1, r2 -> Less
        memory[5] = 16'hb001 | (cond << 8); // B cond, +1
        memory[6] = 16'haf09; // LDU r0, 9 (not-taken marker)
        memory[7] = 16'he800; // HALT
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
    memory[0] = 16'hf000; // IMMHI12 0
    memory[1] = 16'hb100; // BNE +0 (consumes the prefix)
    scenario = 9;
    expect_fault(8'd1, 16'd0, 100);
    if (retired_words !== 0) begin
        $display("FAIL: scenario 9 retired %0d words before the fault", retired_words);
        errors = errors + 1;
    end

    // Scenario 10: prefix transparency - CMPSI, then IMMHI12, then BLT with
    // a wide 16-bit offset {prefix[7:0], imm8}.
    clear_memory;
    memory[0] = 16'haf10; // LDU r1, 0
    memory[1] = 16'hac15; // CMPSI r1, 5 -> Less
    memory[2] = 16'hf001; // IMMHI12 0x001
    memory[3] = 16'hb203; // BLT offset 0x0103 -> target 0x107
    memory[4] = 16'haf09; // LDU r0, 9 (fall-through marker)
    memory[5] = 16'he800; // HALT
    for (index = 6; index < 16'h107; index = index + 1)
        memory[index] = 16'haf21; // filler: LDU r2, 1 (must not run)
    memory[16'h107] = 16'haf02; // LDU r0, 2
    memory[16'h108] = 16'he800; // HALT
    scenario = 10;
    expect_halt(16'd2, 1000);
    if (retired_words !== 6) begin
        $display("FAIL: scenario 10 retired %0d words, expected 6", retired_words);
        errors = errors + 1;
    end

    // Scenario 11: JREL skips, JALREL links the fall-through address into r14.
    clear_memory;
    memory[0] = 16'hb802; // JREL +2 -> 3
    memory[1] = 16'haf09; // LDU r0, 9 (skipped)
    memory[2] = 16'he800; // HALT (skipped)
    memory[3] = 16'hb902; // JALREL +2 -> 6, r14 = 4
    memory[4] = 16'haf09; // LDU r0, 9 (skipped)
    memory[5] = 16'he800; // HALT (skipped)
    memory[6] = 16'he10e; // MOV r0, r14
    memory[7] = 16'he800; // HALT
    scenario = 11;
    expect_halt(16'd4, 100);

    // Scenario 12: JALR with a link field other than r14 faults.
    clear_memory;
    memory[0] = 16'he5d1; // JALR r13, r1 (link field 13 != 14)
    scenario = 12;
    expect_fault(8'd1, 16'd0, 100);

    // Scenario 13: CMPS/CMPU at the 0x7fff/0x8000 sign boundary, prefixed
    // CMPSI/CMPUI, and CMP-class instructions write no register.
    clear_memory;
    memory[0] = 16'hf7ff; // IMMHI12 0x7ff
    memory[1] = 16'haf1f; // LDU r1, 0xf -> r1 = 0x7fff
    memory[2] = 16'hf800; // IMMHI12 0x800
    memory[3] = 16'haf20; // LDU r2, 0 -> r2 = 0x8000
    memory[4] = 16'haf31; // LDU r3, 1
    memory[5] = 16'he012; // CMPS r1, r2: 32767 > -32768 -> Greater
    memory[6] = 16'hb401; // BGT +1
    memory[7] = 16'haf30; // LDU r3, 0
    memory[8] = 16'haf41; // LDU r4, 1
    memory[9] = 16'hef12; // CMPU r1, r2: 0x7fff < 0x8000 -> Less
    memory[10] = 16'hb201; // BLT +1
    memory[11] = 16'haf40; // LDU r4, 0
    memory[12] = 16'haf51; // LDU r5, 1
    memory[13] = 16'hf800; // IMMHI12 0x800
    memory[14] = 16'hac10; // CMPSI r1, 0x8000 (i16 -32768) -> Greater
    memory[15] = 16'hb401; // BGT +1
    memory[16] = 16'haf50; // LDU r5, 0
    memory[17] = 16'haf61; // LDU r6, 1
    memory[18] = 16'hf800; // IMMHI12 0x800
    memory[19] = 16'had10; // CMPUI r1, 0x8000 (u16) -> Less
    memory[20] = 16'hb201; // BLT +1
    memory[21] = 16'haf60; // LDU r6, 0
    memory[22] = 16'ha541; // SHL r4, 1
    memory[23] = 16'ha552; // SHL r5, 2
    memory[24] = 16'ha563; // SHL r6, 3
    memory[25] = 16'h0034; // ADD r0, r3, r4
    memory[26] = 16'h0005; // ADD r0, r0, r5
    memory[27] = 16'h0006; // ADD r0, r0, r6
    memory[28] = 16'hf030; // IMMHI12 0x030
    memory[29] = 16'haf70; // LDU r7, 0 -> r7 = 0x300
    memory[30] = 16'h9170; // STORE r1, [r7+0] (unchanged by CMPS/CMPU)
    memory[31] = 16'h9271; // STORE r2, [r7+1]
    memory[32] = 16'he800; // HALT
    scenario = 13;
    expect_halt(16'd15, 500);
    if (memory[16'h0300] !== 16'h7fff || memory[16'h0301] !== 16'h8000) begin
        $display("FAIL: scenario 13 CMP wrote registers: r1=%h r2=%h",
                 memory[16'h0300], memory[16'h0301]);
        errors = errors + 1;
    end

    // Scenario 14: DEVSEND/DEVRECV address and data phases on device 2,
    // channel 3 (physical word 0xff23).
    clear_memory;
    memory[0] = 16'hf123; // IMMHI12 0x123
    memory[1] = 16'haf14; // LDU r1, 4 -> r1 = 0x1234
    memory[2] = 16'hca31; // DEVSEND r1, dev 2, ch 3
    memory[3] = 16'hc230; // DEVRECV r0, dev 2, ch 3
    memory[4] = 16'he800; // HALT
    scenario = 14;
    check_device_address = 1;
    expect_halt(16'h1234, 100);
    check_device_address = 0;
    if (memory[16'hff23] !== 16'h1234) begin
        $display("FAIL: scenario 14 device write data %h", memory[16'hff23]);
        errors = errors + 1;
    end

    // Scenario 15: device instructions do not consume a prefix, so a data
    // error on DEVRECV reports the instruction address (1), not the prefix
    // address (0).
    clear_memory;
    memory[0] = 16'hf000; // IMMHI12 0
    memory[1] = 16'hc232; // DEVRECV r2, dev 2, ch 3 -> data error
    scenario = 15;
    data_error = 1;
    expect_fault(8'd4, 16'd1, 100);
    data_error = 0;
    if (retired_words !== 1) begin
        $display("FAIL: scenario 15 retired %0d words, expected 1", retired_words);
        errors = errors + 1;
    end

    // Scenario 16: an ordinary retired instruction expires the pending test.
    clear_memory;
    memory[0] = 16'haf10; // LDU r1, 0
    memory[1] = 16'hac10; // CMPSI r1, 0 -> Equal
    memory[2] = 16'he111; // MOV r1, r1 (expires the pending test)
    memory[3] = 16'hb000; // BEQ +0 -> fault: no pending test
    scenario = 16;
    expect_fault(8'd1, 16'd3, 100);

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
