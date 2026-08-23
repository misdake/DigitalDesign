//! Encoding helpers for CpuV3 revision 0.3.

pub type Word = u16;
pub type Register = u8;

pub const ISA_REVISION: (u8, u8) = (0, 4);

pub const LINK_REGISTER: Register = 14;
pub const STACK_REGISTER: Register = 13;
pub const GLOBAL_REGISTER: Register = 15;
pub const DEFAULT_DATA_BASE: Word = 0x4000;
pub const MMIO_BASE: Word = 0xff00;
pub const DEFAULT_STACK_TOP: Word = MMIO_BASE;

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
    LoadSigned = 14,
    LoadUnsigned = 15,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum BranchCondition {
    Zero = 0,
    NonZero = 1,
    Negative = 2,
    NonNegative = 3,
    Positive = 4,
    NonPositive = 5,
    Odd = 6,
    Even = 7,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum SpecialRegister {
    CodeSegment = 0,
    DataSegment = 1,
}

impl BranchCondition {
    pub const fn invert(self) -> Self {
        match self {
            Self::Zero => Self::NonZero,
            Self::NonZero => Self::Zero,
            Self::Negative => Self::NonNegative,
            Self::NonNegative => Self::Negative,
            Self::Positive => Self::NonPositive,
            Self::NonPositive => Self::Positive,
            Self::Odd => Self::Even,
            Self::Even => Self::Odd,
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

pub fn branch(condition: BranchCondition, test: Register, offset: i16) -> Word {
    0xb000 | ((condition as Word) << 8) | (register(test) << 4) | signed4(offset)
}

pub fn jump(link: Option<Register>, offset: i16) -> Word {
    assert!(
        (-128..=127).contains(&offset),
        "CpuV3 short jump offset {offset} is outside -128..127"
    );
    let link = link.map_or(15, |value| {
        assert!(
            value < 15,
            "r15 in the link field encodes a jump without link"
        );
        value
    });
    0xc000 | (register(link) << 8) | Word::from(offset as u8)
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

pub fn jump_and_link_register(link: Register, target: Register) -> Word {
    control(5, link, target)
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
    control(11, dst, src)
}

pub fn read_special(dst: Register, special: SpecialRegister) -> Word {
    control(12, dst, special as Register)
}

/// Writes a boot-time configurable special register.
///
/// CSEG deliberately cannot be written this way: changing the fetch segment
/// and the program counter must be one architectural operation.
pub fn write_data_segment(src: Register) -> Word {
    control(13, SpecialRegister::DataSegment as Register, src)
}

/// Atomically selects the code segment and the offset of the next instruction.
pub fn jump_segment(segment: Register, target: Register) -> Word {
    control(14, segment, target)
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

pub(crate) fn sign_extend(value: Word, bits: u32) -> Word {
    let shift = Word::BITS - bits;
    (((value << shift) as i16) >> shift) as Word
}

pub(crate) fn is_prefix_consumer(instruction: Word) -> bool {
    match instruction >> 12 {
        0x8 | 0x9 | 0xc => true,
        0xa => matches!((instruction >> 8) & 0xf, 0..=4 | 8..=11 | 14..=15),
        0xb => ((instruction >> 8) & 0xf) <= 7,
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
        assert_eq!(branch(BranchCondition::NonZero, 2, -3), 0xb12d);
        assert_eq!(jump(Some(LINK_REGISTER), -2), 0xcefe);
        assert_eq!(load_immediate16(3, 0xabcd), [0xfabc, 0xaf3d]);
        assert_eq!(set_less_than_signed(3, 4), 0xe934);
        assert_eq!(set_less_than_unsigned(3, 4), 0xea34);
        assert_eq!(population_count(3, 4), 0xeb34);
        assert_eq!(read_special(3, SpecialRegister::CodeSegment), 0xec30);
        assert_eq!(write_data_segment(4), 0xed14);
        assert_eq!(jump_segment(3, 4), 0xee34);
        assert_eq!(halt(), 0xe800);
    }

    #[test]
    fn prefix_consumers_are_an_explicit_closed_set() {
        assert!(is_prefix_consumer(load(0, 0, 0)));
        assert!(is_prefix_consumer(immediate_signed(ImmediateOp::Add, 0, 0)));
        assert!(is_prefix_consumer(jump(None, 0)));
        assert!(!is_prefix_consumer(immediate_unsigned(
            ImmediateOp::ShiftLeft,
            0,
            0
        )));
        assert!(!is_prefix_consumer(move_register(0, 0)));
    }
}
