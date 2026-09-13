//! Disassembler for CpuV3 revision 0.8: word -> typed instruction + mnemonic text.
//!
//! `decode` maps one physical word to an `Instruction`; `disassemble_words` walks
//! a stream and merges a `PFX12` prefix with its consumer into one wide
//! operation (a prefix before a non-consumer renders on its own line).

use crate::{
    is_prefix_consumer, AluOp, FpuOp, FpuUnaryOp, ImmediateOp, MultiplyWindow, ShiftOp,
    SpecialRegister, TestCondition, Word,
};

/// One decoded CpuV3 instruction word (or the whole two-word wide operation).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Instruction {
    Alu {
        op: AluOp,
        dst: u8,
        lhs: u8,
        rhs: u8,
    },
    ShiftRegister {
        op: ShiftOp,
        dst: u8,
        src: u8,
    },
    ShiftImmediate {
        op: ShiftOp,
        dst: u8,
        amount: u8,
    },
    Multiply {
        window: MultiplyWindow,
        dst: u8,
        src: u8,
    },
    MultiplyImmediate {
        dst: u8,
        value: u16,
    },
    Load {
        dst: u8,
        base: u8,
        offset: i16,
    },
    Store {
        src: u8,
        base: u8,
        offset: i16,
    },
    Immediate {
        op: ImmediateOp,
        dst: u8,
        value: u16,
    },
    Branch {
        condition: TestCondition,
        offset: i16,
    },
    JumpRelative {
        offset: i16,
        link: bool,
    },
    ConditionalMove {
        condition: TestCondition,
        dst: u8,
        src: u8,
    },
    JumpRegister {
        target: u8,
    },
    JumpAndLinkRegister {
        target: u8,
    },
    DeviceReceive {
        dst: u8,
        device: u8,
        channel: u8,
    },
    DeviceSend {
        src: u8,
        device: u8,
        channel: u8,
    },
    Fpu {
        op: FpuOp,
        a: u8,
        b: u8,
    },
    FpuUnary {
        dst: u8,
        op: FpuUnaryOp,
    },
    Move {
        dst: u8,
        src: u8,
    },
    Not {
        dst: u8,
        src: u8,
    },
    Negate {
        dst: u8,
        src: u8,
    },
    SignExtendByte {
        dst: u8,
        src: u8,
    },
    LeadingZeros {
        dst: u8,
        src: u8,
    },
    PopulationCount {
        dst: u8,
        src: u8,
    },
    SetEqual {
        dst: u8,
        src: u8,
    },
    SetLessThanSigned {
        dst: u8,
        src: u8,
    },
    SetLessThanUnsigned {
        dst: u8,
        src: u8,
    },
    CompareSigned {
        lhs: u8,
        rhs: u8,
    },
    CompareUnsigned {
        lhs: u8,
        rhs: u8,
    },
    Signal {
        src: u8,
        signal_type: u8,
    },
    ReadSpecial {
        dst: u8,
        special: SpecialRegister,
    },
    WriteDataSegment {
        src: u8,
    },
    JumpSegment {
        segment: u8,
        target: u8,
    },
    Prefix {
        payload: u16,
    },
    Invalid {
        word: Word,
    },
}

fn reg(index: Word) -> u8 {
    index as u8
}

fn test_condition(function: u8) -> TestCondition {
    match function & 7 {
        0 => TestCondition::Equal,
        1 => TestCondition::NotEqual,
        2 => TestCondition::LessThan,
        3 => TestCondition::GreaterOrEqual,
        4 => TestCondition::GreaterThan,
        _ => TestCondition::LessOrEqual,
    }
}

/// Decodes one word. Prefix merging is a stream concern; decode() alone keeps
/// the raw 4-bit immediate/offset fields.
pub fn decode(word: Word) -> Instruction {
    let n3 = word >> 12;
    let n2 = ((word >> 8) & 0xf) as u8;
    let n1 = ((word >> 4) & 0xf) as u8;
    let n0 = (word & 0xf) as u8;
    match n3 {
        0 | 1 | 3..=5 => Instruction::Alu {
            op: match n3 {
                0 => AluOp::Add,
                1 => AluOp::Sub,
                3 => AluOp::And,
                4 => AluOp::Or,
                _ => AluOp::Xor,
            },
            dst: reg(n2.into()),
            lhs: reg(n1.into()),
            rhs: reg(n0.into()),
        },
        2 => {
            let shift_op = |op: u8| match op & 3 {
                0 => ShiftOp::Left,
                1 => ShiftOp::RightLogical,
                _ => ShiftOp::RightArithmetic,
            };
            match n2 {
                0..=2 => Instruction::ShiftRegister {
                    op: shift_op(n2),
                    dst: reg(n1.into()),
                    src: reg(n0.into()),
                },
                4..=6 => Instruction::ShiftImmediate {
                    op: shift_op(n2),
                    dst: reg(n1.into()),
                    amount: n0,
                },
                8..=0xa => Instruction::Multiply {
                    window: match n2 {
                        8 => MultiplyWindow::Low,
                        9 => MultiplyWindow::Shift8,
                        _ => MultiplyWindow::Shift16,
                    },
                    dst: reg(n1.into()),
                    src: reg(n0.into()),
                },
                0xc => Instruction::MultiplyImmediate {
                    dst: reg(n1.into()),
                    value: n0.into(),
                },
                // 3, 7, B, D..F are reserved.
                _ => Instruction::Invalid { word },
            }
        }
        6 => match n2 {
            0 => Instruction::Move {
                dst: reg(n1.into()),
                src: reg(n0.into()),
            },
            1 => Instruction::Not {
                dst: reg(n1.into()),
                src: reg(n0.into()),
            },
            2 => Instruction::Negate {
                dst: reg(n1.into()),
                src: reg(n0.into()),
            },
            3 => Instruction::SignExtendByte {
                dst: reg(n1.into()),
                src: reg(n0.into()),
            },
            4 => Instruction::LeadingZeros {
                dst: reg(n1.into()),
                src: reg(n0.into()),
            },
            5 => Instruction::PopulationCount {
                dst: reg(n1.into()),
                src: reg(n0.into()),
            },
            6 => Instruction::SetEqual {
                dst: reg(n1.into()),
                src: reg(n0.into()),
            },
            8 => Instruction::SetLessThanSigned {
                dst: reg(n1.into()),
                src: reg(n0.into()),
            },
            9 => Instruction::SetLessThanUnsigned {
                dst: reg(n1.into()),
                src: reg(n0.into()),
            },
            0xa => Instruction::CompareSigned {
                lhs: reg(n1.into()),
                rhs: reg(n0.into()),
            },
            0xb => Instruction::CompareUnsigned {
                lhs: reg(n1.into()),
                rhs: reg(n0.into()),
            },
            0xc => Instruction::Signal {
                src: reg(n1.into()),
                signal_type: n0,
            },
            0xd if n0 <= 1 => Instruction::ReadSpecial {
                dst: reg(n1.into()),
                special: if n0 == 0 {
                    SpecialRegister::CodeSegment
                } else {
                    SpecialRegister::DataSegment
                },
            },
            0xe if n1 == 1 => Instruction::WriteDataSegment {
                src: reg(n0.into()),
            },
            0xf => Instruction::JumpSegment {
                segment: reg(n1.into()),
                target: reg(n0.into()),
            },
            // 7 is reserved; non-canonical special-register selectors fault.
            _ => Instruction::Invalid { word },
        },
        7 if word & 0x800 == 0 => Instruction::DeviceReceive {
            dst: reg(n0.into()),
            device: n2 & 7,
            channel: n1,
        },
        7 => Instruction::DeviceSend {
            src: reg(n0.into()),
            device: n2 & 7,
            channel: n1,
        },
        8 => Instruction::Load {
            dst: reg(n2.into()),
            base: reg(n1.into()),
            offset: crate::sign_extend(n0.into(), 4) as i16,
        },
        9 => Instruction::Store {
            src: reg(n2.into()),
            base: reg(n1.into()),
            offset: crate::sign_extend(n0.into(), 4) as i16,
        },
        0xa => {
            let op = match n2 {
                0 => ImmediateOp::Add,
                1 => ImmediateOp::Sub,
                2 => ImmediateOp::LoadSigned,
                3 => ImmediateOp::LoadUnsigned,
                4 => ImmediateOp::And,
                5 => ImmediateOp::Or,
                6 => ImmediateOp::Xor,
                7 => ImmediateOp::LoadConstant,
                8 => ImmediateOp::SetEqual,
                9 => ImmediateOp::SetLessThanSigned,
                0xa => ImmediateOp::SetLessThanUnsigned,
                0xb => ImmediateOp::AddConstant,
                0xc => ImmediateOp::CompareSigned,
                0xd => ImmediateOp::CompareUnsigned,
                // E and F are reserved.
                _ => return Instruction::Invalid { word },
            };
            Instruction::Immediate {
                op,
                dst: reg(n1.into()),
                value: n0.into(),
            }
        }
        0xb => match n2 {
            0..=5 => Instruction::Branch {
                condition: test_condition(n2),
                offset: crate::sign_extend(word & 0xff, 8) as i16,
            },
            6 => Instruction::JumpRelative {
                offset: crate::sign_extend(word & 0xff, 8) as i16,
                link: false,
            },
            7 => Instruction::JumpRelative {
                offset: crate::sign_extend(word & 0xff, 8) as i16,
                link: true,
            },
            8..=0xd => Instruction::ConditionalMove {
                condition: test_condition(n2),
                dst: reg(n1.into()),
                src: reg(n0.into()),
            },
            // JREG is canonically `B E 0 target`; JALR is `B F E target`
            // with the link register fixed to r14.
            0xe if n1 == 0 => Instruction::JumpRegister {
                target: reg(n0.into()),
            },
            0xf if n1 == 14 => Instruction::JumpAndLinkRegister {
                target: reg(n0.into()),
            },
            _ => Instruction::Invalid { word },
        },
        0xd => {
            if n2 == FpuOp::Unary as u8 {
                let op = match n0 {
                    0 => FpuUnaryOp::Reciprocal,
                    1 => FpuUnaryOp::ReciprocalSqrt,
                    2 => FpuUnaryOp::SinCos,
                    3 => FpuUnaryOp::Abs,
                    4 => FpuUnaryOp::Neg,
                    5 => FpuUnaryOp::Floor,
                    6 => FpuUnaryOp::Ceil,
                    7 => FpuUnaryOp::Round,
                    8 => FpuUnaryOp::Saturate01,
                    9 => FpuUnaryOp::Sign,
                    10 => FpuUnaryOp::Zero,
                    11 => FpuUnaryOp::AccLoadX,
                    12 => FpuUnaryOp::AccLoadY,
                    13 => FpuUnaryOp::AccLoadZ,
                    14 => FpuUnaryOp::AccLoadW,
                    _ => return Instruction::Invalid { word },
                };
                return Instruction::FpuUnary {
                    dst: reg(n1.into()),
                    op,
                };
            }
            let op = match n2 {
                0 => FpuOp::Load,
                1 => FpuOp::Store,
                2 => FpuOp::Import4,
                3 => FpuOp::Export4,
                4 => FpuOp::Move,
                5 => FpuOp::Pack4,
                6 => FpuOp::Unpack4,
                7 => FpuOp::Transpose4,
                8 => FpuOp::Add,
                9 => FpuOp::Sub,
                10 => FpuOp::Mul,
                11 => FpuOp::Dot4Acc,
                12 => FpuOp::AccStore,
                13 => FpuOp::Compare,
                _ => return Instruction::Invalid { word },
            };
            Instruction::Fpu {
                op,
                a: reg(n1.into()),
                b: reg(n0.into()),
            }
        }
        0xf => Instruction::Prefix {
            payload: word & 0xfff,
        },
        // C and E are fully reserved in revision 0.8; no other major exists.
        _ => Instruction::Invalid { word },
    }
}

fn shift_op_name(op: ShiftOp) -> &'static str {
    match op {
        ShiftOp::Left => "shl",
        ShiftOp::RightLogical => "shr",
        ShiftOp::RightArithmetic => "asr",
    }
}

impl Instruction {
    /// Renders the instruction, given the pending prefix payload if this
    /// instruction consumes one (widened immediate/offset where applicable).
    pub fn text(&self, prefix: Option<u16>) -> String {
        match *self {
            Instruction::Alu { op, dst, lhs, rhs } => {
                let name = match op {
                    AluOp::Add => "add",
                    AluOp::Sub => "sub",
                    AluOp::And => "and",
                    AluOp::Or => "or",
                    AluOp::Xor => "xor",
                };
                format!("{name} r{dst}, r{lhs}, r{rhs}")
            }
            Instruction::ShiftRegister { op, dst, src } => {
                format!("{} r{dst}, r{src}", shift_op_name(op))
            }
            Instruction::ShiftImmediate { op, dst, amount } => {
                format!("{}i r{dst}, {amount}", shift_op_name(op))
            }
            Instruction::Multiply { window, dst, src } => {
                let name = match window {
                    MultiplyWindow::Low => "mul0",
                    MultiplyWindow::Shift8 => "mul8",
                    MultiplyWindow::Shift16 => "mul16",
                };
                format!("{name} r{dst}, r{src}")
            }
            Instruction::MultiplyImmediate { dst, value } => {
                let shown = prefix.map_or(u32::from(value), |payload| {
                    u32::from((payload << 4) | value)
                });
                format!("muli r{dst}, {shown}")
            }
            Instruction::Load { dst, base, offset } => {
                let offset = wide_offset_text(prefix, offset);
                format!("load r{dst}, [r{base} + {offset}]")
            }
            Instruction::Store { src, base, offset } => {
                let offset = wide_offset_text(prefix, offset);
                format!("store r{src}, [r{base} + {offset}]")
            }
            Instruction::Immediate { op, dst, value } => {
                // LDC/ADDC index the shared constant table and never consume
                // a prefix; render the resolved constant signed.
                if matches!(op, ImmediateOp::LoadConstant | ImmediateOp::AddConstant) {
                    let name = if op == ImmediateOp::LoadConstant {
                        "ldc"
                    } else {
                        "addc"
                    };
                    let constant = i32::from(crate::CONSTANT_TABLE[usize::from(value)] as i16);
                    return format!("{name} r{dst}, {constant}");
                }
                let wide = prefix.map(|payload| (payload << 4) | value);
                // ADDI/SUBI read the unprefixed nibble as an unsigned u4 but
                // add/subtract the full 16-bit pattern under a prefix, so the
                // wide rendering stays signed.
                let signed = matches!(
                    op,
                    ImmediateOp::LoadSigned
                        | ImmediateOp::SetEqual
                        | ImmediateOp::SetLessThanSigned
                        | ImmediateOp::CompareSigned
                ) || (matches!(op, ImmediateOp::Add | ImmediateOp::Sub) && wide.is_some());
                let shown: i32 = if let Some(wide) = wide {
                    if signed {
                        wide as i16 as i32
                    } else {
                        i32::from(wide)
                    }
                } else if signed {
                    i32::from(crate::sign_extend(value, 4) as i16)
                } else {
                    i32::from(value)
                };
                let name = match op {
                    ImmediateOp::Add => "addi",
                    ImmediateOp::Sub => "subi",
                    ImmediateOp::LoadSigned => "ldi",
                    ImmediateOp::LoadUnsigned => "ldui",
                    ImmediateOp::And => "andi",
                    ImmediateOp::Or => "ori",
                    ImmediateOp::Xor => "xori",
                    ImmediateOp::SetEqual => "seqi",
                    ImmediateOp::SetLessThanSigned => "slti",
                    ImmediateOp::SetLessThanUnsigned => "sltui",
                    ImmediateOp::CompareSigned => "cmpsi",
                    ImmediateOp::CompareUnsigned => "cmpui",
                    // LDC/ADDC returned above with the resolved constant.
                    ImmediateOp::LoadConstant | ImmediateOp::AddConstant => unreachable!(),
                };
                format!("{name} r{dst}, {shown}")
            }
            Instruction::Branch { condition, offset } => {
                let offset = wide_branch(prefix, offset);
                format!("{} {offset}", condition_name(condition))
            }
            Instruction::JumpRelative { offset, link } => {
                let offset = wide_branch(prefix, offset);
                format!("{} {offset}", if link { "jalrel" } else { "jrel" })
            }
            Instruction::ConditionalMove {
                condition,
                dst,
                src,
            } => format!("mov{} r{dst}, r{src}", condition_suffix(condition)),
            Instruction::JumpRegister { target } => format!("jreg r{target}"),
            Instruction::JumpAndLinkRegister { target } => format!("jalr r{target}"),
            Instruction::DeviceReceive {
                dst,
                device,
                channel,
            } => {
                format!("devrecv r{dst}, dev{device}.ch{channel}")
            }
            Instruction::DeviceSend {
                src,
                device,
                channel,
            } => {
                format!("devsend dev{device}.ch{channel}, r{src}")
            }
            Instruction::Fpu { op, a, b } => {
                let name = match op {
                    FpuOp::Load => return format!("fload f{a}, r{b}"),
                    FpuOp::Store => return format!("fstore r{a}, f{b}"),
                    FpuOp::Import4 => return format!("fimport4 f{a}, [r{b}]"),
                    FpuOp::Export4 => return format!("fexport4 f{a}, [r{b}]"),
                    FpuOp::Move => "fmov",
                    FpuOp::Pack4 => "fpack4",
                    FpuOp::Unpack4 => "funpack4",
                    FpuOp::Transpose4 => "ftranspose4",
                    FpuOp::Add => "fadd",
                    FpuOp::Sub => "fsub",
                    FpuOp::Mul => "fmul",
                    FpuOp::Dot4Acc => "fdot4acc",
                    FpuOp::Compare => "fcmp",
                    FpuOp::AccStore => {
                        return format!("faccstore f{a}, 0b{b:04b}");
                    }
                    FpuOp::Unary => unreachable!(),
                };
                format!("{name} f{a}, f{b}")
            }
            Instruction::FpuUnary { dst, op } => {
                let name = match op {
                    FpuUnaryOp::Reciprocal => "frcp",
                    FpuUnaryOp::ReciprocalSqrt => "frsqrt",
                    FpuUnaryOp::SinCos => "fsincos",
                    FpuUnaryOp::Abs => "fabs",
                    FpuUnaryOp::Neg => "fneg",
                    FpuUnaryOp::Floor => "ffloor",
                    FpuUnaryOp::Ceil => "fceil",
                    FpuUnaryOp::Round => "fround",
                    FpuUnaryOp::Saturate01 => "fsat01",
                    FpuUnaryOp::Sign => "fsign",
                    FpuUnaryOp::Zero => "fzero",
                    FpuUnaryOp::AccLoadX => "faccload.x",
                    FpuUnaryOp::AccLoadY => "faccload.y",
                    FpuUnaryOp::AccLoadZ => "faccload.z",
                    FpuUnaryOp::AccLoadW => "faccload.w",
                };
                format!("{name} f{dst}")
            }
            Instruction::Move { dst, src } => {
                if dst == src {
                    "nop".to_string()
                } else {
                    format!("mov r{dst}, r{src}")
                }
            }
            Instruction::Not { dst, src } => format!("not r{dst}, r{src}"),
            Instruction::Negate { dst, src } => format!("neg r{dst}, r{src}"),
            Instruction::SignExtendByte { dst, src } => format!("sextb r{dst}, r{src}"),
            Instruction::LeadingZeros { dst, src } => format!("clz r{dst}, r{src}"),
            Instruction::PopulationCount { dst, src } => format!("popcnt r{dst}, r{src}"),
            Instruction::SetEqual { dst, src } => format!("seq r{dst}, r{src}"),
            Instruction::SetLessThanSigned { dst, src } => format!("slt r{dst}, r{src}"),
            Instruction::SetLessThanUnsigned { dst, src } => format!("sltu r{dst}, r{src}"),
            Instruction::CompareSigned { lhs, rhs } => format!("cmps r{lhs}, r{rhs}"),
            Instruction::CompareUnsigned { lhs, rhs } => format!("cmpu r{lhs}, r{rhs}"),
            Instruction::Signal { src, signal_type } => match (src, signal_type) {
                (0, 0) => "halt".to_string(),
                (_, 0) => format!("halt r{src}"),
                _ => format!("signal r{src}, {signal_type}"),
            },
            Instruction::ReadSpecial { dst, special } => match special {
                SpecialRegister::CodeSegment => format!("mfsr r{dst}, CSEG"),
                SpecialRegister::DataSegment => format!("mfsr r{dst}, DSEG"),
            },
            Instruction::WriteDataSegment { src } => format!("mtsr DSEG, r{src}"),
            Instruction::JumpSegment { segment, target } => {
                format!("jseg r{segment}, r{target}")
            }
            Instruction::Prefix { payload } => format!("pfx12 0x{payload:03x}"),
            Instruction::Invalid { word } => format!(".word 0x{word:04x}  ; invalid"),
        }
    }
}

fn condition_name(condition: TestCondition) -> &'static str {
    match condition {
        TestCondition::Equal => "beq",
        TestCondition::NotEqual => "bne",
        TestCondition::LessThan => "blt",
        TestCondition::GreaterOrEqual => "bge",
        TestCondition::GreaterThan => "bgt",
        TestCondition::LessOrEqual => "ble",
    }
}

fn condition_suffix(condition: TestCondition) -> &'static str {
    match condition {
        TestCondition::Equal => "eq",
        TestCondition::NotEqual => "ne",
        TestCondition::LessThan => "lt",
        TestCondition::GreaterOrEqual => "ge",
        TestCondition::GreaterThan => "gt",
        TestCondition::LessOrEqual => "le",
    }
}

impl std::fmt::Display for Instruction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text(None))
    }
}

/// Wide load/store offset: the prefix supplies the high 12 bits; the merged
/// 16-bit pattern is a signed offset either way, rendered signed.
fn wide_offset_text(prefix: Option<u16>, offset: i16) -> i32 {
    match prefix {
        Some(payload) => i32::from(((payload << 4) | (offset as u16 & 0xf)) as i16),
        None => i32::from(offset),
    }
}

/// Wide branch offset: the prefix's low byte is the offset's high byte.
fn wide_branch(prefix: Option<u16>, offset: i16) -> i32 {
    match prefix {
        None => i32::from(offset),
        Some(payload) => i32::from((((payload & 0xff) << 8) | (offset as u16 & 0xff)) as i16),
    }
}

/// One disassembled line: the word address, the rendered text, and the raw words.
pub struct DisasmLine {
    pub address: u16,
    pub text: String,
    pub wide: bool,
}

/// Disassembles a word stream at `base`, merging each PFX12 prefix with an
/// eligible consumer. A prefix before a non-consumer renders on its own line.
pub fn disassemble_words(words: &[Word], base: u16) -> Vec<DisasmLine> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < words.len() {
        let word = words[i];
        if let Instruction::Prefix { payload } = decode(word) {
            if i + 1 < words.len() && is_prefix_consumer(words[i + 1]) {
                let text = decode(words[i + 1]).text(Some(payload));
                out.push(DisasmLine {
                    address: base.wrapping_add(i as u16),
                    text,
                    wide: true,
                });
                i += 2;
                continue;
            }
        }
        out.push(DisasmLine {
            address: base.wrapping_add(i as u16),
            text: decode(word).text(None),
            wide: false,
        });
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;

    #[test]
    fn decode_renders_every_family() {
        assert_eq!(
            decode(alu(AluOp::Add, 3, 1, 2)).text(None),
            "add r3, r1, r2"
        );
        assert_eq!(
            decode(shift_register(ShiftOp::RightArithmetic, 3, 4)).text(None),
            "asr r3, r4"
        );
        assert_eq!(
            decode(shift_immediate(ShiftOp::RightLogical, 3, 15)).text(None),
            "shri r3, 15"
        );
        assert_eq!(
            decode(multiply(MultiplyWindow::Shift16, 3, 4)).text(None),
            "mul16 r3, r4"
        );
        assert_eq!(decode(multiply_immediate(3, 9)).text(None), "muli r3, 9");
        assert_eq!(decode(load(3, 4, -1)).text(None), "load r3, [r4 + -1]");
        assert_eq!(decode(store(3, 4, 7)).text(None), "store r3, [r4 + 7]");
        assert_eq!(
            decode(branch(TestCondition::NotEqual, -3)).text(None),
            "bne -3"
        );
        assert_eq!(decode(jump_relative(-2)).text(None), "jrel -2");
        assert_eq!(decode(jump_and_link_relative(-2)).text(None), "jalrel -2");
        assert_eq!(
            decode(conditional_move(TestCondition::GreaterThan, 3, 4)).text(None),
            "movgt r3, r4"
        );
        assert_eq!(decode(jump_register(5)).text(None), "jreg r5");
        assert_eq!(decode(jump_and_link_register(5)).text(None), "jalr r5");
        assert_eq!(
            decode(device_receive(3, 2, 1)).text(None),
            "devrecv r3, dev2.ch1"
        );
        assert_eq!(
            decode(device_send(3, 2, 1)).text(None),
            "devsend dev2.ch1, r3"
        );
        assert_eq!(decode(fpu(FpuOp::Mul, 3, 4)).text(None), "fmul f3, f4");
        assert_eq!(
            decode(fpu(FpuOp::AccStore, 3, 0b0101)).text(None),
            "faccstore f3, 0b0101"
        );
        assert_eq!(
            decode(fpu_unary(3, FpuUnaryOp::AccLoadW)).text(None),
            "faccload.w f3"
        );
        assert_eq!(decode(set_equal(3, 4)).text(None), "seq r3, r4");
        assert_eq!(decode(move_register(0, 0)).text(None), "nop");
        assert_eq!(decode(halt()).text(None), "halt");
        assert_eq!(decode(signal(5, 0)).text(None), "halt r5");
        assert_eq!(decode(signal(5, 1)).text(None), "signal r5, 1");
        assert_eq!(decode(signal(5, 15)).text(None), "signal r5, 15");
    }

    #[test]
    fn reserved_and_noncanonical_encodings_are_invalid() {
        // The revision 0.7 HALT word is invalid in revision 0.8.
        assert_eq!(decode(0xe800), Instruction::Invalid { word: 0xe800 });
        // Majors C and E are fully reserved.
        assert_eq!(decode(0xc000), Instruction::Invalid { word: 0xc000 });
        assert_eq!(decode(0xcabc), Instruction::Invalid { word: 0xcabc });
        assert_eq!(decode(0xe100), Instruction::Invalid { word: 0xe100 });
        assert_eq!(decode(0xefff), Instruction::Invalid { word: 0xefff });
        // Shift/multiply reserved functions 3, 7, B, D..F.
        for function in [0x3u16, 0x7, 0xb, 0xd, 0xe, 0xf] {
            let word = 0x2000 | (function << 8);
            assert_eq!(decode(word), Instruction::Invalid { word });
        }
        // Extended family function 7 is reserved.
        assert_eq!(decode(0x6700), Instruction::Invalid { word: 0x6700 });
        // Non-canonical special-register selectors are invalid.
        assert_eq!(decode(0x6d32), Instruction::Invalid { word: 0x6d32 });
        assert_eq!(decode(0x6e04), Instruction::Invalid { word: 0x6e04 });
        // Immediate reserved functions E and F.
        for function in [0xeu16, 0xf] {
            let word = 0xa000 | (function << 8);
            assert_eq!(decode(word), Instruction::Invalid { word });
        }
        // JREG is canonically `B E 0 target`; JALR is `B F E target`.
        assert_eq!(decode(0xbe15), Instruction::Invalid { word: 0xbe15 });
        assert_eq!(decode(0xbf05), Instruction::Invalid { word: 0xbf05 });
        assert_eq!(decode(0xbfd5), Instruction::Invalid { word: 0xbfd5 });
        assert_eq!(decode(0xbe05), Instruction::JumpRegister { target: 5 });
        assert_eq!(
            decode(0xbfe5),
            Instruction::JumpAndLinkRegister { target: 5 }
        );
    }

    #[test]
    fn constant_table_ops_render_the_resolved_constant() {
        assert_eq!(decode(load_constant(3, 9)).text(None), "ldc r3, -16");
        assert_eq!(decode(load_constant(3, 0)).text(None), "ldc r3, 8");
        assert_eq!(decode(add_constant(3, 2)).text(None), "addc r3, 24");
        assert_eq!(decode(add_constant(2, 15)).text(None), "addc r2, -512");
        // A pending prefix expires before the non-consuming LDC/ADDC and
        // renders on its own line.
        let words = [prefix12(0xabc), load_constant(3, 0), add_constant(3, 2)];
        let lines = disassemble_words(&words, 0);
        let texts: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["pfx12 0xabc", "ldc r3, 8", "addc r3, 24"]);
        assert!(lines.iter().all(|line| !line.wide));
    }

    #[test]
    fn addi_subi_render_unsigned_unprefixed() {
        assert_eq!(
            decode(immediate_unsigned(ImmediateOp::Add, 3, 15)).text(None),
            "addi r3, 15"
        );
        assert_eq!(
            decode(immediate_unsigned(ImmediateOp::Sub, 3, 0)).text(None),
            "subi r3, 0"
        );
        // The prefixed form adds/subtracts the full 16-bit pattern and
        // renders signed.
        let words = prefixed(immediate_unsigned(ImmediateOp::Add, 3, 0), 0xfff0);
        let lines = disassemble_words(&words, 0);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "addi r3, -16");
        assert!(lines[0].wide);
    }

    #[test]
    fn wide_pairs_merge_and_lone_prefixes_stand_alone() {
        let words = [
            prefix12(0xabc),
            immediate_unsigned(ImmediateOp::LoadUnsigned, 3, 0xd),
            prefix12(0x123),
            move_register(1, 2), // prefix expires before a non-consumer
            prefixed_branch(branch(TestCondition::Equal, 0), 0x1234)[0],
            prefixed_branch(branch(TestCondition::Equal, 0), 0x1234)[1],
        ];
        let lines = disassemble_words(&words, 0);
        let texts: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(
            texts,
            ["ldui r3, 43981", "pfx12 0x123", "mov r1, r2", "beq 4660",]
        );
        assert!(lines[0].wide && lines[3].wide);
    }

    #[test]
    fn muli_merges_the_full_prefixed_pattern() {
        let words = prefixed(multiply_immediate(3, 0), 0x1234);
        let lines = disassemble_words(&words, 0);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "muli r3, 4660");
        assert!(lines[0].wide);
    }
}
