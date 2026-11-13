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
    assert_eq!(write_data_segment_page(0, 4), 0x6e24);
    assert_eq!(write_data_segment_page(3, 4), 0x6e54);
    assert_eq!(clean_data_line(4), 0x6e64);
    assert_eq!(wait_data_cache(), 0x6e80);
    assert_eq!(jump_segment(3, 4), 0x6f34);
    assert_eq!(device_receive(3, 2, 1), 0x7213);
    assert_eq!(device_send(3, 2, 1), 0x7a13);
    assert_eq!(load(3, 4, 15), 0x834f);
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
        for subop in (0..=0x07u8).chain(0x0c..=0x0f) {
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
    let reserved = |word: Word| matches!(crate::decode(word), crate::Instruction::Invalid { .. });
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
