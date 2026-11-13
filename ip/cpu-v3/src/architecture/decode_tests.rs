use super::*;
use crate::*;

#[test]
fn memory_offsets_render_unsigned_short_and_signed_wide() {
    for offset in 0..=15 {
        for word in [load(3, 4, offset), store(3, 4, offset)] {
            let instruction = decode(word);
            assert!(instruction
                .text(None)
                .ends_with(&format!("[r4 + {offset}]")));
            for wide in [-32768i16, -16, -1, 0, 8, 15, 16, 32767] {
                let pair = prefixed(word, wide as u16);
                assert!(decode(pair[1])
                    .text(Some(pair[0] & 0xfff))
                    .ends_with(&format!("[r4 + {wide}]")));
            }
        }
    }
    for invalid in [-32768, -1, 16, 256, 32767] {
        assert!(std::panic::catch_unwind(|| load(0, 0, invalid)).is_err());
        assert!(std::panic::catch_unwind(|| store(0, 0, invalid)).is_err());
    }
}

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
    assert_eq!(decode(load(3, 4, 15)).text(None), "load r3, [r4 + 15]");
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
fn vector_rounding_slots_are_reserved_for_every_length_and_mode() {
    for subop in 0x08..=0x0b {
        assert!(FpuVectorSubop::from_field(subop).is_none());
        for len in 0..=3 {
            for mode in 0..=7 {
                let word1 = (16 << 10) | (len << 8) | (u16::from(subop) << 3) | mode;
                let instruction = decode_fpu_pair(0xc000, word1);
                assert!(matches!(instruction, Instruction::FpuReserved { .. }));
                assert!(instruction.text(None).contains("reserved fpu"));
            }
        }
    }
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
    assert_eq!(
        decode(0x6712),
        Instruction::LineCopy {
            source: 1,
            destination: 2,
        }
    );
    // Non-canonical special-register selectors are invalid.
    assert_eq!(decode(0x6d36), Instruction::Invalid { word: 0x6d36 });
    assert_eq!(decode(0x6e04), Instruction::Invalid { word: 0x6e04 });
    assert_eq!(
        decode(0x6e24),
        Instruction::WriteDataSegment {
            index: Some(0),
            src: 4
        }
    );
    assert_eq!(
        decode(0x6e64),
        Instruction::DataCacheCleanLine { address: 4 }
    );
    assert_eq!(decode(0x6e74), Instruction::Invalid { word: 0x6e74 });
    assert_eq!(decode(0x6e80), Instruction::DataCacheWait);
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
