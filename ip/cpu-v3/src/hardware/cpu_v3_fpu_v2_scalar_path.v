// FPU v2 scalar execution-path controller.
//
// A scalar instruction (opcode 0xD) is delivered by CpuV3FpuV2Frontend as a
// two-word pair. Word0 carries Fa in bits [11:6] and Fb in bits [5:0]; the
// frontend forms those register-file read addresses combinationally in the
// word0 cycle, so the synchronous RF presents both operands on the beat shown
// as T0 below. Word1 carries Fd in bits [15:10], the 6-bit subop field in
// bits [9:4] and the mode field in bits [3:0].
//
// Fixed latency, no dynamic waiting:
//   T0 = the cycle where instr_complete is high and instr_opcode == 0xD. The
//        combinational scalar ALU evaluates rf_read_a_data / rf_read_b_data
//        with op = word1_raw[9:4] during this cycle.
//   the edge after T0: the result, the write address and the flags are
//        captured, and the three countdowns are loaded.
//   T1: rf_write_enable is high and the captured word is written back.
//   after T1: busy returns low.
//
// subop == 0xB is CMP: it does not write the register file, it only updates the
// flag_lt / flag_eq / flag_gt registers. Every other instruction leaves those
// flag registers unchanged.
//
// mode (word1_raw[3:0]) is ignored in this stage. The field is reserved for the
// length and rounding controls of a later stage; the pair front-end already
// passes it through, this controller simply does not decode it yet.
//
// The three countdown outputs are the first implementation of the section-16
// resource model:
//   R = read-resource occupancy. The RF read completed before T0, so the
//       countdown is already zero when the instruction is issued.
//   W = write not yet complete. It is high from T0 through the writeback beat.
//   X = execution-resource occupancy. It tracks the same fixed two-beat window
//       as W in this single-latency stage.
// They are driven here as fixed constants rather than by a scheduler because
// this controller has exactly one fixed latency; a later stage replaces the
// constants with the real arbitration.
//
// abort cancels everything in flight: a pending result is not captured, the
// write enable is suppressed (both on the capture edge and, combinationally,
// on the writeback edge), busy drops and all three countdowns are cleared.
module CpuV3FpuV2ScalarPath (
    input wire clk,
    input wire abort,
    input wire instr_complete,
    input wire [3:0] instr_opcode,
    input wire [15:0] word1_raw,
    input wire [31:0] rf_read_a_data,
    input wire [31:0] rf_read_b_data,
    output wire rf_write_enable,
    output wire [8:0] rf_write_address,
    output wire [31:0] rf_write_data,
    output wire flag_lt,
    output wire flag_eq,
    output wire flag_gt,
    output wire busy,
    output wire [3:0] r_wait,
    output wire [3:0] w_wait,
    output wire [3:0] x_wait
);

// Word1 fields. The ISA subop field is the 6-bit word1_raw[9:4]; only its
// low 4 bits reach the 4-bit ALU op port in this stage (opcodes 0x0..0xF).
// mode is word1_raw[3:0] and is intentionally unused in this stage.
wire [5:0] subop = word1_raw[9:4];
wire [5:0] fd = word1_raw[15:10];

wire is_scalar = (instr_opcode == 4'hD);
wire is_cmp = (subop == 6'h0B);
// T0 of this cycle: a live scalar instruction has completed and is not being
// cancelled. It is the only beat that starts work.
wire load_now = instr_complete && is_scalar && !abort;

// Combinational scalar ALU leaf; it owns no register of its own.
wire [31:0] alu_result;
wire alu_lt;
wire alu_eq;
wire alu_gt;
CpuV3FpuV2ScalarAlu scalar_alu (
    .a(rf_read_a_data),
    .b(rf_read_b_data),
    .op(subop[3:0]),
    .result(alu_result),
    .flag_lt(alu_lt),
    .flag_eq(alu_eq),
    .flag_gt(alu_gt)
);

// Captured writeback payload and the comparison flag registers.
reg write_enable_r = 1'b0;
reg [8:0] write_address_r = 9'd0;
reg [31:0] write_data_r = 32'h00000000;
reg flag_lt_r = 1'b0;
reg flag_eq_r = 1'b0;
reg flag_gt_r = 1'b0;

// Section-16 countdowns. The T0 beat is itself the first count, so the value
// stored after the load edge is one less than the value presented during T0;
// the presented value reads 2, 1, 0 over T0, T1, T2.
reg [3:0] r_count = 4'd0;
reg [3:0] w_count = 4'd0;
reg [3:0] x_count = 4'd0;

always @(posedge clk) begin
    if (abort) begin
        write_enable_r <= 1'b0;
        r_count <= 4'd0;
        w_count <= 4'd0;
        x_count <= 4'd0;
    end else begin
        // A captured result occupies the write resource for exactly one beat.
        write_enable_r <= load_now && !is_cmp;
        if (load_now) begin
            write_address_r <= {3'b000, fd};
            write_data_r <= alu_result;
            // CMP only refreshes the flag registers.
            if (is_cmp) begin
                flag_lt_r <= alu_lt;
                flag_eq_r <= alu_eq;
                flag_gt_r <= alu_gt;
            end
            r_count <= 4'd0;
            w_count <= 4'd1;
            x_count <= 4'd1;
        end else begin
            if (r_count != 4'd0)
                r_count <= r_count - 4'd1;
            if (w_count != 4'd0)
                w_count <= w_count - 4'd1;
            if (x_count != 4'd0)
                x_count <= x_count - 4'd1;
        end
    end
end

// abort also gates the write port combinationally, so an abort asserted during
// the writeback beat cancels the register-file write at that edge.
assign rf_write_enable = write_enable_r && !abort;
assign rf_write_address = write_address_r;
assign rf_write_data = write_data_r;
assign flag_lt = flag_lt_r;
assign flag_eq = flag_eq_r;
assign flag_gt = flag_gt_r;

// On the load beat the presented countdowns are 2 / 2 / 0 (R already done);
// afterwards they follow the stored, pre-decremented values.
assign r_wait = load_now ? 4'd0 : r_count;
assign w_wait = load_now ? 4'd2 : w_count;
assign x_wait = load_now ? 4'd2 : x_count;

assign busy = (w_wait != 4'd0);

endmodule
