module CpuV3FpuFrontend (
    input wire clk,
    input wire word_valid,
    input wire [15:0] word,
    input wire abort,
    output wire read_valid,
    output wire [8:0] rf_read_a_address,
    output wire [8:0] rf_read_b_address,
    output wire [15:0] word0_raw,
    output wire [15:0] word1_raw,
    output wire [3:0] instr_opcode,
    output wire instr_complete
);

// A 32-bit FPU v2 instruction arrives as two consecutive 16-bit words. This
// leaf tracks whether the next accepted word is the low half (word0) or the
// high half (word1); one state bit is enough for the two-word sequence.
//
// word0 carries the opcode in bits [15:12]:
//   opcode 0xC / 0xD: [11:6] = Fa, [5:0] = Fb
//   opcode 0xE:       [11:8] = X, [7:2] = Fa, [1:0] = kind (no Fb field)
// The register-file read addresses are formed in the word0 cycle itself so the
// synchronous register file can present the operands on the next edge without
// waiting for word1. read_valid qualifies that word0 cycle; the address outputs
// stay driven from word on every cycle, and only read_valid marks a sample.
//
// This leaf stops at the raw 16-bit halves. Splitting word1 into len/subop/mode
// is the downstream controller's job, so only word1_raw is passed on.
//
// abort is only meaningful while waiting for word1. It discards the pending
// word0, produces no instr_complete pulse for the broken pair, and returns the
// frontend to the wait-word0 state. word0_raw keeps its last latched value
// until the next accepted word0 overwrites it, so an aborted pair is never
// observable through instr_complete.
reg waiting_word1 = 0;
reg [15:0] word0_raw_r = 0;
reg [15:0] word1_raw_r = 0;
reg [3:0] instr_opcode_r = 0;
reg instr_complete_r = 0;

wire accept_word0 = word_valid && !waiting_word1;
wire accept_word1 = word_valid && waiting_word1 && !abort;
wire discard_word0 = abort && waiting_word1;

assign read_valid = accept_word0;
// The top three bits are forced to zero for the architectural F0..F63 mapping;
// the register-file leaf itself never remaps an address.
assign rf_read_a_address = (word[15:12] == 4'hE) ?
    {3'b000, word[7:2]} : {3'b000, word[11:6]};
assign rf_read_b_address = {3'b000, word[5:0]};
assign word0_raw = word0_raw_r;
assign word1_raw = word1_raw_r;
assign instr_opcode = instr_opcode_r;
assign instr_complete = instr_complete_r;

// instr_complete is the registered "next cycle" option: it is high for exactly
// one clock cycle, beginning at the edge that accepts word1, and is low in the
// acceptance cycle itself. It is rewritten every edge, so a word0 or an abort
// in the following cycle cannot stretch the pulse.
always @(posedge clk) begin
    instr_complete_r <= accept_word1;
    if (accept_word0) begin
        word0_raw_r <= word;
        instr_opcode_r <= word[15:12];
        waiting_word1 <= 1'b1;
    end else if (accept_word1 || discard_word0) begin
        waiting_word1 <= 1'b0;
    end
    if (accept_word1)
        word1_raw_r <= word;
end

endmodule
