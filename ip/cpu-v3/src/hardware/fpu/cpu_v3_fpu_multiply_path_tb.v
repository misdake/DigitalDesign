// Testbench for CpuV3FpuMultiplyPath.
//
// Following cpu_v3_fpu_vector_path_tb.v, the parent front-end and the
// register file are behavioral stubs inside this TB rather than instantiated
// leaves. This TB drives instr_complete / instr_opcode / word1_raw / base_a /
// base_b directly and owns a behavioral 2R1W register file, so the multiply
// path can be exercised without the front-end pair protocol.
//
// The reference model is written out separately from the RTL: it sign-extends
// nothing special because a 64-bit signed product already contains the bits
// that matter, and it narrows with an arithmetic right shift by 16
// (result = (a * b) >>> 16), which is exactly product[47:16] of the 72-bit
// widened product. The TB checks the per-cycle read addresses, the per-cycle
// write-enable window and write data, the busy-window length, the destination
// readback and the final register contents.
module tb;
reg clk = 0;
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

// Instruction inputs, driven directly by this TB.
reg instr_complete = 0;
reg [3:0] instr_opcode = 0;
reg [15:0] word1_raw = 0;
reg [5:0] base_a = 0;
reg [5:0] base_b = 0;
reg abort = 0;

// Register-file write port: preparation writes versus multiply-path writes.
reg prep_write_enable = 0;
reg [8:0] prep_write_address = 0;
reg [31:0] prep_write_data = 0;

// Register-file read port: TB readback mux versus the path's generated reads.
reg rb_sel = 0;
reg [8:0] rb_a = 0;
reg [8:0] rb_b = 0;

wire [8:0] mp_read_a_address;
wire [8:0] mp_read_b_address;
wire mp_write_enable;
wire [8:0] mp_write_address;
wire [31:0] mp_write_data;
wire busy;

wire [8:0] rf_read_a_address = rb_sel ? rb_a : mp_read_a_address;
wire [8:0] rf_read_b_address = rb_sel ? rb_b : mp_read_b_address;
wire rf_write_enable = prep_write_enable ? 1'b1 : mp_write_enable;
wire [8:0] rf_write_address =
    prep_write_enable ? prep_write_address : mp_write_address;
wire [31:0] rf_write_data =
    prep_write_enable ? prep_write_data : mp_write_data;
reg [31:0] rf_read_a_data = 0;
reg [31:0] rf_read_b_data = 0;

// Register-file stub: one synchronous write port, two synchronous read ports
// with read-first semantics.
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

// Shared-pipe stub: the real CpuV3FpuMulPipe lives in the unit top; this
// TB-local copy reproduces its exact 3-stage tag-carrying behavior so the
// leaf test needs no resource-claiming sibling modules (see the scalar-path
// TB for the same pattern).
wire mul_in_valid;
wire signed [35:0] mul_in_a;
wire signed [35:0] mul_in_b;
wire [8:0] mul_in_tag;
wire mul_out_valid;
wire signed [63:0] mul_out_product;
wire [8:0] mul_out_tag;

reg mul_s1_valid = 0;
reg signed [35:0] mul_s1_a = 0;
reg signed [35:0] mul_s1_b = 0;
reg [8:0] mul_s1_tag = 0;
reg mul_s2_valid = 0;
reg signed [71:0] mul_s2_prod = 0;
reg [8:0] mul_s2_tag = 0;
reg mul_s3_valid = 0;
reg signed [71:0] mul_s3_prod = 0;
reg [8:0] mul_s3_tag = 0;
always @(posedge clk) begin
    if (abort) begin
        mul_s1_valid <= 0; mul_s2_valid <= 0; mul_s3_valid <= 0;
    end else begin
        mul_s3_valid <= mul_s2_valid;
        if (mul_s2_valid) begin mul_s3_prod <= mul_s2_prod; mul_s3_tag <= mul_s2_tag; end
        mul_s2_valid <= mul_s1_valid;
        if (mul_s1_valid) begin mul_s2_prod <= mul_s1_a * mul_s1_b; mul_s2_tag <= mul_s1_tag; end
        mul_s1_valid <= mul_in_valid;
        if (mul_in_valid) begin
            mul_s1_a <= $signed(mul_in_a); mul_s1_b <= $signed(mul_in_b);
            mul_s1_tag <= mul_in_tag;
        end
    end
end
assign mul_out_valid = mul_s3_valid && !abort;
assign mul_out_product = mul_s3_prod[63:0];
assign mul_out_tag = mul_s3_tag;

CpuV3FpuMultiplyPath multiply_path (
    .clk(clk),
    .abort(abort),
    .instr_complete(instr_complete),
    .instr_opcode(instr_opcode),
    .word1_raw(word1_raw),
    .base_a(base_a),
    .base_b(base_b),
    .rf_read_a_data(rf_read_a_data),
    .rf_read_b_data(rf_read_b_data),
    .rf_read_a_address(mp_read_a_address),
    .rf_read_b_address(mp_read_b_address),
    .mul_in_valid(mul_in_valid),
    .mul_in_a(mul_in_a),
    .mul_in_b(mul_in_b),
    .mul_in_tag(mul_in_tag),
    .mul_out_valid(mul_out_valid),
    .mul_out_product(mul_out_product),
    .mul_out_tag(mul_out_tag),
    .rf_write_enable(mp_write_enable),
    .rf_write_address(mp_write_address),
    .rf_write_data(mp_write_data),
    .busy(busy)
);

// Reference model: register contents plus a per-instruction expected lane
// array. The Q16.16 multiply is independent of the RTL.
reg [31:0] ref_mem [0:511];
reg [31:0] exp_lane [0:3];
reg [31:0] old_lane [0:3];
integer check_count = 0;
integer i;
integer op;
integer seed = 32'h0BADF00D;
reg [31:0] rd_value;
reg [1:0] len_tab [0:2];
integer s;
integer l;

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
    input [8*64-1:0] label;
    begin
        check_count = check_count + 1;
        if (got !== want) begin
            $display("DIGITAL_DESIGN_FAIL: %0s got %08h want %08h",
                label, got, want);
            $finish;
        end
    end
endtask

// Independent result model for one Q16.16 multiply. A 64-bit signed product
// shifted right by 16 and truncated to 32 bits is product[47:16] of the
// 72-bit product the RTL forms.
task mul_ref_op;
    input [31:0] a;
    input [31:0] b;
    output [31:0] result;
    reg signed [63:0] prod;
    begin
        prod = $signed(a) * $signed(b);
        result = prod >>> 16;
    end
endtask

// Clears the reference model, then loads every architectural F register through
// the RF write port.
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

// Overwrites one architectural register through the RF write port and keeps the
// reference model in step.
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

// Issues one scalar MUL (opcode 0xD, subop 0x02) and checks the whole pipeline:
// the read window, the single writeback four beats later and the readback.
task run_scalar_mul;
    input [5:0] fa;
    input [5:0] fb;
    input [5:0] fd;
    integer t;
    integer busy_count;
    begin
        mul_ref_op(ref_mem[{3'b000, fa}], ref_mem[{3'b000, fb}], exp_lane[0]);

        @(negedge clk);
        instr_complete = 1'b1;
        instr_opcode = 4'hD;
        base_a = fa;
        base_b = fb;
        word1_raw = {fd, 6'h02, 4'b0000};
        #1;
        check_value({31'b0, busy}, 32'h1, "mul T0 busy");
        check_value({23'b0, mp_read_a_address}, fa, "mul T0 read A");
        check_value({23'b0, mp_read_b_address}, fb, "mul T0 read B");

        busy_count = 1;
        @(negedge clk);
        instr_complete = 1'b0;
        #1;

        t = 1;
        while (busy) begin
            check_value({31'b0, mp_read_a_address}, 32'h0, "mul read A idle");
            check_value({31'b0, mp_read_b_address}, 32'h0, "mul read B idle");
            if (t == 4) begin
                check_value({31'b0, mp_write_enable}, 32'h1, "mul write en");
                check_value({23'b0, mp_write_address}, fd, "mul write addr");
                check_value(mp_write_data, exp_lane[0], "mul write data");
            end else begin
                check_value({31'b0, mp_write_enable}, 32'h0, "mul write idle");
            end
            busy_count = busy_count + 1;
            t = t + 1;
            @(negedge clk);
        end
        check_value(busy_count, 5, "mul busy window");
        check_value(t, 5, "mul cycle count");
        check_value({31'b0, busy}, 32'h0, "mul busy clear");
        check_value({31'b0, mp_write_enable}, 32'h0, "mul write clear");

        ref_mem[{3'b000, fd}] = exp_lane[0];
        read_reg({3'b000, fd});
        check_value(rd_value, exp_lane[0], "mul readback");
    end
endtask

// Issues one vector multiply: vmuls selects VMULS (subop 0x03) rather than
// VMUL (subop 0x02). It checks the T0 lane-0 read addresses, the per-cycle
// read-address ramp, the per-cycle write-enable window and write data against
// the independent expected lane results, the busy window and the readback.
// One lane past the range is checked too, so a too-long vector is caught.
task run_vector_mul;
    input [5:0] fa;
    input [5:0] fb;
    input [5:0] fd;
    input [1:0] len_field;
    input vmuls;
    integer nlanes;
    integer last_lane;
    integer k;
    integer t;
    integer busy_count;
    reg [5:0] addr_a;
    reg [5:0] addr_b;
    reg [5:0] addr_d;
    begin
        // len 11 is reserved; the hardware clamps it to vec4.
        if (len_field == 2'b11) begin
            nlanes = 4;
            last_lane = 3;
        end else begin
            nlanes = len_field + 2;
            last_lane = len_field + 1;
        end

        // Expected results from the pre-instruction state, so an in-place
        // destination is modelled correctly.
        for (k = 0; k < nlanes; k = k + 1) begin
            addr_a = fa + k[5:0];
            addr_b = vmuls ? fb : (fb + k[5:0]);
            mul_ref_op(ref_mem[{3'b000, addr_a}], ref_mem[{3'b000, addr_b}],
                exp_lane[k]);
        end

        // Issue at the negedge so T0 is the cycle that follows.
        @(negedge clk);
        instr_complete = 1'b1;
        instr_opcode = 4'hC;
        base_a = fa;
        base_b = fb;
        word1_raw = {fd, len_field, vmuls ? 5'h03 : 5'h02, 3'b000};
        #1;
        check_value({31'b0, busy}, 32'h1, "T0 busy");
        check_value({23'b0, mp_read_a_address}, fa, "T0 read A lane0");
        check_value({23'b0, mp_read_b_address}, fb, "T0 read B lane0");

        busy_count = 1;
        @(negedge clk);
        instr_complete = 1'b0;
        #1; // let the deasserted instr_complete settle before sampling T1

        // T1 .. T(last_lane + 4): reads, then the three-stage product drain.
        t = 1;
        while (busy) begin
            if (t <= last_lane) begin
                addr_a = fa + t[5:0];
                addr_b = vmuls ? fb : (fb + t[5:0]);
                check_value({23'b0, mp_read_a_address}, addr_a, "read A lane t");
                check_value({23'b0, mp_read_b_address}, addr_b, "read B lane t");
            end else begin
                check_value({31'b0, mp_read_a_address}, 32'h0, "read A idle");
                check_value({31'b0, mp_read_b_address}, 32'h0, "read B idle");
            end
            if ((t >= 4) && (t <= (last_lane + 4))) begin
                addr_d = fd + (t - 4);
                check_value({31'b0, mp_write_enable}, 32'h1, "write enable t");
                check_value({23'b0, mp_write_address}, addr_d, "write address t");
                check_value(mp_write_data, exp_lane[t-4], "write data t");
            end else begin
                check_value({31'b0, mp_write_enable}, 32'h0, "write idle t");
            end
            busy_count = busy_count + 1;
            t = t + 1;
            @(negedge clk);
        end
        // The window must be exactly last_lane + 5 beats and the port idle.
        check_value(busy_count, last_lane + 5, "busy window beats");
        check_value(t, last_lane + 5, "cycle count");
        check_value({31'b0, busy}, 32'h0, "busy clear");
        check_value({31'b0, mp_write_enable}, 32'h0, "write enable clear");

        // Commit the reference state, then read the destination lanes back.
        for (k = 0; k < nlanes; k = k + 1) begin
            addr_d = fd + k[5:0];
            ref_mem[{3'b000, addr_d}] = exp_lane[k];
        end
        for (k = 0; k < nlanes; k = k + 1) begin
            addr_d = fd + k[5:0];
            read_reg({3'b000, addr_d});
            check_value(rd_value, exp_lane[k], "vector lane readback");
        end
        // The lane just past the range must be untouched.
        addr_d = fd + nlanes;
        if ((fd + nlanes) < 64) begin
            read_reg({3'b000, addr_d});
            check_value(rd_value, ref_mem[{3'b000, addr_d}],
                "lane boundary untouched");
        end
    end
endtask

// Issues a vec4 VMUL and aborts during T5, after lane 0 has committed at the
// end of T4 and while lane 1 is in the write stage. Semantics under test: an
// abort keeps every lane already written and leaves every not-yet-written lane
// at its old value -- it never rolls back and never completes the rest.
task run_mul_abort_mid;
    input [5:0] fa;
    input [5:0] fb;
    input [5:0] fd;
    integer k;
    reg [5:0] addr_a;
    reg [5:0] addr_b;
    reg [5:0] addr_d;
    begin
        for (k = 0; k < 4; k = k + 1) begin
            addr_a = fa + k[5:0];
            addr_b = fb + k[5:0];
            mul_ref_op(ref_mem[{3'b000, addr_a}], ref_mem[{3'b000, addr_b}],
                exp_lane[k]);
            addr_d = fd + k[5:0];
            old_lane[k] = ref_mem[{3'b000, addr_d}];
        end

        // vec4 = len_field 2'b10.
        @(negedge clk);
        instr_complete = 1'b1;
        instr_opcode = 4'hC;
        base_a = fa;
        base_b = fb;
        word1_raw = {fd, 2'b10, 5'h02, 3'b000};
        #1;
        check_value({31'b0, busy}, 32'h1, "abort T0 busy");
        @(negedge clk);
        instr_complete = 1'b0;
        @(negedge clk); // T2
        @(negedge clk); // T3
        @(negedge clk); // T4: lane 0 commits at the end of this cycle
        @(negedge clk); // T5: lane 1 is the pending write
        #1;
        check_value({31'b0, busy}, 32'h1, "abort T5 busy before");
        check_value({31'b0, mp_write_enable}, 32'h1, "abort T5 write pending");
        abort = 1'b1;
        #1;
        check_value({31'b0, busy}, 32'h0, "abort clears busy");
        check_value({31'b0, mp_write_enable}, 32'h0, "abort gates write enable");
        @(posedge clk);
        #1;
        abort = 1'b0;

        // Lane 0 keeps its new value; lanes 1..3 keep their old values.
        ref_mem[{3'b000, fd}] = exp_lane[0];
        for (k = 0; k < 4; k = k + 1) begin
            addr_d = fd + k[5:0];
            if (k >= 1)
                ref_mem[{3'b000, addr_d}] = old_lane[k];
            read_reg({3'b000, addr_d});
            check_value(rd_value, ref_mem[{3'b000, addr_d}],
                "abort mid lane state");
        end
    end
endtask

initial begin
    len_tab[0] = 2'b00; // vec2
    len_tab[1] = 2'b01; // vec3
    len_tab[2] = 2'b10; // vec4

    prepare_registers();

    // Directed scalar MUL: positive, fractional 0.5 * 0.25, negative and both
    // signed overflow corners (the narrow result wraps).
    set_reg(9'd0, 32'h00008000); // 0.5
    set_reg(9'd1, 32'h00004000); // 0.25
    run_scalar_mul(6'd0, 6'd1, 6'd2);
    set_reg(9'd3, 32'hFFFF0000); // -1.0
    set_reg(9'd4, 32'h00018000); // 1.5
    run_scalar_mul(6'd3, 6'd4, 6'd5);
    set_reg(9'd6, 32'h80000000);
    set_reg(9'd7, 32'h80000000);
    run_scalar_mul(6'd6, 6'd7, 6'd8);
    set_reg(9'd9, 32'h7FFFFFFF);
    set_reg(9'd10, 32'h7FFFFFFF);
    run_scalar_mul(6'd9, 6'd10, 6'd11);

    // Directed VMUL and VMULS at every supported length.
    run_vector_mul(6'd1, 6'd16, 6'd32, 2'b00, 1'b0); // VMUL.2
    run_vector_mul(6'd1, 6'd16, 6'd32, 2'b01, 1'b0); // VMUL.3
    run_vector_mul(6'd1, 6'd16, 6'd32, 2'b10, 1'b0); // VMUL.4
    run_vector_mul(6'd1, 6'd16, 6'd32, 2'b11, 1'b0); // reserved -> VMUL.4
    run_vector_mul(6'd1, 6'd16, 6'd32, 2'b01, 1'b1); // VMULS.3

    // Directed lane-boundary operands, including the most negative value.
    set_reg(9'd12, 32'h00018000);
    set_reg(9'd13, 32'hFFFF8000);
    set_reg(9'd14, 32'hFFFFFFFF);
    set_reg(9'd15, 32'h7FFFFFFF);
    set_reg(9'd16, 32'h80000000);
    set_reg(9'd17, 32'h00000001);
    run_vector_mul(6'd12, 6'd16, 6'd40, 2'b10, 1'b0); // VMUL.4
    run_vector_mul(6'd12, 6'd16, 6'd40, 2'b01, 1'b1); // VMULS.3

    // Allowed in-place destination (dst_base == srcA_base): every read happens
    // before any write, so each lane reads its own old value.
    run_vector_mul(6'd20, 6'd24, 6'd20, 2'b10, 1'b0); // VMUL.4 F20, F20, F24

    // abort in the middle of a vec4 VMUL.
    run_mul_abort_mid(6'd12, 6'd20, 6'd40);

    // Random multiply traffic: 2000 instructions mixing scalar MUL, VMUL and
    // VMULS. Vector bases sit in disjoint windows so no partial overlap is
    // created; the hardware does not check overlap, software must.
    for (i = 0; i < 2000; i = i + 1) begin
        op = ($random(seed) & 32'h7FFFFFFF) % 3;
        l = ($random(seed) & 32'h7FFFFFFF) % 3;
        if (op == 0) begin
            run_scalar_mul((($random(seed) & 32'h7FFFFFFF) % 12),
                           (($random(seed) & 32'h7FFFFFFF) % 12),
                           (32 + (($random(seed) & 32'h7FFFFFFF) % 12)));
        end else begin
            run_vector_mul((($random(seed) & 32'h7FFFFFFF) % 12),
                           (16 + (($random(seed) & 32'h7FFFFFFF) % 12)),
                           (32 + (($random(seed) & 32'h7FFFFFFF) % 12)),
                           len_tab[l], (op == 2));
        end
    end
    @(negedge clk);

    // Final full-register sweep against the reference model.
    for (i = 0; i < 64; i = i + 1) begin
        read_reg(i[8:0]);
        check_value(rd_value, ref_mem[i], "final register sweep");
    end

    if (check_count == 0)
        fail("no checks executed");
    $display("checks=%0d", check_count);
    $display("cycles=%0d", cycles);
    $display("DIGITAL_DESIGN_PASS");
    $finish;
end
endmodule
