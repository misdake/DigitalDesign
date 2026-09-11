//! Encoding helpers for CpuV3 revision 0.8.

pub type Word = u16;
pub type Register = u8;

pub const LINK_REGISTER: Register = 14;
pub const STACK_REGISTER: Register = 13;
pub const DEFAULT_DATA_BASE: Word = 0x4000;
/// A zero stack pointer denotes the exclusive top of the 16-bit stack segment.
pub const DEFAULT_STACK_TOP: Word = 0;

/// Three-register integer ALU operations. The discriminant is the major
/// opcode; major 2 is the shift/multiply family, so the values are not
/// contiguous.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum AluOp {
    Add = 0,
    Sub = 1,
    And = 3,
    Or = 4,
    Xor = 5,
}

/// Shift direction within the major-2 shift/multiply family. Register forms
/// use functions 0..=2, immediate forms functions 4..=6.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum ShiftOp {
    Left = 0,
    RightLogical = 1,
    RightArithmetic = 2,
}

/// Post-multiply window selected from the full unsigned 32-bit product:
/// `Low` keeps `[15:0]`, `Shift8` keeps `[23:8]`, `Shift16` keeps `[31:16]`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum MultiplyWindow {
    Low = 0,
    Shift8 = 1,
    Shift16 = 2,
}

/// In-place immediate operations (major A). Functions 7, B, E, and F are
/// reserved and decode as invalid instructions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum ImmediateOp {
    Add = 0,
    Sub = 1,
    LoadSigned = 2,
    LoadUnsigned = 3,
    And = 4,
    Or = 5,
    Xor = 6,
    SetEqual = 8,
    SetLessThanSigned = 9,
    SetLessThanUnsigned = 10,
    CompareSigned = 12,
    CompareUnsigned = 13,
}

/// Predicates tested against the pending test result left by a CMP-class
/// instruction. The same six conditions encode both the conditional branches
/// (B functions 0..=5) and the conditional moves (B functions 8..=D).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum TestCondition {
    Equal = 0,
    NotEqual = 1,
    LessThan = 2,
    GreaterOrEqual = 3,
    GreaterThan = 4,
    LessOrEqual = 5,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum SpecialRegister {
    CodeSegment = 0,
    DataSegment = 1,
}

/// `AccStore`'s `b` field is a 4-bit destination lane write mask (bit 0 = x
/// through bit 3 = w), not a lane index: every set bit writes the same rounded
/// ACC value into that lane, and ACC is cleared afterwards. Mask 0b0000 writes
/// no lane and only clears ACC.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum FpuOp {
    Load = 0,
    Store = 1,
    Import4 = 2,
    Export4 = 3,
    Move = 4,
    Pack4 = 5,
    Unpack4 = 6,
    Transpose4 = 7,
    Add = 8,
    Sub = 9,
    Mul = 10,
    Dot4Acc = 11,
    AccStore = 12,
    Compare = 13,
    Unary = 14,
    // 15 is reserved (formerly FMULS; scalar-by-vector uses an explicit
    // FACCLOAD.X + FACCSTORE 0b1111 splat followed by FMUL).
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum FpuUnaryOp {
    Reciprocal = 0,
    ReciprocalSqrt = 1,
    SinCos = 2,
    Abs = 3,
    Neg = 4,
    Floor = 5,
    Ceil = 6,
    Round = 7,
    Saturate01 = 8,
    Sign = 9,
    Zero = 10,
    // The FACCLOAD.* subops select one source lane (unlike AccStore's write
    // mask) and overwrite ACC with the exact lane value shifted into the
    // accumulator format.
    AccLoadX = 11,
    AccLoadY = 12,
    AccLoadZ = 13,
    AccLoadW = 14,
}

impl TestCondition {
    pub const fn invert(self) -> Self {
        match self {
            Self::Equal => Self::NotEqual,
            Self::NotEqual => Self::Equal,
            Self::LessThan => Self::GreaterOrEqual,
            Self::GreaterOrEqual => Self::LessThan,
            Self::GreaterThan => Self::LessOrEqual,
            Self::LessOrEqual => Self::GreaterThan,
        }
    }
}

fn register(value: Register) -> Word {
    assert!(
        value < 16,
        "CpuV3 register index {value} is outside r0..r15"
    );
    Word::from(value)
}

pub fn fpu(op: FpuOp, a: Register, b: Register) -> Word {
    0xd000 | ((op as Word) << 8) | (register(a) << 4) | register(b)
}

pub fn fpu_unary(dst: Register, op: FpuUnaryOp) -> Word {
    fpu(FpuOp::Unary, dst, op as Register)
}

fn signed4(value: i16) -> Word {
    assert!(
        (-8..=7).contains(&value),
        "CpuV3 signed 4-bit immediate {value} is outside -8..7"
    );
    (value as Word) & 0xf
}

fn unsigned4(value: u8) -> Word {
    assert!(
        value < 16,
        "CpuV3 unsigned 4-bit immediate {value} exceeds 15"
    );
    Word::from(value)
}

fn signed8(value: i16) -> Word {
    assert!(
        (-128..=127).contains(&value),
        "CpuV3 signed 8-bit offset {value} is outside -128..127"
    );
    (value as Word) & 0xff
}

pub fn alu(op: AluOp, dst: Register, lhs: Register, rhs: Register) -> Word {
    ((op as Word) << 12) | (register(dst) << 8) | (register(lhs) << 4) | register(rhs)
}

fn shift_mul(function: Word, dst: Register, operand: Word) -> Word {
    0x2000 | (function << 8) | (register(dst) << 4) | operand
}

/// Destructive register-count shift: `rd = rd shift (rs & 15)`.
pub fn shift_register(op: ShiftOp, dst: Register, src: Register) -> Word {
    shift_mul(op as Word, dst, register(src))
}

/// Destructive immediate shift: `rd = rd shift amount`.
pub fn shift_immediate(op: ShiftOp, dst: Register, amount: u8) -> Word {
    shift_mul(4 + op as Word, dst, unsigned4(amount))
}

/// Destructive unsigned multiply keeping one 16-bit window of the 32-bit
/// product: `rd = (rd * rs) >> S` for `S` in {0, 8, 16}.
pub fn multiply(window: MultiplyWindow, dst: Register, src: Register) -> Word {
    shift_mul(8 + window as Word, dst, register(src))
}

/// Destructive unsigned multiply by an immediate (shift-0 window only). The
/// unprefixed immediate is a `u4`; with `PFX12` it is the full `u16` bit
/// pattern.
pub fn multiply_immediate(dst: Register, value: u8) -> Word {
    shift_mul(0xc, dst, unsigned4(value))
}

pub fn load(dst: Register, base: Register, offset: i16) -> Word {
    0x8000 | (register(dst) << 8) | (register(base) << 4) | signed4(offset)
}

pub fn store(src: Register, base: Register, offset: i16) -> Word {
    0x9000 | (register(src) << 8) | (register(base) << 4) | signed4(offset)
}

pub fn immediate_signed(op: ImmediateOp, dst: Register, value: i16) -> Word {
    0xa000 | ((op as Word) << 8) | (register(dst) << 4) | signed4(value)
}

pub fn immediate_unsigned(op: ImmediateOp, dst: Register, value: u8) -> Word {
    0xa000 | ((op as Word) << 8) | (register(dst) << 4) | unsigned4(value)
}

/// Conditional branch on the pending test result, with a signed 8-bit
/// offset relative to the already-incremented program counter.
pub fn branch(condition: TestCondition, offset: i16) -> Word {
    0xb000 | ((condition as Word) << 8) | signed8(offset)
}

/// Unconditional relative jump (function 6), no link.
pub fn jump_relative(offset: i16) -> Word {
    0xb600 | signed8(offset)
}

/// Unconditional relative jump with link (function 7): r14 receives the
/// address of the next word before the jump.
pub fn jump_and_link_relative(offset: i16) -> Word {
    0xb700 | signed8(offset)
}

/// Conditional move on the pending test result: `rd = rs` when the condition
/// holds. The pending test is consumed whether or not the move writes.
pub fn conditional_move(condition: TestCondition, dst: Register, src: Register) -> Word {
    0xb000 | (((8 + condition as Word) << 8) | (register(dst) << 4) | register(src))
}

/// Indirect jump: `B E 0 target`. The unused middle nibble is canonically 0;
/// any other value is an invalid encoding.
pub fn jump_register(target: Register) -> Word {
    0xbe00 | register(target)
}

/// Indirect jump with link: `B F E target`. The link register is
/// architecturally fixed to r14, so the middle nibble must encode 14; any
/// other value is an invalid encoding.
pub fn jump_and_link_register(target: Register) -> Word {
    0xbfe0 | register(target)
}

/// Reads device `device` channel `channel` into `dst`.
pub fn device_receive(dst: Register, device: u8, channel: u8) -> Word {
    assert!(device < 8, "CpuV3 device index {device} exceeds 7");
    assert!(channel < 16, "CpuV3 device channel {channel} exceeds 15");
    0x7000 | (Word::from(device) << 8) | (Word::from(channel) << 4) | register(dst)
}

/// Writes `src` to device `device` channel `channel`.
pub fn device_send(src: Register, device: u8, channel: u8) -> Word {
    assert!(device < 8, "CpuV3 device index {device} exceeds 7");
    assert!(channel < 16, "CpuV3 device channel {channel} exceeds 15");
    0x7800 | (Word::from(device) << 8) | (Word::from(channel) << 4) | register(src)
}

fn extended(function: Word, a: Register, b: Register) -> Word {
    0x6000 | (function << 8) | (register(a) << 4) | register(b)
}

pub fn move_register(dst: Register, src: Register) -> Word {
    extended(0, dst, src)
}

pub fn not(dst: Register, src: Register) -> Word {
    extended(1, dst, src)
}

pub fn negate(dst: Register, src: Register) -> Word {
    extended(2, dst, src)
}

pub fn sign_extend_byte(dst: Register, src: Register) -> Word {
    extended(3, dst, src)
}

pub fn leading_zeros(dst: Register, src: Register) -> Word {
    extended(4, dst, src)
}

pub fn population_count(dst: Register, src: Register) -> Word {
    extended(5, dst, src)
}

/// Replaces `dst` with `dst == src` as 0 or 1.
pub fn set_equal(dst: Register, src: Register) -> Word {
    extended(6, dst, src)
}

/// Replaces `dst` with the signed comparison `dst < src` as 0 or 1.
pub fn set_less_than_signed(dst: Register, src: Register) -> Word {
    extended(8, dst, src)
}

/// Replaces `dst` with the unsigned comparison `dst < src` as 0 or 1.
pub fn set_less_than_unsigned(dst: Register, src: Register) -> Word {
    extended(9, dst, src)
}

/// Sets the pending test result to the signed ordering of `ra` and `rb`;
/// no register is written.
pub fn compare_signed(ra: Register, rb: Register) -> Word {
    extended(0xa, ra, rb)
}

/// Sets the pending test result to the unsigned ordering of `ra` and `rb`;
/// no register is written.
pub fn compare_unsigned(ra: Register, rb: Register) -> Word {
    extended(0xb, ra, rb)
}

/// Raises signal `signal_type` carrying the value of `src`. Type 0 halts and
/// latches the value at the retirement edge; types 1..=15 are simulator-side
/// events that retire as a NOP in hardware.
pub fn signal(src: Register, signal_type: u8) -> Word {
    extended(0xc, src, unsigned4(signal_type) as Register)
}

/// Halts execution; the halt signal is the value of `r0` latched at the
/// retirement edge. This is `SIGNAL r0, 0`.
pub const fn halt() -> Word {
    0x6c00
}

pub fn read_special(dst: Register, special: SpecialRegister) -> Word {
    extended(0xd, dst, special as Register)
}

/// Writes a boot-time configurable special register.
///
/// CSEG deliberately cannot be written this way: changing the fetch segment
/// and the program counter must be one architectural operation.
pub fn write_data_segment(src: Register) -> Word {
    extended(0xe, SpecialRegister::DataSegment as Register, src)
}

/// Atomically selects the code segment and the offset of the next instruction.
pub fn jump_segment(segment: Register, target: Register) -> Word {
    extended(0xf, segment, target)
}

pub const fn nop() -> Word {
    0x6000
}

/// Neutral 12-bit prefix for the immediately following eligible consumer.
pub fn prefix12(payload: u16) -> Word {
    assert!(payload <= 0x0fff, "CpuV3 prefix payload exceeds 12 bits");
    0xf000 | payload
}

/// Emits the canonical two-word load for any 16-bit value.
pub fn load_immediate16(dst: Register, value: Word) -> [Word; 2] {
    [
        prefix12(value >> 4),
        immediate_unsigned(ImmediateOp::LoadUnsigned, dst, (value & 0xf) as u8),
    ]
}

/// Adds a prefix to a low-nibble consumer. The caller selects the consumer's
/// operation; this helper keeps the two physical words adjacent. Only the
/// consumer's own nibble carries instruction bits, so the payload's low nibble
/// is masked in and its upper bits are not part of the encoding.
pub fn prefixed(consumer: Word, value: Word) -> [Word; 2] {
    // `!0x000f` (not `!0xf`) keeps the mask 16 bits wide: the narrow literal
    // infers `u8` here and would silently drop the consumer's high byte.
    [prefix12(value >> 4), (consumer & !0x000f) | (value & 0xf)]
}

/// Adds a prefix to a B-family relative consumer (branch or relative jump).
/// Unlike `prefixed`, the wide offset is `{prefix[7:0], imm8}`: the prefix
/// supplies the high byte and the consumer's immediate byte the low byte;
/// `prefix[11:8]` is ignored by the consumer, so only the offset's low byte
/// reaches the prefix and the rest is discarded.
pub fn prefixed_branch(consumer: Word, offset: u16) -> [Word; 2] {
    [prefix12((offset >> 8) & 0xff), consumer | (offset & 0xff)]
}

pub(crate) fn sign_extend(value: Word, bits: u32) -> Word {
    let shift = Word::BITS - bits;
    (((value << shift) as i16) >> shift) as Word
}

/// The closed PFX12 consumer set: `LOAD`/`STORE`, `MULI`, every defined
/// major-A operation, and the major-B relative forms 0..=7. Relative
/// consumers use only `payload12[7:0]`; the upper payload bits are ignored.
///
/// This predicate is the single authoritative statement of the consumer set.
/// Three other places encode the same legality rules and must stay aligned:
/// `decode` decides which words are valid instructions at all, `CpuV3Sim::step`
/// dispatches the same major opcodes, and `hardware::CpuV3CoreState` (revision
/// 0.7 until step 3 rewrites the RTL) carries its own copy. The consumer list
/// below is exactly the set of *defined* (non-reserved) forms in the families
/// that consume a prefix, so a reserved slot can never appear here; the
/// exhaustive replay in `prefix_consumers_decode_and_are_never_reserved`
/// enforces that alignment over the whole 16-bit word space.
pub(crate) fn is_prefix_consumer(instruction: Word) -> bool {
    let function = (instruction >> 8) & 0xf;
    match instruction >> 12 {
        0x8 | 0x9 => true,
        0x2 => function == 0xc,
        0xa => matches!(function, 0..=6 | 8..=0xa | 0xc | 0xd),
        0xb => matches!(function, 0..=7),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_reference_encodings_match_the_candidate_specification() {
        assert_eq!(alu(AluOp::Add, 3, 1, 2), 0x0312);
        assert_eq!(alu(AluOp::Sub, 3, 1, 2), 0x1312);
        assert_eq!(alu(AluOp::And, 3, 1, 2), 0x3312);
        assert_eq!(alu(AluOp::Or, 3, 1, 2), 0x4312);
        assert_eq!(alu(AluOp::Xor, 3, 1, 2), 0x5312);
        assert_eq!(shift_register(ShiftOp::Left, 3, 4), 0x2034);
        assert_eq!(shift_register(ShiftOp::RightLogical, 3, 4), 0x2134);
        assert_eq!(shift_register(ShiftOp::RightArithmetic, 3, 4), 0x2234);
        assert_eq!(shift_immediate(ShiftOp::Left, 3, 15), 0x243f);
        assert_eq!(shift_immediate(ShiftOp::RightLogical, 3, 1), 0x2531);
        assert_eq!(shift_immediate(ShiftOp::RightArithmetic, 3, 0), 0x2630);
        assert_eq!(multiply(MultiplyWindow::Low, 3, 4), 0x2834);
        assert_eq!(multiply(MultiplyWindow::Shift8, 3, 4), 0x2934);
        assert_eq!(multiply(MultiplyWindow::Shift16, 3, 4), 0x2a34);
        assert_eq!(multiply_immediate(3, 9), 0x2c39);
        assert_eq!(move_register(3, 4), 0x6034);
        assert_eq!(not(3, 4), 0x6134);
        assert_eq!(negate(3, 4), 0x6234);
        assert_eq!(sign_extend_byte(3, 4), 0x6334);
        assert_eq!(leading_zeros(3, 4), 0x6434);
        assert_eq!(population_count(3, 4), 0x6534);
        assert_eq!(set_equal(3, 4), 0x6634);
        assert_eq!(set_less_than_signed(3, 4), 0x6834);
        assert_eq!(set_less_than_unsigned(3, 4), 0x6934);
        assert_eq!(compare_signed(3, 4), 0x6a34);
        assert_eq!(compare_unsigned(3, 4), 0x6b34);
        assert_eq!(signal(5, 1), 0x6c51);
        assert_eq!(halt(), 0x6c00);
        assert_eq!(nop(), 0x6000);
        assert_eq!(read_special(3, SpecialRegister::CodeSegment), 0x6d30);
        assert_eq!(write_data_segment(4), 0x6e14);
        assert_eq!(jump_segment(3, 4), 0x6f34);
        assert_eq!(device_receive(3, 2, 1), 0x7213);
        assert_eq!(device_send(3, 2, 1), 0x7a13);
        assert_eq!(load(3, 4, -1), 0x834f);
        assert_eq!(store(3, 4, 7), 0x9347);
        assert_eq!(immediate_signed(ImmediateOp::Add, 3, -1), 0xa03f);
        assert_eq!(
            immediate_unsigned(ImmediateOp::LoadUnsigned, 3, 0xd),
            0xa33d
        );
        assert_eq!(immediate_signed(ImmediateOp::SetEqual, 3, -2), 0xa83e);
        assert_eq!(
            immediate_unsigned(ImmediateOp::CompareUnsigned, 3, 7),
            0xad37
        );
        assert_eq!(branch(TestCondition::NotEqual, -3), 0xb1fd);
        assert_eq!(jump_relative(-2), 0xb6fe);
        assert_eq!(jump_and_link_relative(-2), 0xb7fe);
        assert_eq!(conditional_move(TestCondition::LessOrEqual, 3, 4), 0xbd34);
        assert_eq!(jump_register(5), 0xbe05);
        assert_eq!(jump_and_link_register(5), 0xbfe5);
        assert_eq!(load_immediate16(3, 0xabcd), [0xfabc, 0xa33d]);
        assert_eq!(fpu(FpuOp::Mul, 3, 4), 0xda34);
        assert_eq!(fpu_unary(3, FpuUnaryOp::ReciprocalSqrt), 0xde31);
    }

    #[test]
    fn prefixed_branch_uses_the_prefix_low_byte_as_offset_high_byte() {
        assert_eq!(
            prefixed_branch(branch(TestCondition::Equal, 0), 0x1234),
            [0xf012, 0xb034]
        );
        assert_eq!(
            prefixed_branch(jump_and_link_relative(0), 0xfffe),
            [0xf0ff, 0xb7fe]
        );
    }

    #[test]
    fn prefixed_nibble_consumer_keeps_its_own_instruction_bits() {
        // `prefixed` rewrites only the low nibble of the consumer, so the
        // value's high nibble becomes the prefix payload and its low nibble
        // replaces the consumer's immediate field.
        assert_eq!(prefixed(0x8340, 0xabcd), [0xfabc, 0x834d]);
        assert_eq!(prefixed(0x9340, 0x0007), [0xf000, 0x9347]);
    }

    #[test]
    fn prefix_consumers_are_an_explicit_closed_set() {
        assert!(is_prefix_consumer(load(0, 0, 0)));
        assert!(is_prefix_consumer(store(0, 0, 0)));
        assert!(is_prefix_consumer(multiply_immediate(0, 0)));
        for word in [
            immediate_signed(ImmediateOp::Add, 0, 0),
            immediate_signed(ImmediateOp::Sub, 0, 0),
            immediate_signed(ImmediateOp::LoadSigned, 0, 0),
            immediate_unsigned(ImmediateOp::LoadUnsigned, 0, 0),
            immediate_unsigned(ImmediateOp::And, 0, 0),
            immediate_unsigned(ImmediateOp::Or, 0, 0),
            immediate_unsigned(ImmediateOp::Xor, 0, 0),
            immediate_signed(ImmediateOp::SetEqual, 0, 0),
            immediate_signed(ImmediateOp::SetLessThanSigned, 0, 0),
            immediate_unsigned(ImmediateOp::SetLessThanUnsigned, 0, 0),
            immediate_signed(ImmediateOp::CompareSigned, 0, 0),
            immediate_unsigned(ImmediateOp::CompareUnsigned, 0, 0),
        ] {
            assert!(is_prefix_consumer(word), "{word:#06x}");
        }
        assert!(is_prefix_consumer(branch(TestCondition::Equal, 0)));
        assert!(is_prefix_consumer(jump_relative(0)));
        assert!(is_prefix_consumer(jump_and_link_relative(0)));
        // Shift immediates, register shifts, and register multiplies never
        // consume a prefix.
        assert!(!is_prefix_consumer(shift_immediate(ShiftOp::Left, 0, 0)));
        assert!(!is_prefix_consumer(shift_register(ShiftOp::Left, 0, 0)));
        assert!(!is_prefix_consumer(multiply(MultiplyWindow::Low, 0, 0)));
        // Conditional moves and register jumps are not relative forms.
        assert!(!is_prefix_consumer(conditional_move(
            TestCondition::Equal,
            0,
            0
        )));
        assert!(!is_prefix_consumer(jump_register(0)));
        assert!(!is_prefix_consumer(jump_and_link_register(0)));
        // Reserved immediate functions do not consume a prefix.
        assert!(!is_prefix_consumer(0xa700));
        assert!(!is_prefix_consumer(0xab00));
        assert!(!is_prefix_consumer(0xae00));
        assert!(!is_prefix_consumer(0xaf00));
        // Device instructions have no immediate and never consume a prefix.
        assert!(!is_prefix_consumer(device_receive(0, 0, 0)));
        assert!(!is_prefix_consumer(device_send(0, 0, 0)));
        assert!(!is_prefix_consumer(move_register(0, 0)));
        assert!(!is_prefix_consumer(fpu(FpuOp::Add, 0, 0)));
    }

    /// Replays the whole 16-bit word space to hold the consumer set, the
    /// decoder's legality rule, and the simulator's major dispatch together.
    /// A reserved function slot must never become a prefix consumer, and every
    /// prefix consumer must decode to a defined instruction.
    #[test]
    fn prefix_consumers_decode_and_are_never_reserved() {
        let reserved =
            |word: Word| matches!(crate::decode(word), crate::Instruction::Invalid { .. });
        for word in 0..=Word::MAX {
            // 1. A consumer is always a defined instruction: a reserved slot
            //    must never be able to swallow a prefix.
            if is_prefix_consumer(word) {
                assert!(
                    !reserved(word),
                    "{word:#06x} consumes a prefix but decodes as reserved"
                );
            }
            let major = word >> 12;
            let function = (word >> 8) & 0xf;
            // 2. The families whose every defined form is prefix-eligible make
            //    every legal word a consumer, so no prefix can be silently
            //    skipped by a defined form: LOAD/STORE have no other form, and
            //    major A's consumer list is exactly its defined function set.
            if matches!(major, 0x8 | 0x9) {
                assert!(is_prefix_consumer(word), "{word:#06x}");
            }
            if major == 0xa && !reserved(word) {
                assert!(is_prefix_consumer(word), "{word:#06x}");
            }
            // 3. The shift/multiply family is prefix-eligible only for MULI;
            //    its shifts and register multiplies stay non-consumers.
            if major == 0x2 {
                assert_eq!(
                    is_prefix_consumer(word),
                    function == 0xc && !reserved(word),
                    "{word:#06x}"
                );
            }
            // 4. The B-family relative forms are prefix-eligible and its
            //    non-relative forms (conditional moves, register jumps) never
            //    are, even when their canonical fields are valid.
            if major == 0xb && !reserved(word) {
                assert_eq!(is_prefix_consumer(word), function <= 7, "{word:#06x}");
            }
        }
    }
}
