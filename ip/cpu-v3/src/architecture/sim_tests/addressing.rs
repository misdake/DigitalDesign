use super::*;

#[test]
fn executes_loop_and_unified_memory_round_trip() {
    let mut program = vec![];
    program.extend(load_immediate16(0, 0));
    program.extend(load_immediate16(1, 5));
    program.extend(load_immediate16(2, 0x4000));
    program.extend([
        alu(AluOp::Add, 0, 0, 1),
        immediate_unsigned(ImmediateOp::Sub, 1, 1),
        immediate_signed(ImmediateOp::CompareSigned, 1, 0),
        branch(TestCondition::NotEqual, -4),
        store(0, 2, 0),
        load(3, 2, 0),
        halt(),
    ]);
    let mut machine = CpuV3Sim::default();
    machine.load_program(0, &program).unwrap();

    assert_eq!(
        machine.run(100).unwrap(),
        RunOutcome::Halted {
            steps: 29,
            signal: 15
        }
    );
    assert_eq!(machine.register(3), Some(15));
    assert_eq!(machine.memory(0x4000), 15);
    assert_eq!(machine.retired_words(), 29);
}

#[test]
fn line_copy_moves_one_aligned_cache_line_between_segments() {
    let mut program = Vec::new();
    program.extend(load_immediate16(1, 1));
    program.push(write_data_segment(1));
    program.extend(load_immediate16(2, 0x20));
    program.extend(load_immediate16(4, 2));
    program.push(crate::write_data_segment_page(1, 4));
    program.extend(load_immediate16(3, 0x4020));
    program.extend([line_copy(2, 3), halt()]);

    let source: Vec<u16> = (0..16).map(|index| 0x9000 + index).collect();
    let mut machine = CpuV3Sim::default();
    machine.load_program(0, &program).unwrap();
    machine.load_segment(1, 0x20, &source).unwrap();
    assert_eq!(
        machine.run(64).unwrap(),
        RunOutcome::Halted {
            steps: 12,
            signal: 0
        }
    );
    for (index, expected) in source.into_iter().enumerate() {
        assert_eq!(
            machine.physical_memory(PhysicalWordAddress::new(0x8000 + 0x20 + index as u32,)),
            expected
        );
    }
}

#[test]
fn boot_code_establishes_fixed_segments_and_enters_an_application() {
    let mut boot = vec![];
    boot.extend(load_immediate16(1, 1));
    boot.extend(load_immediate16(2, 0x0020));
    boot.extend(load_immediate16(3, 2));
    boot.extend([write_data_segment(3), jump_segment(1, 2)]);

    let mut application = vec![
        read_special(4, SpecialRegister::CodeSegment),
        read_special(5, SpecialRegister::DataSegment),
    ];
    application.extend(load_immediate16(6, 0x1234));
    application.extend([load(0, 6, 0), halt()]);

    let mut machine = CpuV3Sim::default();
    machine.load_program(0, &boot).unwrap();
    machine.load_segment(1, 0x0020, &application).unwrap();
    machine.load_segment(2, 0x1234, &[0xbeef]).unwrap();

    assert_eq!(
        machine.run(32).unwrap(),
        RunOutcome::Halted {
            steps: 14,
            signal: 0xbeef,
        }
    );
    assert_eq!(machine.code_segment(), 1);
    assert_eq!(machine.data_segment(), 2);
    assert_eq!(machine.register(4), Some(1));
    assert_eq!(machine.register(5), Some(2));
}

#[test]
fn fitted_memory_rejects_an_unimplemented_segment() {
    let mut program = vec![];
    program.extend(load_immediate16(1, 1));
    program.extend([write_data_segment(1), load(0, 0, 0)]);

    let mut machine = CpuV3Sim::with_physical_memory_words(1 << 16);
    machine.load_program(0, &program).unwrap();
    assert_eq!(
        machine.run(8),
        Err(Fault {
            kind: FaultKind::PhysicalAddressOutOfRange {
                address: PhysicalWordAddress::new(0x0001_0000),
            },
            address: 3,
            instruction: load(0, 0, 0),
        })
    );
}

#[test]
fn high_offsets_are_ordinary_memory_in_the_selected_data_segment() {
    let mut program = vec![];
    program.extend(load_immediate16(1, 3));
    program.extend(load_immediate16(2, 0xff00));
    program.extend(load_immediate16(3, 0x55aa));
    program.extend([write_data_segment(1), store(3, 2, 0), load(0, 2, 0), halt()]);

    let mut machine = CpuV3Sim::default();
    machine.load_program(0, &program).unwrap();
    machine.attach_device(0, Box::new(EchoDevice { channels: [0; 16] }));
    assert_eq!(
        machine.run(24).unwrap(),
        RunOutcome::Halted {
            steps: 10,
            signal: 0x55aa
        }
    );
    assert_eq!(
        machine.physical_memory(PhysicalWordAddress::new(0x0003_ff00)),
        0x55aa
    );
    assert_eq!(machine.memory(0xff00), 0);
    assert_eq!(machine.device::<EchoDevice>(0).unwrap().channels[0], 0);
}
