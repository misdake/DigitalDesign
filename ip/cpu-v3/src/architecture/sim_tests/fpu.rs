use super::*;

#[test]
fn fpu_scalar_two_word_instructions_retire_as_pairs() {
    let mut program = vec![];
    program.extend(load_immediate16(1, 3));
    program.extend(load_immediate16(2, 4));
    program.extend(fpu_aux(
        FpuAuxKind::IntegerRegister,
        1,
        0,
        0,
        FpuAuxSubop::I16tof,
        0,
    )); // f0 = 3.0
    program.extend(fpu_aux(
        FpuAuxKind::IntegerRegister,
        2,
        0,
        1,
        FpuAuxSubop::I16tof,
        0,
    )); // f1 = 4.0
    program.extend(fpu_scalar(0, 1, 2, FpuScalarSubop::Add, 0)); // f2 = 7.0
    program.extend(fpu_scalar(0, 1, 3, FpuScalarSubop::Mul, 0)); // f3 = 12.0
    program.extend(fpu_scalar(2, 0, 4, FpuScalarSubop::Mov, 0)); // f4 = 7.0
    program.extend(fpu_aux(
        FpuAuxKind::IntegerRegister,
        3,
        3,
        0,
        FpuAuxSubop::Ftoi16,
        0,
    )); // r3 = trunc(f3) = 12
    program.push(halt());
    let words = program.len();
    let mut machine = CpuV3Sim::default();
    machine.load_program(0, &program).unwrap();
    assert!(matches!(
        machine.run(64).unwrap(),
        RunOutcome::Halted { .. }
    ));
    assert_eq!(machine.fpu_register(0), Some(3 << 16));
    assert_eq!(machine.fpu_register(1), Some(4 << 16));
    assert_eq!(machine.fpu_register(2), Some(7 << 16));
    assert_eq!(machine.fpu_register(3), Some(12 << 16));
    assert_eq!(machine.fpu_register(4), Some(7 << 16));
    assert_eq!(machine.register(3), Some(12));
    // Every FPU instruction retires two words.
    assert_eq!(machine.retired_words(), words as u64);
}

#[test]
fn fpu_vector_lanes_dot_and_stride_follow_the_range_model() {
    let mut program = vec![];
    program.extend(load_immediate16(1, 0x0100));
    program.extend(fpu_aux(
        FpuAuxKind::IntegerRegister,
        1,
        0,
        0,
        FpuAuxSubop::Fld,
        3,
    )); // FLDV4 f0..f3 = 1,2,3,4
    program.extend(fpu_vector(
        0,
        0,
        4,
        FpuVectorLength::Vec4,
        FpuVectorSubop::VAdd,
        0,
    )); // f4..f7 = 2,4,6,8
    program.extend(fpu_vector(
        0,
        0,
        8,
        FpuVectorLength::Vec4,
        FpuVectorSubop::VMul,
        0,
    )); // f8..f11 = 1,4,9,16
    program.extend(fpu_vector(
        0,
        0,
        16,
        FpuVectorLength::Vec4,
        FpuVectorSubop::VMulS,
        0,
    )); // f16..f19 = 1,2,3,4
    program.extend(fpu_vector(
        0,
        0,
        12,
        FpuVectorLength::Vec4,
        FpuVectorSubop::DotStore,
        0,
    )); // f12 = 30.0
    program.push(halt());
    let mut machine = CpuV3Sim::default();
    machine.load_program(0, &program).unwrap();
    machine
        .load_program(
            0x0100,
            &[
                0x0000, 0x0001, 0x0000, 0x0002, 0x0000, 0x0003, 0x0000, 0x0004,
            ],
        )
        .unwrap();
    assert!(matches!(
        machine.run(64).unwrap(),
        RunOutcome::Halted { .. }
    ));
    assert_eq!(
        [
            machine.fpu_register(0),
            machine.fpu_register(1),
            machine.fpu_register(2),
            machine.fpu_register(3),
        ],
        [Some(1 << 16), Some(2 << 16), Some(3 << 16), Some(4 << 16)]
    );
    assert_eq!(
        [
            machine.fpu_register(4),
            machine.fpu_register(5),
            machine.fpu_register(6),
            machine.fpu_register(7),
        ],
        [Some(2 << 16), Some(4 << 16), Some(6 << 16), Some(8 << 16)]
    );
    assert_eq!(
        [
            machine.fpu_register(8),
            machine.fpu_register(9),
            machine.fpu_register(10),
            machine.fpu_register(11),
        ],
        [Some(1 << 16), Some(4 << 16), Some(9 << 16), Some(16 << 16)]
    );
    assert_eq!(
        [
            machine.fpu_register(16),
            machine.fpu_register(17),
            machine.fpu_register(18),
            machine.fpu_register(19),
        ],
        [Some(1 << 16), Some(2 << 16), Some(3 << 16), Some(4 << 16)]
    );
    assert_eq!(machine.fpu_register(12), Some(30 << 16));
    assert_eq!(machine.fpu_accumulator(), 0);
}

#[test]
fn fpu_dot_stride_reads_the_second_source_with_a_step() {
    let mut program = vec![];
    program.extend(load_immediate16(1, 0x0100));
    program.extend(fpu_aux(
        FpuAuxKind::IntegerRegister,
        1,
        0,
        0,
        FpuAuxSubop::Fld,
        3,
    )); // FLDV4 f0..f3 = 1,2,3,4
        // B = f8, f11, f14, f17 (stride 3), all set to 1.0 by I16TOF.
    program.extend(load_immediate16(2, 1));
    for fd in [8u8, 11, 14, 17] {
        program.extend(fpu_aux(
            FpuAuxKind::IntegerRegister,
            2,
            0,
            fd,
            FpuAuxSubop::I16tof,
            0,
        ));
    }
    program.extend(fpu_vector(
        0,
        8,
        20,
        FpuVectorLength::Vec4,
        FpuVectorSubop::DotStore,
        FpuDotStride::Stride3 as u8,
    )); // f20 = 1+2+3+4 = 10.0
    program.push(halt());
    let mut machine = CpuV3Sim::default();
    machine.load_program(0, &program).unwrap();
    machine
        .load_program(
            0x0100,
            &[
                0x0000, 0x0001, 0x0000, 0x0002, 0x0000, 0x0003, 0x0000, 0x0004,
            ],
        )
        .unwrap();
    assert!(matches!(
        machine.run(64).unwrap(),
        RunOutcome::Halted { .. }
    ));
    assert_eq!(machine.fpu_register(20), Some(10 << 16));
}

#[test]
fn fpu_aux_memory_moves_low_half_first_and_bridges_raw_halves() {
    let mut program = vec![];
    program.extend(load_immediate16(1, 0x0100));
    program.extend(load_immediate16(2, 0x0102));
    program.extend(load_immediate16(3, 0xbeef));
    program.extend(fpu_aux(
        FpuAuxKind::IntegerRegister,
        1,
        0,
        0,
        FpuAuxSubop::Fld,
        0,
    )); // f0 = {mem[0x0101], mem[0x0100]}
    program.extend(fpu_aux(
        FpuAuxKind::IntegerRegister,
        2,
        0,
        0,
        FpuAuxSubop::Fst,
        0,
    )); // mem[0x0102..] = f0, low half first
    program.extend(fpu_aux(
        FpuAuxKind::IntegerRegister,
        3,
        0,
        1,
        FpuAuxSubop::Ilo2f,
        0,
    )); // f1 low = 0xbeef
    program.extend(fpu_aux(
        FpuAuxKind::IntegerRegister,
        3,
        0,
        2,
        FpuAuxSubop::Ihi2f,
        0,
    )); // f2 high = 0xbeef
    program.extend(fpu_aux(
        FpuAuxKind::IntegerRegister,
        4,
        0,
        0,
        FpuAuxSubop::Flo2i,
        0,
    )); // r4 = f0 low
    program.extend(fpu_aux(
        FpuAuxKind::IntegerRegister,
        5,
        0,
        0,
        FpuAuxSubop::Fhi2i,
        0,
    )); // r5 = f0 high
    program.push(halt());
    let mut machine = CpuV3Sim::default();
    machine.load_program(0, &program).unwrap();
    machine.load_program(0x0100, &[0x1234, 0x5678]).unwrap();
    assert!(matches!(
        machine.run(64).unwrap(),
        RunOutcome::Halted { .. }
    ));
    assert_eq!(machine.fpu_register(0), Some(0x5678_1234));
    assert_eq!(machine.memory(0x0102), 0x1234);
    assert_eq!(machine.memory(0x0103), 0x5678);
    assert_eq!(machine.fpu_register(1), Some(0x0000_beef));
    assert_eq!(machine.fpu_register(2), Some(0xbeef_0000_u32 as i32));
    assert_eq!(machine.register(4), Some(0x1234));
    assert_eq!(machine.register(5), Some(0x5678));
}

#[test]
fn fpu_special_functions_are_bit_exact_with_the_reference_model() {
    for input in [
        0,
        1,
        -1,
        0x0001_0000,
        -0x0001_0000,
        0x0002_0000,
        0x7fff_ffff,
        i32::MIN,
        0x1234_5678,
        -0x1234_5678,
    ] {
        let (sin, cos) = sincos_q16(input);
        assert_eq!(
            run_special(input),
            (rcp_q16(input), rsqrt_q16(input), sin, cos),
            "input {input:#010x}"
        );
    }
}

#[test]
fn fpu_scalar_arithmetic_wraps_and_rounds_half_up() {
    let mut program = vec![];
    program.extend(load_immediate16(1, 0x7fff));
    program.extend(load_immediate16(2, 1));
    program.extend(fpu_aux(
        FpuAuxKind::IntegerRegister,
        1,
        0,
        0,
        FpuAuxSubop::I16tof,
        0,
    )); // f0 = 32767.0
    program.extend(fpu_aux(
        FpuAuxKind::IntegerRegister,
        2,
        0,
        1,
        FpuAuxSubop::I16tof,
        0,
    )); // f1 = 1.0
    program.extend(fpu_scalar(0, 1, 2, FpuScalarSubop::Add, 0)); // wraps to i32::MIN
    program.extend(fpu_aux(
        FpuAuxKind::IntegerRegister,
        3,
        2,
        0,
        FpuAuxSubop::Ftoi16,
        0,
    )); // r3 = -32768 (0x8000)
    program.extend(load_immediate16(4, 0x0100));
    program.extend(fpu_aux(
        FpuAuxKind::IntegerRegister,
        4,
        0,
        4,
        FpuAuxSubop::Fld,
        0,
    )); // f4 = 0.5 from memory
    program.extend(fpu_scalar(4, 0, 5, FpuScalarSubop::Round, 0)); // 0.5 -> 1.0
    program.extend(fpu_aux(
        FpuAuxKind::IntegerRegister,
        6,
        5,
        0,
        FpuAuxSubop::Ftoi16,
        0,
    )); // r6 = 1
    program.extend(load_immediate16(7, 0x0102));
    program.extend(fpu_aux(
        FpuAuxKind::IntegerRegister,
        7,
        0,
        6,
        FpuAuxSubop::Fld,
        0,
    )); // f6 = -0.5 from memory
    program.extend(fpu_scalar(6, 0, 7, FpuScalarSubop::Round, 0)); // -0.5 -> 0.0
    program.extend(fpu_aux(
        FpuAuxKind::IntegerRegister,
        8,
        7,
        0,
        FpuAuxSubop::Ftoi16,
        0,
    )); // r8 = 0
    program.push(halt());
    let mut machine = CpuV3Sim::default();
    machine.load_program(0, &program).unwrap();
    machine
        .load_program(
            0x0100,
            &[0x8000, 0x0000, 0x8000, 0xffff], // +0.5, -0.5 (low half first)
        )
        .unwrap();
    assert!(matches!(
        machine.run(128).unwrap(),
        RunOutcome::Halted { .. }
    ));
    assert_eq!(machine.register(3), Some(0x8000));
    assert_eq!(machine.register(6), Some(1));
    assert_eq!(machine.register(8), Some(0));
}

#[test]
fn fpu_out_of_range_register_ranges_fault() {
    // FLDV2 with Fd = 63 needs F63 and F64; F64 does not exist.
    let mut machine = CpuV3Sim::default();
    let [broken0, broken1] = fpu_aux_raw(
        FpuAuxKind::IntegerRegister,
        1,
        0,
        63,
        FpuAuxSubop::Fld as u8,
        1,
    );
    machine
        .load_program(0, &[broken0, broken1, halt()])
        .unwrap();
    assert_eq!(
        machine.step(),
        Err(Fault {
            kind: FaultKind::InvalidInstruction,
            address: 0,
            instruction: broken0,
        })
    );
    assert_eq!(machine.pc(), 0);

    // VADD.4 with Fa = 61 needs F61..F64.
    let [word0, word1] = fpu_vector_raw(
        61,
        0,
        0,
        FpuVectorLength::Vec4 as u8,
        FpuVectorSubop::VAdd as u8,
        0,
    );
    let mut machine = CpuV3Sim::default();
    machine.load_program(0, &[word0, word1, halt()]).unwrap();
    assert_eq!(
        machine.step(),
        Err(Fault {
            kind: FaultKind::InvalidInstruction,
            address: 0,
            instruction: word0,
        })
    );
    assert_eq!(machine.pc(), 0);
}

#[test]
fn fpu_compare_sets_the_existing_pending_test() {
    let mut program = vec![];
    program.extend(load_immediate16(0, 5));
    program.extend(load_immediate16(1, 9));
    program.extend(fpu_aux(
        FpuAuxKind::IntegerRegister,
        0,
        0,
        0,
        FpuAuxSubop::I16tof,
        0,
    )); // f0 = 5.0
    program.extend(fpu_aux(
        FpuAuxKind::IntegerRegister,
        1,
        0,
        1,
        FpuAuxSubop::I16tof,
        0,
    )); // f1 = 9.0
    program.extend(fpu_scalar(0, 1, 0, FpuScalarSubop::Cmp, 0));
    program.push(branch(TestCondition::LessThan, 1));
    program.push(halt());
    program.push(crate::move_register(0, 1));
    program.push(halt());
    let mut machine = CpuV3Sim::default();
    machine.load_program(0, &program).unwrap();
    assert_eq!(
        machine.run(64).unwrap(),
        RunOutcome::Halted {
            steps: 10,
            signal: 9
        }
    );
}

#[test]
fn vector_rounding_slots_fault_without_side_effects() {
    for subop in 0x08..=0x0b {
        for len in 0..=2 {
            let word1 = (16 << 10) | (len << 8) | (subop << 3);
            let mut machine = CpuV3Sim::default();
            machine.load_program(0, &[0xc000, word1, halt()]).unwrap();
            machine.fpu_write(0, -0x18000);
            machine.fpu_write(16, 0x12345678);
            let registers = machine.fpu_registers;
            assert_eq!(
                machine.step(),
                Err(Fault {
                    kind: FaultKind::InvalidInstruction,
                    address: 0,
                    instruction: 0xc000,
                }),
            );
            assert_eq!(machine.pc(), 0);
            assert_eq!(machine.retired_words(), 0);
            assert_eq!(machine.fpu_registers, registers);
        }
    }
}

#[test]
fn fpu_reserved_pairs_fault_and_rewind_to_the_first_word() {
    let mut machine = CpuV3Sim::default();
    // Reserved vector subop 0x10 in word1.
    machine.load_program(0, &[0xc000, 0x0080, halt()]).unwrap();
    assert_eq!(
        machine.step(),
        Err(Fault {
            kind: FaultKind::InvalidInstruction,
            address: 0,
            instruction: 0xc000
        })
    );
    assert_eq!(machine.pc(), 0);
    assert_eq!(machine.retired_words(), 0);

    // Reserved SINCOS mode 11.
    let mut machine = CpuV3Sim::default();
    machine.load_program(0, &[0xd000, 0x00e3, halt()]).unwrap();
    assert_eq!(
        machine.step(),
        Err(Fault {
            kind: FaultKind::InvalidInstruction,
            address: 0,
            instruction: 0xd000
        })
    );
    assert_eq!(machine.pc(), 0);

    // DOT keeps mode[2] reserved even when mode[1:0] names a stride.
    let [dot0, dot1] = fpu_vector_raw(
        0,
        0,
        0,
        FpuVectorLength::Vec2 as u8,
        FpuVectorSubop::Dot as u8,
        0b100,
    );
    let mut machine = CpuV3Sim::default();
    machine.load_program(0, &[dot0, dot1, halt()]).unwrap();
    assert_eq!(
        machine.step(),
        Err(Fault {
            kind: FaultKind::InvalidInstruction,
            address: 0,
            instruction: dot0
        })
    );
    assert_eq!(machine.pc(), 0);

    // A vector range past F63 faults before any register is written.
    let [far0, far1] = fpu_vector_raw(
        61,
        0,
        0,
        FpuVectorLength::Vec4 as u8,
        FpuVectorSubop::VAdd as u8,
        0,
    );
    let mut machine = CpuV3Sim::default();
    machine.load_program(0, &[far0, far1, halt()]).unwrap();
    assert_eq!(
        machine.step(),
        Err(Fault {
            kind: FaultKind::InvalidInstruction,
            address: 0,
            instruction: far0
        })
    );
    assert_eq!(machine.pc(), 0);
}
