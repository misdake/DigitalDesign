//! Disassembler for CpuV3 revision 0.12: word -> typed instruction + mnemonic text.
//!
//! `decode` maps one physical word to an `Instruction`; `disassemble_words` walks
//! a stream and merges a `PFX12` prefix with its consumer into one wide
//! operation (a prefix before a non-consumer renders on its own line).

use crate::{
    fpu_aux_fa, fpu_aux_field_error, fpu_aux_x, fpu_fa, fpu_fb, fpu_fd, fpu_mode,
    fpu_scalar_field_error, fpu_scalar_subop_field, fpu_vector_field_error, fpu_vector_len_field,
    fpu_vector_mode, fpu_vector_subop_field, is_prefix_consumer, AluOp, FpuAuxKind, FpuAuxSubop,
    FpuDotStride, FpuMulMode, FpuOpcode, FpuScalarSubop, FpuSinCosMode, FpuVectorLength,
    FpuVectorSubop, ImmediateOp, MultiplyWindow, ShiftOp, SpecialRegister, TestCondition, Word,
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
    LineCopy {
        source: u8,
        destination: u8,
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
        index: Option<u8>,
        src: u8,
    },
    DataCacheCleanLine {
        address: u8,
    },
    DataCacheWait,
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
                // 3, B, D..F are reserved.
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
            7 => Instruction::LineCopy {
                source: reg(n1.into()),
                destination: reg(n0.into()),
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
            0xd if n0 <= 5 => Instruction::ReadSpecial {
                dst: reg(n1.into()),
                special: match n0 {
                    0 => SpecialRegister::CodeSegment,
                    1 => SpecialRegister::DataSegment,
                    2 => SpecialRegister::DataSegment0,
                    3 => SpecialRegister::DataSegment1,
                    4 => SpecialRegister::DataSegment2,
                    5 => SpecialRegister::DataSegment3,
                    _ => unreachable!(),
                },
            },
            0xe if (1..=5).contains(&n1) => Instruction::WriteDataSegment {
                index: if n1 >= 2 { Some(n1 - 2) } else { None },
                src: reg(n0.into()),
            },
            0xe if n1 == crate::DATA_CACHE_CLEAN_LINE_SELECTOR => Instruction::DataCacheCleanLine {
                address: reg(n0.into()),
            },
            0xe if n1 == crate::DATA_CACHE_WAIT_SELECTOR && n0 == 0 => Instruction::DataCacheWait,
            0xf => Instruction::JumpSegment {
                segment: reg(n1.into()),
                target: reg(n0.into()),
            },
            // Non-canonical special-register selectors fault.
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
            offset: i16::from(n0),
        },
        9 => Instruction::Store {
            src: reg(n2.into()),
            base: reg(n1.into()),
            offset: i16::from(n0),
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
            // Check scalar mode bits and the SINCOS dual-output destination range.
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
                FpuScalarSubop::Mul => {
                    let name = match FpuMulMode::from_mode(mode).unwrap_or(FpuMulMode::Shift16) {
                        FpuMulMode::Shift16 => "mul",
                        FpuMulMode::Low32 => "mul.lo32",
                        FpuMulMode::Shift8 => "mul.shr8",
                        FpuMulMode::Shift24 => "mul.shr24",
                    };
                    format!("{name} f{fd}, f{fa}, f{fb}")
                }
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
            Instruction::LineCopy {
                source,
                destination,
            } => format!("lcopy r{source}, r{destination}"),
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
                SpecialRegister::DataSegment0 => format!("mfsr r{dst}, DSEG0"),
                SpecialRegister::DataSegment1 => format!("mfsr r{dst}, DSEG1"),
                SpecialRegister::DataSegment2 => format!("mfsr r{dst}, DSEG2"),
                SpecialRegister::DataSegment3 => format!("mfsr r{dst}, DSEG3"),
            },
            Instruction::WriteDataSegment { index: None, src } => format!("mtsr DSEG, r{src}"),
            Instruction::WriteDataSegment {
                index: Some(index),
                src,
            } => format!("mtsr DSEG{index}, r{src}"),
            Instruction::DataCacheCleanLine { address } => format!("dcleanl r{address}"),
            Instruction::DataCacheWait => "dwait".to_string(),
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
#[path = "decode_tests.rs"]
mod tests;
