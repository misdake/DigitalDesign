module tb;
reg clk = 0;
reg word_valid = 0;
reg [15:0] word = 0;
reg abort = 0;
always #5 clk = ~clk;

localparam integer MAX_CYCLES = 500000;
integer cycles = 0;
always @(posedge clk) begin
    cycles <= cycles + 1;
    if (cycles > MAX_CYCLES) begin
        $display("DIGITAL_DESIGN_FAIL: cycle limit exceeded");
        $finish;
    end
end

// This leaf testbench previously instantiated the real front-end and
// register-file leaves. That made the framework count them as physical
// children of the scalar path (double BSRAM claims in the system build:
// the same leaf was claimed through the unit top and again here). They are
// now behavioral stubs inside this TB; the full leaf interconnection is
// covered by the CpuV3FpuV2 unit testbench instead.

// Front-end stub: two-word acceptance with the leaf's exact contract
// (read_valid during the word0 beat, instr_complete one cycle after the
// word1 beat, abort discards a pending word0).
wire read_valid;
reg [8:0] hold_a = 0;
reg [8:0] hold_b = 0;
reg fe_waiting_word1 = 0;
reg [3:0] instr_opcode = 0;
reg [15:0] word0_raw = 0;
reg [15:0] word1_raw = 0;
reg instr_complete = 0;
always @(posedge clk) begin
    instr_complete <= word_valid && fe_waiting_word1 && !abort;
    if (word_valid && !fe_waiting_word1) begin
        fe_waiting_word1 <= 1'b1;
        instr_opcode <= word[15:12];
        word0_raw <= word;
        hold_a <= {3'b000, word[11:6]};
        hold_b <= {3'b000, word[5:0]};
    end else if ((word_valid && fe_waiting_word1 && !abort) ||
                 (abort && fe_waiting_word1)) begin
        fe_waiting_word1 <= 1'b0;
    end
    if (word_valid && fe_waiting_word1 && !abort)
        word1_raw <= word;
end
assign read_valid = word_valid && !fe_waiting_word1;

// Register-file write port: the testbench drives it during the preparation
// phase (initial register load), the scalar path drives it during execution.
reg prep_write_enable = 0;
reg [8:0] prep_write_address = 0;
reg [31:0] prep_write_data = 0;

// Register-file read port: the held operand address during execution, a
// testbench-driven address during preparation and readback.
reg rb_sel = 0;
reg [8:0] rb_a = 0;
reg [8:0] rb_b = 0;

wire [8:0] rf_read_a_address = rb_sel ? rb_a : hold_a;
wire [8:0] rf_read_b_address = rb_sel ? rb_b : hold_b;
reg [31:0] rf_read_a_data = 0;
reg [31:0] rf_read_b_data = 0;
wire sp_write_enable;
wire [8:0] sp_write_address;
wire [31:0] sp_write_data;
wire flag_lt;
wire flag_eq;
wire flag_gt;
wire busy;
wire [3:0] sp_r_wait;
wire [3:0] sp_w_wait;
wire [3:0] sp_x_wait;

wire rf_write_enable = prep_write_enable ? 1'b1 : sp_write_enable;
wire [8:0] rf_write_address = prep_write_enable ? prep_write_address : sp_write_address;
wire [31:0] rf_write_data = prep_write_enable ? prep_write_data : sp_write_data;

// Register-file stub: one synchronous write port (testbench preparation or
// the scalar path), two synchronous read ports with read-first semantics.
reg [31:0] rf_mem [0:511];
integer rf_init;
always @(posedge clk) begin
    if (rf_write_enable)
        rf_mem[rf_write_address] <= rf_write_data;
    rf_read_a_data <= rf_mem[rf_read_a_address];
    rf_read_b_data <= rf_mem[rf_read_b_address];
end
initial begin
    for (rf_init = 0; rf_init < 512; rf_init = rf_init + 1)
        rf_mem[rf_init] = 32'b0;
end

CpuV3FpuV2ScalarPath scalar_path (
    .clk(clk),
    .abort(abort),
    .instr_complete(instr_complete),
    .instr_opcode(instr_opcode),
    .word1_raw(word1_raw),
    .rf_read_a_data(rf_read_a_data),
    .rf_read_b_data(rf_read_b_data),
    .rf_write_enable(sp_write_enable),
    .rf_write_address(sp_write_address),
    .rf_write_data(sp_write_data),
    .flag_lt(flag_lt),
    .flag_eq(flag_eq),
    .flag_gt(flag_gt),
    .busy(busy),
    .r_wait(sp_r_wait),
    .w_wait(sp_w_wait),
    .x_wait(sp_x_wait)
);

// Independent reference model: register contents and the CMP flags. It follows
// exactly the same wrap-only Q16.16 rules as the ALU leaf, but is written out
// separately so the comparison does not reuse the implementation.
reg [31:0] ref_mem [0:511];
integer ref_flag_lt = 0;
integer ref_flag_eq = 0;
integer ref_flag_gt = 0;
integer check_count = 0;
integer i;
integer seed = 32'h0BADF00D;
reg [31:0] rd_value;
reg [5:0] rfa;
reg [5:0] rfb;
reg [5:0] rfd;
reg [3:0] rop;
reg [31:0] result_a;
reg [31:0] result_b;

function [31:0] ref_floor;
    input [31:0] x;
    begin
        ref_floor = {x[31:16], 16'h0000};
    end
endfunction

task fail;
    input [8*96-1:0] msg;
    begin
        $display("DIGITAL_DESIGN_FAIL: %0s", msg);
        $finish;
    end
endtask

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

// Clears the reference model, then loads every architectural F register
// through the RF write port. This is the testbench preparation phase; the
// scalar path is idle and therefore never drives the write port here.
task prepare_registers;
    integer k;
    begin
        for (k = 0; k < 512; k = k + 1)
            ref_mem[k] = 32'h00000000;
        for (k = 0; k < 64; k = k + 1)
            ref_mem[k] = $random(seed);
        for (k = 0; k < 64; k = k + 1) begin
            @(negedge clk);
            prep_write_enable = 1'b1;
            prep_write_address = k[8:0];
            prep_write_data = ref_mem[k];
        end
        @(negedge clk);
        prep_write_enable = 1'b0;
        @(negedge clk);
    end
endtask

// Overwrites one architectural register through the RF write port and keeps
// the reference model in step. Used to stage directed boundary operands.
task set_reg;
    input [8:0] addr;
    input [31:0] value;
    begin
        @(negedge clk);
        prep_write_enable = 1'b1;
        prep_write_address = addr;
        prep_write_data = value;
        @(posedge clk);
        ref_mem[addr] = value;
        @(negedge clk);
        prep_write_enable = 1'b0;
    end
endtask

// Reads one register through RF port A and leaves the word in rd_value.
task read_reg;
    input [8:0] addr;
    begin
        @(negedge clk);
        rb_sel = 1'b1;
        rb_a = addr;
        rb_b = addr;
        @(posedge clk);
        #1;
        rd_value = rf_read_a_data;
        rb_sel = 1'b0;
    end
endtask

// Independent result model for one scalar op.
task alu_reference;
    input [31:0] a;
    input [31:0] b;
    input [3:0] op;
    output [31:0] result;
    reg [31:0] fl;
    reg [31:0] ce;
    reg signed [31:0] sa;
    reg signed [31:0] sb;
    begin
        sa = a;
        sb = b;
        fl = ref_floor(a);
        ce = fl + ((a[15:0] != 16'h0000) ? 32'h00010000 : 32'h00000000);
        case (op)
            4'h0: result = a + b;
            4'h1: result = a - b;
            4'h3: result = (sa < sb) ? a : b;
            4'h4: result = (sa > sb) ? a : b;
            4'h5: result = a[31] ? (32'h00000000 - a) : a;
            4'h6: result = 32'h00000000 - a;
            4'h7: result = fl;
            4'h8: result = ce;
            4'h9: result = ref_floor(a + 32'h00008000);
            4'hA: result = a[31] ? ce : fl;
            4'hB: result = 32'h00000000;
            4'hF: result = a;
            default: result = 32'h00000000;
        endcase
    end
endtask

// Streams one opcode-0xD word pair through the front-end, then waits for the
// fixed T0/T1/T2 window. It checks the countdowns, the flags, the write enable
// and finally reads the destination register back against the reference model.
task run_scalar;
    input [5:0] fa;
    input [5:0] fb;
    input [5:0] fd;
    input [3:0] subop;
    input [3:0] mode;
    reg [15:0] w0;
    reg [15:0] w1;
    reg [31:0] a;
    reg [31:0] b;
    reg [31:0] result;
    reg signed [31:0] sa;
    reg signed [31:0] sb;
    reg is_cmp;
    begin
        a = ref_mem[fa];
        b = ref_mem[fb];
        sa = a;
        sb = b;
        is_cmp = (subop == 4'hB);
        // subop 0x02 (MUL) is owned by the multiply path: the scalar path
        // does not fire at all (no write, no countdown).
        alu_reference(a, b, subop, result);

        w0 = {4'hD, fa, fb};
        // word1 layout: Fd[15:10], 6-bit subop [9:4], mode [3:0]
        w1 = {fd, 2'b00, subop, mode};

        @(negedge clk);
        word_valid = 1'b1;
        word = w0;
        abort = 1'b0;
        @(negedge clk);
        word = w1;
        @(negedge clk);
        word_valid = 1'b0;
        word = 16'h0000;
        // The instr_complete pulse is high in the cycle that just started (T0).
        #1;
        // MUL (subop 0x02) does not fire the scalar path at all.
        check_value(busy, (subop == 4'h2) ? 1'b0 : 1'b1, "T0 busy");
        check_value(sp_w_wait, (subop == 4'h2) ? 4'd0 : 4'd2, "T0 w_wait");
        check_value(sp_x_wait, (subop == 4'h2) ? 4'd0 : 4'd2, "T0 x_wait");
        check_value(sp_r_wait, 4'd0, "T0 r_wait");

        @(posedge clk);
        #1;
        // T1: writeback (except CMP) and the decremented countdown.
        if (is_cmp) begin
            check_value({31'b0, flag_lt}, (sa < sb) ? 32'h1 : 32'h0, "T1 CMP flag_lt");
            check_value({31'b0, flag_eq}, (sa == sb) ? 32'h1 : 32'h0, "T1 CMP flag_eq");
            check_value({31'b0, flag_gt}, (sa > sb) ? 32'h1 : 32'h0, "T1 CMP flag_gt");
            ref_flag_lt = (sa < sb);
            ref_flag_eq = (sa == sb);
            ref_flag_gt = (sa > sb);
        end else begin
            check_value({31'b0, flag_lt}, ref_flag_lt, "T1 flag_lt holds");
            check_value({31'b0, flag_eq}, ref_flag_eq, "T1 flag_eq holds");
            check_value({31'b0, flag_gt}, ref_flag_gt, "T1 flag_gt holds");
        end
        check_value({31'b0, sp_write_enable}, (is_cmp || subop == 4'h2) ? 32'h0 : 32'h1,
            "T1 write enable");
        check_value(sp_w_wait, (subop == 4'h2) ? 4'd0 : 4'd1, "T1 w_wait");
        check_value(busy, (subop == 4'h2) ? 1'b0 : 1'b1, "T1 busy");

        @(posedge clk);
        #1;
        // T2: the write has committed and the path is idle again.
        check_value({31'b0, sp_write_enable}, 32'h0, "T2 write disabled");
        check_value(sp_w_wait, 4'd0, "T2 w_wait");
        check_value(sp_x_wait, 4'd0, "T2 x_wait");
        check_value(busy, 1'b0, "T2 busy");

        if (!is_cmp && subop != 4'h2)
            ref_mem[fd] = result;
        read_reg({3'b000, fd});
        if (is_cmp || subop == 4'h2)
            check_value(rd_value, ref_mem[fd], "CMP/MUL leave RF unchanged here");
        else
            check_value(rd_value, ref_mem[fd], "writeback readback");
    end
endtask

// Streams a scalar pair and aborts on the T0 beat: the result must not be
// captured and the destination register must stay unchanged.
task run_scalar_abort_t0;
    input [5:0] fa;
    input [5:0] fb;
    input [5:0] fd;
    input [3:0] subop;
    begin
        @(negedge clk);
        word_valid = 1'b1;
        word = {4'hD, fa, fb};
        abort = 1'b0;
        @(negedge clk);
        word = {fd, 2'b00, subop, 4'h0};
        @(negedge clk);
        word_valid = 1'b0;
        word = 16'h0000;
        abort = 1'b1; // T0, cancel before the capture edge
        #1;
        check_value(busy, 1'b0, "abort T0 busy clear");
        check_value(sp_w_wait, 4'd0, "abort T0 w_wait clear");
        check_value(sp_x_wait, 4'd0, "abort T0 x_wait clear");
        check_value({31'b0, sp_write_enable}, 32'h0, "abort T0 write disabled");
        @(posedge clk);
        #1;
        check_value(busy, 1'b0, "abort T0 busy after edge");
        abort = 1'b0;
        read_reg({3'b000, fd});
        check_value(rd_value, ref_mem[fd], "abort T0 leaves RF unchanged");
    end
endtask

// Streams a scalar pair and aborts during T1: the already captured result must
// be suppressed at the writeback edge.
task run_scalar_abort_t1;
    input [5:0] fa;
    input [5:0] fb;
    input [5:0] fd;
    input [3:0] subop;
    begin
        @(negedge clk);
        word_valid = 1'b1;
        word = {4'hD, fa, fb};
        abort = 1'b0;
        @(negedge clk);
        word = {fd, 2'b00, subop, 4'h0};
        @(negedge clk);
        word_valid = 1'b0;
        word = 16'h0000;
        @(posedge clk);
        #1;
        // We are in T1 with a pending captured result.
        check_value({31'b0, sp_write_enable}, 32'h1, "abort T1 write pending");
        abort = 1'b1;
        @(posedge clk);
        #1;
        check_value({31'b0, sp_write_enable}, 32'h0, "abort T1 write gated");
        check_value(busy, 1'b0, "abort T1 busy clear");
        abort = 1'b0;
        read_reg({3'b000, fd});
        check_value(rd_value, ref_mem[fd], "abort T1 leaves RF unchanged");
    end
endtask

initial begin
    prepare_registers();

    // Directed end-to-end coverage: one instruction per scalar subop. The
    // operands come from the prepared register file; the reference model
    // computes the expected writeback and flags independently.
    run_scalar(6'd1, 6'd2, 6'd3, 4'h0, 4'h0);      // ADD
    run_scalar(6'd4, 6'd5, 6'd6, 4'h1, 4'h0);      // SUB
    run_scalar(6'd7, 6'd8, 6'd9, 4'h3, 4'h0);      // MIN
    run_scalar(6'd10, 6'd11, 6'd12, 4'h4, 4'h0);   // MAX
    run_scalar(6'd13, 6'd0, 6'd14, 4'h5, 4'h0);    // ABS
    run_scalar(6'd15, 6'd0, 6'd16, 4'h6, 4'h0);    // NEG
    run_scalar(6'd17, 6'd0, 6'd18, 4'h7, 4'h0);    // FLOOR
    run_scalar(6'd19, 6'd0, 6'd20, 4'h8, 4'h0);    // CEIL
    run_scalar(6'd21, 6'd0, 6'd22, 4'h9, 4'h0);    // ROUND
    run_scalar(6'd23, 6'd0, 6'd24, 4'hA, 4'h0);    // TRUNC
    run_scalar(6'd25, 6'd0, 6'd26, 4'hF, 4'h0);    // MOV
    run_scalar(6'd27, 6'd28, 6'd29, 4'hB, 4'h0);   // CMP

    // Directed register operands that force ALU boundary behaviour.
    set_reg(9'd32, 32'h00018000);
    set_reg(9'd33, 32'hFFFF8000);
    set_reg(9'd34, 32'hFFFFFFFF);
    run_scalar(6'd32, 6'd33, 6'd35, 4'h0, 4'h0);   // ADD with a negative fraction
    run_scalar(6'd34, 6'd0, 6'd36, 4'h7, 4'h0);    // FLOOR of -1
    run_scalar(6'd33, 6'd0, 6'd37, 4'h9, 4'h0);    // ROUND of -0.5 (half up)
    run_scalar(6'd33, 6'd0, 6'd38, 4'hA, 4'h0);    // TRUNC of -0.5
    run_scalar(6'd34, 6'd34, 6'd39, 4'hB, 4'h0);   // CMP equal -> eq

    // Back-to-back pair with no idle beat between the two word pairs. The two
    // instructions touch disjoint registers, so there is no RAW hazard.
    begin
        alu_reference(ref_mem[1], ref_mem[2], 4'h0, result_a);
        alu_reference(ref_mem[4], ref_mem[5], 4'h1, result_b);
        @(negedge clk);
        word_valid = 1'b1;
        word = {4'hD, 6'd1, 6'd2};
        abort = 1'b0;
        @(negedge clk);
        word = {6'd3, 6'h00, 4'h0};   // F3 = F1 + F2
        @(negedge clk);
        word = {4'hD, 6'd4, 6'd5};
        @(negedge clk);
        word = {6'd6, 6'h01, 4'h0};   // F6 = F4 - F5
        @(negedge clk);
        word_valid = 1'b0;
        word = 16'h0000;
        repeat (4) @(posedge clk);
        ref_mem[3] = result_a;
        ref_mem[6] = result_b;
        read_reg(9'd3);
        check_value(rd_value, ref_mem[3], "back-to-back first write");
        read_reg(9'd6);
        check_value(rd_value, ref_mem[6], "back-to-back second write");
    end

    // abort cancellation, on T0 and on T1.
    run_scalar_abort_t0(6'd1, 6'd2, 6'd40, 4'h0);
    run_scalar_abort_t1(6'd1, 6'd2, 6'd41, 4'h0);

    // Random scalar traffic: 2000 random subop/Fa/Fb/Fd instructions, each
    // checked against the reference model for countdowns, flags and RF content.
    for (i = 0; i < 2000; i = i + 1) begin
        rfa = $random(seed);
        rfb = $random(seed);
        rfd = $random(seed);
        rop = $random(seed);
        run_scalar(rfa, rfb, rfd, rop, $random(seed));
    end
    @(negedge clk);

    // Final full-register sweep against the reference model, plus a final flag
    // comparison.
    for (i = 0; i < 64; i = i + 1) begin
        read_reg(i[8:0]);
        check_value(rd_value, ref_mem[i], "final register sweep");
    end
    check_value({31'b0, flag_lt}, ref_flag_lt, "final flag_lt");
    check_value({31'b0, flag_eq}, ref_flag_eq, "final flag_eq");
    check_value({31'b0, flag_gt}, ref_flag_gt, "final flag_gt");

    if (check_count == 0)
        fail("no checks executed");
    $display("checks=%0d", check_count);
    $display("cycles=%0d", cycles);
    $display("DIGITAL_DESIGN_PASS");
    $finish;
end
endmodule
