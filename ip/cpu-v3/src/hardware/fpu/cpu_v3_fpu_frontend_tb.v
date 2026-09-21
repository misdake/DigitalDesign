module tb;
reg clk = 0;
reg word_valid = 0;
reg [15:0] word = 0;
reg abort = 0;
wire read_valid;
wire [8:0] rf_read_a_address;
wire [8:0] rf_read_b_address;
wire [15:0] word0_raw;
wire [15:0] word1_raw;
wire [3:0] instr_opcode;
wire instr_complete;

CpuV3FpuFrontend dut(.*);
always #5 clk = ~clk;

localparam integer MAX_CYCLES = 200000;
integer cycles = 0;
always @(posedge clk) begin
    cycles <= cycles + 1;
    if (cycles > MAX_CYCLES) begin
        $display("DIGITAL_DESIGN_FAIL: cycle limit exceeded");
        $finish;
    end
end

// Independent reference model of the two-word handshake. It mirrors the spec
// formula for both the combinational read addresses and the registered latch
// outputs, including the next-cycle instr_complete phase.
reg ref_waiting_word1 = 0;
reg [15:0] ref_word0_raw = 0;
reg [15:0] ref_word1_raw = 0;
reg [3:0] ref_instr_opcode = 0;
reg ref_instr_complete = 0;

integer check_count = 0;
integer complete_pulses = 0;
integer base_pulses = 0;
integer i;
integer seed = 32'h0BADF00D;
reg [15:0] w0;
reg [15:0] w1;

reg [8:0] exp_a;
reg [8:0] exp_b;
reg exp_read_valid;
reg [15:0] exp_word0;
reg [15:0] exp_word1;
reg [3:0] exp_opcode;
reg exp_complete;
reg was_waiting_word1;

task check_value;
    input [31:0] got;
    input [31:0] want;
    input [8*48-1:0] label;
    begin
        check_count = check_count + 1;
        if (got !== want) begin
            $display("DIGITAL_DESIGN_FAIL: %0s got %08h want %08h",
                label, got, want);
            $finish;
        end
    end
endtask

// Drives one input cycle. Inputs change at the falling edge; the combinational
// outputs are sampled #1 later in that same cycle, and the registered outputs
// are sampled #1 after the rising edge. The reference state is then advanced.
task run_cycle;
    input valid;
    input [15:0] w;
    input ab;
    begin
        @(negedge clk);
        word_valid = valid;
        word = w;
        abort = ab;
        was_waiting_word1 = ref_waiting_word1;

        exp_read_valid = valid && !was_waiting_word1;
        exp_a = (w[15:12] == 4'hE) ? {3'b000, w[7:2]} : {3'b000, w[11:6]};
        exp_b = {3'b000, w[5:0]};
        #1;
        check_value(read_valid, exp_read_valid, "read_valid");
        check_value(rf_read_a_address, exp_a, "rf_read_a_address");
        check_value(rf_read_b_address, exp_b, "rf_read_b_address");
        check_value(|rf_read_a_address[8:6], 1'b0, "read_a top bits zero");
        check_value(|rf_read_b_address[8:6], 1'b0, "read_b top bits zero");

        exp_complete = valid && was_waiting_word1 && !ab;
        if (exp_complete) complete_pulses = complete_pulses + 1;
        exp_word0 = ref_word0_raw;
        exp_word1 = ref_word1_raw;
        exp_opcode = ref_instr_opcode;
        if (valid && !was_waiting_word1) begin
            exp_word0 = w;
            exp_opcode = w[15:12];
        end
        if (valid && was_waiting_word1 && !ab)
            exp_word1 = w;

        @(posedge clk);
        #1;
        check_value(word0_raw, exp_word0, "word0_raw");
        check_value(word1_raw, exp_word1, "word1_raw");
        check_value(instr_opcode, exp_opcode, "instr_opcode");
        check_value(instr_complete, exp_complete, "instr_complete");

        if (valid && !was_waiting_word1) begin
            ref_word0_raw = w;
            ref_instr_opcode = w[15:12];
            ref_waiting_word1 = 1;
        end
        if (valid && was_waiting_word1 && !ab) begin
            ref_word1_raw = w;
            ref_waiting_word1 = 0;
        end else if (ab && was_waiting_word1) begin
            ref_waiting_word1 = 0;
        end
        ref_instr_complete = exp_complete;
    end
endtask

initial begin
    @(negedge clk);

    // Directed opcode 0xC pair: word[11:6] = Fa, word[5:0] = Fb.
    run_cycle(1'b1, 16'hCA95, 1'b0);
    check_value(rf_read_a_address, 9'h02A, "directed C read_a");
    check_value(rf_read_b_address, 9'h015, "directed C read_b");
    run_cycle(1'b1, 16'hBEEF, 1'b0);
    check_value(word0_raw, 16'hCA95, "directed C word0_raw");
    check_value(word1_raw, 16'hBEEF, "directed C word1_raw");
    check_value(instr_opcode, 4'hC, "directed C opcode");

    // Directed opcode 0xD pair.
    run_cycle(1'b1, 16'hDFC1, 1'b0);
    check_value(rf_read_a_address, 9'h03F, "directed D read_a");
    check_value(rf_read_b_address, 9'h001, "directed D read_b");
    run_cycle(1'b1, 16'h1234, 1'b0);
    check_value(word0_raw, 16'hDFC1, "directed D word0_raw");
    check_value(word1_raw, 16'h1234, "directed D word1_raw");
    check_value(instr_opcode, 4'hD, "directed D opcode");

    // Directed opcode 0xE pair: Fa must be read from word[7:2], not [11:6],
    // and the X field occupies word[11:8] with kind in word[1:0].
    run_cycle(1'b1, 16'hE5EB, 1'b0);
    check_value(rf_read_a_address, 9'h03A, "directed E read_a word[7:2]");
    check_value(rf_read_b_address, 9'h02B, "directed E read_b word[5:0]");
    run_cycle(1'b1, 16'hABCD, 1'b0);
    check_value(word0_raw, 16'hE5EB, "directed E word0_raw");
    check_value(word1_raw, 16'hABCD, "directed E word1_raw");
    check_value(instr_opcode, 4'hE, "directed E opcode");

    // Back-to-back instructions with no idle cycle between pairs. Exactly one
    // complete pulse must appear for each of the three word1 beats.
    base_pulses = complete_pulses;
    run_cycle(1'b1, 16'hC0F0, 1'b0);
    run_cycle(1'b1, 16'h0001, 1'b0);
    run_cycle(1'b1, 16'hD0F0, 1'b0);
    run_cycle(1'b1, 16'h0002, 1'b0);
    run_cycle(1'b1, 16'hE0F3, 1'b0);
    run_cycle(1'b1, 16'h0003, 1'b0);
    check_value(complete_pulses - base_pulses, 3, "back-to-back pulses");

    // abort with no word while waiting for word1 discards the pending word0.
    run_cycle(1'b1, 16'hC123, 1'b0);
    run_cycle(1'b0, 16'h0000, 1'b1);
    run_cycle(1'b1, 16'hD456, 1'b0);
    check_value(rf_read_a_address, 9'h011, "abort recovery read_a");
    run_cycle(1'b1, 16'h789A, 1'b0);
    check_value(word0_raw, 16'hD456, "abort recovery word0_raw");
    check_value(word1_raw, 16'h789A, "abort recovery word1_raw");
    check_value(instr_opcode, 4'hD, "abort recovery opcode");

    // abort that coincides with a would-be word1 must discard that word too.
    run_cycle(1'b1, 16'hE999, 1'b0);
    run_cycle(1'b1, 16'hAAAA, 1'b1);
    run_cycle(1'b1, 16'hC777, 1'b0);
    check_value(rf_read_a_address, 9'h01D, "abort-with-word1 read_a");
    run_cycle(1'b1, 16'h8888, 1'b0);
    check_value(word0_raw, 16'hC777, "abort-with-word1 word0_raw");
    check_value(instr_opcode, 4'hC, "abort-with-word1 opcode");

    // Random traffic: 2000 random word pairs with randomly inserted aborts.
    for (i = 0; i < 2000; i = i + 1) begin
        w0 = $random(seed);
        w1 = $random(seed);
        run_cycle(1'b1, w0, $random(seed) & 1'b1);
        if (($random(seed) % 4) == 0)
            run_cycle(1'b1, w1, 1'b1);
        else
            run_cycle(1'b1, w1, 1'b0);
    end

    if (check_count == 0) begin
        $display("DIGITAL_DESIGN_FAIL: no checks executed");
        $finish;
    end
    if (complete_pulses == 0) begin
        $display("DIGITAL_DESIGN_FAIL: instr_complete never pulsed");
        $finish;
    end
    $display("checks=%0d", check_count);
    $display("instr_complete_pulses=%0d", complete_pulses);
    $display("DIGITAL_DESIGN_PASS");
    $finish;
end
endmodule
