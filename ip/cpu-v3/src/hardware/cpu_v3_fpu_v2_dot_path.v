// FPU v2 dot-product execution-path controller (opcode 0xC, subops 0x0D..0x0F).
//
// The dot family keeps one 64-bit Q32.32 accumulator (ACC) across instructions
// and adds the complete signed 72-bit multiplier product of every lane straight
// into it. There is deliberately no per-lane narrowing: the accumulator sees the
// full product, and only DOTSTORE narrows once at the end, taking the finished
// sum's Q16.16 view to the destination register.
//
// The instruction word split follows CpuV3FpuV2Frontend. Word1 carries
// {Fd[15:10], len[9:8], subop[7:3], mode[2:0]} with len 00 = vec2, 01 = vec3,
// 10 = vec4 and 11 reserved and clamped to vec4 (last_lane = len + 1). Word0
// carries Fa in bits [11:6] and Fb in bits [5:0]; the parent latches both 6-bit
// register bases and presents them as base_a / base_b on the beat this module
// starts, exactly as for the other FPU v2 execution paths.
//
// Supported subops (section 6.3, section 14):
//   0x0D DOT     : ACC  = sum(A+i * B+i)      fresh sum, ACC cleared at T0
//   0x0E DOTADD  : ACC += sum(A+i * B+i)      continues the current ACC
//   0x0F DOTSTORE: D[0] = q16(sum(A+i * B+i)); ACC = 0
//
// DOTSTORE is not an ACC spill: it computes a complete dot product of its own.
// It runs the exact same lane pipeline as DOT, clearing ACC on its first beat
// and reading A/B with the same len/mode/stride decoding as DOT. When the last
// lane's product reaches the accumulator stage it captures that completed sum
// (including the last lane), writes D[0] = sum[47:16] on the following beat and
// clears ACC. Fd is the destination base, so D[0] is Fd.
//
// A is always stride 1; mode[1:0] selects the B stride: 00 = +1, 01 = +3,
// 10 = +4, 11 is reserved and treated as +1. Lane i therefore reads
// A = base_a + i and B = base_b + i * stride.
//
// ACC accumulate: gowinsynthesis fuses it with the multiplier into a
// MULTADDALU18X18 DSP macro regardless of syn_dspstyle placement (all forms
// tried). That fusion is accepted deliberately: it is the DSP-efficient form
// and passes timing. See fpu-design-v2 §14.
//
// Multiply pipeline (II = 1, the same inferred $signed 36x36 -> 72-bit
// multiplier and three register stages as the multiply path). T0 is the cycle
// where instr_complete is high:
//   T0          : present the lane-0 read addresses.
//   T1..T(last) : present the lane-n read addresses while lane n-1 sits on the
//                 read buses.
//   T(k+4)      : the lane-k product has drained through the three stages and
//                 is added into ACC at the end of this cycle.
//   T(last+4)   : the last lane is accumulated; busy drops one beat later for
//                 DOT / DOTADD.
//   T(last+5)   : DOTSTORE presents its register write (the captured sum); busy
//                 drops one beat later.
// The read window is T0..T(last) for all three subops. Busy covers
// T0..T(last+4) for DOT / DOTADD, i.e. last_lane + 5 beats, and T0..T(last+5)
// for DOTSTORE, i.e. last_lane + 6 beats.
//
// ACC reset semantics:
//   - abort leaves ACC untouched; it only voids the in-flight pipeline.
//   - DOT and DOTSTORE clear ACC on their first beat, so a stale value is never
//     accumulated.
//   - DOTSTORE clears ACC once more after its writeback beat.
module CpuV3FpuV2DotPath (
    input wire clk,
    input wire abort,
    input wire instr_complete,
    input wire [3:0] instr_opcode,
    input wire [15:0] word1_raw,
    input wire [5:0] base_a,
    input wire [5:0] base_b,
    input wire [31:0] rf_read_a_data,
    input wire [31:0] rf_read_b_data,
    output wire [8:0] rf_read_a_address,
    output wire [8:0] rf_read_b_address,
    output wire rf_write_enable,
    output wire [8:0] rf_write_address,
    output wire [31:0] rf_write_data,
    output wire busy,
    output wire [63:0] acc_out
);

// Word1 fields.
wire [1:0] len_field = word1_raw[9:8];
wire [4:0] subop = word1_raw[7:3];
wire [2:0] mode_field = word1_raw[2:0];
wire [5:0] fd_field = word1_raw[15:10];

wire is_vector = (instr_opcode == 4'hC);
wire is_dot = is_vector && (subop == 5'h0D);
wire is_dotadd = is_vector && (subop == 5'h0E);
wire is_dotstore = is_vector && (subop == 5'h0F);
wire is_dot_family = is_dot || is_dotadd || is_dotstore;

// T0 of this cycle: a live supported dot instruction completed and is not
// cancelled. All three subops run the shared lane pipeline; DOT and DOTSTORE
// clear ACC on this first beat while DOTADD continues the current ACC.
wire load_now = instr_complete && is_dot_family && !abort;

// len 11 is reserved and clamps to vec4, so last_lane stays in 0..3.
wire [2:0] decoded_last_lane =
    (len_field == 2'b11) ? 3'd3 : (len_field + 3'd1);

// mode[1:0] is the B stride: 00 -> +1, 01 -> +3, 10 -> +4, 11 -> +1.
wire [2:0] decoded_stride =
    (mode_field[1:0] == 2'b01) ? 3'd3 :
    (mode_field[1:0] == 2'b10) ? 3'd4 : 3'd1;

// 64-bit Q32.32 accumulator. Fabric registers plus a LUT adder; the module
// attribute keeps the wide add out of the DSP block.
reg signed [63:0] acc_r = 64'sd0;

// Latched instruction state, loaded on the T0 edge.
reg run_r = 1'b0;
reg [2:0] lane_r = 3'd0;
reg [2:0] last_lane_r = 3'd0;
reg [5:0] base_a_r = 6'd0;
reg [5:0] base_b_r = 6'd0;
reg [2:0] stride_r = 3'd1;

// DOTSTORE completion state. store_mode_r marks the running instruction as a
// DOTSTORE, acc_lane_r counts the products already accumulated so the final
// lane can be recognised, and store_r is the one-beat register-file write that
// carries the captured sum and its destination.
reg store_mode_r = 1'b0;
reg [2:0] acc_lane_r = 3'd0;
reg store_r = 1'b0;
reg [8:0] store_addr_r = 9'd0;
reg [31:0] store_data_r = 32'h00000000;

// Stage 1: latched operands.
reg s1_valid_r = 1'b0;
reg signed [35:0] s1_a_r = 36'sd0;
reg signed [35:0] s1_b_r = 36'sd0;

// Stage 2: first product register.
reg s2_valid_r = 1'b0;
reg signed [71:0] s2_prod_r = 72'sd0;

// Stage 3: second product register; this is the product that reaches ACC.
reg s3_valid_r = 1'b0;
reg signed [71:0] s3_prod_r = 72'sd0;

// Read-address generation. During T0 the unlatched bases are used directly;
// afterwards the latched bases and the incrementing lane index are used. A has
// stride 1, B has the decoded stride.
wire reading = run_r && !abort && (lane_r <= last_lane_r);
wire [5:0] read_index_a = base_a_r + lane_r;
wire [5:0] lane_b_step = {3'b000, lane_r} * {3'b000, stride_r};
wire [5:0] read_index_b = base_b_r + lane_b_step;

assign rf_read_a_address =
    load_now ? {3'b000, base_a} :
    (reading ? {3'b000, read_index_a} : 9'd0);
assign rf_read_b_address =
    load_now ? {3'b000, base_b} :
    (reading ? {3'b000, read_index_b} : 9'd0);

// The lane whose RF data is on the buses this cycle sits one lane behind the
// read index, because the synchronous RF latched the previous beat's address.
wire data_valid = run_r && !abort &&
    (lane_r >= 3'd1) && ((lane_r - 3'd1) <= last_lane_r);

// Inferred 36x36 -> 72-bit signed multiplier: the stage-1 registers feed this
// combinational product and stage 2 registers it.
wire signed [71:0] product_next = $signed(s1_a_r) * $signed(s1_b_r);

// Full-width signed accumulation: sign-extend ACC, add the complete 72-bit
// product and keep the low 64 bits (Q32.32 wrap). NOTE: gowinsynthesis fuses
// this accumulate into a MULTADDALU18X18 DSP macro (the multiplier's ALU
// section); every syn_dspstyle placement was tried and ignored, and the
// mapping is accepted deliberately — it is the DSP-efficient form and passes
// timing. See fpu-design-v2 §14.
wire signed [71:0] acc_ext = {{8{acc_r[63]}}, acc_r};
wire signed [71:0] acc_sum = acc_ext + s3_prod_r;

// The product in stage 3 during this cycle is the final lane of a DOTSTORE
// exactly when the running instruction is a DOTSTORE and every lane up to
// last_lane_r has already been accumulated.
wire final_store_lane = s3_valid_r && store_mode_r &&
    (acc_lane_r == last_lane_r);

always @(posedge clk) begin
    if (abort) begin
        run_r <= 1'b0;
        s1_valid_r <= 1'b0;
        s2_valid_r <= 1'b0;
        s3_valid_r <= 1'b0;
        store_mode_r <= 1'b0;
        store_r <= 1'b0;
        // ACC is deliberately preserved across abort.
    end else if (load_now) begin
        // All three subops run the shared lane pipeline from T0. DOT and
        // DOTSTORE start a fresh sum; DOTADD continues the current ACC.
        run_r <= 1'b1;
        lane_r <= 3'd1;
        last_lane_r <= decoded_last_lane;
        base_a_r <= base_a;
        base_b_r <= base_b;
        stride_r <= decoded_stride;
        store_mode_r <= is_dotstore;
        acc_lane_r <= 3'd0;
        store_r <= 1'b0;
        store_addr_r <= {3'b000, fd_field};
        if (is_dot || is_dotstore)
            acc_r <= 64'sd0;
        // No lane reaches a product stage before T4, so it starts empty.
        s1_valid_r <= 1'b0;
        s2_valid_r <= 1'b0;
        s3_valid_r <= 1'b0;
    end else begin
        // Read sequencer: one lane per beat; it stops one beat after the last
        // lane, so the read window is exactly T0..T(last_lane).
        if (run_r) begin
            if (lane_r > (last_lane_r + 3'd1))
                run_r <= 1'b0;
            else
                lane_r <= lane_r + 3'd1;
        end

        // Stage 1 captures the operands present on the read buses this beat.
        s1_valid_r <= data_valid;
        if (data_valid) begin
            s1_a_r <= $signed(rf_read_a_data);
            s1_b_r <= $signed(rf_read_b_data);
        end

        // Stage 2 captures the product.
        s2_valid_r <= s1_valid_r;
        if (s1_valid_r)
            s2_prod_r <= product_next;

        // Stage 3 captures the product again; this beat's product reaches ACC.
        s3_valid_r <= s2_valid_r;
        if (s2_valid_r)
            s3_prod_r <= s2_prod_r;

        // Accumulate the lane whose product is in stage 3. The complete signed
        // 72-bit product is added with no per-lane narrowing. On the final lane
        // of a DOTSTORE the completed sum (including this lane) is captured for
        // the next beat's register write; ACC keeps that sum through the write
        // beat and is cleared once the writeback is done.
        if (s3_valid_r) begin
            acc_r <= acc_sum[63:0];
            if (final_store_lane) begin
                store_data_r <= acc_sum[47:16];
                store_r <= 1'b1;
                store_mode_r <= 1'b0;
            end
            acc_lane_r <= acc_lane_r + 3'd1;
        end

        // DOTSTORE completes its writeback this beat and leaves ACC clean.
        if (store_r) begin
            store_r <= 1'b0;
            acc_r <= 64'sd0;
        end
    end
end

// The write port is gated combinationally by abort, so an abort on the
// writeback beat cancels the register-file write at that edge.
assign rf_write_enable = store_r && !abort;
assign rf_write_address = store_addr_r;
assign rf_write_data = store_data_r;

assign busy = (load_now || run_r || s1_valid_r || s2_valid_r || s3_valid_r ||
    store_r) && !abort;

// Observation port: the ACC register is exposed directly.
assign acc_out = acc_r;

endmodule
