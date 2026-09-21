// FPU v2 multiply execution-path controller.
//
// One module covers the whole multiply family of the FPU v2 instruction set:
//
//   opcode 0xC (VECTOR) subop 0x02 VMUL : D+i = q16(A+i * B+i)
//   opcode 0xC (VECTOR) subop 0x03 VMULS: D+i = q16(A+i * B0)
//   opcode 0xD (SCALAR) subop 0x02 MUL : Fd   = q16(Fa * Fb)
//
// The instruction word split follows CpuV3FpuFrontend. A vector word1 is
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
// All arithmetic is Q16.16 with wrap: products are narrowed at the write port
// by taking product[47:16]. The multiplier itself is the shared
// CpuV3FpuMulPipe instance in the unit top (the core serializes
// instructions, so this path, the dot path and SINCOS never multiply at
// once). This
// controller sequences the lanes and hands each to the pipe with its
// destination tag; the tag comes back with the product.
//
// Lane timing (II = 1 into the pipe). T0 is the cycle where instr_complete is
// high and the opcode/subop select this path:
//   T0         : present the lane-0 read addresses (base_a, base_b).
//   edge after : the synchronous RF captures lane 0; the run state latches.
//   T(k)       : present the lane-k read addresses (k <= last_lane) while the
//                lane (k-1) operands sit on the read buses.
//   T(k+1)     : the lane-k operands are on the buses; in_valid fires and the
//                pipe captures them (mul stage 1).
//   T(k+3)     : out_valid for lane k; lane k is written back with the
//                narrowed product this beat.
// So the read window is T0 .. T(last_lane), and busy covers T0 through the
// beat where the last lane's product returns.
//
// abort discards every in-flight lane (the shared pipe voids them), keeps
// every lane already written, gates the write port combinationally and drops
// busy in the same cycle.
module CpuV3FpuMultiplyPath (
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
    // Shared multiplier pipe (in the unit top). The operands are sign-extended
    // to the pipe's signed-36 buses so the architectural signed-32 Q16.16
    // values keep their exact product.
    output wire mul_in_valid,
    output wire signed [35:0] mul_in_a,
    output wire signed [35:0] mul_in_b,
    output wire [8:0] mul_in_tag,
    input wire mul_out_valid,
    input wire signed [63:0] mul_out_product,
    input wire [8:0] mul_out_tag,
    // Register-file write port.
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
// Destination tag for the lane whose operands are on the buses this cycle;
// seeded with Fd and incremented in lock step with the read lane.
reg [8:0] waddr_r = 9'd0;
// Entries issued to the shared pipe but not yet returned. busy must cover
// the drain.
reg [3:0] outstanding_r = 4'd0;

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

// The lane whose RF data is on the buses this cycle sits one lane behind the
// read index, because the synchronous RF latched the previous beat's address.
wire data_valid = run_r && !abort &&
    (lane_r >= 3'd1) && ((lane_r - 3'd1) <= last_lane_r);

always @(posedge clk) begin
    if (abort) begin
        run_r <= 1'b0;
        outstanding_r <= 4'd0;
    end else if (load_now) begin
        run_r <= 1'b1;
        lane_r <= 3'd1;
        last_lane_r <= decoded_last_lane;
        base_a_r <= base_a;
        base_b_r <= base_b;
        is_vmuls_r <= is_vmuls;
        waddr_r <= {3'b000, fd_field};
        // Nothing of this instruction is in the pipe yet.
        outstanding_r <= 4'd0;
    end else begin
        if (run_r) begin
            if (lane_r > (last_lane_r + 3'd1))
                run_r <= 1'b0;
            else
                lane_r <= lane_r + 3'd1;
            waddr_r <= waddr_r + 9'd1;
        end
        // The pipe is shared: only this instruction's own entries count.
        // Products returning while we have none outstanding belong to the
        // other path and must not decrement (4-bit wrap would never clear).
        outstanding_r <= outstanding_r + {3'b0, data_valid} -
            {3'b0, mul_out_valid && (outstanding_r != 4'd0)};
    end
end

// Shared pipe drive.
assign mul_in_valid = data_valid;
assign mul_in_a = {{4{rf_read_a_data[31]}}, rf_read_a_data};
assign mul_in_b = {{4{rf_read_b_data[31]}}, rf_read_b_data};
assign mul_in_tag = waddr_r;

// The write port fires only for entries this instruction issued: the pipe is
// shared, and the dot path's products must never leak into an RF write here.
// abort also gates the port combinationally, so an abort asserted during a
// writeback beat cancels the register-file write at that edge.
assign rf_write_enable = mul_out_valid && (outstanding_r != 4'd0) && !abort;
assign rf_write_address = mul_out_tag;
assign rf_write_data = mul_out_product[47:16];

assign busy = (load_now || run_r || (outstanding_r != 4'd0) ||
    mul_out_valid) && !abort;

endmodule
