use super::*;

#[test]
fn line_copy_maps_both_pointers_without_modifying_registers() {
    let mut program = Vec::new();
    program.extend(load_immediate16(2, 0x120));
    program.extend(load_immediate16(3, 0x4420));
    program.extend([crate::line_copy(2, 3), crate::wait_data_cache(), halt()]);
    let words: Vec<u16> = (0..16).map(|i| 0xab00 + i).collect();
    let mut machine = CpuV3Sim::default();
    machine.load_program(0, &program).unwrap();
    machine.load_segment(0, 0x120, &words).unwrap();
    assert!(matches!(
        machine.run(32).unwrap(),
        RunOutcome::Halted { .. }
    ));
    assert_eq!(machine.register(2), Some(0x120));
    assert_eq!(machine.register(3), Some(0x4420));
    for (i, word) in words.into_iter().enumerate() {
        assert_eq!(
            machine.physical_memory(PhysicalWordAddress::new(0x4420 + i as u32)),
            word
        );
        assert_eq!(
            machine.physical_memory(PhysicalWordAddress::new(0x120 + i as u32)),
            word
        );
    }
}

#[test]
fn revision_three_comparisons_cover_overflow_edges() {
    let mut program = vec![];
    program.extend(load_immediate16(1, 0x8000));
    program.extend(load_immediate16(2, 0x7fff));
    program.extend([
        crate::move_register(3, 1),
        set_less_than_signed(3, 2),
        crate::move_register(4, 1),
        set_less_than_unsigned(4, 2),
        population_count(5, 1),
        halt(),
    ]);
    let mut machine = CpuV3Sim::default();
    machine.load_program(0, &program).unwrap();
    machine.run(20).unwrap();

    assert_eq!(machine.register(3), Some(1));
    assert_eq!(machine.register(4), Some(0));
    assert_eq!(machine.register(5), Some(1));
}

#[test]
fn four_data_pages_translate_each_logical_quarter_independently() {
    let mut program = vec![];
    for (register, page) in [(1, 8), (2, 9), (3, 10), (4, 11)] {
        program.extend(load_immediate16(register, page));
        program.push(crate::write_data_segment_page(register - 1, register));
    }
    for (base, offset) in [(5, 0x0010), (6, 0x4010), (7, 0x8010), (8, 0xc010)] {
        program.extend(load_immediate16(base, offset));
    }
    program.extend(load_immediate16(9, 0x5aa5));
    program.extend([
        store(9, 5, 0),
        store(9, 6, 0),
        store(9, 7, 0),
        store(9, 8, 0),
        halt(),
    ]);

    let mut machine = CpuV3Sim::default();
    machine.load_program(0, &program).unwrap();
    machine.run(64).unwrap();

    assert_eq!(machine.data_segments(), [8, 9, 10, 11]);
    for address in [0x0002_0010, 0x0002_4010, 0x0002_8010, 0x0002_c010] {
        assert_eq!(
            machine.physical_memory(PhysicalWordAddress::new(address)),
            0x5aa5
        );
    }
}

#[test]
fn compare_instructions_respect_signedness_at_the_sign_boundary() {
    // r1 = 0x7fff (signed 32767), r2 = 0x8000 (signed -32768, unsigned 32768).
    let mut program = vec![];
    program.extend(load_immediate16(1, 0x7fff));
    program.extend(load_immediate16(2, 0x8000));
    // r3: signed register compare -> Greater.
    program.extend([
        immediate_unsigned(ImmediateOp::LoadUnsigned, 3, 1),
        compare_signed(1, 2),
        branch(TestCondition::GreaterThan, 1),
        immediate_unsigned(ImmediateOp::LoadUnsigned, 3, 0),
    ]);
    // r4: unsigned register compare -> Less.
    program.extend([
        immediate_unsigned(ImmediateOp::LoadUnsigned, 4, 1),
        compare_unsigned(1, 2),
        branch(TestCondition::LessThan, 1),
        immediate_unsigned(ImmediateOp::LoadUnsigned, 4, 0),
    ]);
    // r5: signed immediate compare against the 16-bit pattern 0x8000
    // (i16 -32768) -> Greater.
    program.push(immediate_unsigned(ImmediateOp::LoadUnsigned, 5, 1));
    program.extend(prefixed(
        0xa000 | ((ImmediateOp::CompareSigned as Word) << 8) | 0x10,
        0x8000,
    ));
    program.extend([
        branch(TestCondition::GreaterThan, 1),
        immediate_unsigned(ImmediateOp::LoadUnsigned, 5, 0),
    ]);
    // r6: unsigned immediate compare against 0x8000 -> Less.
    program.push(immediate_unsigned(ImmediateOp::LoadUnsigned, 6, 1));
    program.extend(prefixed(
        0xa000 | ((ImmediateOp::CompareUnsigned as Word) << 8) | 0x10,
        0x8000,
    ));
    program.extend([
        branch(TestCondition::LessThan, 1),
        immediate_unsigned(ImmediateOp::LoadUnsigned, 6, 0),
    ]);
    // r7/r8: unprefixed immediate compares sign-extend the nibble.
    program.extend([immediate_signed(ImmediateOp::LoadSigned, 1, -1)]);
    program.extend([
        immediate_unsigned(ImmediateOp::LoadUnsigned, 7, 1),
        immediate_signed(ImmediateOp::CompareSigned, 1, 0),
        branch(TestCondition::LessThan, 1),
        immediate_unsigned(ImmediateOp::LoadUnsigned, 7, 0),
    ]);
    program.extend([
        immediate_unsigned(ImmediateOp::LoadUnsigned, 8, 1),
        immediate_unsigned(ImmediateOp::CompareUnsigned, 1, 0),
        branch(TestCondition::GreaterThan, 1),
        immediate_unsigned(ImmediateOp::LoadUnsigned, 8, 0),
    ]);
    program.push(halt());

    let mut machine = CpuV3Sim::default();
    machine.load_program(0, &program).unwrap();
    assert!(matches!(
        machine.run(100).unwrap(),
        RunOutcome::Halted { .. }
    ));
    for (register, name) in [
        (3, "CMPS"),
        (4, "CMPU"),
        (5, "CMPSI wide"),
        (6, "CMPUI wide"),
        (7, "CMPSI"),
        (8, "CMPUI"),
    ] {
        assert_eq!(machine.register(register), Some(1), "{name}");
    }
    // CMP-class instructions write no register: r1/r2 keep their values.
    assert_eq!(machine.register(2), Some(0x8000));
}

#[test]
fn shift_amounts_are_masked_to_four_bits() {
    // Register-count shifts use rs & 15: amounts 0/15/16/31 exercise the
    // mask boundary, and the high 12 bits of rs are ignored entirely.
    let mut program = vec![];
    for (slot, amount) in [0u16, 15, 16, 31].into_iter().enumerate() {
        program.extend(load_immediate16(2, amount | 0x8000));
        program.extend(load_immediate16(3, 0x0003));
        program.extend([
            shift_register(ShiftOp::Left, 3, 2),
            move_register((4 + slot) as u8, 3),
        ]);
    }
    program.push(halt());
    let mut machine = CpuV3Sim::default();
    machine.load_program(0, &program).unwrap();
    machine.run(64).unwrap();
    // 3 << 0 == 3, 3 << 15 == 0x8000; 16 and 31 mask back to 0 and 15.
    for (slot, expected) in [0x0003u16, 0x8000, 0x0003, 0x8000].into_iter().enumerate() {
        assert_eq!(
            machine.register((4 + slot) as u8),
            Some(expected),
            "slot {slot}"
        );
    }
}

#[test]
fn destructive_read_modify_write_uses_the_old_value() {
    // rd == rs: the destructive form reads the old rd value before
    // writing the result back into the same register.
    let mut program = vec![];
    program.extend(load_immediate16(1, 0x00ff));
    program.extend([
        shift_register(ShiftOp::Left, 1, 1), // 0xff << 15 = 0x8000
        multiply(MultiplyWindow::Low, 1, 1), // 0x8000^2 low = 0
    ]);
    program.extend(load_immediate16(2, 0x00ff));
    program.extend([
        // 0xff * 0xff = 0xfe01: MUL8 keeps [23:8] = 0xfe, MUL16 keeps 0.
        multiply(MultiplyWindow::Shift8, 2, 2),
    ]);
    program.extend(load_immediate16(3, 0xffff));
    program.extend([
        // 0xffff * 0xffff = 0xfffe0001: MUL16 keeps 0xfffe.
        multiply(MultiplyWindow::Shift16, 3, 3),
    ]);
    program.extend([move_register(0, 1), halt()]);
    let mut machine = CpuV3Sim::default();
    machine.load_program(0, &program).unwrap();
    machine.run(32).unwrap();
    assert_eq!(machine.register(1), Some(0));
    assert_eq!(machine.register(2), Some(0x00fe));
    assert_eq!(machine.register(3), Some(0xfffe));
}

#[test]
fn multiply_windows_select_product_bytes() {
    let mut program = vec![];
    program.extend(load_immediate16(1, 0xffff));
    program.extend(load_immediate16(2, 0xffff));
    // 0xffff * 0xffff = 0xfffe0001.
    program.extend([multiply(MultiplyWindow::Low, 2, 1)]); // [15:0]
    program.extend(load_immediate16(3, 0xffff));
    program.extend([multiply(MultiplyWindow::Shift8, 3, 1)]); // [23:8]
    program.extend(load_immediate16(4, 0xffff));
    program.extend([multiply(MultiplyWindow::Shift16, 4, 1)]); // [31:16]
    program.push(halt());
    let mut machine = CpuV3Sim::default();
    machine.load_program(0, &program).unwrap();
    machine.run(32).unwrap();
    assert_eq!(machine.register(2), Some(0x0001));
    assert_eq!(machine.register(3), Some(0xfe00));
    assert_eq!(machine.register(4), Some(0xfffe));
}

#[test]
fn muli_immediate_is_an_unsigned_bit_pattern() {
    // Short form: 15 is fifteen, not minus one.
    let mut program = vec![];
    program.extend(load_immediate16(1, 2));
    program.push(multiply_immediate(1, 15));
    // Wide form: the prefixed pattern 0x8000 is unsigned 32768.
    program.extend(load_immediate16(2, 2));
    program.extend(prefixed(multiply_immediate(2, 0), 0x8000));
    program.push(halt());
    let mut machine = CpuV3Sim::default();
    machine.load_program(0, &program).unwrap();
    machine.run(16).unwrap();
    assert_eq!(machine.register(1), Some(30));
    assert_eq!(machine.register(2), Some(0));
}

#[test]
fn immediate_shifts_cover_the_extreme_amounts() {
    let mut program = vec![];
    program.extend(load_immediate16(1, 0x8001));
    program.extend([
        shift_immediate(ShiftOp::Left, 1, 0),             // unchanged
        shift_immediate(ShiftOp::RightLogical, 1, 15),    // 0x8001 >> 15 = 1
        shift_immediate(ShiftOp::Left, 1, 15),            // 1 << 15 = 0x8000
        shift_immediate(ShiftOp::RightArithmetic, 1, 15), // sign fills: 0xffff
        move_register(0, 1),
        halt(),
    ]);
    let mut machine = CpuV3Sim::default();
    machine.load_program(0, &program).unwrap();
    assert_eq!(
        machine.run(16).unwrap(),
        RunOutcome::Halted {
            steps: 8,
            signal: 0xffff
        }
    );
}
