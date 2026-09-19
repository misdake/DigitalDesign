// FPU v2 multiply execution-path controller.
//
// One module covers the whole multiply family of the FPU v2 instruction set:
//
//   opcode 0xC (VECTOR) subop 0x02 VMUL : D+i = q16(A+i * B+i)
//   opcode 0xC (VECTOR) subop 0x03 VMULS: D+i = q16(A+i * B0)
//   opcode 0xD (SCALAR) subop 0x02 MUL : Fd   = q16(Fa * Fb)
//
// The instruction word split follows CpuV3FpuV2Frontend. A vector word1 is
// {Fd[15:10], len[9:8], subop[7:3], mode[2:0]} with len 00 = vec2, 01 = vec3,
// 10 = vec4 and 11 clamped to vec4 (last_lane = len + 1). A scalar word1 is
// {Fd[15:10], subop[9:4], mode[3:0]}. The parent latches the two 6-bit register
// bases and presents them as base_a / base_b on the beat this instruction
// starts, exactly as for the other FPU v2 execution paths.
//
// A vector is a consecutive register-range view, not an architectural type:
// lane i computes A = Fa + i, B = Fb + i, D = Fd + i. VMULS reuses the lane-0
// B operand (base_b) for every lane, so the B read address is constant. A
// scalar MUL is the single-lane case (last_lane = 0) with B = base_b.
//
// All arithmetic is Q16.16 with wrap: each 32-bit signed operand is
// sign-extended to 36 bits, multiplied into a 72-bit product, and narrowed at
// the write port by taking product[47:16]. The multiplier is written as an
// inferred $signed(a) * $signed(b) so the synthesis tool can map it to a DSP
// block; no hard macro (e.g. MULT36X36) is instantiated.
//
// Multiply pipeline (II = 1, three register stages after the operand bus). T0
// is the cycle where instr_complete is high:
//   T0         : present the lane-0 read addresses (base_a, base_b).
//   edge after : the synchronous RF captures lane 0; the run state latches.
//   T(k)       : present the lane-k read addresses (k <= last_lane) while the
//                lane (k-1) operands sit on the read buses.
//   T(k+1)     : lane k operands are on the buses; stage 1 latches them.
//   T(k+3)     : the lane-k product is ready after stage 2.
//   T(k+4)     : stage 3 narrows the product and lane k is written back.
// So the read window is T0 .. T(last_lane), the write window is
// T4 .. T(last_lane + 4) and busy covers T0 .. T(last_lane + 4). The write
// address travels next to the data through the stage registers instead of
// being recomputed from a cycle counter.
//
// abort discards every lane still inside the pipeline, keeps every lane already
// written, gates the write port combinationally and drops busy in the same
// cycle.
module CpuV3FpuV2MultiplyPath (
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
    output wire busy
);

wire is_vector = (instr_opcode == 4'hC);
wire is_scalar = (instr_opcode == 4'hD);

// Word1 fields. For a vector the 5-bit subop is word1_raw[7:3]; for a scalar
// the 6-bit subop is word1_raw[9:4]. Fd is word1_raw[15:10] in both encodings.
wire [1:0] len_field = word1_raw[9:8];
wire [4:0] vec_subop = word1_raw[7:3];
wire [5:0] scalar_subop = word1_raw[9:4];
wire [5:0] fd_field = word1_raw[15:10];

wire is_vmuls = is_vector && (vec_subop == 5'h03);

// T0 of this cycle: a live supported multiply completed and is not cancelled.
wire load_vector = instr_complete && is_vector &&
    ((vec_subop == 5'h02) || (vec_subop == 5'h03)) && !abort;
wire load_scalar = instr_complete && is_scalar &&
    (scalar_subop == 6'h02) && !abort;
wire load_now = load_vector || load_scalar;

// len 11 is reserved; it is clamped to vec4 so the controller always leaves a
// defined state. A scalar MUL is a single lane.
wire [2:0] decoded_last_lane =
    is_scalar ? 3'd0 :
    (len_field == 2'b11) ? 3'd3 : (len_field + 3'd1);

// Latched instruction state, loaded on the T0 edge.
reg run_r = 1'b0;
reg [2:0] lane_r = 3'd0;
reg [2:0] last_lane_r = 3'd0;
reg [5:0] base_a_r = 6'd0;
reg [5:0] base_b_r = 6'd0;
reg is_vmuls_r = 1'b0;
// Write address for the lane whose operands are on the buses this cycle. It is
// seeded with Fd and incremented in lock step with the read lane, then carried
// through the three pipeline stages.
reg [8:0] waddr_r = 9'd0;

// Stage 1: latched operands.
reg s1_valid_r = 1'b0;
reg signed [35:0] s1_a_r = 36'sd0;
reg signed [35:0] s1_b_r = 36'sd0;
reg [8:0] s1_waddr_r = 9'd0;

// Stage 2: first product register.
reg s2_valid_r = 1'b0;
reg signed [71:0] s2_prod_r = 72'sd0;
reg [8:0] s2_waddr_r = 9'd0;

// Stage 3: second product register; narrowing is combinational at the port.
reg s3_valid_r = 1'b0;
reg signed [71:0] s3_prod_r = 72'sd0;
reg [8:0] s3_waddr_r = 9'd0;

// Read-address generation for the lane presented this cycle. During T0 the
// unlatched bases are used directly; afterwards the latched bases and the
// incrementing lane index are used. VMULS holds B at its lane-0 base.
wire reading = run_r && !abort && (lane_r <= last_lane_r);
wire [5:0] read_index_a = base_a_r + lane_r;
wire [5:0] read_index_b = is_vmuls_r ? base_b_r : (base_b_r + lane_r);

assign rf_read_a_address =
    load_now ? {3'b000, base_a} :
    (reading ? {3'b000, read_index_a} : 9'd0);
assign rf_read_b_address =
    load_now ? {3'b000, base_b} :
    (reading ? {3'b000, read_index_b} : 9'd0);

// The lane whose RF data is on the buses this cycle. The RF captured the read
// address presented on the previous beat, so it is one lane behind the read
// index.
wire data_valid = run_r && !abort &&
    (lane_r >= 3'd1) && ((lane_r - 3'd1) <= last_lane_r);

// Inferred 36x36 -> 72-bit signed multiplier (two operand registers are the
// stage-1 state, this product is registered by stage 2).
wire signed [71:0] product_next = $signed(s1_a_r) * $signed(s1_b_r);

always @(posedge clk) begin
    if (abort) begin
        run_r <= 1'b0;
        s1_valid_r <= 1'b0;
        s2_valid_r <= 1'b0;
        s3_valid_r <= 1'b0;
    end else if (load_now) begin
        run_r <= 1'b1;
        lane_r <= 3'd1;
        last_lane_r <= decoded_last_lane;
        base_a_r <= base_a;
        base_b_r <= base_b;
        is_vmuls_r <= is_vmuls;
        waddr_r <= {3'b000, fd_field};
        // No lane reaches a write stage before T4, so the pipeline starts
        // empty.
        s1_valid_r <= 1'b0;
        s2_valid_r <= 1'b0;
        s3_valid_r <= 1'b0;
    end else begin
        // Read sequencer: lane_r walks one lane per beat while the run is
        // live, and the write address follows it.
        if (run_r) begin
            if (lane_r > (last_lane_r + 3'd1))
                run_r <= 1'b0;
            else
                lane_r <= lane_r + 3'd1;
            waddr_r <= waddr_r + 9'd1;
        end

        // Stage 1 captures the operands (and the write address) present on the
        // buses this beat.
        s1_valid_r <= data_valid;
        if (data_valid) begin
            s1_a_r <= $signed(rf_read_a_data);
            s1_b_r <= $signed(rf_read_b_data);
            s1_waddr_r <= waddr_r;
        end

        // Stage 2 captures the product.
        s2_valid_r <= s1_valid_r;
        if (s1_valid_r) begin
            s2_prod_r <= product_next;
            s2_waddr_r <= s1_waddr_r;
        end

        // Stage 3 captures the product again; the port narrows it.
        s3_valid_r <= s2_valid_r;
        if (s2_valid_r) begin
            s3_prod_r <= s2_prod_r;
            s3_waddr_r <= s2_waddr_r;
        end
    end
end

// abort also gates the write port combinationally, so an abort asserted during
// the writeback beat cancels the register-file write at that edge.
assign rf_write_enable = s3_valid_r && !abort;
assign rf_write_address = s3_waddr_r;
assign rf_write_data = s3_prod_r[47:16];

assign busy = (load_now || run_r || s1_valid_r || s2_valid_r || s3_valid_r) &&
    !abort;

endmodule
