//! Disassembler for CpuV3 revision 0.7: word -> typed instruction + mnemonic text.
//!
//! `decode` maps one physical word to an `Instruction`; `disassemble_words` walks
//! a stream and merges an `IMMHI12` prefix with its consumer into one wide
//! operation (a prefix before a non-consumer renders on its own line).

use crate::{
    is_prefix_consumer, AluOp, FpuOp, FpuUnaryOp, ImmediateOp, SpecialRegister, TestCondition, Word,
};

/// One decoded CpuV3 instruction word (or the whole two-word wide operation).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Instruction {
    Alu { op: AluOp, dst: u8, lhs: u8, rhs: u8 },
    Load { dst: u8, base: u8, offset: i16 },
    Store { src: u8, base: u8, offset: i16 },
    Immediate { op: ImmediateOp, dst: u8, value: u16 },
    Branch { condition: TestCondition, offset: i16 },
    JumpRelative { offset: i16, link: bool },
    DeviceReceive { dst: u8, device: u8, channel: u8 },
    DeviceSend { src: u8, device: u8, channel: u8 },
    Fpu { op: FpuOp, a: u8, b: u8 },
    FpuUnary { dst: u8, op: FpuUnaryOp },
    PopulationCount { dst: u8, src: u8 },
    Move { dst: u8, src: u8 },
    Not { dst: u8, src: u8 },
    Negate { dst: u8, src: u8 },
    JumpRegister { target: u8 },
    JumpAndLinkRegister { target: u8 },
    SignExtendByte { dst: u8, src: u8 },
    LeadingZeros { dst: u8, src: u8 },
    SetLessThanSigned { dst: u8, rhs: u8 },
    SetLessThanUnsigned { dst: u8, rhs: u8 },
    CompareSigned { lhs: u8, rhs: u8 },
    CompareUnsigned { lhs: u8, rhs: u8 },
    ReadSpecial { dst: u8, special: SpecialRegister },
    WriteDataSegment { src: u8 },
    JumpSegment { segment: u8, target: u8 },
    Halt,
    Prefix { high: u16 },
    Invalid { word: Word },
}

fn reg(index: Word) -> u8 {
    index as u8
}

/// Decodes one word. Prefix merging is a stream concern; decode() alone keeps
/// the raw 4-bit immediate/offset fields.
pub fn decode(word: Word) -> Instruction {
    let n3 = word >> 12;
    let n2 = ((word >> 8) & 0xf) as u8;
    let n1 = ((word >> 4) & 0xf) as u8;
    let n0 = (word & 0xf) as u8;
    match n3 {
        0..=7 => Instruction::Alu {
            op: match n3 {
                0 => AluOp::Add,
                1 => AluOp::Sub,
                2 => AluOp::Mul,
                3 => AluOp::And,
                4 => AluOp::Or,
                5 => AluOp::Xor,
                6 => AluOp::ShiftLeft,
                _ => AluOp::ShiftRightArithmetic,
            },
            dst: reg(n2.into()),
            lhs: reg(n1.into()),
            rhs: reg(n0.into()),
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
                2 => ImmediateOp::And,
                3 => ImmediateOp::Or,
                4 => ImmediateOp::Xor,
                5 => ImmediateOp::ShiftLeft,
                6 => ImmediateOp::ShiftRightLogical,
                7 => ImmediateOp::ShiftRightArithmetic,
                8 => ImmediateOp::Multiply,
                9 => ImmediateOp::CompareEqual,
                10 => ImmediateOp::CompareLessThanSigned,
                11 => ImmediateOp::CompareLessThanUnsigned,
                12 => ImmediateOp::CompareSigned,
                13 => ImmediateOp::CompareUnsigned,
                14 => ImmediateOp::LoadSigned,
                _ => ImmediateOp::LoadUnsigned,
            };
            Instruction::Immediate {
                op,
                dst: reg(n1.into()),
                value: n0.into(),
            }
        }
        0xb => match n2 {
            0..=5 => Instruction::Branch {
                condition: match n2 {
                    0 => TestCondition::Equal,
                    1 => TestCondition::NotEqual,
                    2 => TestCondition::LessThan,
                    3 => TestCondition::GreaterOrEqual,
                    4 => TestCondition::GreaterThan,
                    _ => TestCondition::LessOrEqual,
                },
                offset: crate::sign_extend(word & 0xff, 8) as i16,
            },
            8 => Instruction::JumpRelative {
                offset: crate::sign_extend(word & 0xff, 8) as i16,
                link: false,
            },
            9 => Instruction::JumpRelative {
                offset: crate::sign_extend(word & 0xff, 8) as i16,
                link: true,
            },
            _ => Instruction::Invalid { word },
        },
        0xc if word & 0x800 == 0 => Instruction::DeviceReceive {
            dst: reg(n0.into()),
            device: n2 & 7,
            channel: n1,
        },
        0xc => Instruction::DeviceSend {
            src: reg(n0.into()),
            device: n2 & 7,
            channel: n1,
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
        0xe => match n2 {
            0 => Instruction::PopulationCount {
                dst: reg(n1.into()),
                src: reg(n0.into()),
            },
            1 => Instruction::Move {
                dst: reg(n1.into()),
                src: reg(n0.into()),
            },
            2 => Instruction::Not {
                dst: reg(n1.into()),
                src: reg(n0.into()),
            },
            3 => Instruction::Negate {
                dst: reg(n1.into()),
                src: reg(n0.into()),
            },
            4 if n1 == 0 => Instruction::JumpRegister {
                target: reg(n0.into()),
            },
            5 if n1 == 14 => Instruction::JumpAndLinkRegister {
                target: reg(n0.into()),
            },
            6 => Instruction::SignExtendByte {
                dst: reg(n1.into()),
                src: reg(n0.into()),
            },
            7 => Instruction::LeadingZeros {
                dst: reg(n1.into()),
                src: reg(n0.into()),
            },
            8 if n1 == 0 && n0 == 0 => Instruction::Halt,
            9 => Instruction::SetLessThanSigned {
                dst: reg(n1.into()),
                rhs: reg(n0.into()),
            },
            10 => Instruction::SetLessThanUnsigned {
                dst: reg(n1.into()),
                rhs: reg(n0.into()),
            },
            11 => Instruction::CompareSigned {
                lhs: reg(n1.into()),
                rhs: reg(n0.into()),
            },
            12 => Instruction::CompareUnsigned {
                lhs: reg(n1.into()),
                rhs: reg(n0.into()),
            },
            13 => Instruction::ReadSpecial {
                dst: reg(n1.into()),
                special: if n0 == 0 {
                    SpecialRegister::CodeSegment
                } else {
                    SpecialRegister::DataSegment
                },
            },
            14 if n1 == 1 => Instruction::WriteDataSegment {
                src: reg(n0.into()),
            },
            15 => Instruction::JumpSegment {
                segment: reg(n1.into()),
                target: reg(n0.into()),
            },
            _ => Instruction::Invalid { word },
        },
        0xf => Instruction::Prefix {
            high: word & 0xfff,
        },
        _ => Instruction::Invalid { word },
    }
}

impl Instruction {
    /// Renders the instruction, given the pending prefix value if this
    /// instruction consumes one (widened immediate/offset where applicable).
    pub fn text(&self, prefix: Option<u16>) -> String {
        match *self {
            Instruction::Alu { op, dst, lhs, rhs } => {
                let name = match op {
                    AluOp::Add => "add",
                    AluOp::Sub => "sub",
                    AluOp::Mul => "mul",
                    AluOp::And => "and",
                    AluOp::Or => "or",
                    AluOp::Xor => "xor",
                    AluOp::ShiftLeft => "shl",
                    AluOp::ShiftRightArithmetic => "asr",
                };
                format!("{name} r{dst}, r{lhs}, r{rhs}")
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
                let wide = prefix.map(|high| (high << 4) | value);
                let signed = matches!(
                    op,
                    ImmediateOp::Add
                        | ImmediateOp::Sub
                        | ImmediateOp::Multiply
                        | ImmediateOp::CompareEqual
                        | ImmediateOp::CompareLessThanSigned
                        | ImmediateOp::CompareSigned
                        | ImmediateOp::LoadSigned
                );
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
                    ImmediateOp::And => "andi",
                    ImmediateOp::Or => "ori",
                    ImmediateOp::Xor => "xori",
                    ImmediateOp::ShiftLeft => "shli",
                    ImmediateOp::ShiftRightLogical => "shri",
                    ImmediateOp::ShiftRightArithmetic => "asri",
                    ImmediateOp::Multiply => "muli",
                    ImmediateOp::CompareEqual => "cmpeqi",
                    ImmediateOp::CompareLessThanSigned => "slti",
                    ImmediateOp::CompareLessThanUnsigned => "sltui",
                    ImmediateOp::CompareSigned => "cmpsi",
                    ImmediateOp::CompareUnsigned => "cmpui",
                    ImmediateOp::LoadSigned => "ldi",
                    ImmediateOp::LoadUnsigned => "ldui",
                };
                format!("{name} r{dst}, {shown}")
            }
            Instruction::Branch { condition, offset } => {
                let offset = wide_branch(prefix, offset);
                let name = match condition {
                    TestCondition::Equal => "beq",
                    TestCondition::NotEqual => "bne",
                    TestCondition::LessThan => "blt",
                    TestCondition::GreaterOrEqual => "bge",
                    TestCondition::GreaterThan => "bgt",
                    TestCondition::LessOrEqual => "ble",
                };
                format!("{name} {offset}")
            }
            Instruction::JumpRelative { offset, link } => {
                let offset = wide_branch(prefix, offset);
                format!("{} {offset}", if link { "jalrel" } else { "jrel" })
            }
            Instruction::DeviceReceive { dst, device, channel } => {
                format!("devrecv r{dst}, dev{device}.ch{channel}")
            }
            Instruction::DeviceSend { src, device, channel } => {
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
            Instruction::PopulationCount { dst, src } => format!("popcnt r{dst}, r{src}"),
            Instruction::Move { dst, src } => {
                if dst == src {
                    "nop".to_string()
                } else {
                    format!("mov r{dst}, r{src}")
                }
            }
            Instruction::Not { dst, src } => format!("not r{dst}, r{src}"),
            Instruction::Negate { dst, src } => format!("neg r{dst}, r{src}"),
            Instruction::JumpRegister { target } => format!("jreg r{target}"),
            Instruction::JumpAndLinkRegister { target } => format!("jalr r{target}"),
            Instruction::SignExtendByte { dst, src } => format!("sextb r{dst}, r{src}"),
            Instruction::LeadingZeros { dst, src } => format!("clz r{dst}, r{src}"),
            Instruction::SetLessThanSigned { dst, rhs } => format!("slt r{dst}, r{rhs}"),
            Instruction::SetLessThanUnsigned { dst, rhs } => format!("sltu r{dst}, r{rhs}"),
            Instruction::CompareSigned { lhs, rhs } => format!("cmps r{lhs}, r{rhs}"),
            Instruction::CompareUnsigned { lhs, rhs } => format!("cmpu r{lhs}, r{rhs}"),
            Instruction::ReadSpecial { dst, special } => match special {
                SpecialRegister::CodeSegment => format!("mfsr r{dst}, CSEG"),
                SpecialRegister::DataSegment => format!("mfsr r{dst}, DSEG"),
            },
            Instruction::WriteDataSegment { src } => format!("mtsr DSEG, r{src}"),
            Instruction::JumpSegment { segment, target } => {
                format!("jseg r{segment}, r{target}")
            }
            Instruction::Halt => "halt".to_string(),
            Instruction::Prefix { high } => format!("immhi12 0x{high:03x}"),
            Instruction::Invalid { word } => format!(".word 0x{word:04x}  ; invalid"),
        }
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
        Some(high) => i32::from(((high << 4) | (offset as u16 & 0xf)) as i16),
        None => i32::from(offset),
    }
}

/// Wide branch offset: the prefix's low byte is the offset's high byte.
fn wide_branch(prefix: Option<u16>, offset: i16) -> i32 {
    match prefix {
        None => i32::from(offset),
        Some(high) => i32::from((((high & 0xff) << 8) | (offset as u16 & 0xff)) as i16),
    }
}

/// One disassembled line: the word address, the rendered text, and the raw words.
pub struct DisasmLine {
    pub address: u16,
    pub text: String,
    pub wide: bool,
}

/// Disassembles a word stream at `base`, merging each IMMHI12 prefix with an
/// eligible consumer. A prefix before a non-consumer renders on its own line.
pub fn disassemble_words(words: &[Word], base: u16) -> Vec<DisasmLine> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < words.len() {
        let word = words[i];
        if let Instruction::Prefix { high } = decode(word) {
            if i + 1 < words.len() && is_prefix_consumer(words[i + 1]) {
                let text = decode(words[i + 1]).text(Some(high));
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
        assert_eq!(decode(alu(AluOp::Add, 3, 1, 2)).text(None), "add r3, r1, r2");
        assert_eq!(decode(load(3, 4, -1)).text(None), "load r3, [r4 + -1]");
        assert_eq!(decode(store(3, 4, 7)).text(None), "store r3, [r4 + 7]");
        assert_eq!(decode(branch(TestCondition::NotEqual, -3)).text(None), "bne -3");
        assert_eq!(decode(jump_relative(-2)).text(None), "jrel -2");
        assert_eq!(decode(jump_and_link_relative(-2)).text(None), "jalrel -2");
        assert_eq!(decode(device_receive(3, 2, 1)).text(None), "devrecv r3, dev2.ch1");
        assert_eq!(decode(device_send(3, 2, 1)).text(None), "devsend dev2.ch1, r3");
        assert_eq!(decode(fpu(FpuOp::Mul, 3, 4)).text(None), "fmul f3, f4");
        assert_eq!(
            decode(fpu(FpuOp::AccStore, 3, 0b0101)).text(None),
            "faccstore f3, 0b0101"
        );
        assert_eq!(
            decode(fpu_unary(3, FpuUnaryOp::AccLoadW)).text(None),
            "faccload.w f3"
        );
        assert_eq!(decode(move_register(0, 0)).text(None), "nop");
        assert_eq!(decode(halt()).text(None), "halt");
        assert_eq!(decode(0xb600).text(None), ".word 0xb600  ; invalid");
    }

    #[test]
    fn wide_pairs_merge_and_lone_prefixes_stand_alone() {
        let words = [
            immediate_high12(0xabc),
            immediate_unsigned(ImmediateOp::LoadUnsigned, 3, 0xd),
            immediate_high12(0x123),
            move_register(1, 2), // prefix expires before a non-consumer
            prefixed_branch(branch(TestCondition::Equal, 0), 0x1234)[0],
            prefixed_branch(branch(TestCondition::Equal, 0), 0x1234)[1],
        ];
        let lines = disassemble_words(&words, 0);
        let texts: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(
            texts,
            [
                "ldui r3, 43981",
                "immhi12 0x123",
                "mov r1, r2",
                "beq 4660",
            ]
        );
        assert!(lines[0].wide && lines[3].wide);
    }
}
