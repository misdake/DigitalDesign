// FPU v2 dot-product execution-path controller (opcode 0xC, subops 0x0D..0x0F).
//
// The dot family keeps one 64-bit Q32.32 accumulator (ACC) across instructions
// and adds the complete signed 72-bit multiplier product of every lane straight
// into it. There is deliberately no per-lane narrowing: the accumulator sees the
// full product, and only DOTSTORE narrows once at the end, taking the Q16.16
// view of the completed sum to the destination register.
//
// The instruction word split follows CpuV3FpuFrontend. Word1 carries
// {Fd[15:10], len[9:8], subop[7:3], mode[2:0]} with len 00 = vec2, 01 = vec3,
// 10 = vec4 and 11 reserved and clamped to vec4 (last_lane = len + 1). Word0
// carries Fa in bits [11:6] and Fb in bits [5:0]; the parent latches both 6-bit
// register bases and presents them as base_a / base_b on the beat this module
// starts, exactly as for the other FPU v2 execution paths.
//
// Supported subops (section 6.3):
//   0x0D DOT     : ACC  = sum(A+i * B+i)      fresh sum, ACC cleared at T0
//   0x0E DOTADD  : ACC += sum(A+i * B+i)      continues the current ACC
//   0x0F DOTSTORE: D[0] = q16(sum); ACC = 0   computes its own dot, stores, clears
//
// A is always stride 1; mode[1:0] selects the B stride: 00 = +1, 01 = +3,
// 10 = +4, 11 is reserved and treated as +1. Lane i therefore reads
// A = base_a + i and B = base_b + i * stride.
//
// The multiplier is the shared CpuV3FpuMulPipe instance in the unit top (the
// core serializes instructions, so this path and the multiply path never
// multiply at once). This controller sequences lanes and hands each to the
// pipe with its destination tag; products return three cycles later and are
// accumulated (or, for DOTSTORE's final lane, captured for the writeback).
//
// gowinsynthesis note: the 64-bit accumulate fuses with the multiplier into a
// MULTADDALU18X18 macro regardless of syn_dspstyle placement (all forms tried);
// the fusion is accepted deliberately (DSP-efficient, passes timing). See
// fpu-design-v2 §14.
//
// Lane timing (II = 1). T0 is the cycle where instr_complete is high:
//   T0         : present the lane-0 read addresses.
//   T(k)       : present the lane-k read addresses (k <= last_lane).
//   T(k+1)     : the lane-k operands are on the buses; the pipe captures them.
//   T(k+3)     : lane k's product returns; it is accumulated this beat.
// The read window is T0..T(last_lane); busy covers T0 through the last
// accumulation (DOTSTORE: through the writeback beat one cycle later).
//
// ACC reset semantics:
//   - abort leaves ACC untouched; it only voids the in-flight pipeline.
//   - DOT clears ACC on its first beat, so a stale value is never accumulated.
//   - DOTSTORE clears ACC after its writeback beat.
module CpuV3FpuDotPath (
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
    // Shared multiplier pipe (in the unit top).
    output wire mul_in_valid,
    output wire [31:0] mul_in_a,
    output wire [31:0] mul_in_b,
    output wire [8:0] mul_in_tag,
    input wire mul_out_valid,
    input wire signed [63:0] mul_out_product,
    input wire [8:0] mul_out_tag,
    // Register-file write port (only DOTSTORE's narrowed sum uses it).
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

// T0 of this cycle: a live supported dot instruction completed and is not
// cancelled. All three subops run the shared lane pipeline.
wire load_now = instr_complete && (is_dot || is_dotadd || is_dotstore) &&
    !abort;

// len 11 is reserved and clamps to vec4, so last_lane stays in 0..3.
wire [2:0] decoded_last_lane =
    (len_field == 2'b11) ? 3'd3 : (len_field + 3'd1);

// mode[1:0] is the B stride: 00 -> +1, 01 -> +3, 10 -> +4, 11 -> +1.
wire [2:0] decoded_stride =
    (mode_field[1:0] == 2'b01) ? 3'd3 :
    (mode_field[1:0] == 2'b10) ? 3'd4 : 3'd1;

// 64-bit Q32.32 accumulator (fabric registers; the accumulate itself fuses
// into the multiplier's DSP macro, see the header note).
reg signed [63:0] acc_r = 64'sd0;

// Latched instruction state, loaded on the T0 edge.
reg run_r = 1'b0;
reg [2:0] lane_r = 3'd0;
reg [2:0] last_lane_r = 3'd0;
reg [5:0] base_a_r = 6'd0;
reg [5:0] base_b_r = 6'd0;
reg [2:0] stride_r = 3'd1;

// Pipe bookkeeping: entries issued minus returned, and (for DOTSTORE) how
// many lanes have been accumulated, so the final product is recognised.
reg [3:0] outstanding_r = 4'd0;
reg store_mode_r = 1'b0;
reg [2:0] acc_lane_r = 3'd0;

// DOTSTORE writeback: the narrowed completed sum, one beat after the final
// accumulation.
reg store_r = 1'b0;
reg [8:0] store_addr_r = 9'd0;
reg [31:0] store_data_r = 32'h00000000;

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

// The product returning this beat completes the DOTSTORE exactly when every
// lane up to last_lane_r has already been accumulated.
wire final_store_lane = mul_out_valid && store_mode_r &&
    (acc_lane_r == last_lane_r);

// Accumulate the returning product into ACC. The complete signed 72-bit
// product is added; the low 64 bits are kept (Q32.32 wrap). The sum that
// includes the final lane is the DOTSTORE writeback value.
wire signed [63:0] acc_plus = acc_r + mul_out_product[63:0];

always @(posedge clk) begin
    if (abort) begin
        run_r <= 1'b0;
        outstanding_r <= 4'd0;
        store_mode_r <= 1'b0;
        store_r <= 1'b0;
        // ACC is deliberately preserved across abort.
    end else if (load_now) begin
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
        outstanding_r <= 4'd0;
        // DOT and DOTSTORE start a fresh sum; DOTADD continues the ACC.
        if (is_dot || is_dotstore)
            acc_r <= 64'sd0;
    end else begin
        // Read sequencer: one lane per beat; it stops one beat after the last
        // lane, so the read window is exactly T0..T(last_lane).
        if (run_r) begin
            if (lane_r > (last_lane_r + 3'd1))
                run_r <= 1'b0;
            else
                lane_r <= lane_r + 3'd1;
        end

        // Pipe bookkeeping: an entry leaves as another returns.
        // The pipe is shared: only this instruction's own entries count.
        // Products returning while we have none outstanding belong to the
        // other path and must not decrement (4-bit wrap would never clear).
        outstanding_r <= outstanding_r + {3'b0, data_valid} -
            {3'b0, mul_out_valid && (outstanding_r != 4'd0)};

        // Accumulate the returning product (full width, no per-lane
        // narrowing). The final lane of a DOTSTORE also captures the narrowed
        // sum for the writeback beat. Only entries this instruction issued
        // count: the pipe is shared, and the multiply path's products must
        // never leak into ACC.
        if (mul_out_valid && (outstanding_r != 4'd0)) begin
            acc_r <= acc_plus;
            acc_lane_r <= acc_lane_r + 3'd1;
            if (final_store_lane) begin
                store_data_r <= acc_plus[47:16];
                store_r <= 1'b1;
                store_mode_r <= 1'b0;
            end
        end

        // DOTSTORE completes its writeback this beat and leaves ACC clean.
        if (store_r) begin
            store_r <= 1'b0;
            acc_r <= 64'sd0;
        end
    end
end

// Shared pipe drive: tags carry no destination for dot (ACC is the target),
// so the tag is just the lane number for observability.
assign mul_in_valid = data_valid;
assign mul_in_a = rf_read_a_data;
assign mul_in_b = rf_read_b_data;
assign mul_in_tag = {6'b0, lane_r - 3'd1};

// The write port is gated combinationally by abort, so an abort on the
// writeback beat cancels the register-file write at that edge.
assign rf_write_enable = store_r && !abort;
assign rf_write_address = store_addr_r;
assign rf_write_data = store_data_r;

assign busy = (load_now || run_r || (outstanding_r != 4'd0) ||
    mul_out_valid || store_r) && !abort;

// Observation port: the ACC register is exposed directly.
assign acc_out = acc_r;

endmodule
