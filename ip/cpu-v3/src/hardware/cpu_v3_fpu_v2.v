// FPU v2 unit top level.
//
// This module is the integration boundary between the CPU V3 core and the
// FPU v2 leaves built so far: the two-word front-end, the 2R1W BSRAM register
// file and the scalar execution path. It is deliberately thin: it wires the
// three leaves together and exposes the external memory channel the core uses
// to implement FLD/FST until the internal store buffer of Stage 6 replaces it.
//
// Two-word instruction flow (fpu design doc, section 26.2):
//   - word0 (opcode in bits [15:12], one of 0xC / 0xD / 0xE) is presented for
//     one word_valid beat. The front-end decodes Fa/Fb and drives the two
//     register-file read addresses from that word.
//   - word1 is presented on the following word_valid beat. instr_complete
//     pulses one cycle after that beat, and that is the scalar path's T0.
//
// Operand read-address hold:
//   The front-end forms rf_read_a/b_address combinationally from `word`, so
//   during the word1 beat those addresses reflect word1 fields, not word0.
//   The scalar path samples its operands at T0, one beat after word1. The
//   synchronous register file must therefore still be addressed with the
//   word0 address during the word1 beat, so the data it latches at the end of
//   that beat is available at T0. This top captures the front-end address on
//   read_valid (the word0 beat) and feeds the RF from that capture. It is the
//   same one-beat hold the scalar-path leaf testbench applies; the front-end
//   leaf itself is unchanged.
//
// External memory channel (ext_*):
//   Contract: the core asserts ext_access only while the unit is idle, that is
//   while busy == 0 and no instruction word pair is in flight. Under that
//   guarantee ext_access takes over the register-file write port and read
//   port B:
//     - ext_write_enable / ext_write_address / ext_write_data drive the RF
//       write port and mask the scalar path's write port.
//     - ext_read_address drives RF read port B, and ext_read_data is that
//       port's registered data.
//   RF read port A always belongs to the front-end: FLD only writes and FST
//   only reads port B, so no read port A mux is needed. This whole channel is
//   the first-version FLD/FST implementation and is removed when the internal
//   store buffer arrives in Stage 6.
module CpuV3FpuV2 (
    input wire clk,
    input wire abort,
    input wire word_valid,
    input wire [15:0] word,
    output wire busy,
    output wire flag_lt,
    output wire flag_eq,
    output wire flag_gt,
    output wire instr_complete,
    input wire ext_access,
    input wire ext_write_enable,
    input wire [8:0] ext_write_address,
    input wire [31:0] ext_write_data,
    input wire [8:0] ext_read_address,
    output wire [31:0] ext_read_data
);

// Front-end leaf: two-word pair decode and operand-address generation.
wire read_valid;
wire [8:0] fe_read_a_address;
wire [8:0] fe_read_b_address;
wire [15:0] word0_raw;
wire [15:0] word1_raw;
wire [3:0] instr_opcode;
wire fe_instr_complete;

CpuV3FpuV2Frontend frontend (
    .clk(clk),
    .word_valid(word_valid),
    .word(word),
    .abort(abort),
    .read_valid(read_valid),
    .rf_read_a_address(fe_read_a_address),
    .rf_read_b_address(fe_read_b_address),
    .word0_raw(word0_raw),
    .word1_raw(word1_raw),
    .instr_opcode(instr_opcode),
    .instr_complete(fe_instr_complete)
);

// One-beat operand-address hold, captured on the word0 beat (read_valid).
reg [8:0] held_read_a_address = 9'd0;
reg [8:0] held_read_b_address = 9'd0;
always @(posedge clk) begin
    if (read_valid) begin
        held_read_a_address <= fe_read_a_address;
        held_read_b_address <= fe_read_b_address;
    end
end

// Read port A stays with the front-end. Read port B switches to the external
// address while the core owns the register file.
wire [8:0] rf_read_a_address = held_read_a_address;
wire [8:0] rf_read_b_address =
    ext_access ? ext_read_address : held_read_b_address;

// Scalar path write port. While ext_access is high the external channel owns
// the RF write port and the scalar write port is masked.
wire sp_write_enable;
wire [8:0] sp_write_address;
wire [31:0] sp_write_data;

wire rf_write_enable = ext_access ? ext_write_enable : sp_write_enable;
wire [8:0] rf_write_address =
    ext_access ? ext_write_address : sp_write_address;
wire [31:0] rf_write_data =
    ext_access ? ext_write_data : sp_write_data;

wire [31:0] rf_read_a_data;
wire [31:0] rf_read_b_data;

CpuV3FpuV2RegisterRam rf (
    .clk(clk),
    .write_enable(rf_write_enable),
    .write_address(rf_write_address),
    .write_data(rf_write_data),
    .read_a_address(rf_read_a_address),
    .read_b_address(rf_read_b_address),
    .read_a_data(rf_read_a_data),
    .read_b_data(rf_read_b_data)
);

// Scalar execution path. It consumes the front-end pair and the RF operand
// data; abort reaches both the front-end and the path.
CpuV3FpuV2ScalarPath scalar_path (
    .clk(clk),
    .abort(abort),
    .instr_complete(fe_instr_complete),
    .instr_opcode(instr_opcode),
    .word1_raw(word1_raw),
    .rf_read_a_data(rf_read_a_data),
    .rf_read_b_data(rf_read_b_data),
    .rf_write_enable(sp_write_enable),
    .rf_write_address(sp_write_address),
    .rf_write_data(sp_write_data),
    .flag_lt(flag_lt),
    .flag_eq(flag_eq),
    .flag_gt(flag_gt),
    .busy(busy)
);

assign instr_complete = fe_instr_complete;
assign ext_read_data = rf_read_b_data;

endmodule
