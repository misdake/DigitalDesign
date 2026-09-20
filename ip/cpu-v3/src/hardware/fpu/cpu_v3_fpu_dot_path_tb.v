// Testbench for CpuV3FpuDotPath.
//
// Following cpu_v3_fpu_multiply_path_tb.v, the parent front-end and the
// register file are behavioral stubs inside this TB rather than instantiated
// leaves. This TB drives instr_complete / instr_opcode / word1_raw / base_a /
// base_b directly and owns a behavioral 2R1W register file, so the dot path can
// be exercised without the front-end pair protocol.
//
// The reference model is independent of the RTL. It keeps a 64-bit signed
// accumulator and, for every lane, forms the exact 64-bit signed product
// floor(a * b) of the two Q16.16 operands. Because a 32x32 signed product fits
// in 64 bits, its low 64 bits are the low 64 bits of the RTL's sign-extended
// 36x36 product, so `ref_acc + product` models the RTL's full 72-bit add into
// the 64-bit Q32.32 ACC with 2's-complement wrap. DOTSTORE is modelled as a
// complete dot product of its own: `D[0] = q16(sum(A[i] * B[i]))` followed by
// `ref_acc = 0`, so it narrows the fresh sum into the destination and leaves ACC
// clean.
//
// The TB checks the per-cycle read-address sequence (including the B stride),
// the per-cycle ACC value against the running partial sum, the DOTSTORE write
// beat and the ACC clear, the busy-window length, the abort behaviour, the
// DOT-to-DOTSTORE cross relationship and the final register contents.
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

localparam [4:0] DOT = 5'h0D;
localparam [4:0] DOTADD = 5'h0E;
localparam [4:0] DOTSTORE = 5'h0F;

// Instruction inputs, driven directly by this TB.
reg instr_complete = 0;
reg [3:0] instr_opcode = 0;
reg [15:0] word1_raw = 0;
reg [5:0] base_a = 0;
reg [5:0] base_b = 0;
reg abort = 0;

// Register-file write port: preparation writes versus dot-path writes.
reg prep_write_enable = 0;
reg [8:0] prep_write_address = 0;
reg [31:0] prep_write_data = 0;

// Register-file read port: TB readback mux versus the path's generated reads.
reg rb_sel = 0;
reg [8:0] rb_a = 0;
reg [8:0] rb_b = 0;

wire [8:0] dp_read_a_address;
wire [8:0] dp_read_b_address;
wire dp_write_enable;
wire [8:0] dp_write_address;
wire [31:0] dp_write_data;
wire busy;
wire [63:0] acc_out;

wire [8:0] rf_read_a_address = rb_sel ? rb_a : dp_read_a_address;
wire [8:0] rf_read_b_address = rb_sel ? rb_b : dp_read_b_address;
wire rf_write_enable = prep_write_enable ? 1'b1 : dp_write_enable;
wire [8:0] rf_write_address =
    prep_write_enable ? prep_write_address : dp_write_address;
wire [31:0] rf_write_data =
    prep_write_enable ? prep_write_data : dp_write_data;
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
// leaf test needs no resource-claiming sibling modules (same pattern as the
// scalar-path and multiply-path TBs).
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

CpuV3FpuDotPath dot_path (
    .clk(clk),
    .abort(abort),
    .instr_complete(instr_complete),
    .instr_opcode(instr_opcode),
    .word1_raw(word1_raw),
    .base_a(base_a),
    .base_b(base_b),
    .rf_read_a_data(rf_read_a_data),
    .rf_read_b_data(rf_read_b_data),
    .rf_read_a_address(dp_read_a_address),
    .rf_read_b_address(dp_read_b_address),
    .mul_in_valid(mul_in_valid),
    .mul_in_a(mul_in_a),
    .mul_in_b(mul_in_b),
    .mul_in_tag(mul_in_tag),
    .mul_out_valid(mul_out_valid),
    .mul_out_product(mul_out_product),
    .mul_out_tag(mul_out_tag),
    .rf_write_enable(dp_write_enable),
    .rf_write_address(dp_write_address),
    .rf_write_data(dp_write_data),
    .busy(busy),
    .acc_out(acc_out)
);

// Reference model: register contents, the architectural ACC and a per-lane
// expected product array.
reg [31:0] ref_mem [0:511];
reg signed [63:0] ref_acc;
reg signed [63:0] exp_prod [0:3];
integer check_count = 0;
integer i;
integer op;
integer seed = 32'h0D07F00D;
reg [31:0] rd_value;
reg [1:0] len_tab [0:3];
reg [2:0] mode_tab [0:3];
integer l;
integer m;
reg signed [63:0] cross_acc;
reg signed [63:0] cross_widen;

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

task check_value64;
    input [63:0] got;
    input [63:0] want;
    input [8*64-1:0] label;
    begin
        check_count = check_count + 1;
        if (got !== want) begin
            $display("DIGITAL_DESIGN_FAIL: %0s got %016h want %016h",
                label, got, want);
            $finish;
        end
    end
endtask

// Independent 64-bit signed product of two Q16.16 operands. A 32x32 signed
// product already fits in 64 bits, so this is the low 64 bits of the RTL's
// sign-extended 36x36 product.
task prod_ref;
    input [31:0] a;
    input [31:0] b;
    output [63:0] result;
    reg signed [63:0] pa;
    reg signed [63:0] pb;
    begin
        pa = $signed(a);
        pb = $signed(b);
        result = pa * pb;
    end
endtask

// Narrow-Q32.32-to-Q16.16 view: the low 32 bits of the arithmetic shift right
// by 16, i.e. ACC[47:16].
task narrow_ref;
    input [63:0] value;
    output [31:0] result;
    begin
        result = $signed(value) >>> 16;
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
        ref_acc = 64'sd0;
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

// Issues one dot-family instruction (DOT, DOTADD or DOTSTORE) and checks the
// whole pipeline: the T0 lane-0 addresses, the per-cycle A/B read addresses
// with the mode stride, the per-cycle ACC against the running partial sum, the
// register write port (a single DOTSTORE write on T(last_lane + 5)), the busy
// window and the final ACC. DOTSTORE computes a fresh dot product of its own,
// so the written word is `q16(sum)` and the final ACC is zero; DOT leaves
// `ACC = sum` and DOTADD leaves `ACC = ref_acc + sum`.
task run_dot;
    input [5:0] fa;
    input [5:0] fb;
    input [5:0] fd;
    input [1:0] len_field;
    input [4:0] subop;
    input [2:0] mode_field;
    integer last_lane;
    integer nlanes;
    integer stride;
    integer k;
    integer t;
    integer busy_count;
    reg [5:0] addr_a;
    reg [5:0] addr_b;
    reg signed [63:0] sum_lanes;
    reg signed [63:0] exp_acc;
    reg signed [63:0] final_acc;
    reg [31:0] exp_data;
    reg is_store;
    begin
        is_store = (subop == DOTSTORE);

        if (len_field == 2'b11)
            last_lane = 3;
        else
            last_lane = len_field + 1;
        nlanes = last_lane + 1;

        if (mode_field[1:0] == 2'b01)
            stride = 3;
        else if (mode_field[1:0] == 2'b10)
            stride = 4;
        else
            stride = 1;

        // Expected per-lane products and the fresh lane sum. DOT and DOTSTORE
        // start from zero; DOTADD continues the current ACC.
        sum_lanes = 64'sd0;
        for (k = 0; k < nlanes; k = k + 1) begin
            addr_a = fa + k[5:0];
            addr_b = fb + (k * stride);
            prod_ref(ref_mem[addr_a], ref_mem[addr_b], exp_prod[k]);
            sum_lanes = sum_lanes + exp_prod[k];
        end
        if (subop == DOTADD)
            final_acc = ref_acc + sum_lanes;
        else
            final_acc = sum_lanes;
        narrow_ref(sum_lanes, exp_data);

        // Issue at the negedge so T0 is the cycle that follows.
        @(negedge clk);
        instr_complete = 1'b1;
        instr_opcode = 4'hC;
        base_a = fa;
        base_b = fb;
        word1_raw = {fd, len_field, subop, mode_field};
        #1;
        check_value({31'b0, busy}, 32'h1, "dot T0 busy");
        check_value({23'b0, dp_read_a_address}, fa, "dot T0 read A lane0");
        check_value({23'b0, dp_read_b_address}, fb, "dot T0 read B lane0");
        check_value({31'b0, dp_write_enable}, 32'h0, "dot T0 write idle");
        // The accumulator is still the pre-instruction value at T0; DOT and
        // DOTSTORE clear it only at the end of this cycle.
        check_value64(acc_out, ref_acc, "dot T0 acc");

        busy_count = 1;
        @(negedge clk);
        instr_complete = 1'b0;
        #1; // let the deasserted instr_complete settle before sampling T1

        // T1 .. T(last_lane + 4): reads, then the three-stage product drain and
        // the accumulation. DOTSTORE adds one more beat for its register write.
        t = 1;
        while (busy) begin
            if (t <= last_lane) begin
                addr_a = fa + t[5:0];
                addr_b = fb + (t * stride);
                check_value({23'b0, dp_read_a_address}, addr_a,
                    "dot read A lane t");
                check_value({23'b0, dp_read_b_address}, addr_b,
                    "dot read B lane t");
            end else begin
                check_value({31'b0, dp_read_a_address}, 32'h0,
                    "dot read A idle");
                check_value({31'b0, dp_read_b_address}, 32'h0,
                    "dot read B idle");
            end

            // DOTSTORE writes its captured sum on T(last_lane + 5).
            if (is_store && (t == last_lane + 5)) begin
                check_value({31'b0, dp_write_enable}, 32'h1,
                    "store write enable");
                check_value({23'b0, dp_write_address}, fd,
                    "store write address");
                check_value(dp_write_data, exp_data, "store write data");
            end else begin
                check_value({31'b0, dp_write_enable}, 32'h0, "dot write idle");
            end

            // ACC visible during cycle T(t): lanes whose product is already
            // latched at the end of T(k+4), i.e. k + 5 <= t. DOT and DOTSTORE
            // start from zero; DOTADD from ref_acc. DOTSTORE holds the completed
            // sum on its writeback beat and clears ACC on the following beat,
            // which the post-loop check covers.
            if (subop == DOTADD)
                exp_acc = ref_acc;
            else
                exp_acc = 64'sd0;
            for (k = 0; k < nlanes; k = k + 1)
                if ((k + 5) <= t)
                    exp_acc = exp_acc + exp_prod[k];
            check_value64(acc_out, exp_acc, "dot acc t");

            busy_count = busy_count + 1;
            t = t + 1;
            @(negedge clk);
        end
        // The window must be exactly last_lane + 5 beats for DOT / DOTADD and
        // last_lane + 6 for DOTSTORE, and the port idle.
        if (is_store)
            check_value(busy_count, last_lane + 6, "store busy window");
        else
            check_value(busy_count, last_lane + 5, "dot busy window");
        check_value({31'b0, busy}, 32'h0, "dot busy clear");
        check_value({31'b0, dp_write_enable}, 32'h0, "dot write clear");

        if (is_store) begin
            check_value64(acc_out, 64'd0, "store final acc clear");
            ref_mem[{3'b000, fd}] = exp_data;
            ref_acc = 64'sd0;
        end else begin
            check_value64(acc_out, final_acc, "dot final acc");
            ref_acc = final_acc;
        end
    end
endtask

// Convenience wrapper: a DOTSTORE is a full dot product, so it reuses run_dot
// with subop 0x0F. fa/fb are the same source bases as DOT; the difference is
// the final narrowed write and the ACC clear that run_dot already models.
task run_dotstore;
    input [5:0] fa;
    input [5:0] fb;
    input [5:0] fd;
    input [1:0] len_field;
    input [2:0] mode_field;
    begin
        run_dot(fa, fb, fd, len_field, DOTSTORE, mode_field);
    end
endtask

// Seeds ACC with a DOT, then issues one dot-family instruction and asserts
// abort at cycle T(abort_t), after some lanes have already been accumulated.
// Semantics under test: abort preserves the partial ACC exactly (it neither
// rolls back nor adds the in-flight lane), voids the pipeline, gates the write
// and drops busy. DOTSTORE is included because it too runs the lane pipeline
// and has already cleared ACC and accumulated the early lanes by the time abort
// arrives.
task run_dot_abort_mid;
    input [5:0] fa;
    input [5:0] fb;
    input [5:0] fd;
    input [1:0] len_field;
    input [4:0] subop;
    input [2:0] mode_field;
    input integer abort_t;
    integer last_lane;
    integer nlanes;
    integer stride;
    integer k;
    integer t;
    reg [5:0] addr_a;
    reg [5:0] addr_b;
    reg signed [63:0] partial;
    begin
        if (len_field == 2'b11)
            last_lane = 3;
        else
            last_lane = len_field + 1;
        nlanes = last_lane + 1;

        if (mode_field[1:0] == 2'b01)
            stride = 3;
        else if (mode_field[1:0] == 2'b10)
            stride = 4;
        else
            stride = 1;

        for (k = 0; k < nlanes; k = k + 1) begin
            addr_a = fa + k[5:0];
            addr_b = fb + (k * stride);
            prod_ref(ref_mem[addr_a], ref_mem[addr_b], exp_prod[k]);
        end

        @(negedge clk);
        instr_complete = 1'b1;
        instr_opcode = 4'hC;
        base_a = fa;
        base_b = fb;
        word1_raw = {fd, len_field, subop, mode_field};
        #1;
        check_value({31'b0, busy}, 32'h1, "abort T0 busy");

        @(negedge clk);
        instr_complete = 1'b0;
        #1;
        t = 1;
        while (t < abort_t) begin
            @(negedge clk);
            #1;
            t = t + 1;
        end

        // Partial sum visible during cycle T(abort_t). DOT and DOTSTORE start
        // from zero; DOTADD continues the current ACC.
        if (subop == DOTADD)
            partial = ref_acc;
        else
            partial = 64'sd0;
        for (k = 0; k < nlanes; k = k + 1)
            if ((k + 5) <= t)
                partial = partial + exp_prod[k];
        check_value64(acc_out, partial, "abort pre acc");

        abort = 1'b1;
        #1;
        check_value({31'b0, busy}, 32'h0, "abort clears busy");
        check_value({31'b0, dp_write_enable}, 32'h0, "abort gates write");
        check_value({31'b0, dp_read_a_address}, 32'h0, "abort read A idle");
        check_value({31'b0, dp_read_b_address}, 32'h0, "abort read B idle");

        @(posedge clk);
        #1;
        check_value64(acc_out, partial, "abort keeps acc");
        check_value({31'b0, busy}, 32'h0, "abort stays idle");
        abort = 1'b0;
        ref_acc = partial;
    end
endtask

initial begin
    len_tab[0] = 2'b00; // vec2
    len_tab[1] = 2'b01; // vec3
    len_tab[2] = 2'b10; // vec4
    len_tab[3] = 2'b11; // reserved -> vec4
    mode_tab[0] = 3'b000; // B stride +1
    mode_tab[1] = 3'b001; // B stride +3
    mode_tab[2] = 3'b010; // B stride +4
    mode_tab[3] = 3'b011; // reserved -> +1

    prepare_registers();

    // Directed operands: positive, fractional and negative Q16.16 values.
    set_reg(9'd0, 32'h00008000);  //  0.5
    set_reg(9'd1, 32'h00004000);  //  0.25
    set_reg(9'd2, 32'h00002000);  //  0.125
    set_reg(9'd3, 32'hFFFF8000);  // -0.5
    set_reg(9'd16, 32'h00004000); //  0.25
    set_reg(9'd17, 32'h00008000); //  0.5
    set_reg(9'd18, 32'hFFFFC000); // -0.25
    set_reg(9'd19, 32'h00010000); //  1.0
    set_reg(9'd20, 32'hFFFF8000); // -0.5
    set_reg(9'd22, 32'h00008000); //  0.5
    set_reg(9'd24, 32'h00004000); //  0.25
    set_reg(9'd25, 32'hFFFF0000); // -1.0
    set_reg(9'd28, 32'h00010000); //  1.0

    // DOT at every supported length, each followed by a DOTSTORE on the same
    // operands. DOT leaves ACC = sum; DOTSTORE recomputes the same sum, writes
    // q16(sum) to Fd and clears ACC.
    run_dot(6'd0, 6'd16, 6'd0, 2'b00, DOT, 3'b000);        // DOT.2
    run_dotstore(6'd0, 6'd16, 6'd32, 2'b00, 3'b000);
    run_dot(6'd0, 6'd16, 6'd0, 2'b01, DOT, 3'b000);        // DOT.3
    run_dotstore(6'd0, 6'd16, 6'd33, 2'b01, 3'b000);
    run_dot(6'd0, 6'd16, 6'd0, 2'b10, DOT, 3'b000);        // DOT.4
    run_dotstore(6'd0, 6'd16, 6'd34, 2'b10, 3'b000);
    run_dot(6'd0, 6'd16, 6'd0, 2'b11, DOT, 3'b000);        // reserved -> DOT.4
    run_dotstore(6'd0, 6'd16, 6'd35, 2'b11, 3'b000);

    // Directed cross-validation: the ACC computed by DOT and the word written by
    // DOTSTORE for the same operands must agree after the Q16.16 narrowing, and
    // widening the written word must recover the DOT ACC's upper bits.
    run_dot(6'd0, 6'd16, 6'd0, 2'b10, DOT, 3'b000);
    cross_acc = ref_acc;
    run_dotstore(6'd0, 6'd16, 6'd42, 2'b10, 3'b000);
    check_value(ref_mem[9'd42], cross_acc[47:16], "cross narrow");
    cross_widen = $signed(ref_mem[9'd42]);
    cross_widen = cross_widen <<< 16;
    check_value64(cross_widen, {cross_acc[63:16], 16'b0}, "cross widen");

    // B stride 3, stride 4 and the reserved stride (treated as +1). The
    // per-cycle read-B checks in run_dot cover the address sequences.
    run_dot(6'd0, 6'd16, 6'd0, 2'b10, DOT, 3'b001);        // stride +3
    run_dotstore(6'd0, 6'd16, 6'd36, 2'b10, 3'b001);
    run_dot(6'd0, 6'd16, 6'd0, 2'b10, DOT, 3'b010);        // stride +4
    run_dotstore(6'd0, 6'd16, 6'd37, 2'b10, 3'b010);
    run_dot(6'd0, 6'd16, 6'd0, 2'b10, DOT, 3'b011);        // reserved -> +1
    run_dotstore(6'd0, 6'd16, 6'd38, 2'b10, 3'b011);

    // DOTADD continuing an existing accumulator, twice, then stored.
    run_dot(6'd0, 6'd16, 6'd0, 2'b10, DOT, 3'b000);
    run_dot(6'd0, 6'd16, 6'd0, 2'b10, DOTADD, 3'b000);
    run_dot(6'd0, 6'd16, 6'd0, 2'b10, DOTADD, 3'b000);
    run_dotstore(6'd0, 6'd16, 6'd39, 2'b10, 3'b000);

    // Negative-heavy operands.
    set_reg(9'd4, 32'h80000000);  // large negative
    set_reg(9'd5, 32'h7FFFFFFF);  // large positive
    set_reg(9'd21, 32'hFFFFFFFF); // -1 LSB
    run_dot(6'd0, 6'd16, 6'd0, 2'b10, DOT, 3'b000);
    run_dotstore(6'd0, 6'd16, 6'd40, 2'b10, 3'b000);

    // abort in the middle of a DOTADD: ACC keeps the lanes already accumulated
    // and never receives the in-flight one.
    run_dot(6'd0, 6'd16, 6'd0, 2'b10, DOT, 3'b000);
    run_dot_abort_mid(6'd0, 6'd16, 6'd0, 2'b10, DOTADD, 3'b000, 6);
    run_dotstore(6'd0, 6'd16, 6'd41, 2'b10, 3'b000);

    // abort in the middle of a DOTSTORE pipeline: ACC is already the fresh sum's
    // early lanes (it was cleared at T0), abort preserves that partial sum and
    // no register write is issued.
    run_dot(6'd0, 6'd16, 6'd0, 2'b10, DOT, 3'b000);
    run_dot_abort_mid(6'd0, 6'd16, 6'd43, 2'b10, DOTSTORE, 3'b000, 6);
    run_dotstore(6'd0, 6'd16, 6'd44, 2'b10, 3'b000);

    // Random dot traffic: 2000 instructions mixing DOT, DOTADD and DOTSTORE.
    // Source windows (A: 0..15, B: 16..31) and destination window (D: 32..47)
    // are disjoint; the reference ACC and reference register file are updated
    // by the same tasks that drive the RTL.
    for (i = 0; i < 2000; i = i + 1) begin
        op = ($random(seed) & 32'h7FFFFFFF) % 3;
        l = ($random(seed) & 32'h7FFFFFFF) % 4;
        m = ($random(seed) & 32'h7FFFFFFF) % 4;
        if (op == 0) begin
            run_dot((($random(seed) & 32'h7FFFFFFF) % 12),
                    16 + (($random(seed) & 32'h7FFFFFFF) % 4),
                    6'd0,
                    len_tab[l], DOT, mode_tab[m]);
        end else if (op == 1) begin
            run_dot((($random(seed) & 32'h7FFFFFFF) % 12),
                    16 + (($random(seed) & 32'h7FFFFFFF) % 4),
                    6'd0,
                    len_tab[l], DOTADD, mode_tab[m]);
        end else begin
            run_dotstore((($random(seed) & 32'h7FFFFFFF) % 12),
                    16 + (($random(seed) & 32'h7FFFFFFF) % 4),
                    32 + (($random(seed) & 32'h7FFFFFFF) % 12),
                    len_tab[l], mode_tab[m]);
        end
        // Refresh one source register now and then so the operand mix keeps
        // changing over the run.
        if ((i % 53) == 0)
            set_reg((($random(seed) & 32'h7FFFFFFF) % 32), $random(seed));
    end
    @(negedge clk);

    // Final ACC and full-register sweep against the reference model.
    check_value64(acc_out, ref_acc, "final acc sweep");
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
