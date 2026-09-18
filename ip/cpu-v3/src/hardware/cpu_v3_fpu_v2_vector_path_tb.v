// Testbench for CpuV3FpuV2VectorPath.
//
// Following cpu_v3_fpu_v2_scalar_path_tb.v, the parent front-end and the
// register file are behavioral stubs inside this TB rather than instantiated
// leaves. (Instantiating the real leaves here would make the framework count
// them as physical children again, double-claiming the BSRAM; the full leaf
// interconnection is covered by the CpuV3FpuV2 unit testbench.) Only the
// combinational CpuV3FpuV2ScalarAlu leaf is instantiated, through the vector
// path itself. This TB drives instr_complete / instr_opcode / word1_raw /
// base_a / base_b directly and owns a behavioral 2R1W register file.
//
// The reference model is written out separately from the RTL: it computes the
// expected lane results from the pre-instruction register state, and it checks
// the busy window against last_lane + 3 beats, the per-cycle read addresses,
// the per-cycle write-enable window, and the final RF contents.
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

// Register-file write port: preparation writes versus vector-path writes.
reg prep_write_enable = 0;
reg [8:0] prep_write_address = 0;
reg [31:0] prep_write_data = 0;

// Register-file read port: TB readback mux versus the path's generated reads.
reg rb_sel = 0;
reg [8:0] rb_a = 0;
reg [8:0] rb_b = 0;

wire [8:0] vp_read_a_address;
wire [8:0] vp_read_b_address;
wire vp_write_enable;
wire [8:0] vp_write_address;
wire [31:0] vp_write_data;
wire busy;

wire [8:0] rf_read_a_address = rb_sel ? rb_a : vp_read_a_address;
wire [8:0] rf_read_b_address = rb_sel ? rb_b : vp_read_b_address;
wire rf_write_enable = prep_write_enable ? 1'b1 : vp_write_enable;
wire [8:0] rf_write_address =
    prep_write_enable ? prep_write_address : vp_write_address;
wire [31:0] rf_write_data =
    prep_write_enable ? prep_write_data : vp_write_data;
reg [31:0] rf_read_a_data = 0;
reg [31:0] rf_read_b_data = 0;

// Register-file stub: one synchronous write port, two synchronous read ports
// with read-first semantics. Address balance with the vector path is checked by
// the simulation itself (an out-of-range index is an X in the reference sweep).
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

CpuV3FpuV2VectorPath vector_path (
    .clk(clk),
    .abort(abort),
    .instr_complete(instr_complete),
    .instr_opcode(instr_opcode),
    .word1_raw(word1_raw),
    .base_a(base_a),
    .base_b(base_b),
    .rf_read_a_data(rf_read_a_data),
    .rf_read_b_data(rf_read_b_data),
    .rf_read_a_address(vp_read_a_address),
    .rf_read_b_address(vp_read_b_address),
    .rf_write_enable(vp_write_enable),
    .rf_write_address(vp_write_address),
    .rf_write_data(vp_write_data),
    .busy(busy)
);

// Reference model: register contents plus a per-instruction expected lane
// array. It follows the same wrap-only Q16.16 rules as the ALU leaf, but is
// written out independently so the comparison does not reuse the RTL.
reg [31:0] ref_mem [0:511];
reg [31:0] exp_lane [0:3];
reg [31:0] old_lane [0:3];
integer check_count = 0;
integer i;
integer seed = 32'h0BADF00D;
reg [31:0] rd_value;
reg [4:0] sub_tab [0:6];
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

// Independent result model for one vector lane op.
task vec_ref_op;
    input [31:0] a;
    input [31:0] b;
    input [4:0] subop;
    output [31:0] result;
    reg signed [31:0] sa;
    reg signed [31:0] sb;
    begin
        sa = a;
        sb = b;
        case (subop)
            5'h00: result = a + b;                        // VADD
            5'h01: result = a - b;                        // VSUB
            5'h04: result = (sa < sb) ? a : b;            // VMIN
            5'h05: result = (sa > sb) ? a : b;            // VMAX
            5'h06: result = a[31] ? (32'h00000000 - a) : a; // VABS
            5'h07: result = 32'h00000000 - a;             // VNEG
            5'h0C: result = a;                            // VMOV
            default: result = 32'h00000000;
        endcase
    end
endtask

// Clears the reference model, then loads every architectural F register through
// the RF write port. The vector path is idle here, so the write-port mux is
// owned by the preparation side.
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
// reference model in step, for staging directed boundary operands.
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

// Issues one opcode-0xC instruction and checks the whole fixed lane pipeline:
// the T0 lane-0 read addresses, the per-cycle read-address ramp, the per-cycle
// write-enable window and write data (against the independent expected lane
// results), the busy-window length, and the destination-lane readback. One lane
// past the vector range is checked too, so a too-long vector is caught.
task run_vector;
    input [5:0] fa;
    input [5:0] fb;
    input [5:0] fd;
    input [1:0] len_field;
    input [4:0] subop;
    input [2:0] mode;
    integer nlanes;
    integer last_lane;
    integer k;
    integer busy_count;
    reg [5:0] addr_a;
    reg [5:0] addr_b;
    reg [5:0] addr_d;
    begin
        nlanes = len_field + 2;
        last_lane = len_field + 1;

        // Expected results from the pre-instruction state, so an in-place
        // destination is modelled correctly.
        for (k = 0; k < nlanes; k = k + 1) begin
            addr_a = fa + k[5:0];
            addr_b = fb + k[5:0];
            vec_ref_op(ref_mem[{3'b000, addr_a}], ref_mem[{3'b000, addr_b}],
                subop, exp_lane[k]);
        end

        // Issue at the negedge so T0 is the cycle that follows.
        @(negedge clk);
        instr_complete = 1'b1;
        instr_opcode = 4'hC;
        base_a = fa;
        base_b = fb;
        word1_raw = {fd, len_field, subop, mode};
        #1;
        check_value({31'b0, busy}, 32'h1, "T0 busy");
        check_value({23'b0, vp_read_a_address}, fa, "T0 read A lane0");
        check_value({23'b0, vp_read_b_address}, fb, "T0 read B lane0");

        busy_count = 1;
        @(negedge clk);
        instr_complete = 1'b0;
        #1; // let the deasserted instr_complete settle before sampling T1

        // T1 .. T(last_lane+2): one lane per cycle while busy is high.
        k = 1;
        while (busy) begin
            if (k <= last_lane) begin
                addr_a = fa + k[5:0];
                addr_b = fb + k[5:0];
                check_value({23'b0, vp_read_a_address}, addr_a, "read A lane k");
                check_value({23'b0, vp_read_b_address}, addr_b, "read B lane k");
            end else begin
                check_value({31'b0, vp_read_a_address}, 32'h0, "read A idle");
                check_value({31'b0, vp_read_b_address}, 32'h0, "read B idle");
            end
            if ((k >= 2) && (k <= (last_lane + 2))) begin
                addr_d = fd + (k - 2);
                check_value({31'b0, vp_write_enable}, 32'h1, "write enable k");
                check_value({23'b0, vp_write_address}, addr_d, "write address k");
                check_value(vp_write_data, exp_lane[k-2], "write data k");
            end else begin
                check_value({31'b0, vp_write_enable}, 32'h0, "write idle k");
            end
            busy_count = busy_count + 1;
            k = k + 1;
            @(negedge clk);
        end
        // The window must be exactly last_lane + 3 beats and the port idle now.
        check_value(busy_count, last_lane + 3, "busy window beats");
        check_value(k, last_lane + 3, "cycle count");
        check_value({31'b0, busy}, 32'h0, "busy clear");
        check_value({31'b0, vp_write_enable}, 32'h0, "write enable clear");

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

// Issues a vec4 VADD and aborts in T3, after lane 0 has committed (end of T2)
// and while lane 1 is in the write stage. Semantics under test: an abort keeps
// every lane already written and leaves every not-yet-written lane at its old
// value -- it never rolls back and never completes the remaining lanes.
task run_vector_abort_mid;
    input [5:0] fa;
    input [5:0] fb;
    input [5:0] fd;
    input [4:0] subop;
    integer k;
    reg [5:0] addr_a;
    reg [5:0] addr_b;
    reg [5:0] addr_d;
    begin
        for (k = 0; k < 4; k = k + 1) begin
            addr_a = fa + k[5:0];
            addr_b = fb + k[5:0];
            vec_ref_op(ref_mem[{3'b000, addr_a}], ref_mem[{3'b000, addr_b}],
                subop, exp_lane[k]);
            addr_d = fd + k[5:0];
            old_lane[k] = ref_mem[{3'b000, addr_d}];
        end

        // vec4 = len_field 2'b10.
        @(negedge clk);
        instr_complete = 1'b1;
        instr_opcode = 4'hC;
        base_a = fa;
        base_b = fb;
        word1_raw = {fd, 2'b10, subop, 3'b000};
        #1;
        check_value({31'b0, busy}, 32'h1, "abort T0 busy");
        @(negedge clk);
        instr_complete = 1'b0;
        @(negedge clk); // T2: lane 0 write commits at the end of this cycle
        @(negedge clk); // T3: lane 1 is the pending write
        #1;
        check_value({31'b0, busy}, 32'h1, "abort T3 busy before");
        check_value({31'b0, vp_write_enable}, 32'h1, "abort T3 write pending");
        abort = 1'b1;
        #1;
        check_value({31'b0, busy}, 32'h0, "abort clears busy");
        check_value({31'b0, vp_write_enable}, 32'h0, "abort gates write enable");
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
    // subop and length tables for the directed sweep.
    sub_tab[0] = 5'h00; // VADD
    sub_tab[1] = 5'h01; // VSUB
    sub_tab[2] = 5'h04; // VMIN
    sub_tab[3] = 5'h05; // VMAX
    sub_tab[4] = 5'h06; // VABS
    sub_tab[5] = 5'h07; // VNEG
    sub_tab[6] = 5'h0C; // VMOV
    len_tab[0] = 2'b00; // vec2
    len_tab[1] = 2'b01; // vec3
    len_tab[2] = 2'b10; // vec4

    prepare_registers();

    // Directed sweep: every supported subop at every vector length, spot-
    // checking the lane count. Bases are disjoint (A 1.., B 16.., D 32..).
    for (s = 0; s < 7; s = s + 1)
        for (l = 0; l < 3; l = l + 1)
            run_vector(6'd1, 6'd16, 6'd32, len_tab[l], sub_tab[s], 3'b000);

    // mode is ignored by this stage: repeating with a non-zero mode must give
    // the same result and the same timing.
    run_vector(6'd1, 6'd16, 6'd32, 2'b10, 5'h00, 3'b101);
    run_vector(6'd1, 6'd16, 6'd32, 2'b00, 5'h0C, 3'b111);

    // Directed lane-boundary operands, including the most negative value whose
    // abs/neg wraps to itself.
    set_reg(9'd0, 32'h00018000);
    set_reg(9'd1, 32'hFFFF8000);
    set_reg(9'd2, 32'hFFFFFFFF);
    set_reg(9'd3, 32'h7FFFFFFF);
    set_reg(9'd4, 32'h80000000);
    set_reg(9'd5, 32'h00000001);
    run_vector(6'd0, 6'd4, 6'd8, 2'b10, 5'h00, 3'b000); // VADD.4
    run_vector(6'd0, 6'd4, 6'd8, 2'b00, 5'h01, 3'b000); // VSUB.2
    run_vector(6'd0, 6'd4, 6'd8, 2'b01, 5'h04, 3'b000); // VMIN.3
    run_vector(6'd4, 6'd0, 6'd8, 2'b10, 5'h05, 3'b000); // VMAX.4
    run_vector(6'd4, 6'd4, 6'd8, 2'b10, 5'h06, 3'b000); // VABS.4
    run_vector(6'd4, 6'd4, 6'd8, 2'b10, 5'h07, 3'b000); // VNEG.4

    // Allowed in-place destination (dst_base == srcA_base): each lane reads its
    // own old value before that same register is written two cycles later.
    run_vector(6'd0, 6'd4, 6'd0, 2'b10, 5'h00, 3'b000); // VADD.4 F0, F0, F4

    // abort in the middle of a vec4 instruction.
    run_vector_abort_mid(6'd0, 6'd4, 6'd16, 5'h00);

    // Random vector traffic: 2000 instructions across all supported subops and
    // lengths. Bases are constrained to disjoint windows so no partial overlap
    // is created; the hardware does not check overlap, software must.
    for (i = 0; i < 2000; i = i + 1) begin
        s = ($random(seed) & 32'h7FFFFFFF) % 7;
        l = ($random(seed) & 32'h7FFFFFFF) % 3;
        run_vector((($random(seed) & 32'h7FFFFFFF) % 12),
                   (16 + (($random(seed) & 32'h7FFFFFFF) % 12)),
                   (32 + (($random(seed) & 32'h7FFFFFFF) % 12)),
                   len_tab[l], sub_tab[s], 3'b000);
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
