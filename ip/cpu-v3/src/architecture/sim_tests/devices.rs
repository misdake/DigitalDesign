use super::*;

#[test]
fn nonzero_signal_types_retire_as_events() {
    let mut program = vec![];
    program.extend(load_immediate16(1, 0x77));
    program.extend([signal(1, 1), signal(1, 15), move_register(0, 1), halt()]);
    let mut machine = CpuV3Sim::default();
    machine.load_program(0, &program).unwrap();
    // Each nonzero SIGNAL surfaces exactly once at its retirement edge and
    // execution continues; step() observes the events, run() ignores them.
    assert_eq!(machine.step(), Ok(StepOutcome::Running));
    assert_eq!(machine.step(), Ok(StepOutcome::Running));
    assert_eq!(
        machine.step(),
        Ok(StepOutcome::Signaled(SignalEvent {
            signal_type: 1,
            value: 0x77
        }))
    );
    assert_eq!(machine.retired_words(), 3);
    assert_eq!(
        machine.step(),
        Ok(StepOutcome::Signaled(SignalEvent {
            signal_type: 15,
            value: 0x77
        }))
    );

    let mut machine = CpuV3Sim::default();
    machine.load_program(0, &program).unwrap();
    assert_eq!(
        machine.run(16).unwrap(),
        RunOutcome::Halted {
            steps: 6,
            signal: 0x77
        }
    );
    assert_eq!(machine.retired_words(), 6);
}

#[test]
fn signal_type_zero_latches_the_selected_register() {
    // HALT is SIGNAL r0, 0; a nonzero source register halts with its value.
    let mut program = vec![];
    program.extend(load_immediate16(1, 0x1234));
    program.extend([signal(1, 0)]);
    let mut machine = CpuV3Sim::default();
    machine.load_program(0, &program).unwrap();
    assert_eq!(
        machine.run(8).unwrap(),
        RunOutcome::Halted {
            steps: 3,
            signal: 0x1234
        }
    );
    // After the halt the same latched signal is re-reported.
    assert_eq!(machine.step(), Ok(StepOutcome::Halted { signal: 0x1234 }));
}

#[test]
fn device_instructions_route_to_an_attached_device() {
    let mut machine = CpuV3Sim::default();
    machine
        .load_program(0, &device_round_trip_program())
        .unwrap();
    machine.attach_device(2, Box::new(EchoDevice { channels: [0; 16] }));
    assert_eq!(
        machine.run(16).unwrap(),
        RunOutcome::Halted {
            steps: 5,
            signal: 0x1234,
        }
    );
    let device: &EchoDevice = machine.device(2).unwrap();
    assert_eq!(device.channels[3], 0x1234);
    // Device traffic never aliases an ordinary physical memory word.
    assert_eq!(machine.memory(0xff23), 0);
}

#[test]
fn unconnected_device_reads_zero_and_writes_are_ignored() {
    let mut machine = CpuV3Sim::default();
    machine
        .load_program(0, &device_round_trip_program())
        .unwrap();
    assert_eq!(
        machine.run(16).unwrap(),
        RunOutcome::Halted {
            steps: 5,
            signal: 0,
        }
    );
    assert_eq!(machine.memory(0xff23), 0);
}
