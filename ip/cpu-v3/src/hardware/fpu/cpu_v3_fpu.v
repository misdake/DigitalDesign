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
module CpuV3Fpu (
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

CpuV3FpuFrontend frontend (
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

// Read-port ownership: the vector, multiply and dot paths drive both ports for
// their whole busy windows (starting at their T0), otherwise port A serves
// the front-end's held operand address and port B switches to the external
// channel while the core owns the register file. The special-function path is
// blocking (design section 4.2) and owns both ports while active, so it sits at
// the head of each mux. The execution paths are never busy at once (the core
// serializes instructions), and the multiply mux arm simply takes precedence so
// the select stays defined.
wire vp_busy;
wire [8:0] vp_read_a_address;
wire [8:0] vp_read_b_address;
wire mp_busy;
wire [8:0] mp_read_a_address;
wire [8:0] mp_read_b_address;
wire dp_busy;
wire [8:0] dp_read_a_address;
wire [8:0] dp_read_b_address;
wire sf_busy;
wire [8:0] sf_read_a_address;
wire [8:0] sf_read_b_address;
wire [8:0] rf_read_a_address =
    sf_busy ? sf_read_a_address :
    dp_busy ? dp_read_a_address :
    mp_busy ? mp_read_a_address :
    vp_busy ? vp_read_a_address : held_read_a_address;
wire [8:0] rf_read_b_address =
    sf_busy ? sf_read_b_address :
    dp_busy ? dp_read_b_address :
    mp_busy ? mp_read_b_address :
    vp_busy ? vp_read_b_address :
    ext_access ? ext_read_address : held_read_b_address;

// Scalar path write port. While ext_access is high the external channel owns
// the RF write port and the scalar write port is masked.
wire sp_write_enable;
wire [8:0] sp_write_address;
wire [31:0] sp_write_data;
wire sp_busy;

wire vp_write_enable;
wire [8:0] vp_write_address;
wire [31:0] vp_write_data;
wire mp_write_enable;
wire [8:0] mp_write_address;
wire [31:0] mp_write_data;
wire dp_write_enable;
wire [8:0] dp_write_address;
wire [31:0] dp_write_data;
wire sf_write_enable;
wire [8:0] sf_write_address;
wire [31:0] sf_write_data;
wire rf_write_enable = ext_access ? ext_write_enable :
                       vp_write_enable | mp_write_enable | dp_write_enable |
                       sp_write_enable | sf_write_enable;
wire [8:0] rf_write_address =
    ext_access ? ext_write_address :
    vp_write_enable ? vp_write_address :
    mp_write_enable ? mp_write_address :
    dp_write_enable ? dp_write_address :
    sf_write_enable ? sf_write_address : sp_write_address;
wire [31:0] rf_write_data =
    ext_access ? ext_write_data :
    vp_write_enable ? vp_write_data :
    mp_write_enable ? mp_write_data :
    dp_write_enable ? dp_write_data :
    sf_write_enable ? sf_write_data : sp_write_data;

wire [31:0] rf_read_a_data;
wire [31:0] rf_read_b_data;

CpuV3FpuRegisterRam rf (
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
CpuV3FpuScalarPath scalar_path (
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
    .busy(sp_busy)
);

// Special-function execution path (opcode 0xD subops 0x0C RCP and 0x0D
// RSQRT). It is blocking: while active it owns both RF read ports and drives
// its own LUT addresses into the two asymmetric mirrors. Subop 0x0E (SINCOS)
// is deliberately not routed anywhere in this step: it is a defined no-op
// until the SINCOS datapath lands, and the scalar path filters it out too.
CpuV3FpuSpecialPath special_path (
    .clk(clk),
    .abort(abort),
    .instr_complete(fe_instr_complete),
    .instr_opcode(instr_opcode),
    .word1_raw(word1_raw),
    .rf_read_a_data(rf_read_a_data),
    .rf_read_b_data(rf_read_b_data),
    .rf_read_a_address(sf_read_a_address),
    .rf_read_b_address(sf_read_b_address),
    .rf_write_enable(sf_write_enable),
    .rf_write_address(sf_write_address),
    .rf_write_data(sf_write_data),
    .busy(sf_busy),
    .r_wait(),
    .w_wait(),
    .x_wait()
);

// Vector execution path (opcode 0xC). It consumes the same front-end pair;
// the bases come from the front-end's latched word0.
CpuV3FpuVectorPath vector_path (
    .clk(clk),
    .abort(abort),
    .instr_complete(fe_instr_complete),
    .instr_opcode(instr_opcode),
    .word1_raw(word1_raw),
    .base_a(word0_raw[11:6]),
    .base_b(word0_raw[5:0]),
    .rf_read_a_data(rf_read_a_data),
    .rf_read_b_data(rf_read_b_data),
    .rf_read_a_address(vp_read_a_address),
    .rf_read_b_address(vp_read_b_address),
    .rf_write_enable(vp_write_enable),
    .rf_write_address(vp_write_address),
    .rf_write_data(vp_write_data),
    .busy(vp_busy)
);

// Shared 36x36 multiply pipe. The core serializes instructions, so the
// multiply and dot paths are never active at once; the operand buses are
// muxed by which path is busy (multiply wins the tie so the select is
// always defined).
wire mp_mul_in_valid;
wire [31:0] mp_mul_in_a;
wire [31:0] mp_mul_in_b;
wire [8:0] mp_mul_in_tag;
wire dp_mul_in_valid;
wire [31:0] dp_mul_in_a;
wire [31:0] dp_mul_in_b;
wire [8:0] dp_mul_in_tag;
wire mul_in_valid = mp_busy ? mp_mul_in_valid : dp_mul_in_valid;
wire [31:0] mul_in_a = mp_busy ? mp_mul_in_a : dp_mul_in_a;
wire [31:0] mul_in_b = mp_busy ? mp_mul_in_b : dp_mul_in_b;
wire [8:0] mul_in_tag = mp_busy ? mp_mul_in_tag : dp_mul_in_tag;
wire mul_out_valid;
wire signed [63:0] mul_out_product;
wire [8:0] mul_out_tag;

CpuV3FpuMulPipe mul_pipe (
    .clk(clk),
    .abort(abort),
    .in_valid(mul_in_valid),
    .in_a(mul_in_a),
    .in_b(mul_in_b),
    .in_tag(mul_in_tag),
    .out_valid(mul_out_valid),
    .out_product(mul_out_product),
    .out_tag(mul_out_tag)
);

// Multiply execution path (VMUL/VMULS/scalar MUL). Same front-end pair
// contract as the vector path.
CpuV3FpuMultiplyPath multiply_path (
    .clk(clk),
    .abort(abort),
    .instr_complete(fe_instr_complete),
    .instr_opcode(instr_opcode),
    .word1_raw(word1_raw),
    .base_a(word0_raw[11:6]),
    .base_b(word0_raw[5:0]),
    .rf_read_a_data(rf_read_a_data),
    .rf_read_b_data(rf_read_b_data),
    .rf_read_a_address(mp_read_a_address),
    .rf_read_b_address(mp_read_b_address),
    .mul_in_valid(mp_mul_in_valid),
    .mul_in_a(mp_mul_in_a),
    .mul_in_b(mp_mul_in_b),
    .mul_in_tag(mp_mul_in_tag),
    .mul_out_valid(mul_out_valid),
    .mul_out_product(mul_out_product),
    .mul_out_tag(mul_out_tag),
    .rf_write_enable(mp_write_enable),
    .rf_write_address(mp_write_address),
    .rf_write_data(mp_write_data),
    .busy(mp_busy)
);

// Dot-product path (DOT/DOTADD/DOTSTORE, opcode 0xC subops 0x0D..0x0F).
// Same front-end pair contract as the vector path.
wire [63:0] dp_acc;
CpuV3FpuDotPath dot_path (
    .clk(clk),
    .abort(abort),
    .instr_complete(fe_instr_complete),
    .instr_opcode(instr_opcode),
    .word1_raw(word1_raw),
    .base_a(word0_raw[11:6]),
    .base_b(word0_raw[5:0]),
    .rf_read_a_data(rf_read_a_data),
    .rf_read_b_data(rf_read_b_data),
    .rf_read_a_address(dp_read_a_address),
    .rf_read_b_address(dp_read_b_address),
    .mul_in_valid(dp_mul_in_valid),
    .mul_in_a(dp_mul_in_a),
    .mul_in_b(dp_mul_in_b),
    .mul_in_tag(dp_mul_in_tag),
    .mul_out_valid(mul_out_valid),
    .mul_out_product(mul_out_product),
    .mul_out_tag(mul_out_tag),
    .rf_write_enable(dp_write_enable),
    .rf_write_address(dp_write_address),
    .rf_write_data(dp_write_data),
    .busy(dp_busy),
    .acc_out(dp_acc)
);

assign busy = sp_busy | vp_busy | mp_busy | dp_busy | sf_busy;
assign instr_complete = fe_instr_complete;
assign ext_read_data = rf_read_b_data;

endmodule
