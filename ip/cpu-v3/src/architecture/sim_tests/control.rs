use super::*;

#[test]
fn prefix_and_consumer_retire_as_one_wide_operation() {
    let mut machine = CpuV3Sim::default();
    machine.load_program(0, &[0xf400, 0x8100, halt()]).unwrap();
    machine.load_program(0x4000, &[0xbeef]).unwrap();
    assert_eq!(machine.step(), Ok(StepOutcome::Running));
    assert_eq!(machine.retired_words(), 0);
    assert_eq!(machine.step(), Ok(StepOutcome::Running));
    assert_eq!(machine.register(1), Some(0xbeef));
    assert_eq!(machine.retired_words(), 2);
}

#[test]
fn non_consumer_retires_and_expires_the_prefix() {
    let mut machine = CpuV3Sim::default();
    machine
        .load_program(
            0,
            &[
                0xfabc,
                move_register(1, 1),
                immediate_unsigned(ImmediateOp::LoadUnsigned, 3, 0xd),
                halt(),
            ],
        )
        .unwrap();
    assert_eq!(
        machine.run(10).unwrap(),
        RunOutcome::Halted {
            steps: 4,
            signal: 0
        }
    );
    assert_eq!(machine.register(3), Some(13));
    assert_eq!(machine.retired_words(), 4);
}

#[test]
fn constant_table_ops_cover_signed_entries_and_expire_the_prefix() {
    let mut machine = CpuV3Sim::default();
    machine
        .load_program(
            0,
            &[
                crate::load_constant(1, 0),  // r1 = 8
                crate::load_constant(2, 7),  // r2 = 512
                crate::load_constant(3, 9),  // r3 = -16 (0xfff0)
                crate::load_constant(4, 15), // r4 = -512 (0xfe00)
                crate::add_constant(1, 4),   // r1 = 8 + 64 = 72
                crate::add_constant(3, 15),  // r3 = -16 + -512 = -528 (0xfdf0)
                // A pending prefix expires unused before the non-consuming
                // ADDC and retires separately.
                prefix12(0xabc),
                crate::add_constant(1, 12), // r1 = 72 + -64 = 8
                halt(),
            ],
        )
        .unwrap();
    assert_eq!(
        machine.run(20).unwrap(),
        RunOutcome::Halted {
            steps: 9,
            signal: 0
        }
    );
    assert_eq!(machine.register(1), Some(8));
    assert_eq!(machine.register(2), Some(512));
    assert_eq!(machine.register(3), Some(0xfdf0));
    assert_eq!(machine.register(4), Some(0xfe00));
    assert_eq!(machine.retired_words(), 9);
}

#[test]
fn addi_subi_read_the_unprefixed_immediate_as_unsigned() {
    let mut machine = CpuV3Sim::default();
    machine
        .load_program(
            0,
            &[
                immediate_unsigned(ImmediateOp::Add, 1, 15), // r1 = 15
                immediate_unsigned(ImmediateOp::Add, 1, 0),  // r1 = 15
                immediate_unsigned(ImmediateOp::Sub, 1, 15), // r1 = 0
                immediate_unsigned(ImmediateOp::Sub, 1, 0),  // r1 = 0
                // The high nibbles are additions, not negative offsets:
                // the old signed reading would have made this r1 = -1.
                immediate_unsigned(ImmediateOp::Add, 1, 8), // r1 = 8
                halt(),
            ],
        )
        .unwrap();
    assert_eq!(
        machine.run(10).unwrap(),
        RunOutcome::Halted {
            steps: 6,
            signal: 0
        }
    );
    assert_eq!(machine.register(1), Some(8));
}

#[test]
fn prefixed_addi_subi_use_the_full_16_bit_pattern() {
    let mut machine = CpuV3Sim::default();
    let [p0, add] = prefixed(immediate_unsigned(ImmediateOp::Add, 1, 0), 0x0010);
    let [p1, sub] = prefixed(immediate_unsigned(ImmediateOp::Sub, 1, 0), 0x0008);
    machine
        .load_program(0, &[p0, add, p1, sub, halt()])
        .unwrap();
    assert_eq!(
        machine.run(10).unwrap(),
        RunOutcome::Halted {
            steps: 5,
            signal: 0
        }
    );
    assert_eq!(machine.register(1), Some(8));
}

#[test]
fn all_six_predicates_test_the_pending_ordering() {
    for (condition, taken) in [
        (TestCondition::Equal, false),
        (TestCondition::NotEqual, true),
        (TestCondition::LessThan, true),
        (TestCondition::GreaterOrEqual, false),
        (TestCondition::GreaterThan, false),
        (TestCondition::LessOrEqual, true),
    ] {
        // r1 = 3, r2 = 5: the signed pending ordering is Less.
        let mut program = vec![];
        program.extend(load_immediate16(1, 3));
        program.extend(load_immediate16(2, 5));
        program.extend([
            compare_signed(1, 2),
            branch(condition, 1),
            immediate_unsigned(ImmediateOp::LoadUnsigned, 0, 9),
            halt(),
        ]);
        let mut machine = CpuV3Sim::default();
        machine.load_program(0, &program).unwrap();
        let expected = if taken { 0 } else { 9 };
        assert_eq!(
            machine.run(16).unwrap(),
            RunOutcome::Halted {
                steps: if taken { 7 } else { 8 },
                signal: expected,
            },
            "condition {condition:?}"
        );
    }
}

#[test]
fn branch_offset_is_a_signed_byte_relative_to_the_next_word() {
    assert_eq!(branch(TestCondition::Equal, -128), 0xb080);
    assert_eq!(branch(TestCondition::Equal, 127), 0xb07f);

    // r1 = 0 -> pending Equal; the taken branch skips two words.
    let mut program = vec![];
    program.extend(load_immediate16(1, 0));
    program.extend([
        compare_signed(1, 1),
        branch(TestCondition::Equal, 2),
        immediate_unsigned(ImmediateOp::LoadUnsigned, 0, 1),
        immediate_unsigned(ImmediateOp::LoadUnsigned, 0, 2),
        halt(),
    ]);
    let mut machine = CpuV3Sim::default();
    machine.load_program(0, &program).unwrap();
    assert_eq!(
        machine.run(16).unwrap(),
        RunOutcome::Halted {
            steps: 5,
            signal: 0,
        }
    );
}

#[test]
fn prefixed_branch_forms_a_wide_offset_and_retires_two_words() {
    let mut program = vec![immediate_signed(ImmediateOp::CompareSigned, 0, 0)];
    program.extend(prefixed_branch(branch(TestCondition::Equal, 0), 0x0103));
    assert_eq!(program[1], prefix12(0x01));
    assert_eq!(program[2], 0xb003);
    let filler = immediate_unsigned(ImmediateOp::LoadUnsigned, 1, 1);
    program.extend(std::iter::repeat_n(filler, 0x103));
    program.extend([immediate_unsigned(ImmediateOp::LoadUnsigned, 1, 2), halt()]);

    let mut machine = CpuV3Sim::default();
    machine.load_program(0, &program).unwrap();
    assert_eq!(
        machine.run(0x200).unwrap(),
        RunOutcome::Halted {
            steps: 5,
            signal: 0,
        }
    );
    // The branch target executed exactly once; the filler did not run.
    assert_eq!(machine.register(1), Some(2));
    assert_eq!(machine.retired_words(), 5);
}

#[test]
fn conditional_branch_without_a_pending_test_faults() {
    let mut machine = CpuV3Sim::default();
    machine
        .load_program(0, &[branch(TestCondition::Equal, 0)])
        .unwrap();
    assert_eq!(
        machine.step(),
        Err(Fault {
            kind: FaultKind::InvalidInstruction,
            address: 0,
            instruction: 0xb000,
        })
    );
}

#[test]
fn stale_branch_fault_reports_the_prefix_address_and_retires_nothing() {
    let mut machine = CpuV3Sim::default();
    let words = prefixed_branch(branch(TestCondition::NotEqual, 0), 0);
    machine.load_program(0, &words).unwrap();
    assert_eq!(machine.step(), Ok(StepOutcome::Running));
    assert_eq!(
        machine.step(),
        Err(Fault {
            kind: FaultKind::InvalidInstruction,
            address: 0,
            instruction: words[1],
        })
    );
    assert_eq!(machine.retired_words(), 0);
}

#[test]
fn reserved_encodings_fault() {
    // Majors C/D/E are now FPU v2 first words, so only the non-canonical
    // JREG/JALR link fields remain invalid single words.
    for word in [0xbe10, 0xbf00, 0x2700, 0x2712, 0x27ff] {
        let mut machine = CpuV3Sim::default();
        machine.load_program(0, &[word]).unwrap();
        assert_eq!(
            machine.step(),
            Err(Fault {
                kind: FaultKind::InvalidInstruction,
                address: 0,
                instruction: word,
            }),
            "word {word:#06x}"
        );
    }
}

#[test]
fn conditional_move_consumes_the_pending_test() {
    // r1 = 3, r2 = 5: signed pending ordering is Less; MOVLT writes,
    // MOVGE does not but still consumes the test.
    let mut program = vec![];
    program.extend(load_immediate16(1, 3));
    program.extend(load_immediate16(2, 5));
    program.extend(load_immediate16(3, 0));
    program.extend([
        compare_signed(1, 2),
        conditional_move(TestCondition::LessThan, 3, 2),
        compare_signed(1, 2),
        conditional_move(TestCondition::GreaterOrEqual, 3, 1),
        move_register(0, 3),
        halt(),
    ]);
    let mut machine = CpuV3Sim::default();
    machine.load_program(0, &program).unwrap();
    assert_eq!(
        machine.run(32).unwrap(),
        RunOutcome::Halted {
            steps: 12,
            signal: 5
        }
    );

    let mut machine = CpuV3Sim::default();
    machine
        .load_program(0, &[conditional_move(TestCondition::Equal, 1, 2)])
        .unwrap();
    assert_eq!(
        machine.step(),
        Err(Fault {
            kind: FaultKind::InvalidInstruction,
            address: 0,
            instruction: conditional_move(TestCondition::Equal, 1, 2),
        })
    );
}

#[test]
fn prefixes_are_transparent_to_the_pending_test() {
    // CMP; PFX12; BR: the prefix sits between producer and consumer.
    let mut machine = CpuV3Sim::default();
    machine
        .load_program(
            0,
            &[
                immediate_signed(ImmediateOp::CompareSigned, 0, 0),
                prefix12(0),
                branch(TestCondition::Equal, 1),
                immediate_unsigned(ImmediateOp::LoadUnsigned, 0, 9),
                halt(),
            ],
        )
        .unwrap();
    assert_eq!(
        machine.run(8).unwrap(),
        RunOutcome::Halted {
            steps: 4,
            signal: 0,
        }
    );
    assert_eq!(machine.retired_words(), 4);
}

#[test]
fn unconditional_jumps_expire_the_pending_test() {
    let mut machine = CpuV3Sim::default();
    machine
        .load_program(
            0,
            &[
                immediate_signed(ImmediateOp::CompareSigned, 0, 0),
                jump_relative(2),
                nop(),
                nop(),
                branch(TestCondition::Equal, 0),
            ],
        )
        .unwrap();
    assert_eq!(machine.step(), Ok(StepOutcome::Running));
    assert_eq!(machine.step(), Ok(StepOutcome::Running));
    assert_eq!(machine.pc(), 4);
    assert_eq!(
        machine.step(),
        Err(Fault {
            kind: FaultKind::InvalidInstruction,
            address: 4,
            instruction: 0xb000,
        })
    );
}

#[test]
fn jump_and_link_relative_links_the_fall_through_address() {
    let mut machine = CpuV3Sim::default();
    machine
        .load_program(0, &[jump_and_link_relative(2), nop(), nop(), halt()])
        .unwrap();
    assert_eq!(
        machine.run(4).unwrap(),
        RunOutcome::Halted {
            steps: 2,
            signal: 0,
        }
    );
    assert_eq!(machine.register(LINK_REGISTER), Some(1));
    assert_eq!(machine.retired_words(), 2);
}

#[test]
fn jump_and_link_register_requires_the_fixed_link_register() {
    let mut machine = CpuV3Sim::default();
    machine.load_program(0, &[0xbf51]).unwrap();
    assert_eq!(
        machine.step(),
        Err(Fault {
            kind: FaultKind::InvalidInstruction,
            address: 0,
            instruction: 0xbf51,
        })
    );

    let mut machine = CpuV3Sim::default();
    machine
        .load_program(
            0,
            &[
                immediate_unsigned(ImmediateOp::LoadUnsigned, 2, 3),
                jump_and_link_register(2),
                halt(),
                halt(),
            ],
        )
        .unwrap();
    assert_eq!(
        machine.run(4).unwrap(),
        RunOutcome::Halted {
            steps: 3,
            signal: 0,
        }
    );
    assert_eq!(machine.register(LINK_REGISTER), Some(2));
}

#[test]
fn all_six_conditional_moves_test_the_pending_ordering() {
    for (condition, writes) in [
        (TestCondition::Equal, false),
        (TestCondition::NotEqual, true),
        (TestCondition::LessThan, true),
        (TestCondition::GreaterOrEqual, false),
        (TestCondition::GreaterThan, false),
        (TestCondition::LessOrEqual, true),
    ] {
        // r1 = 3, r2 = 5: the signed pending ordering is Less.
        let mut program = vec![];
        program.extend(load_immediate16(1, 3));
        program.extend(load_immediate16(2, 5));
        program.extend(load_immediate16(3, 0));
        program.extend([
            compare_signed(1, 2),
            conditional_move(condition, 3, 2),
            move_register(0, 3),
            halt(),
        ]);
        let mut machine = CpuV3Sim::default();
        machine.load_program(0, &program).unwrap();
        let expected = if writes { 5 } else { 0 };
        assert_eq!(
            machine.run(16).unwrap(),
            RunOutcome::Halted {
                steps: 10,
                signal: expected,
            },
            "condition {condition:?}"
        );
    }
}

#[test]
fn conditional_move_consumes_pending_even_when_not_taken() {
    let mut program = vec![];
    program.extend(load_immediate16(1, 3));
    program.extend(load_immediate16(2, 5));
    program.extend([
        compare_signed(1, 2),
        // Not taken (the ordering is Less), but the pending test is gone.
        conditional_move(TestCondition::GreaterOrEqual, 1, 2),
        branch(TestCondition::Equal, 0),
    ]);
    let mut machine = CpuV3Sim::default();
    machine.load_program(0, &program).unwrap();
    assert_eq!(
        machine.run(16),
        Err(Fault {
            kind: FaultKind::InvalidInstruction,
            address: 6,
            instruction: branch(TestCondition::Equal, 0),
        })
    );
}

#[test]
fn jalr_and_jalrel_link_r14_and_jreg_returns() {
    // JALR path: call the subroutine at offset 8, which returns via
    // JREG r14 with the call's fall-through address in r0.
    let mut program = vec![];
    program.extend(load_immediate16(2, 8));
    program.push(jump_and_link_register(2));
    program.push(halt()); // returns here, halting with r0
    program.extend([nop(); 4]);
    program.extend([
        move_register(0, LINK_REGISTER),
        jump_register(LINK_REGISTER),
    ]);
    let mut machine = CpuV3Sim::default();
    machine.load_program(0, &program).unwrap();
    assert_eq!(
        machine.run(16).unwrap(),
        RunOutcome::Halted {
            steps: 6,
            signal: 3
        }
    );
    assert_eq!(machine.register(LINK_REGISTER), Some(3));

    // JALREL path: same flow through a relative call.
    let program = [
        jump_and_link_relative(2),
        move_register(0, LINK_REGISTER),
        halt(),
        jump_register(LINK_REGISTER),
    ];
    let mut machine = CpuV3Sim::default();
    machine.load_program(0, &program).unwrap();
    assert_eq!(
        machine.run(8).unwrap(),
        RunOutcome::Halted {
            steps: 4,
            signal: 1
        }
    );
}
