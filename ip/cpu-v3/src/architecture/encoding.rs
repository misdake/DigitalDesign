//! Encoding helpers for CpuV3 revision 0.6.

pub type Word = u16;
pub type Register = u8;

pub const ISA_REVISION: (u8, u8) = (0, 6);

pub const LINK_REGISTER: Register = 14;
pub const STACK_REGISTER: Register = 13;
pub const DEFAULT_DATA_BASE: Word = 0x4000;
/// A zero stack pointer denotes the exclusive top of the 16-bit stack segment.
pub const DEFAULT_STACK_TOP: Word = 0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum AluOp {
    Add = 0,
    Sub = 1,
    Mul = 2,
    And = 3,
    Or = 4,
    Xor = 5,
    ShiftLeft = 6,
    ShiftRightArithmetic = 7,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum ImmediateOp {
    Add = 0,
    Sub = 1,
    And = 2,
    Or = 3,
    Xor = 4,
    ShiftLeft = 5,
    ShiftRightLogical = 6,
    ShiftRightArithmetic = 7,
    Multiply = 8,
    CompareEqual = 9,
    CompareLessThanSigned = 10,
    CompareLessThanUnsigned = 11,
    CompareSigned = 12,
    CompareUnsigned = 13,
    LoadSigned = 14,
    LoadUnsigned = 15,
}

/// Predicates tested against the pending test result left by a CMP-class
/// instruction. Conditions 6 and 7 are reserved; 8 and 9 encode the
/// unconditional relative jumps (without and with link).
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

/// Unconditional relative jump (condition 8), no link.
pub fn jump_relative(offset: i16) -> Word {
    0xb800 | signed8(offset)
}

/// Unconditional relative jump with link (condition 9): r14 receives the
/// address of the next word before the jump.
pub fn jump_and_link_relative(offset: i16) -> Word {
    0xb900 | signed8(offset)
}

/// Reads device `device` channel `channel` into `dst`.
pub fn device_receive(dst: Register, device: u8, channel: u8) -> Word {
    assert!(device < 8, "CpuV3 device index {device} exceeds 7");
    assert!(channel < 16, "CpuV3 device channel {channel} exceeds 15");
    0xc000 | (Word::from(device) << 8) | (Word::from(channel) << 4) | register(dst)
}

/// Writes `src` to device `device` channel `channel`.
pub fn device_send(src: Register, device: u8, channel: u8) -> Word {
    assert!(device < 8, "CpuV3 device index {device} exceeds 7");
    assert!(channel < 16, "CpuV3 device channel {channel} exceeds 15");
    0xc800 | (Word::from(device) << 8) | (Word::from(channel) << 4) | register(src)
}

fn control(function: Word, dst: Register, src: Register) -> Word {
    0xe000 | (function << 8) | (register(dst) << 4) | register(src)
}

pub fn move_register(dst: Register, src: Register) -> Word {
    control(1, dst, src)
}

pub fn not(dst: Register, src: Register) -> Word {
    control(2, dst, src)
}

pub fn negate(dst: Register, src: Register) -> Word {
    control(3, dst, src)
}

pub fn jump_register(target: Register) -> Word {
    control(4, 0, target)
}

/// Indirect jump with link: the link register is architecturally fixed to
/// r14, so only the target register is encoded.
pub fn jump_and_link_register(target: Register) -> Word {
    control(5, LINK_REGISTER, target)
}

pub fn sign_extend_byte(dst: Register, src: Register) -> Word {
    control(6, dst, src)
}

pub fn leading_zeros(dst: Register, src: Register) -> Word {
    control(7, dst, src)
}

/// Replaces `dst` with the signed comparison `dst < rhs` as 0 or 1.
pub fn set_less_than_signed(dst: Register, rhs: Register) -> Word {
    control(9, dst, rhs)
}

/// Replaces `dst` with the unsigned comparison `dst < rhs` as 0 or 1.
pub fn set_less_than_unsigned(dst: Register, rhs: Register) -> Word {
    control(10, dst, rhs)
}

pub fn population_count(dst: Register, src: Register) -> Word {
    control(0, dst, src)
}

/// Sets the pending test result to the signed ordering of `rd` and `rs`;
/// no register is written.
pub fn compare_signed(rd: Register, rs: Register) -> Word {
    control(11, rd, rs)
}

/// Sets the pending test result to the unsigned ordering of `rd` and `rs`;
/// no register is written.
pub fn compare_unsigned(rd: Register, rs: Register) -> Word {
    control(12, rd, rs)
}

pub fn read_special(dst: Register, special: SpecialRegister) -> Word {
    control(13, dst, special as Register)
}

/// Writes a boot-time configurable special register.
///
/// CSEG deliberately cannot be written this way: changing the fetch segment
/// and the program counter must be one architectural operation.
pub fn write_data_segment(src: Register) -> Word {
    control(14, SpecialRegister::DataSegment as Register, src)
}

/// Atomically selects the code segment and the offset of the next instruction.
pub fn jump_segment(segment: Register, target: Register) -> Word {
    control(15, segment, target)
}

pub const fn halt() -> Word {
    0xe800
}

pub const fn nop() -> Word {
    0xe100
}

pub fn immediate_high12(high: u16) -> Word {
    assert!(high <= 0x0fff, "CpuV3 immediate high part exceeds 12 bits");
    0xf000 | high
}

/// Emits the canonical two-word load for any 16-bit value.
pub fn load_immediate16(dst: Register, value: Word) -> [Word; 2] {
    [
        immediate_high12(value >> 4),
        immediate_unsigned(ImmediateOp::LoadUnsigned, dst, (value & 0xf) as u8),
    ]
}

/// Adds a prefix to a low-nibble consumer. The caller selects the consumer's
/// operation; this helper keeps the two physical words adjacent.
pub fn prefixed(consumer: Word, value: Word) -> [Word; 2] {
    [
        immediate_high12(value >> 4),
        (consumer & !0xf) | (value & 0xf),
    ]
}

/// Adds a prefix to a B-family consumer (branch or relative jump). Unlike
/// `prefixed`, the wide offset is `{prefix[7:0], imm8}`: the prefix supplies
/// the high byte and the consumer's immediate byte the low byte.
pub fn prefixed_branch(consumer: Word, offset: u16) -> [Word; 2] {
    [immediate_high12(offset >> 8), consumer | (offset & 0xff)]
}

pub(crate) fn sign_extend(value: Word, bits: u32) -> Word {
    let shift = Word::BITS - bits;
    (((value << shift) as i16) >> shift) as Word
}

pub(crate) fn is_prefix_consumer(instruction: Word) -> bool {
    match instruction >> 12 {
        0x8 | 0x9 => true,
        0xa => !matches!((instruction >> 8) & 0xf, 5..=7),
        0xb => matches!((instruction >> 8) & 0xf, 0..=5 | 8 | 9),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_reference_encodings_match_the_candidate_specification() {
        assert_eq!(alu(AluOp::Add, 3, 1, 2), 0x0312);
        assert_eq!(load(3, 4, -1), 0x834f);
        assert_eq!(store(3, 4, 7), 0x9347);
        assert_eq!(branch(TestCondition::NotEqual, -3), 0xb1fd);
        assert_eq!(jump_relative(-2), 0xb8fe);
        assert_eq!(jump_and_link_relative(-2), 0xb9fe);
        assert_eq!(device_receive(3, 2, 1), 0xc213);
        assert_eq!(device_send(3, 2, 1), 0xca13);
        assert_eq!(load_immediate16(3, 0xabcd), [0xfabc, 0xaf3d]);
        assert_eq!(compare_signed(3, 4), 0xeb34);
        assert_eq!(compare_unsigned(3, 4), 0xec34);
        assert_eq!(set_less_than_signed(3, 4), 0xe934);
        assert_eq!(set_less_than_unsigned(3, 4), 0xea34);
        assert_eq!(population_count(3, 4), 0xe034);
        assert_eq!(read_special(3, SpecialRegister::CodeSegment), 0xed30);
        assert_eq!(write_data_segment(4), 0xee14);
        assert_eq!(jump_segment(3, 4), 0xef34);
        assert_eq!(halt(), 0xe800);
    }

    #[test]
    fn prefixed_branch_uses_the_prefix_low_byte_as_offset_high_byte() {
        assert_eq!(
            prefixed_branch(branch(TestCondition::Equal, 0), 0x1234),
            [0xf012, 0xb034]
        );
        assert_eq!(
            prefixed_branch(jump_and_link_relative(0), 0xfffe),
            [0xf0ff, 0xb9fe]
        );
    }

    #[test]
    fn prefix_consumers_are_an_explicit_closed_set() {
        assert!(is_prefix_consumer(load(0, 0, 0)));
        assert!(is_prefix_consumer(immediate_signed(ImmediateOp::Add, 0, 0)));
        assert!(is_prefix_consumer(immediate_signed(
            ImmediateOp::CompareSigned,
            0,
            0
        )));
        assert!(is_prefix_consumer(branch(TestCondition::Equal, 0)));
        assert!(is_prefix_consumer(jump_relative(0)));
        assert!(is_prefix_consumer(jump_and_link_relative(0)));
        assert!(!is_prefix_consumer(immediate_unsigned(
            ImmediateOp::ShiftLeft,
            0,
            0
        )));
        // Reserved branch conditions do not consume a prefix.
        assert!(!is_prefix_consumer(0xb600));
        assert!(!is_prefix_consumer(0xbf00));
        // Device instructions have no immediate and never consume a prefix.
        assert!(!is_prefix_consumer(device_receive(0, 0, 0)));
        assert!(!is_prefix_consumer(device_send(0, 0, 0)));
        assert!(!is_prefix_consumer(move_register(0, 0)));
    }
}
