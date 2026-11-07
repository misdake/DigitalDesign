use super::*;

#[test]
fn cycle_model_signal_events_are_single_cycle_and_model_only() {
    let mut state = CpuV3CoreState::default();
    state.registers[5] = 0x2a;

    // A nonzero SIGNAL retires as a NOP and records the event.
    state.instruction = cpu_v3::signal(5, 1);
    state.execute(0, true, false, false);
    assert_eq!(state.phase, Phase::FetchRequest);
    assert_eq!(state.retired_words, 1);
    assert_eq!(
        state.signal_event(),
        Some(crate::SignalEvent {
            signal_type: 1,
            value: 0x2a,
        })
    );
    // Reading does not clear the event; take does.
    assert!(state.signal_event().is_some());
    assert!(state.take_signal_event().is_some());
    assert_eq!(state.signal_event(), None);

    // A fresh nonzero SIGNAL re-arms the event; the next executed
    // instruction ends the single-cycle window.
    state.instruction = cpu_v3::signal(5, 15);
    state.execute(0, true, false, false);
    assert_eq!(
        state.signal_event().map(|event| event.signal_type),
        Some(15)
    );
    state.instruction = 0x0322; // ADD r3, r2, r2 (any ordinary instruction)
    state.execute(0, true, false, false);
    assert_eq!(state.signal_event(), None);

    // Type 0 halts and latches the selected register, exactly like the
    // architectural HALT path.
    let mut state = CpuV3CoreState::default();
    state.registers[7] = 0x1234;
    state.instruction = cpu_v3::signal(7, 0);
    state.execute(0, true, false, false);
    assert_eq!(state.phase, Phase::Halted);
    assert_eq!(state.halt_signal, 0x1234);
    assert_eq!(state.retired_words, 1);
    assert_eq!(state.signal_event(), None);
}

#[test]
fn emulator_async_store_overlaps_alu_and_blocks_next_memory_operation() {
    let mut state = CpuV3CoreState::default();
    state.registers[1] = 0x0100;
    state.registers[2] = 10;

    state.instruction = 0x9210; // STORE r2, [r1]
    state.execute(0, true, false, false);
    assert_eq!(state.phase, Phase::FetchRequest);
    assert!(state.async_store.valid);
    assert!(!state.async_store.issued);
    assert_eq!(state.async_store.address, 0x0100);
    assert_eq!(state.async_store.write_data, 10);

    state.instruction = 0x0322; // ADD r3, r2, r2
    state.execute(0, true, false, false);
    assert_eq!(state.phase, Phase::FetchRequest);
    assert!(state.async_store.valid);
    assert!(state.gpr_write_enable);
    assert_eq!(state.gpr_write_data, 20);

    state.instruction = 0x8010; // LOAD r0, [r1]
    state.execute(0, true, false, false);
    assert_eq!(state.phase, Phase::AsyncStoreWait);
    assert!(!state.pending_data.write);
    assert_eq!(state.pending_data.address, 0x0100);
}

#[test]
fn emulator_line_copy_waits_for_prior_stores_and_cache_acceptance() {
    let mut state = CpuV3CoreState::default();
    state.phase = Phase::Execute;
    state.registers[4] = 3;
    state.instruction = cpu_v3::write_data_segment(4);
    state.execute(0, true, false, false);
    state.phase = Phase::Execute;
    state.retired_words = 0;
    state.registers[1] = 0x0120;
    state.registers[2] = 0x8120;
    state.instruction = cpu_v3::line_copy(1, 2);

    state.execute(0, false, false, false);
    assert_eq!(state.phase, Phase::Execute);
    assert_eq!(state.retired_words, 0);

    state.async_store.valid = true;
    state.execute(0, true, false, false);
    assert_eq!(state.phase, Phase::Execute);
    assert_eq!(state.retired_words, 0);

    state.async_store.valid = false;
    state.execute(0, true, false, false);
    assert_eq!(state.phase, Phase::FetchRequest);
    assert_eq!(state.retired_words, 1);
}

#[test]
fn emulator_dual_address_copy_drains_and_preserves_registers() {
    let mut state = CpuV3CoreState::default();
    state.phase = Phase::Execute;
    state.registers[4] = 3;
    state.instruction = cpu_v3::write_data_segment(4);
    state.execute(0, true, false, false);
    state.phase = Phase::Execute;
    state.retired_words = 0;
    state.registers[1] = 0x0120;
    state.registers[2] = 0x4420;
    state.instruction = cpu_v3::line_copy(1, 2);

    state.execute(0, false, false, false);
    assert_eq!(state.phase, Phase::Execute);
    assert_eq!(state.retired_words, 0);

    state.async_store.valid = true;
    state.execute(0, true, false, false);
    assert_eq!(state.phase, Phase::Execute);
    assert_eq!(state.retired_words, 0);

    state.async_store.valid = false;
    state.execute(0, true, false, false);
    assert_eq!(state.phase, Phase::FetchRequest);
    assert_eq!(state.retired_words, 1);
    assert_eq!(state.registers[1], 0x120);
    assert_eq!(state.registers[2], 0x4420);
}

#[test]
fn emulator_matches_oracle_for_compiler_control_memory_and_multiply() {
    let program = compile(
        r#"
                static VALUE: u16 = 7;
                fn twice(value: u16) -> u16 { value + value }
                fn main() {
                    let mut total: u16 = 0;
                    let mut i: u16 = 1;
                    while i < 6 {
                        total = total + twice(i);
                        i = i + 1;
                    }
                    halt(total + VALUE + mul_16x4(3, 4));
                }
            "#,
    );
    let mut oracle = CpuV3Sim::default();
    oracle.load_program(0, &program).unwrap();
    let outcome = oracle.run(10_000).unwrap();
    let RunOutcome::Halted { signal, .. } = outcome else {
        panic!("oracle did not halt")
    };

    let mut memory = HashMap::new();
    load(&mut memory, 0, &program);
    // The compiler's static-data initialization runs from code, so the
    // external memory begins with the same zero-filled state as CpuV3Sim.
    let core = run_core(memory, 20_000);
    assert_eq!(core.halt_signal, signal);
    assert_eq!(core.retired_words as u64, oracle.retired_words());
}

#[test]
fn emulator_matches_segmented_fetch_data_and_special_register_semantics() {
    let mut boot = Vec::new();
    boot.extend(cpu_v3::load_immediate16(1, 1));
    boot.extend(cpu_v3::load_immediate16(2, 0x20));
    boot.extend(cpu_v3::load_immediate16(3, 2));
    boot.extend([cpu_v3::write_data_segment(3), cpu_v3::jump_segment(1, 2)]);
    let mut application = vec![
        cpu_v3::read_special(4, SpecialRegister::CodeSegment),
        cpu_v3::read_special(5, SpecialRegister::DataSegment),
    ];
    application.extend(cpu_v3::load_immediate16(6, 0x1234));
    application.extend([cpu_v3::load(0, 6, 0), cpu_v3::halt()]);

    let mut memory = HashMap::new();
    load(&mut memory, 0, &boot);
    load(&mut memory, 0x0001_0020, &application);
    memory.insert(0x0002_1234, 0xbeef);
    let core = run_core(memory, 1_000);
    assert_eq!(core.halt_signal, 0xbeef);
    assert_eq!((core.code_segment, core.data_segment), (1, 2));
}

#[test]
fn emulator_matches_oracle_for_reserved_prefix_and_comparison_edges() {
    let mut program = Vec::new();
    program.extend(cpu_v3::load_immediate16(1, 0x8000));
    program.extend(cpu_v3::load_immediate16(2, 0x7fff));
    program.extend(cpu_v3::load_immediate16(6, 3));
    program.extend(cpu_v3::load_immediate16(7, 5));
    program.extend([
        cpu_v3::move_register(8, 6),
        cpu_v3::multiply(cpu_v3::MultiplyWindow::Low, 8, 7),
        cpu_v3::move_register(3, 1),
        cpu_v3::set_less_than_signed(3, 2),
        cpu_v3::move_register(4, 1),
        cpu_v3::set_less_than_unsigned(4, 2),
        cpu_v3::population_count(5, 1),
        cpu_v3::shift_immediate(cpu_v3::ShiftOp::RightLogical, 1, 15),
        cpu_v3::alu(AluOp::Add, 0, 3, 4),
        cpu_v3::alu(AluOp::Add, 0, 0, 5),
        cpu_v3::alu(AluOp::Add, 0, 0, 8),
        cpu_v3::immediate_signed(ImmediateOp::CompareSigned, 0, 0),
        cpu_v3::branch(TestCondition::NotEqual, 1),
        cpu_v3::immediate_unsigned(ImmediateOp::LoadUnsigned, 0, 0),
        cpu_v3::halt(),
    ]);
    let mut oracle = CpuV3Sim::default();
    oracle.load_program(0, &program).unwrap();
    let RunOutcome::Halted { signal, .. } = oracle.run(1_000).unwrap() else {
        panic!("oracle did not halt")
    };
    let mut memory = HashMap::new();
    load(&mut memory, 0, &program);
    let core = run_core(memory, 2_000);
    assert_eq!(core.halt_signal, signal);
    assert_eq!(core.retired_words as u64, oracle.retired_words());
}

#[test]
fn emulator_matches_oracle_for_dedicated_device_instructions() {
    let mut program = Vec::new();
    program.extend(cpu_v3::load_immediate16(1, 0x1234));
    program.extend([
        cpu_v3::device_send(1, 2, 3),
        cpu_v3::device_receive(0, 2, 3),
        cpu_v3::halt(),
    ]);
    let mut oracle = CpuV3Sim::default();
    oracle.load_program(0, &program).unwrap();
    struct EchoDevice([u16; 16]);
    impl cpu_v3::Device for EchoDevice {
        fn read(&mut self, _memory: &mut [u16], channel: u8) -> u16 {
            self.0[usize::from(channel)]
        }
        fn write(&mut self, _memory: &mut [u16], channel: u8, value: u16) {
            self.0[usize::from(channel)] = value;
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }
    oracle.attach_device(2, Box::new(EchoDevice([0; 16])));
    let RunOutcome::Halted { signal, .. } = oracle.run(1_000).unwrap() else {
        panic!("oracle did not halt")
    };
    assert_eq!(signal, 0x1234);
    let mut memory = HashMap::new();
    load(&mut memory, 0, &program);
    let core = run_core(memory, 1_000);
    assert_eq!(core.halt_signal, signal);
    assert_eq!(core.retired_words as u64, oracle.retired_words());
}
