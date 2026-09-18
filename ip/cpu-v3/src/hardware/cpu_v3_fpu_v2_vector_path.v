// FPU v2 vector execution-path controller (opcode 0xC, subops of section 6.3).
//
// A vector instruction is delivered by CpuV3FpuV2Frontend as a two-word pair.
// Word0 carries Fa in bits [11:6] and Fb in bits [5:0]; the parent latches both
// 6-bit bases and presents them as base_a / base_b on the beat this module
// starts. Word1 carries Fd in bits [15:10], len in bits [9:8], the 5-bit subop
// in bits [7:3] and mode in bits [2:0]:
//
//   00 = vec2 (2 lanes), 01 = vec3 (3 lanes), 10 = vec4 (4 lanes), 11 reserved
//
// A vector is a consecutive register-range view, not an architectural type:
// lane i computes
//   A = Fa + i, B = Fb + i, D = Fd + i   (i = 0 .. last_lane)
// with last_lane = len + 1, so vec2 has last_lane 1 and vec4 has last_lane 3.
// All arithmetic is Q16.16 with wrap, exactly the scalar rule. Partial overlap
// between the destination and a source range is a software contract; this
// hardware never checks it.
//
// Supported subops (section 6.3): 0x00 VADD, 0x01 VSUB, 0x04 VMIN, 0x05 VMAX,
// 0x06 VABS (B ignored), 0x07 VNEG (B ignored), 0x0C VMOV (B ignored). They map
// onto the combinational CpuV3FpuV2ScalarAlu leaf, one lane per cycle, so the
// vector path adds no second arithmetic datapath. Any other subop maps to an
// ALU op code the leaf does not list, so it yields the leaf's defined zero
// rather than propagating an X.
//
// Lane pipeline (II = 1). T0 is the cycle where instr_complete is high and
// instr_opcode == 0xC:
//   T0         : present the lane-0 read addresses (Fa, Fb) combinationally.
//   edge after : the synchronous RF captures lane 0; base/len/subop/Fd latch.
//   T(k)       : present the lane-k read addresses (k <= last_lane) while the
//                RF data captured at T(k-1) is evaluated by the scalar ALU.
//   T(i+2)     : lane i is captured at the end of T(i+1) and written now.
// So lane i's write lands one beat after lane i+1's read address is issued, and
// the read+write window is last_lane + 3 beats:
//   reads  at T0 .. T(last_lane)
//   writes at T2 .. T(last_lane + 2)
//   busy   at T0 .. T(last_lane + 2)
//
// abort clears the controller immediately: the RF write port is gated
// combinationally as well as cleared on the edge, and busy drops to zero in the
// same cycle.
module CpuV3FpuV2VectorPath (
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

// Word1 fields. mode (word1_raw[2:0]) is reserved and ignored in this stage.
wire [1:0] len_field = word1_raw[9:8];
wire [4:0] subop = word1_raw[7:3];

wire is_vector = (instr_opcode == 4'hC);
// T0 of this cycle: a live vector instruction completed and is not cancelled.
wire load_now = instr_complete && is_vector && !abort;

// subop -> scalar ALU op. The seven supported subops select the exact entries
// of the scalar leaf; unlisted subops select 4'h2, which the leaf does not
// list, so the result is its defined zero.
reg [3:0] alu_op;
always @(*) begin
    case (subop)
        5'h00: alu_op = 4'h0; // VADD
        5'h01: alu_op = 4'h1; // VSUB
        5'h04: alu_op = 4'h3; // VMIN
        5'h05: alu_op = 4'h4; // VMAX
        5'h06: alu_op = 4'h5; // VABS
        5'h07: alu_op = 4'h6; // VNEG
        5'h0C: alu_op = 4'hF; // VMOV
        default: alu_op = 4'h2; // unlisted -> leaf returns zero
    endcase
end

// One combinational scalar ALU instance is shared by every lane; the lane
// sequencer reuses it once per cycle. The comparison flags are unused here.
wire [31:0] alu_result;
CpuV3FpuV2ScalarAlu vector_alu (
    .a(rf_read_a_data),
    .b(rf_read_b_data),
    .op(alu_op),
    .result(alu_result),
    .flag_lt(),
    .flag_eq(),
    .flag_gt()
);

// Latched instruction state, loaded on the T0 edge.
reg run_r = 1'b0;
reg [2:0] lane_r = 3'd0;
reg [2:0] last_lane_r = 3'd0;
reg [5:0] base_a_r = 6'd0;
reg [5:0] base_b_r = 6'd0;
reg [5:0] fd_r = 6'd0;

// Captured writeback stage: one lane per cycle.
reg wr_enable_r = 1'b0;
reg [8:0] wr_address_r = 9'd0;
reg [31:0] wr_data_r = 32'h00000000;

// Field decode at the T0 edge. len 11 is reserved; it is clamped to vec4 so the
// controller always leaves a defined state.
wire [2:0] decoded_last_lane =
    (len_field == 2'b11) ? 3'd3 : (len_field + 3'd1);

// Read-address generation for the lane presented this cycle. During T0 the
// unlatched word0 bases are used directly; afterwards the latched bases and the
// incrementing lane index are used.
wire reading = run_r && !abort && (lane_r <= last_lane_r);
wire [5:0] read_index_a = base_a_r + lane_r;
wire [5:0] read_index_b = base_b_r + lane_r;

assign rf_read_a_address =
    load_now ? {3'b000, base_a} :
    (reading ? {3'b000, read_index_a} : 9'd0);
assign rf_read_b_address =
    load_now ? {3'b000, base_b} :
    (reading ? {3'b000, read_index_b} : 9'd0);

// The lane whose RF data is on the bus this cycle, and the register index that
// data maps to. The writeback stage is one cycle behind the read stage.
wire data_valid = run_r && !abort &&
    (lane_r >= 3'd1) && ((lane_r - 3'd1) <= last_lane_r);
wire [2:0] data_lane = lane_r - 3'd1;
wire [5:0] write_index = fd_r + {3'b000, data_lane};

always @(posedge clk) begin
    if (abort) begin
        run_r <= 1'b0;
        wr_enable_r <= 1'b0;
    end else if (load_now) begin
        run_r <= 1'b1;
        lane_r <= 3'd1;
        last_lane_r <= decoded_last_lane;
        base_a_r <= base_a;
        base_b_r <= base_b;
        fd_r <= word1_raw[15:10];
        // No lane can be written before T2, so the stage starts empty.
        wr_enable_r <= 1'b0;
    end else if (run_r) begin
        // Capture the lane whose data was latched at the previous edge.
        wr_enable_r <= data_valid;
        if (data_valid) begin
            wr_address_r <= {3'b000, write_index};
            wr_data_r <= alu_result;
        end
        // The read stage is done once lane_r passes the last lane by one; from
        // then on only the final writeback remains before run_r drops.
        if (lane_r > (last_lane_r + 3'd1))
            run_r <= 1'b0;
        else
            lane_r <= lane_r + 3'd1;
    end else begin
        wr_enable_r <= 1'b0;
    end
end

assign rf_write_enable = wr_enable_r && !abort;
assign rf_write_address = wr_address_r;
assign rf_write_data = wr_data_r;

assign busy = (load_now || run_r || wr_enable_r) && !abort;

endmodule
