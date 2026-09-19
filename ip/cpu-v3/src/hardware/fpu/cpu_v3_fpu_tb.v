// Testbench for the FPU v2 unit top level.
//
// It drives only the top-level ports: the two-word instruction stream plus the
// external memory channel (ext_*) and abort. It never reaches into the leaves.
// End-to-end behaviour is checked as:
//   - the external channel loads F registers (the FLD role) and reads them
//     back (the FST role),
//   - scalar instructions stream through the unit and their register writeback
//     is read back through the external channel and compared against an
//     independent Q16.16 reference model,
//   - the CMP flags are sampled on the scalar path's T1 beat,
//   - the ext_access non-overlap contract is monitored on every edge,
//   - abort is sampled at T0 and T1 and must leave the register file intact.
// A random phase runs more than 1000 "ext write several registers -> random
// scalar instruction -> ext readback compare" sequences.
module tb;
reg clk = 0;
reg word_valid = 0;
reg [15:0] word = 0;
reg abort = 0;
reg ext_access = 0;
reg ext_write_enable = 0;
reg [8:0] ext_write_address = 0;
reg [31:0] ext_write_data = 0;
reg [8:0] ext_read_address = 0;

wire busy;
wire flag_lt;
wire flag_eq;
wire flag_gt;
wire instr_complete;
wire [31:0] ext_read_data;

CpuV3Fpu dut (.*);

always #5 clk = ~clk;

localparam integer MAX_CYCLES = 2000000;
integer cycles = 0;
always @(posedge clk) begin
    cycles <= cycles + 1;
    if (cycles > MAX_CYCLES) begin
        $display("DIGITAL_DESIGN_FAIL: cycle limit exceeded");
        $finish;
    end
end

// The core-side contract: ext_access is never asserted while the unit is busy.
// This monitor fails the run on any violation, so the random sequence is also
// a check of the non-overlap contract.
always @(posedge clk) begin
    if (ext_access && busy) begin
        $display("DIGITAL_DESIGN_FAIL: ext_access asserted while busy");
        $finish;
    end
end

// Independent reference model: register contents and the expected writeback.
reg [31:0] ref_mem [0:511];
integer check_count = 0;
integer seed = 32'h0BADF00D;
integer i;
integer j;
integer n;
reg [31:0] rd_value;
reg [5:0] rfa;
reg [5:0] rfb;
reg [5:0] rfd;
reg [3:0] rop;

function [31:0] ref_floor;
    input [31:0] x;
    begin
        ref_floor = {x[31:16], 16'h0000};
    end
endfunction

// Maps an arbitrary word onto the twelve scalar ALU subops exercised here.
// CMP (0xB) is included so the flag path is hit by the random phase too.
function [3:0] op_from_index;
    input [31:0] r;
    begin
        case (r % 12)
            0: op_from_index = 4'h0;
            1: op_from_index = 4'h1;
            2: op_from_index = 4'h3;
            3: op_from_index = 4'h4;
            4: op_from_index = 4'h5;
            5: op_from_index = 4'h6;
            6: op_from_index = 4'h7;
            7: op_from_index = 4'h8;
            8: op_from_index = 4'h9;
            9: op_from_index = 4'hA;
            10: op_from_index = 4'hB;
            default: op_from_index = 4'hF;
        endcase
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

// Independent Q16.16 reference for one scalar op. ADD/SUB rely on the 32-bit
// register width for wrap semantics, matching the ALU overflow policy.
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

// Writes one 32-bit F value through the external channel (the FLD role). The
// reference model is updated at the commit edge.
task ext_write;
    input [8:0] addr;
    input [31:0] val;
    begin
        if (busy !== 1'b0)
            fail("ext_write while busy");
        @(negedge clk);
        ext_access = 1'b1;
        ext_write_enable = 1'b1;
        ext_write_address = addr;
        ext_write_data = val;
        @(posedge clk);
        ref_mem[addr] = val;
        @(negedge clk);
        ext_write_enable = 1'b0;
    end
endtask

// Reads one 32-bit F value through the external channel (the FST role) and
// compares it against the reference model.
task ext_read_check;
    input [8:0] addr;
    input [31:0] want;
    input [8*48-1:0] label;
    begin
        if (busy !== 1'b0)
            fail("ext_read while busy");
        @(negedge clk);
        ext_access = 1'b1;
        ext_read_address = addr;
        @(posedge clk);
        #1;
        check_value(ext_read_data, want, label);
        @(negedge clk);
    end
endtask

// Returns the unit to core control (idle) after an external burst.
task ext_release;
    begin
        @(negedge clk);
        ext_access = 1'b0;
        ext_write_enable = 1'b0;
    end
endtask

// Streams one opcode-0xD word pair and checks the fixed T0/T1/T2 window on the
// top-level outputs. CMP flags are sampled at T1. The independently computed
// writeback is folded into the reference model on a non-CMP op.
task run_scalar;
    input [5:0] fa;
    input [5:0] fb;
    input [5:0] fd;
    input [3:0] subop;
    input [3:0] mode;
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
        alu_reference(a, b, subop, result);

        @(negedge clk);
        ext_access = 1'b0;
        ext_write_enable = 1'b0;
        word_valid = 1'b1;
        word = {4'hD, fa, fb};
        abort = 1'b0;
        @(negedge clk);
        word = {fd, 2'b00, subop, mode};
        @(negedge clk);
        word_valid = 1'b0;
        word = 16'h0000;
        #1;
        // T0: the instruction has completed and is in flight.
        check_value({31'b0, busy}, 32'h1, "T0 busy");
        check_value({31'b0, instr_complete}, 32'h1, "T0 instr_complete");
        @(posedge clk);
        #1;
        // T1: writeback (except CMP) and the CMP flag capture.
        check_value({31'b0, busy}, 32'h1, "T1 busy");
        if (is_cmp) begin
            check_value({31'b0, flag_lt}, (sa < sb) ? 32'h1 : 32'h0,
                "T1 CMP flag_lt");
            check_value({31'b0, flag_eq}, (sa == sb) ? 32'h1 : 32'h0,
                "T1 CMP flag_eq");
            check_value({31'b0, flag_gt}, (sa > sb) ? 32'h1 : 32'h0,
                "T1 CMP flag_gt");
        end
        @(posedge clk);
        #1;
        // T2: the path is idle again.
        check_value({31'b0, busy}, 32'h0, "T2 busy");
        check_value({31'b0, instr_complete}, 32'h0, "T2 instr_complete");
        if (!is_cmp)
            ref_mem[fd] = result;
    end
endtask

// Streams a scalar ADD and aborts on T0: nothing may be captured.
task run_abort_t0;
    input [5:0] fa;
    input [5:0] fb;
    input [5:0] fd;
    input [3:0] subop;
    begin
        @(negedge clk);
        ext_access = 1'b0;
        ext_write_enable = 1'b0;
        word_valid = 1'b1;
        word = {4'hD, fa, fb};
        abort = 1'b0;
        @(negedge clk);
        word = {fd, 2'b00, subop, 4'h0};
        @(negedge clk);
        word_valid = 1'b0;
        word = 16'h0000;
        abort = 1'b1; // T0, before the capture edge
        #1;
        check_value({31'b0, busy}, 32'h0, "abort T0 busy clear");
        @(posedge clk);
        #1;
        check_value({31'b0, busy}, 32'h0, "abort T0 busy after edge");
        abort = 1'b0;
    end
endtask

// Streams a scalar ADD and aborts during T1: the already captured result must
// be suppressed at the writeback edge.
task run_abort_t1;
    input [5:0] fa;
    input [5:0] fb;
    input [5:0] fd;
    input [3:0] subop;
    begin
        @(negedge clk);
        ext_access = 1'b0;
        ext_write_enable = 1'b0;
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
        // T1: a pending writeback exists.
        check_value({31'b0, busy}, 32'h1, "abort T1 busy pending");
        abort = 1'b1;
        @(posedge clk);
        #1;
        check_value({31'b0, busy}, 32'h0, "abort T1 busy clear");
        abort = 1'b0;
    end
endtask

initial begin
    for (i = 0; i < 512; i = i + 1)
        ref_mem[i] = 32'h00000000;

    @(negedge clk);

    // Bring-up: load all 64 architectural registers through the external
    // channel (the FLD role) and read all of them back (the FST role).
    for (i = 0; i < 64; i = i + 1)
        ext_write(i[8:0], $random(seed));
    for (i = 0; i < 64; i = i + 1)
        ext_read_check(i[8:0], ref_mem[i], "ext init readback");
    ext_release();

    // Directed end-to-end: external operands, scalar instruction, external
    // readback. ADD, SUB and CMP are explicit; the remaining scalar ops are
    // covered by the random phase below (and in detail by the scalar path TB).
    ext_write(9'd0, 32'h00018000);
    ext_write(9'd1, 32'h00024000);
    run_scalar(6'd0, 6'd1, 6'd2, 4'h0, 4'h0);
    ext_read_check(9'd2, ref_mem[2], "directed ADD readback");

    ext_write(9'd3, 32'h00050000);
    ext_write(9'd4, 32'h00020000);
    run_scalar(6'd3, 6'd4, 6'd5, 4'h1, 4'h0);
    ext_read_check(9'd5, ref_mem[5], "directed SUB readback");

    ext_write(9'd6, 32'hFFFFFFFF);
    ext_write(9'd7, 32'hFFFFFFFF);
    run_scalar(6'd6, 6'd7, 6'd8, 4'hB, 4'h0);
    ext_read_check(9'd8, ref_mem[8], "directed CMP leaves F8");

    ext_write(9'd9, 32'h80000000);
    ext_write(9'd10, 32'h7FFFFFFF);
    run_scalar(6'd9, 6'd10, 6'd11, 4'hB, 4'h0);
    ext_read_check(9'd11, ref_mem[11], "directed CMP leaves F11");
    ext_release();

    // abort cancellation must leave the destination register untouched.
    ext_write(9'd20, 32'h12345678);
    run_abort_t0(6'd0, 6'd1, 6'd20, 4'h0);
    ext_read_check(9'd20, 32'h12345678, "abort T0 leaves F20");

    ext_write(9'd21, 32'h89ABCDEF);
    run_abort_t1(6'd0, 6'd1, 6'd21, 4'h0);
    ext_read_check(9'd21, 32'h89ABCDEF, "abort T1 leaves F21");
    ext_release();

    // Random end-to-end sequences: external writes, one random scalar
    // instruction, external readback. Each writeback is compared against the
    // reference model, and the edge monitor enforces the ext_access contract.
    for (i = 0; i < 1200; i = i + 1) begin
        n = 1 + ($random(seed) & 32'h00000003);
        for (j = 0; j < n; j = j + 1) begin
            rfa = $random(seed);
            ext_write({3'b000, rfa}, $random(seed));
        end
        ext_release();

        rfa = $random(seed);
        rfb = $random(seed);
        rfd = $random(seed);
        rop = op_from_index($random(seed));
        run_scalar(rfa, rfb, rfd, rop, 4'h0);
        ext_read_check({3'b000, rfd}, ref_mem[rfd], "random readback");
    end
    ext_release();

    if (check_count == 0)
        fail("no checks executed");
    $display("checks=%0d", check_count);
    $display("cycles=%0d", cycles);
    $display("DIGITAL_DESIGN_PASS");
    $finish;
end
endmodule
