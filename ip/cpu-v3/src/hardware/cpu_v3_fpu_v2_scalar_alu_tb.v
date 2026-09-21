module tb;
reg [31:0] a = 0;
reg [31:0] b = 0;
reg [3:0] op = 0;
wire [31:0] result;
wire flag_lt;
wire flag_eq;
wire flag_gt;

CpuV3FpuV2ScalarAlu dut(.*);

// The ALU is purely combinational, so there is no clock and the testbench
// drives inputs with #delay instead. A step counter still enforces the
// mandatory upper bound on simulation work.
localparam integer MAX_STEPS = 20000;
integer steps = 0;
integer check_count = 0;
integer i;
integer seed = 32'h0BADF00D;
reg [31:0] ra;
reg [31:0] rb;
reg [3:0] rop;

// Independent Q16.16 reference model. ADD and SUB rely on the 32-bit reg width
// for wrap semantics, matching the wrap-only overflow policy.
function [31:0] ref_floor;
    input [31:0] x;
    begin
        ref_floor = {x[31:16], 16'h0000};
    end
endfunction

task fail;
    input [8*80-1:0] msg;
    begin
        $display("DIGITAL_DESIGN_FAIL: %0s", msg);
        $finish;
    end
endtask

// Drives one (a, b, op) stimulus, samples the combinational outputs #1 later,
// and compares them against the reference model. Every reference computation is
// checked, including the flags whenever the op is CMP.
task check_alu;
    input [31:0] av;
    input [31:0] bv;
    input [3:0] ov;
    reg [31:0] want;
    reg [31:0] fl;
    reg [31:0] ce;
    reg [31:0] rnd;
    reg [31:0] tr;
    reg signed [31:0] sa;
    reg signed [31:0] sb;
    begin
        if (steps >= MAX_STEPS)
            fail("step limit exceeded");
        steps = steps + 1;

        a = av;
        b = bv;
        op = ov;
        #1;

        check_count = check_count + 1;
        sa = av;
        sb = bv;

        fl = ref_floor(av);
        ce = fl + ((av[15:0] != 16'h0000) ? 32'h00010000 : 32'h00000000);
        rnd = ref_floor(av + 32'h00008000);
        tr = av[31] ? ce : fl;

        case (ov)
            4'h0: want = av + bv;
            4'h1: want = av - bv;
            4'h3: want = (sa < sb) ? av : bv;
            4'h4: want = (sa > sb) ? av : bv;
            4'h5: want = av[31] ? (32'h00000000 - av) : av;
            4'h6: want = 32'h00000000 - av;
            4'h7: want = fl;
            4'h8: want = ce;
            4'h9: want = rnd;
            4'hA: want = tr;
            4'hB: want = 32'h00000000;
            4'hF: want = av;
            default: want = 32'h00000000;
        endcase

        if (result !== want) begin
            $display("DIGITAL_DESIGN_FAIL: op=%h a=%08h b=%08h got=%08h want=%08h",
                ov, av, bv, result, want);
            $finish;
        end

        // The flag outputs must never be X, even for the unlisted ops.
        if ((flag_lt === 1'bx) || (flag_eq === 1'bx) || (flag_gt === 1'bx))
            fail("flag output propagated X");

        if (ov == 4'hB) begin
            if ((flag_lt !== (sa < sb)) || (flag_eq !== (sa == sb)) ||
                (flag_gt !== (sa > sb))) begin
                $display("DIGITAL_DESIGN_FAIL: CMP flags a=%08h b=%08h got=%b/%b/%b",
                    av, bv, flag_lt, flag_eq, flag_gt);
                $finish;
            end
        end
    end
endtask

initial begin
    // Directed ADD, including wrap at both ends and the fractional carry.
    check_alu(32'h00010000, 32'h00020000, 4'h0);
    check_alu(32'hFFFFFFFF, 32'h00010000, 4'h0);
    check_alu(32'h80000000, 32'h80000000, 4'h0);
    check_alu(32'h7FFFFFFF, 32'h00000001, 4'h0);
    check_alu(32'h0000FFFF, 32'h00000001, 4'h0);
    check_alu(32'hFFFF8000, 32'h00008000, 4'h0);

    // Directed SUB, including wrap and the negative fractional boundary.
    check_alu(32'h00030000, 32'h00010000, 4'h1);
    check_alu(32'h00000000, 32'h00010000, 4'h1);
    check_alu(32'h80000000, 32'h00010000, 4'h1);
    check_alu(32'h7FFFFFFF, 32'hFFFFFFFF, 4'h1);
    check_alu(32'h00010000, 32'h0000FFFF, 4'h1);

    // Directed signed MIN.
    check_alu(32'h00010000, 32'hFFFFFFFF, 4'h3);
    check_alu(32'h80000000, 32'h7FFFFFFF, 4'h3);
    check_alu(32'h0000FFFF, 32'h00010000, 4'h3);
    check_alu(32'h00010000, 32'h00010000, 4'h3);
    check_alu(32'hFFFFFFFE, 32'hFFFFFFFF, 4'h3);

    // Directed signed MAX.
    check_alu(32'h00010000, 32'hFFFFFFFF, 4'h4);
    check_alu(32'h80000000, 32'h7FFFFFFF, 4'h4);
    check_alu(32'h0000FFFF, 32'h00010000, 4'h4);
    check_alu(32'h00010000, 32'h00010000, 4'h4);
    check_alu(32'hFFFFFFFE, 32'hFFFFFFFF, 4'h4);

    // Directed ABS: 0x80000000 must stay itself under wrap.
    check_alu(32'h80000000, 32'h00000000, 4'h5);
    check_alu(32'hFFFFFFFF, 32'h00000000, 4'h5);
    check_alu(32'h0000FFFF, 32'h00000000, 4'h5);
    check_alu(32'h7FFFFFFF, 32'h00000000, 4'h5);
    check_alu(32'h00000000, 32'h00000000, 4'h5);

    // Directed NEG.
    check_alu(32'h00000000, 32'h00000000, 4'h6);
    check_alu(32'h00010000, 32'h00000000, 4'h6);
    check_alu(32'h80000000, 32'h00000000, 4'h6);
    check_alu(32'hFFFFFFFF, 32'h00000000, 4'h6);
    check_alu(32'h0000FFFF, 32'h00000000, 4'h6);

    // Directed FLOOR, including negative fractions and exact integers.
    check_alu(32'h0000FFFF, 32'h00000000, 4'h7);
    check_alu(32'h00010000, 32'h00000000, 4'h7);
    check_alu(32'hFFFFFFFF, 32'h00000000, 4'h7);
    check_alu(32'h80000000, 32'h00000000, 4'h7);
    check_alu(32'hFFFF7FFF, 32'h00000000, 4'h7);
    check_alu(32'h7FFFFFFF, 32'h00000000, 4'h7);

    // Directed CEIL, including exact-integer inputs that must not step up.
    check_alu(32'h00000000, 32'h00000000, 4'h8);
    check_alu(32'h00010000, 32'h00000000, 4'h8);
    check_alu(32'h00000001, 32'h00000000, 4'h8);
    check_alu(32'hFFFFFFFF, 32'h00000000, 4'h8);
    check_alu(32'h0000FFFF, 32'h00000000, 4'h8);
    check_alu(32'hFFFF7FFF, 32'h00000000, 4'h8);

    // Directed ROUND around the half-up boundary 0x00008000.
    check_alu(32'h00008000, 32'h00000000, 4'h9);
    check_alu(32'h00007FFF, 32'h00000000, 4'h9);
    check_alu(32'h00000000, 32'h00000000, 4'h9);
    check_alu(32'hFFFF8000, 32'h00000000, 4'h9);
    check_alu(32'hFFFF7FFF, 32'h00000000, 4'h9);
    check_alu(32'h7FFFFFFF, 32'h00000000, 4'h9);

    // Directed TRUNC, including negative fractions and negative integers.
    check_alu(32'h0000FFFF, 32'h00000000, 4'hA);
    check_alu(32'h00010000, 32'h00000000, 4'hA);
    check_alu(32'hFFFFFFFF, 32'h00000000, 4'hA);
    check_alu(32'hFFFF8000, 32'h00000000, 4'hA);
    check_alu(32'h80000000, 32'h00000000, 4'hA);
    check_alu(32'h8000FFFF, 32'h00000000, 4'hA);

    // Directed CMP, result forced to zero with signed flags.
    check_alu(32'h00010000, 32'h00020000, 4'hB);
    check_alu(32'h00020000, 32'h00010000, 4'hB);
    check_alu(32'h00010000, 32'h00010000, 4'hB);
    check_alu(32'h80000000, 32'h7FFFFFFF, 4'hB);
    check_alu(32'hFFFFFFFF, 32'h00000000, 4'hB);
    check_alu(32'hFFFFFFFF, 32'hFFFFFFFE, 4'hB);

    // Directed MOV.
    check_alu(32'h80000000, 32'h00000000, 4'hF);
    check_alu(32'hDEADBEEF, 32'h00000000, 4'hF);
    check_alu(32'h00000000, 32'h00000000, 4'hF);
    check_alu(32'h0000FFFF, 32'h00000000, 4'hF);

    // Unlisted op codes must still produce a defined (zero) result.
    check_alu(32'h12345678, 32'h87654321, 4'h2);
    check_alu(32'h12345678, 32'h87654321, 4'hD);
    check_alu(32'h12345678, 32'h87654321, 4'hE);

    // Random traffic: 6000 random (a, b, op) triples over the full op space.
    for (i = 0; i < 6000; i = i + 1) begin
        ra = $random(seed);
        rb = $random(seed);
        rop = $random(seed);
        check_alu(ra, rb, rop);
    end

    if (check_count == 0)
        fail("no checks executed");
    $display("checks=%0d", check_count);
    $display("DIGITAL_DESIGN_PASS");
    $finish;
end
endmodule
