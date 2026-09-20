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

/// In-place immediate operations (major A). `Add`/`Sub` take an unsigned u4
/// (0..=15) unprefixed — a negative adjustment is `Sub`'s job — and the full
/// 16-bit pattern under `PFX12` like every other consumer. Functions E and F
/// are reserved and decode as invalid instructions.
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
    LoadConstant = 7,
    SetEqual = 8,
    SetLessThanSigned = 9,
    SetLessThanUnsigned = 10,
    AddConstant = 11,
    CompareSigned = 12,
    CompareUnsigned = 13,
}

/// The 16-entry constant table shared by `LDC` (fn 7) and `ADDC` (fn B),
/// indexed by the immediate nibble. The table is symmetric: with
/// `MAG = [8, 16, 24, 32, 64, 128, 256, 512]`, indices 0..=7 hold `MAG[k]`
/// and indices 8..=15 hold `-MAG[k - 8]`. The magnitudes sit just past the
/// `0..=15` range `ADDI`/`LDI`/`LDUI` already cover (with `SUBI` covering the
/// downward side); they target struct sizes, pointer strides, and small
/// stack-frame offsets. Shown signed:
/// 8, 16, 24, 32, 64, 128, 256, 512, -8, -16, -24, -32, -64, -128, -256, -512.
pub const CONSTANT_TABLE: [Word; 16] = [
    0x0008, 0x0010, 0x0018, 0x0020, 0x0040, 0x0080, 0x0100, 0x0200, 0xfff8, 0xfff0, 0xffe8, 0xffe0,
    0xffc0, 0xff80, 0xff00, 0xfe00,
];

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

/// FPU v2 major opcode, word0 bits [15:12] (design `fpu-design-v2` section 5).
/// Every FPU v2 instruction is 32 bits wide, fetched as two 16-bit words, and
/// never consumes a `PFX12` prefix.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum FpuOpcode {
    Vector = 0xc,
    Scalar = 0xd,
    Aux = 0xe,
}

impl FpuOpcode {
    /// Returns the FPU major opcode for a word0, or `None` for any other
    /// major. Only 0xC/0xD/0xE start an FPU v2 pair.
    pub const fn from_word0(word0: Word) -> Option<Self> {
        match word0 >> 12 {
            0xc => Some(Self::Vector),
            0xd => Some(Self::Scalar),
            0xe => Some(Self::Aux),
            _ => None,
        }
    }
}

/// Vector length field, word1 bits [9:8]. `11` is reserved; software must not
/// emit it, and the architecture rejects it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum FpuVectorLength {
    Vec2 = 0b00,
    Vec3 = 0b01,
    Vec4 = 0b10,
}

impl FpuVectorLength {
    pub const fn from_field(field: u8) -> Option<Self> {
        match field & 0b11 {
            0b00 => Some(Self::Vec2),
            0b01 => Some(Self::Vec3),
            0b10 => Some(Self::Vec4),
            _ => None,
        }
    }

    /// Number of scalar F registers the range occupies.
    pub const fn lanes(self) -> u8 {
        self as u8 + 2
    }
}

/// Vector subop, word1 bits [7:3] (design section 6.3). `0x10..0x1F` are
/// reserved (0x10 was the rejected CROSS3 candidate).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum FpuVectorSubop {
    VAdd = 0x00,
    VSub = 0x01,
    VMul = 0x02,
    VMulS = 0x03,
    VMin = 0x04,
    VMax = 0x05,
    VAbs = 0x06,
    VNeg = 0x07,
    VFloor = 0x08,
    VCeil = 0x09,
    VRound = 0x0a,
    VTrunc = 0x0b,
    VMove = 0x0c,
    Dot = 0x0d,
    DotAdd = 0x0e,
    DotStore = 0x0f,
}

impl FpuVectorSubop {
    pub const fn from_field(field: u8) -> Option<Self> {
        match field {
            0x00 => Some(Self::VAdd),
            0x01 => Some(Self::VSub),
            0x02 => Some(Self::VMul),
            0x03 => Some(Self::VMulS),
            0x04 => Some(Self::VMin),
            0x05 => Some(Self::VMax),
            0x06 => Some(Self::VAbs),
            0x07 => Some(Self::VNeg),
            0x08 => Some(Self::VFloor),
            0x09 => Some(Self::VCeil),
            0x0a => Some(Self::VRound),
            0x0b => Some(Self::VTrunc),
            0x0c => Some(Self::VMove),
            0x0d => Some(Self::Dot),
            0x0e => Some(Self::DotAdd),
            0x0f => Some(Self::DotStore),
            _ => None,
        }
    }

    /// Unary subops ignore `Fb`; the front end still reads it, but the range
    /// check must not require `Fb` to cover the full vector.
    pub const fn ignores_second_source(self) -> bool {
        matches!(
            self,
            Self::VAbs
                | Self::VNeg
                | Self::VFloor
                | Self::VCeil
                | Self::VRound
                | Self::VTrunc
                | Self::VMove
        )
    }

    /// Whether `mode[1:0]` is the second-source stride (DOT family).
    pub const fn uses_stride_mode(self) -> bool {
        matches!(self, Self::Dot | Self::DotAdd | Self::DotStore)
    }
}

/// Scalar subop, word1 bits [9:4] (design section 7.3). `0x10..0x3F` are
/// reserved.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum FpuScalarSubop {
    Add = 0x00,
    Sub = 0x01,
    Mul = 0x02,
    Min = 0x03,
    Max = 0x04,
    Abs = 0x05,
    Neg = 0x06,
    Floor = 0x07,
    Ceil = 0x08,
    Round = 0x09,
    Trunc = 0x0a,
    Cmp = 0x0b,
    Rcp = 0x0c,
    Rsqrt = 0x0d,
    SinCos = 0x0e,
    Mov = 0x0f,
}

impl FpuScalarSubop {
    pub const fn from_field(field: u8) -> Option<Self> {
        match field {
            0x00 => Some(Self::Add),
            0x01 => Some(Self::Sub),
            0x02 => Some(Self::Mul),
            0x03 => Some(Self::Min),
            0x04 => Some(Self::Max),
            0x05 => Some(Self::Abs),
            0x06 => Some(Self::Neg),
            0x07 => Some(Self::Floor),
            0x08 => Some(Self::Ceil),
            0x09 => Some(Self::Round),
            0x0a => Some(Self::Trunc),
            0x0b => Some(Self::Cmp),
            0x0c => Some(Self::Rcp),
            0x0d => Some(Self::Rsqrt),
            0x0e => Some(Self::SinCos),
            0x0f => Some(Self::Mov),
            _ => None,
        }
    }
}

/// AUX kind field, word0 bits [1:0] (design section 10.1).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum FpuAuxKind {
    IntegerRegister = 0b00,
    ConstantIndex = 0b01,
    Selector = 0b10,
    Reserved = 0b11,
}

impl FpuAuxKind {
    pub const fn from_field(field: u8) -> Self {
        match field & 0b11 {
            0b00 => Self::IntegerRegister,
            0b01 => Self::ConstantIndex,
            0b10 => Self::Selector,
            _ => Self::Reserved,
        }
    }
}

/// AUX subop, word1 bits [9:4], for kind `00` (design section 10.2).
/// `0x08..0x3F` are reserved (constant-table operations land here later).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum FpuAuxSubop {
    Fld = 0x00,
    Fst = 0x01,
    Ilo2f = 0x02,
    Ihi2f = 0x03,
    Flo2i = 0x04,
    Fhi2i = 0x05,
    I16tof = 0x06,
    Ftoi16 = 0x07,
}

impl FpuAuxSubop {
    pub const fn from_field(field: u8) -> Option<Self> {
        match field {
            0x00 => Some(Self::Fld),
            0x01 => Some(Self::Fst),
            0x02 => Some(Self::Ilo2f),
            0x03 => Some(Self::Ihi2f),
            0x04 => Some(Self::Flo2i),
            0x05 => Some(Self::Fhi2i),
            0x06 => Some(Self::I16tof),
            0x07 => Some(Self::Ftoi16),
            _ => None,
        }
    }
}

/// `DOT`/`DOTADD`/`DOTSTORE` second-source stride, vector `mode[1:0]`
/// (design section 6.5). The first source is always stride 1. `11` is
/// reserved.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum FpuDotStride {
    Stride1 = 0b00,
    Stride3 = 0b01,
    Stride4 = 0b10,
}

impl FpuDotStride {
    pub const fn from_mode(mode: u8) -> Option<Self> {
        match mode & 0b11 {
            0b00 => Some(Self::Stride1),
            0b01 => Some(Self::Stride3),
            0b10 => Some(Self::Stride4),
            _ => None,
        }
    }

    /// Register step between consecutive second-source lanes.
    pub const fn step(self) -> u8 {
        match self {
            Self::Stride1 => 1,
            Self::Stride3 => 3,
            Self::Stride4 => 4,
        }
    }
}

/// `SINCOS` result selection, scalar `mode[1:0]` (design section 9.3). `11` is
/// reserved; `mode[3:2]` must be zero.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum FpuSinCosMode {
    SinCos = 0b00,
    Sin = 0b01,
    Cos = 0b10,
}

impl FpuSinCosMode {
    pub const fn from_mode(mode: u8) -> Option<Self> {
        match mode & 0b11 {
            0b00 => Some(Self::SinCos),
            0b01 => Some(Self::Sin),
            0b10 => Some(Self::Cos),
            _ => None,
        }
    }
}

/// Vector/scalar word0 Fa, bits [11:6].
pub const fn fpu_fa(word0: Word) -> u8 {
    ((word0 >> 6) & 0x3f) as u8
}

/// Vector/scalar word0 Fb, bits [5:0].
pub const fn fpu_fb(word0: Word) -> u8 {
    (word0 & 0x3f) as u8
}

/// AUX word0 integer-register index X, bits [11:8].
pub const fn fpu_aux_x(word0: Word) -> u8 {
    ((word0 >> 8) & 0xf) as u8
}

/// AUX word0 Fa, bits [7:2].
pub const fn fpu_aux_fa(word0: Word) -> u8 {
    ((word0 >> 2) & 0x3f) as u8
}

/// Word1 destination Fd, bits [15:10].
pub const fn fpu_fd(word1: Word) -> u8 {
    ((word1 >> 10) & 0x3f) as u8
}

/// Word1 vector length field, bits [9:8].
pub const fn fpu_vector_len_field(word1: Word) -> u8 {
    ((word1 >> 8) & 0b11) as u8
}

/// Word1 vector subop field, bits [7:3].
pub const fn fpu_vector_subop_field(word1: Word) -> u8 {
    ((word1 >> 3) & 0x1f) as u8
}

/// Word1 scalar/AUX subop field, bits [9:4].
pub const fn fpu_scalar_subop_field(word1: Word) -> u8 {
    ((word1 >> 4) & 0x3f) as u8
}

/// Word1 mode field, bits [3:0] (scalar/AUX).
pub const fn fpu_mode(word1: Word) -> u8 {
    (word1 & 0xf) as u8
}

/// Word1 vector mode field, bits [2:0]. Bit 3 belongs to the 5-bit subop.
pub const fn fpu_vector_mode(word1: Word) -> u8 {
    (word1 & 0b111) as u8
}

fn fpu_register(value: u8) -> Word {
    assert!(value < 64, "FPU register index {value} is outside f0..f63");
    Word::from(value)
}

/// Validates the second-source read for one vector subop, returning the
/// highest register index it touches.
fn vector_second_source_last(fb: u8, lanes: usize, subop: FpuVectorSubop, mode: u8) -> usize {
    if subop.ignores_second_source() || subop == FpuVectorSubop::VMulS {
        return usize::from(fb);
    }
    if subop.uses_stride_mode() {
        let step = usize::from(
            FpuDotStride::from_mode(mode)
                .expect("validated DOT mode")
                .step(),
        );
        return usize::from(fb) + (lanes - 1) * step;
    }
    usize::from(fb) + lanes - 1
}

/// Single source of truth for the vector encoding contract: reserved modes and
/// any operand range that crosses past `F63`. Returns the rejection reason.
///
/// [`fpu_vector`] asserts on it, `decode_fpu_pair` reports `FpuReserved`, and
/// `CpuV3Sim` raises `InvalidInstruction`, so the three can never drift.
pub fn fpu_vector_field_error(
    fa: u8,
    fb: u8,
    fd: u8,
    len: FpuVectorLength,
    subop: FpuVectorSubop,
    mode: u8,
) -> Option<&'static str> {
    if subop.uses_stride_mode() {
        if mode & 0b100 != 0 || FpuDotStride::from_mode(mode).is_none() {
            return Some("FPU DOT mode is not a defined stride");
        }
    } else if mode != 0 {
        return Some("FPU vector mode must be zero for a non-DOT subop");
    }
    let lanes = usize::from(len.lanes());
    if usize::from(fa) + lanes > 64 || usize::from(fd) + lanes > 64 {
        return Some("FPU source or destination range crosses past f63");
    }
    if !subop.ignores_second_source()
        && subop != FpuVectorSubop::VMulS
        && vector_second_source_last(fb, lanes, subop, mode) >= 64
    {
        return Some("FPU Fb range ends past f63");
    }
    None
}

/// Builds a two-word `0xC` VECTOR instruction:
/// `word0 = {0xC, Fa[5:0], Fb[5:0]}`,
/// `word1 = {Fd[5:0], len[1:0], subop[4:0], mode[2:0]}`.
///
/// Rejects every reserved field: non-DOT subops require `mode == 0`; the DOT
/// family requires `mode[2] == 0` and a defined stride in `mode[1:0]`. It also
/// rejects any source or destination range that would cross past `F63`. Use
/// [`fpu_vector_raw`] only for invalid-encoding tests.
pub fn fpu_vector(
    fa: u8,
    fb: u8,
    fd: u8,
    len: FpuVectorLength,
    subop: FpuVectorSubop,
    mode: u8,
) -> [Word; 2] {
    if let Some(reason) = fpu_vector_field_error(fa, fb, fd, len, subop, mode) {
        panic!("FPU vector encoding rejected: {reason}");
    }
    fpu_vector_raw(fa, fb, fd, len as u8, subop as u8, mode)
}

/// Raw VECTOR builder for reserved or arbitrary encodings; fields must fit the
/// hardware bit widths. No mode or range validation: only invalid-encoding
/// tests and the RTL extractor lock use this.
pub fn fpu_vector_raw(
    fa: u8,
    fb: u8,
    fd: u8,
    len_field: u8,
    subop_field: u8,
    mode: u8,
) -> [Word; 2] {
    assert!(len_field < 4, "FPU vector len {len_field} exceeds 2 bits");
    assert!(
        subop_field < 32,
        "FPU vector subop {subop_field} exceeds 5 bits"
    );
    assert!(mode < 8, "FPU vector mode {mode} exceeds 3 bits");
    [
        0xc000 | (fpu_register(fa) << 6) | fpu_register(fb),
        (fpu_register(fd) << 10)
            | (Word::from(len_field) << 8)
            | (Word::from(subop_field) << 3)
            | Word::from(mode),
    ]
}

/// Builds a two-word `0xD` SCALAR instruction:
/// `word0 = {0xD, Fa[5:0], Fb[5:0]}`,
/// `word1 = {Fd[5:0], subop[5:0], mode[3:0]}`.
///
/// Only `SINCOS` uses `mode`: it requires `mode[3:2] == 0` and a defined
/// `mode[1:0]`, and the dual-output form needs `Fd <= 62`. Every other subop
/// requires `mode == 0`. Use [`fpu_scalar_raw`] only for invalid tests.
pub fn fpu_scalar(fa: u8, fb: u8, fd: u8, subop: FpuScalarSubop, mode: u8) -> [Word; 2] {
    if let Some(reason) = fpu_scalar_field_error(fa, fb, fd, subop, mode) {
        panic!("FPU scalar encoding rejected: {reason}");
    }
    fpu_scalar_raw(fa, fb, fd, subop as u8, mode)
}

/// Single source of truth for the scalar encoding contract: the `SINCOS` mode
/// bits and its dual-output destination range. See [`fpu_vector_field_error`].
pub fn fpu_scalar_field_error(
    _fa: u8,
    _fb: u8,
    fd: u8,
    subop: FpuScalarSubop,
    mode: u8,
) -> Option<&'static str> {
    if subop == FpuScalarSubop::SinCos {
        if mode & 0b1100 != 0 {
            return Some("FPU SINCOS mode[3:2] must be zero");
        }
        if FpuSinCosMode::from_mode(mode).is_none() {
            return Some("FPU SINCOS mode[1:0] is reserved");
        }
        if mode == FpuSinCosMode::SinCos as u8 && usize::from(fd) + 1 >= 64 {
            return Some("FPU SINCOS dual output needs Fd <= 62");
        }
    } else if mode != 0 {
        return Some("FPU scalar mode must be zero for a non-SINCOS subop");
    }
    None
}

/// Raw SCALAR builder for reserved or arbitrary encodings; fields must fit the
/// hardware bit widths. No mode validation.
pub fn fpu_scalar_raw(fa: u8, fb: u8, fd: u8, subop_field: u8, mode: u8) -> [Word; 2] {
    assert!(
        subop_field < 64,
        "FPU scalar subop {subop_field} exceeds 6 bits"
    );
    assert!(mode < 16, "FPU scalar mode {mode} exceeds 4 bits");
    [
        0xd000 | (fpu_register(fa) << 6) | fpu_register(fb),
        (fpu_register(fd) << 10) | (Word::from(subop_field) << 4) | Word::from(mode),
    ]
}

/// Builds a two-word `0xE` AUX instruction:
/// `word0 = {0xE, X[3:0], Fa[5:0], kind[1:0]}`,
/// `word1 = {Fd[5:0], subop[5:0], mode[3:0]}`.
///
/// Only `kind = 00` is defined; the strict builder rejects the reserved and
/// not-yet-implemented kinds. `FLD`/`FST` use `mode[1:0] + 1` lanes with
/// `mode[3:2] == 0`; every other subop requires `mode == 0`. Ranges that would
/// cross past `F63` are rejected.
pub fn fpu_aux(kind: FpuAuxKind, x: u8, fa: u8, fd: u8, subop: FpuAuxSubop, mode: u8) -> [Word; 2] {
    if let Some(reason) = fpu_aux_field_error(kind, fa, fd, subop, mode) {
        panic!("FPU AUX encoding rejected: {reason}");
    }
    fpu_aux_raw(kind, x, fa, fd, subop as u8, mode)
}

/// Single source of truth for the AUX encoding contract: the implemented kind,
/// the `FLD`/`FST` mode bits, and their destination/source register range. See
/// [`fpu_vector_field_error`].
pub fn fpu_aux_field_error(
    kind: FpuAuxKind,
    fa: u8,
    fd: u8,
    subop: FpuAuxSubop,
    mode: u8,
) -> Option<&'static str> {
    if kind != FpuAuxKind::IntegerRegister {
        return Some("FPU AUX kind is not implemented");
    }
    let is_memory = matches!(subop, FpuAuxSubop::Fld | FpuAuxSubop::Fst);
    if is_memory {
        if mode >= 4 {
            return Some("FPU FLD/FST mode exceeds 2 bits");
        }
        let lanes = usize::from(mode) + 1;
        let base = if subop == FpuAuxSubop::Fld { fd } else { fa };
        if usize::from(base) + lanes > 64 {
            return Some("FPU range crosses past f63");
        }
    } else if mode != 0 {
        return Some("FPU AUX mode must be zero for a non-memory subop");
    }
    None
}

/// Raw AUX builder for reserved or not-yet-defined kinds/subops; fields must
/// fit the hardware bit widths. No validation: only invalid-encoding tests and
/// the RTL extractor lock use this.
pub fn fpu_aux_raw(kind: FpuAuxKind, x: u8, fa: u8, fd: u8, subop: u8, mode: u8) -> [Word; 2] {
    assert!(x < 16, "FPU AUX X index {x} exceeds 4 bits");
    assert!(subop < 64, "FPU AUX subop {subop} exceeds 6 bits");
    assert!(mode < 16, "FPU AUX mode {mode} exceeds 4 bits");
    [
        0xe000 | (Word::from(x) << 8) | (fpu_register(fa) << 2) | (kind as Word),
        (fpu_register(fd) << 10) | (Word::from(subop) << 4) | Word::from(mode),
    ]
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

/// `LDC rd, k4`: loads the shared constant `CONST[k4]` (see
/// [`CONSTANT_TABLE`]). Never consumes `PFX12`.
pub fn load_constant(dst: Register, index: u8) -> Word {
    immediate_unsigned(ImmediateOp::LoadConstant, dst, index)
}

/// `ADDC rd, k4`: wrapping `rd = rd + CONST[k4]` (see [`CONSTANT_TABLE`]).
/// Never consumes `PFX12`.
pub fn add_constant(dst: Register, index: u8) -> Word {
    immediate_unsigned(ImmediateOp::AddConstant, dst, index)
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
/// major-A operation except `LDC`/`ADDC`, and the major-B relative forms
/// 0..=7. Relative consumers use only `payload12[7:0]`; the upper payload
/// bits are ignored.
///
/// This predicate is the single authoritative statement of the consumer set.
/// Three other places encode the same legality rules and must stay aligned:
/// `decode` decides which words are valid instructions at all, `CpuV3Sim::step`
/// dispatches the same major opcodes, and `hardware::CpuV3CoreState` carries
/// its own copy. The consumer list below is exactly the set of *defined*
/// (non-reserved) forms in the families that consume a prefix, so a reserved
/// slot can never appear here; the
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
        assert_eq!(immediate_unsigned(ImmediateOp::Add, 3, 15), 0xa03f);
        assert_eq!(
            immediate_unsigned(ImmediateOp::LoadUnsigned, 3, 0xd),
            0xa33d
        );
        assert_eq!(immediate_signed(ImmediateOp::SetEqual, 3, -2), 0xa83e);
        assert_eq!(
            immediate_unsigned(ImmediateOp::CompareUnsigned, 3, 7),
            0xad37
        );
        assert_eq!(load_constant(3, 12), 0xa73c);
        assert_eq!(add_constant(3, 4), 0xab34);
        assert_eq!(branch(TestCondition::NotEqual, -3), 0xb1fd);
        assert_eq!(jump_relative(-2), 0xb6fe);
        assert_eq!(jump_and_link_relative(-2), 0xb7fe);
        assert_eq!(conditional_move(TestCondition::LessOrEqual, 3, 4), 0xbd34);
        assert_eq!(jump_register(5), 0xbe05);
        assert_eq!(jump_and_link_register(5), 0xbfe5);
        assert_eq!(load_immediate16(3, 0xabcd), [0xfabc, 0xa33d]);
    }

    /// The design's worked examples must encode to these exact words. They are
    /// the frozen reference for both the disassembler and the RTL extractor.
    #[test]
    fn fpu_v2_design_examples_encode_to_exact_words() {
        // VADD.3 F20, F4, F8
        assert_eq!(
            fpu_vector(4, 8, 20, FpuVectorLength::Vec3, FpuVectorSubop::VAdd, 0),
            [0xc108, 0x5100]
        );
        // VMULS.3 F0, F0, F4
        assert_eq!(
            fpu_vector(0, 4, 0, FpuVectorLength::Vec3, FpuVectorSubop::VMulS, 0),
            [0xc004, 0x0118]
        );
        // DOTSTORE.4.S1 F20, F16, F0
        assert_eq!(
            fpu_vector(
                16,
                0,
                20,
                FpuVectorLength::Vec4,
                FpuVectorSubop::DotStore,
                0
            ),
            [0xc400, 0x5278]
        );
        // RSQRT F4, F3
        assert_eq!(
            fpu_scalar(4, 3, 4, FpuScalarSubop::Rsqrt, 0),
            [0xd103, 0x10d0]
        );
        // FLD f1, [r2] (kind 00, X = 2, Fa unused)
        assert_eq!(
            fpu_aux(FpuAuxKind::IntegerRegister, 2, 0, 1, FpuAuxSubop::Fld, 0),
            [0xe200, 0x0400]
        );
        // FST [r1], f2 (kind 00, X = 1, Fa = 2; Fd is unused)
        assert_eq!(
            fpu_aux(FpuAuxKind::IntegerRegister, 1, 2, 0, FpuAuxSubop::Fst, 0),
            [0xe108, 0x0010]
        );
    }

    /// The raw builders must round-trip every field, including reserved and
    /// not-yet-defined values, so the disassembler and the RTL extractor lock
    /// can be exercised independently of the strict builder validation.
    #[test]
    fn fpu_raw_field_extractors_round_trip_every_field_value() {
        for len_field in 0..4u8 {
            for subop_field in 0..32u8 {
                let mode = 0b101;
                let [word0, word1] = fpu_vector_raw(37, 5, 61, len_field, subop_field, mode);
                assert_eq!(FpuOpcode::from_word0(word0), Some(FpuOpcode::Vector));
                assert_eq!(fpu_fa(word0), 37);
                assert_eq!(fpu_fb(word0), 5);
                assert_eq!(fpu_fd(word1), 61);
                assert_eq!(fpu_vector_len_field(word1), len_field);
                assert_eq!(fpu_vector_subop_field(word1), subop_field);
                assert_eq!(fpu_vector_mode(word1), mode);
            }
        }
        for subop_field in 0..64u8 {
            let [word0, word1] = fpu_scalar_raw(12, 51, 63, subop_field, 0xd);
            assert_eq!(FpuOpcode::from_word0(word0), Some(FpuOpcode::Scalar));
            assert_eq!(fpu_fa(word0), 12);
            assert_eq!(fpu_fb(word0), 51);
            assert_eq!(fpu_fd(word1), 63);
            assert_eq!(fpu_scalar_subop_field(word1), subop_field);
            assert_eq!(fpu_mode(word1), 0xd);
        }
        for kind in [
            FpuAuxKind::IntegerRegister,
            FpuAuxKind::ConstantIndex,
            FpuAuxKind::Selector,
            FpuAuxKind::Reserved,
        ] {
            for subop in 0..64u8 {
                let [word0, word1] = fpu_aux_raw(kind, 9, 42, 17, subop, 0b0011);
                assert_eq!(FpuOpcode::from_word0(word0), Some(FpuOpcode::Aux));
                assert_eq!(fpu_aux_x(word0), 9);
                assert_eq!(fpu_aux_fa(word0), 42);
                assert_eq!(FpuAuxKind::from_field((word0 & 3) as u8), kind);
                assert_eq!(fpu_fd(word1), 17);
                assert_eq!(fpu_scalar_subop_field(word1), subop);
                assert_eq!(fpu_mode(word1), 0b0011);
            }
        }
    }

    /// The strict builders must accept every defined in-range encoding and
    /// reject the reserved mode, kind, and range-overflow cases.
    #[test]
    fn fpu_strict_builders_accept_defined_encodings() {
        for len in [
            FpuVectorLength::Vec2,
            FpuVectorLength::Vec3,
            FpuVectorLength::Vec4,
        ] {
            let last = 64 - len.lanes();
            for subop in 0..=0x0fu8 {
                let subop = FpuVectorSubop::from_field(subop).unwrap();
                let mode = if subop.uses_stride_mode() { 0b01 } else { 0 };
                let _ = fpu_vector(0, 0, last, len, subop, mode);
            }
        }
        for subop in 0..=0x0fu8 {
            let subop = FpuScalarSubop::from_field(subop).unwrap();
            let mode = if subop == FpuScalarSubop::SinCos {
                0b10
            } else {
                0
            };
            let _ = fpu_scalar(0, 0, 63, subop, mode);
        }
        for subop in 0..=0x07u8 {
            let subop = FpuAuxSubop::from_field(subop).unwrap();
            let mode = if matches!(subop, FpuAuxSubop::Fld | FpuAuxSubop::Fst) {
                3
            } else {
                0
            };
            let _ = fpu_aux(FpuAuxKind::IntegerRegister, 0, 0, 0, subop, mode);
        }
    }

    #[test]
    #[should_panic(expected = "must be zero")]
    fn fpu_vector_rejects_a_mode_on_a_non_dot_subop() {
        let _ = fpu_vector(0, 0, 0, FpuVectorLength::Vec2, FpuVectorSubop::VAdd, 1);
    }

    #[test]
    #[should_panic(expected = "not a defined stride")]
    fn fpu_vector_rejects_a_reserved_dot_stride() {
        let _ = fpu_vector(0, 0, 0, FpuVectorLength::Vec2, FpuVectorSubop::Dot, 0b11);
    }

    #[test]
    #[should_panic(expected = "not a defined stride")]
    fn fpu_vector_rejects_a_dot_mode_bit_two() {
        let _ = fpu_vector(
            0,
            0,
            0,
            FpuVectorLength::Vec2,
            FpuVectorSubop::DotAdd,
            0b100,
        );
    }

    #[test]
    #[should_panic(expected = "crosses past f63")]
    fn fpu_vector_rejects_a_destination_range_overflow() {
        let _ = fpu_vector(0, 0, 62, FpuVectorLength::Vec3, FpuVectorSubop::VAdd, 0);
    }

    #[test]
    #[should_panic(expected = "Fb range ends")]
    fn fpu_vector_rejects_a_strided_second_source_overflow() {
        let _ = fpu_vector(
            0,
            60,
            0,
            FpuVectorLength::Vec4,
            FpuVectorSubop::Dot,
            FpuDotStride::Stride4 as u8,
        );
    }

    #[test]
    #[should_panic(expected = "must be zero")]
    fn fpu_scalar_rejects_a_mode_on_a_non_sincos_subop() {
        let _ = fpu_scalar(0, 0, 0, FpuScalarSubop::Add, 1);
    }

    #[test]
    #[should_panic(expected = "mode[3:2]")]
    fn fpu_scalar_rejects_a_sincos_mode_bit() {
        let _ = fpu_scalar(0, 0, 0, FpuScalarSubop::SinCos, 0b0100);
    }

    #[test]
    #[should_panic(expected = "dual output")]
    fn fpu_scalar_rejects_a_sincos_destination_overflow() {
        let _ = fpu_scalar(0, 0, 63, FpuScalarSubop::SinCos, 0);
    }

    #[test]
    #[should_panic(expected = "not implemented")]
    fn fpu_aux_rejects_a_reserved_kind() {
        let _ = fpu_aux(FpuAuxKind::Reserved, 0, 0, 0, FpuAuxSubop::Fld, 0);
    }

    #[test]
    #[should_panic(expected = "crosses past f63")]
    fn fpu_aux_rejects_a_vector_load_range_overflow() {
        let _ = fpu_aux(FpuAuxKind::IntegerRegister, 0, 0, 62, FpuAuxSubop::Fld, 3);
    }

    #[test]
    fn fpu_reserved_fields_are_rejected() {
        assert_eq!(FpuVectorLength::from_field(0b11), None);
        assert_eq!(FpuVectorSubop::from_field(0x10), None);
        assert_eq!(FpuVectorSubop::from_field(0x1f), None);
        assert_eq!(FpuScalarSubop::from_field(0x10), None);
        assert_eq!(FpuScalarSubop::from_field(0x3f), None);
        assert_eq!(FpuAuxSubop::from_field(0x08), None);
        assert_eq!(FpuDotStride::from_mode(0b11), None);
        assert_eq!(FpuSinCosMode::from_mode(0b11), None);
        assert_eq!(FpuOpcode::from_word0(0xb000), None);
        assert_eq!(FpuOpcode::from_word0(0xf000), None);
    }

    #[test]
    fn constant_table_matches_the_specification() {
        let magnitudes = [8, 16, 24, 32, 64, 128, 256, 512];
        for (index, magnitude) in magnitudes.into_iter().enumerate() {
            assert_eq!(CONSTANT_TABLE[index], magnitude, "index {index}");
            // The table is symmetric: CONST[k + 8] == -CONST[k].
            assert_eq!(
                CONSTANT_TABLE[index + 8],
                magnitude.wrapping_neg(),
                "index {}",
                index + 8
            );
        }
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
        // LDC/ADDC never consume a prefix; neither do the reserved immediate
        // functions E and F.
        assert!(!is_prefix_consumer(0xa700));
        assert!(!is_prefix_consumer(0xab00));
        assert!(!is_prefix_consumer(0xae00));
        assert!(!is_prefix_consumer(0xaf00));
        // Device instructions have no immediate and never consume a prefix.
        assert!(!is_prefix_consumer(device_receive(0, 0, 0)));
        assert!(!is_prefix_consumer(device_send(0, 0, 0)));
        assert!(!is_prefix_consumer(move_register(0, 0)));
        // FPU v2 instructions are two-word fetch barriers and never consume a
        // PFX12 prefix, exactly like the old FPU.
        for word in [
            fpu_vector(0, 0, 0, FpuVectorLength::Vec2, FpuVectorSubop::VAdd, 0)[0],
            fpu_scalar(0, 0, 0, FpuScalarSubop::Add, 0)[0],
            fpu_aux(FpuAuxKind::IntegerRegister, 0, 0, 0, FpuAuxSubop::Fld, 0)[0],
        ] {
            assert!(!is_prefix_consumer(word), "{word:#06x}");
        }
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
            //    major A's consumer list is exactly its defined function set
            //    minus the non-consuming LDC/ADDC.
            if matches!(major, 0x8 | 0x9) {
                assert!(is_prefix_consumer(word), "{word:#06x}");
            }
            if major == 0xa && !reserved(word) {
                assert_eq!(
                    is_prefix_consumer(word),
                    !matches!(function, 7 | 0xb),
                    "{word:#06x}"
                );
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
