//! Disassembler for CpuV3 revision 0.8: word -> typed instruction + mnemonic text.
//!
//! `decode` maps one physical word to an `Instruction`; `disassemble_words` walks
//! a stream and merges a `PFX12` prefix with its consumer into one wide
//! operation (a prefix before a non-consumer renders on its own line).

use crate::{
    fpu_aux_fa, fpu_aux_field_error, fpu_aux_x, fpu_fa, fpu_fb, fpu_fd, fpu_mode,
    fpu_scalar_field_error, fpu_scalar_subop_field, fpu_vector_field_error, fpu_vector_len_field,
    fpu_vector_mode, fpu_vector_subop_field, is_prefix_consumer, AluOp, FpuAuxKind, FpuAuxSubop,
    FpuDotStride, FpuOpcode, FpuScalarSubop, FpuSinCosMode, FpuVectorLength, FpuVectorSubop,
    ImmediateOp, MultiplyWindow, ShiftOp, SpecialRegister, TestCondition, Word,
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
    /// One complete two-word `0xC` VECTOR instruction.
    FpuVector {
        fa: u8,
        fb: u8,
        fd: u8,
        len: FpuVectorLength,
        subop: FpuVectorSubop,
        mode: u8,
    },
    /// One complete two-word `0xD` SCALAR instruction.
    FpuScalar {
        fa: u8,
        fb: u8,
        fd: u8,
        subop: FpuScalarSubop,
        mode: u8,
    },
    /// One complete two-word `0xE` AUX instruction. The subop is raw because
    /// only kind-`00` subops are defined so far.
    FpuAux {
        kind: FpuAuxKind,
        x: u8,
        fa: u8,
        fd: u8,
        subop: u8,
        mode: u8,
    },
    /// A first FPU word seen without its second word. `decode` maps a single
    /// word; the stream disassembler and the simulator always merge the pair.
    FpuWord0 {
        word: Word,
    },
    /// A complete FPU pair whose subop, length, or mode field is reserved.
    FpuReserved {
        word0: Word,
        word1: Word,
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
        // FPU v2 (major 0xC / 0xD / 0xE) is a 32-bit instruction fetched as two
        // 16-bit words. `decode` maps one physical word, so it can only report
        // the first half; `decode_fpu_pair` merges the pair, and
        // `disassemble_words` and `CpuV3Sim` always do.
        0xc..=0xe => Instruction::FpuWord0 { word },
        0xf => Instruction::Prefix {
            payload: word & 0xfff,
        },
        // No other major exists.
        _ => Instruction::Invalid { word },
    }
}

/// Merges a complete FPU v2 word pair (word0 with opcode 0xC/0xD/0xE and the
/// following word1). Returns `FpuReserved` when a subop, length, or mode field
/// is reserved; returns `FpuWord0` when `word0` is not an FPU opcode.
pub fn decode_fpu_pair(word0: Word, word1: Word) -> Instruction {
    let Some(opcode) = FpuOpcode::from_word0(word0) else {
        return Instruction::FpuWord0 { word: word0 };
    };
    let fd = fpu_fd(word1);
    let mode = fpu_mode(word1);
    let vector_mode = fpu_vector_mode(word1);
    match opcode {
        FpuOpcode::Vector => {
            let Some(len) = FpuVectorLength::from_field(fpu_vector_len_field(word1)) else {
                return Instruction::FpuReserved { word0, word1 };
            };
            let Some(subop) = FpuVectorSubop::from_field(fpu_vector_subop_field(word1)) else {
                return Instruction::FpuReserved { word0, word1 };
            };
            // Reused by the strict builder and `CpuV3Sim`, so reserved modes and
            // ranges past F63 reject identically in all three.
            if fpu_vector_field_error(fpu_fa(word0), fpu_fb(word0), fd, len, subop, vector_mode)
                .is_some()
            {
                return Instruction::FpuReserved { word0, word1 };
            }
            Instruction::FpuVector {
                fa: fpu_fa(word0),
                fb: fpu_fb(word0),
                fd,
                len,
                subop,
                mode: vector_mode,
            }
        }
        FpuOpcode::Scalar => {
            let Some(subop) = FpuScalarSubop::from_field(fpu_scalar_subop_field(word1)) else {
                return Instruction::FpuReserved { word0, word1 };
            };
            // Only SINCOS uses the scalar mode field; the shared contract also
            // rejects its `Fd = 63` dual-output overflow.
            if fpu_scalar_field_error(fpu_fa(word0), fpu_fb(word0), fd, subop, mode).is_some() {
                return Instruction::FpuReserved { word0, word1 };
            }
            Instruction::FpuScalar {
                fa: fpu_fa(word0),
                fb: fpu_fb(word0),
                fd,
                subop,
                mode,
            }
        }
        FpuOpcode::Aux => {
            let kind = FpuAuxKind::from_field((word0 & 3) as u8);
            let subop = fpu_scalar_subop_field(word1);
            let Some(aux_subop) = FpuAuxSubop::from_field(subop) else {
                return Instruction::FpuReserved { word0, word1 };
            };
            // The shared contract rejects unimplemented kinds, the reserved
            // FLD/FST mode bits, and their register-range overflow.
            if fpu_aux_field_error(kind, fpu_aux_fa(word0), fd, aux_subop, mode).is_some() {
                return Instruction::FpuReserved { word0, word1 };
            }
            Instruction::FpuAux {
                kind,
                x: fpu_aux_x(word0),
                fa: fpu_aux_fa(word0),
                fd,
                subop,
                mode,
            }
        }
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
                ) || (matches!(op, ImmediateOp::Add | ImmediateOp::Sub)
                    && wide.is_some());
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
            Instruction::FpuVector {
                fa,
                fb,
                fd,
                len,
                subop,
                mode,
            } => {
                let length = len.lanes();
                let name = vector_subop_name(subop);
                let stride = if matches!(
                    subop,
                    FpuVectorSubop::Dot | FpuVectorSubop::DotAdd | FpuVectorSubop::DotStore
                ) {
                    match FpuDotStride::from_mode(mode).unwrap_or(FpuDotStride::Stride1) {
                        FpuDotStride::Stride1 => ".s1",
                        FpuDotStride::Stride3 => ".s3",
                        FpuDotStride::Stride4 => ".s4",
                    }
                } else {
                    ""
                };
                format!("{name}.{length}{stride} f{fd}, f{fa}, f{fb}")
            }
            Instruction::FpuScalar {
                fa,
                fb,
                fd,
                subop,
                mode,
            } => match subop {
                FpuScalarSubop::Mov => format!("mov f{fd}, f{fa}"),
                FpuScalarSubop::Cmp => format!("cmp f{fa}, f{fb}"),
                FpuScalarSubop::SinCos => {
                    let name = match FpuSinCosMode::from_mode(mode).unwrap_or(FpuSinCosMode::SinCos)
                    {
                        FpuSinCosMode::SinCos => "sincos",
                        FpuSinCosMode::Sin => "sin",
                        FpuSinCosMode::Cos => "cos",
                    };
                    format!("{name} f{fd}, f{fa}")
                }
                FpuScalarSubop::Add
                | FpuScalarSubop::Sub
                | FpuScalarSubop::Mul
                | FpuScalarSubop::Min
                | FpuScalarSubop::Max => {
                    format!("{} f{fd}, f{fa}, f{fb}", scalar_subop_name(subop))
                }
                _ => format!("{} f{fd}, f{fa}", scalar_subop_name(subop)),
            },
            Instruction::FpuAux {
                x,
                fa,
                fd,
                subop,
                mode,
                ..
            } => match subop {
                0x00 | 0x01 => {
                    let memory = if subop == 0x00 { "fld" } else { "fst" };
                    if mode == 0 {
                        if subop == 0x00 {
                            format!("{memory} f{fd}, [r{x}]")
                        } else {
                            format!("{memory} [r{x}], f{fa}")
                        }
                    } else {
                        let length = mode + 1;
                        let memory = if subop == 0x00 { "fldv" } else { "fstv" };
                        if subop == 0x00 {
                            format!("{memory}.{length} f{fd}, [r{x}]")
                        } else {
                            format!("{memory}.{length} [r{x}], f{fa}")
                        }
                    }
                }
                0x02 => format!("ilo2f f{fd}, r{x}"),
                0x03 => format!("ihi2f f{fd}, r{x}"),
                0x04 => format!("flo2i r{x}, f{fa}"),
                0x05 => format!("fhi2i r{x}, f{fa}"),
                0x06 => format!("i16tof f{fd}, r{x}"),
                _ => format!("ftoi16 r{x}, f{fa}"),
            },
            Instruction::FpuWord0 { word } => format!(".fpuword 0x{word:04x}"),
            Instruction::FpuReserved { word0, word1 } => {
                format!(".word 0x{word0:04x} 0x{word1:04x}  ; reserved fpu")
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

fn vector_subop_name(subop: FpuVectorSubop) -> &'static str {
    match subop {
        FpuVectorSubop::VAdd => "vadd",
        FpuVectorSubop::VSub => "vsub",
        FpuVectorSubop::VMul => "vmul",
        FpuVectorSubop::VMulS => "vmuls",
        FpuVectorSubop::VMin => "vmin",
        FpuVectorSubop::VMax => "vmax",
        FpuVectorSubop::VAbs => "vabs",
        FpuVectorSubop::VNeg => "vneg",
        FpuVectorSubop::VFloor => "vfloor",
        FpuVectorSubop::VCeil => "vceil",
        FpuVectorSubop::VRound => "vround",
        FpuVectorSubop::VTrunc => "vtrunc",
        FpuVectorSubop::VMove => "vmov",
        FpuVectorSubop::Dot => "dot",
        FpuVectorSubop::DotAdd => "dotadd",
        FpuVectorSubop::DotStore => "dotstore",
    }
}

fn scalar_subop_name(subop: FpuScalarSubop) -> &'static str {
    match subop {
        FpuScalarSubop::Add => "add",
        FpuScalarSubop::Sub => "sub",
        FpuScalarSubop::Mul => "mul",
        FpuScalarSubop::Min => "min",
        FpuScalarSubop::Max => "max",
        FpuScalarSubop::Abs => "abs",
        FpuScalarSubop::Neg => "neg",
        FpuScalarSubop::Floor => "floor",
        FpuScalarSubop::Ceil => "ceil",
        FpuScalarSubop::Round => "round",
        FpuScalarSubop::Trunc => "trunc",
        FpuScalarSubop::Cmp => "cmp",
        FpuScalarSubop::Rcp => "rcp",
        FpuScalarSubop::Rsqrt => "rsqrt",
        FpuScalarSubop::SinCos => "sincos",
        FpuScalarSubop::Mov => "mov",
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
        // An FPU v2 instruction is one 32-bit operation spread over two
        // physical words. It never consumes a prefix, so a preceding PFX12
        // renders on its own line first.
        if FpuOpcode::from_word0(word).is_some() {
            if i + 1 < words.len() {
                let text = decode_fpu_pair(word, words[i + 1]).text(None);
                out.push(DisasmLine {
                    address: base.wrapping_add(i as u16),
                    text,
                    wide: true,
                });
                i += 2;
            } else {
                out.push(DisasmLine {
                    address: base.wrapping_add(i as u16),
                    text: decode(word).text(None),
                    wide: false,
                });
                i += 1;
            }
            continue;
        }
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
        assert_eq!(decode(set_equal(3, 4)).text(None), "seq r3, r4");
        assert_eq!(decode(move_register(0, 0)).text(None), "nop");
        assert_eq!(decode(halt()).text(None), "halt");
        assert_eq!(decode(signal(5, 0)).text(None), "halt r5");
        assert_eq!(decode(signal(5, 1)).text(None), "signal r5, 1");
        assert_eq!(decode(signal(5, 15)).text(None), "signal r5, 15");
    }

    #[test]
    fn fpu_pairs_render_every_family() {
        let text = |pair: [Word; 2]| decode_fpu_pair(pair[0], pair[1]).text(None);
        assert_eq!(
            text(fpu_vector(
                4,
                8,
                20,
                FpuVectorLength::Vec3,
                FpuVectorSubop::VAdd,
                0
            )),
            "vadd.3 f20, f4, f8"
        );
        assert_eq!(
            text(fpu_vector(
                0,
                4,
                0,
                FpuVectorLength::Vec3,
                FpuVectorSubop::VMulS,
                0
            )),
            "vmuls.3 f0, f0, f4"
        );
        assert_eq!(
            text(fpu_vector(
                16,
                0,
                20,
                FpuVectorLength::Vec4,
                FpuVectorSubop::DotStore,
                0
            )),
            "dotstore.4.s1 f20, f16, f0"
        );
        assert_eq!(
            text(fpu_vector(
                4,
                8,
                20,
                FpuVectorLength::Vec4,
                FpuVectorSubop::Dot,
                FpuDotStride::Stride4 as u8
            )),
            "dot.4.s4 f20, f4, f8"
        );
        assert_eq!(
            text(fpu_scalar(3, 4, 5, FpuScalarSubop::Mul, 0)),
            "mul f5, f3, f4"
        );
        assert_eq!(
            text(fpu_scalar(3, 4, 3, FpuScalarSubop::Rsqrt, 0)),
            "rsqrt f3, f3"
        );
        assert_eq!(
            text(fpu_scalar(3, 4, 3, FpuScalarSubop::Cmp, 0)),
            "cmp f3, f4"
        );
        assert_eq!(
            text(fpu_scalar(3, 4, 5, FpuScalarSubop::SinCos, 0)),
            "sincos f5, f3"
        );
        assert_eq!(
            text(fpu_scalar(3, 4, 5, FpuScalarSubop::SinCos, 1)),
            "sin f5, f3"
        );
        assert_eq!(
            text(fpu_aux(
                FpuAuxKind::IntegerRegister,
                2,
                0,
                1,
                FpuAuxSubop::Fld,
                0
            )),
            "fld f1, [r2]"
        );
        assert_eq!(
            text(fpu_aux(
                FpuAuxKind::IntegerRegister,
                1,
                2,
                0,
                FpuAuxSubop::Fst,
                0
            )),
            "fst [r1], f2"
        );
        assert_eq!(
            text(fpu_aux(
                FpuAuxKind::IntegerRegister,
                1,
                0,
                4,
                FpuAuxSubop::Fld,
                2
            )),
            "fldv.3 f4, [r1]"
        );
        assert_eq!(
            text(fpu_aux(
                FpuAuxKind::IntegerRegister,
                5,
                3,
                0,
                FpuAuxSubop::Ilo2f,
                0
            )),
            "ilo2f f0, r5"
        );
        assert_eq!(
            text(fpu_aux(
                FpuAuxKind::IntegerRegister,
                5,
                3,
                0,
                FpuAuxSubop::Fhi2i,
                0
            )),
            "fhi2i r5, f3"
        );
    }

    #[test]
    fn fpu_pairs_do_not_consume_a_prefix_and_merge_streaming() {
        let [word0, word1] = fpu_scalar(3, 4, 5, FpuScalarSubop::Mul, 0);
        let words = [prefix12(0xabc), word0, word1, halt()];
        let lines = disassemble_words(&words, 0);
        let texts: Vec<&str> = lines.iter().map(|line| line.text.as_str()).collect();
        assert_eq!(texts, ["pfx12 0xabc", "mul f5, f3, f4", "halt"]);
        // The prefix renders alone; the FPU pair is one wide two-word line.
        assert!(!lines[0].wide);
        assert!(lines[1].wide);
        assert_eq!(lines[1].address, 1);
        // A trailing first word without its second half is reported as such.
        let lone = disassemble_words(&[word0], 0);
        assert_eq!(lone.len(), 1);
        assert_eq!(lone[0].text, ".fpuword 0xd0c4");
        assert!(!lone[0].wide);
    }

    #[test]
    fn fpu_reserved_pairs_and_lone_first_words_are_reported() {
        // Reserved vector subop 0x10.
        assert!(matches!(
            decode_fpu_pair(0xc000, 0x0080),
            Instruction::FpuReserved { .. }
        ));
        // Reserved vector length 11.
        assert!(matches!(
            decode_fpu_pair(0xc000, 0b0000_0011_0000_0000),
            Instruction::FpuReserved { .. }
        ));
        // Reserved DOT stride 11.
        assert!(matches!(
            decode_fpu_pair(0xc000, 0x0068 | 0b11),
            Instruction::FpuReserved { .. }
        ));
        // DOT mode bit 2 is reserved even when mode[1:0] names a stride.
        assert!(matches!(
            decode_fpu_pair(0xc000, 0x0068 | 0b100),
            Instruction::FpuReserved { .. }
        ));
        // VADD.4 with Fa=61 overflows F63 and is rejected by the decoder too.
        assert!(matches!(
            decode_fpu_pair(0xcf40, 0x0200),
            Instruction::FpuReserved { .. }
        ));
        // Non-DOT vector subops reject a nonzero mode.
        assert!(matches!(
            decode_fpu_pair(0xc000, 0x0001),
            Instruction::FpuReserved { .. }
        ));
        // FLDV4 with Fd=62 overflows F63.
        assert!(matches!(
            decode_fpu_pair(0xe100, 0xf803),
            Instruction::FpuReserved { .. }
        ));
        // Reserved scalar subop 0x10.
        assert!(matches!(
            decode_fpu_pair(0xd000, 0x0100),
            Instruction::FpuReserved { .. }
        ));
        // SINCOS mode 11 is reserved.
        assert!(matches!(
            decode_fpu_pair(0xd000, 0x00e3),
            Instruction::FpuReserved { .. }
        ));
        // AUX reserved kind 11.
        assert!(matches!(
            decode_fpu_pair(0xe003, 0x0000),
            Instruction::FpuReserved { .. }
        ));
        // A non-FPU first word is not silently merged.
        assert!(matches!(
            decode_fpu_pair(0x6000, 0x0000),
            Instruction::FpuWord0 { .. }
        ));
    }

    #[test]
    fn reserved_and_noncanonical_encodings_are_invalid() {
        // Majors C/D/E are FPU v2 first words; one word is only half of a
        // two-word instruction. Reserved *pairs* are handled by
        // `decode_fpu_pair` above.
        assert_eq!(decode(0xc000), Instruction::FpuWord0 { word: 0xc000 });
        assert_eq!(decode(0xcabc), Instruction::FpuWord0 { word: 0xcabc });
        assert_eq!(decode(0xd000), Instruction::FpuWord0 { word: 0xd000 });
        assert_eq!(decode(0xe100), Instruction::FpuWord0 { word: 0xe100 });
        assert_eq!(decode(0xefff), Instruction::FpuWord0 { word: 0xefff });
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
