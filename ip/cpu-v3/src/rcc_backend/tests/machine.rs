use super::*;

#[test]
fn device_intrinsics_use_the_dedicated_device_path() {
    let source = r#"
            const ECHO_DEVICE: u16 = 1 + 1;
            const ECHO_CHANNEL: u16 = 1 + 2;
            fn main() {
                dev_send(ECHO_DEVICE, ECHO_CHANNEL, 0x1234);
                halt(dev_recv(ECHO_DEVICE, ECHO_CHANNEL));
            }
        "#;
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
    let program = compile(source, CompilerOptions::default());
    let mut machine = cpu_v3::CpuV3Sim::default();
    machine.load_program(0, &program.words).unwrap();
    machine.attach_device(2, Box::new(EchoDevice([0; 16])));
    let signal = match machine.run(10_000).unwrap() {
        cpu_v3::RunOutcome::Halted { signal, .. } => signal,
        outcome => panic!("CpuV3 program did not halt: {outcome:?}"),
    };
    assert_eq!(signal, 0x1234);
    assert_eq!(machine.memory(0xff23), 0);
    assert_eq!(machine.device::<EchoDevice>(2).unwrap().0[3], 0x1234);
}

#[test]
fn device_indices_above_seven_are_rejected() {
    let result = std::panic::catch_unwind(|| {
        compile(
                "const INVALID_DEVICE: u16 = 8;\nconst TEST_CHANNEL: u16 = 0;\nfn main() { dev_send(INVALID_DEVICE, TEST_CHANNEL, 0); halt(0); }",
                CompilerOptions::default(),
            )
    });
    let error = result.expect_err("CpuV3 v0.6 supports only eight devices");
    let message = error
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| error.downcast_ref::<&str>().copied())
        .unwrap_or("");
    assert!(message.contains("device index 8"), "{message}");
}

#[test]
fn halt_and_signal_use_the_signal_encoding() {
    let source = r#"
            fn main() {
                signal(1, 5);
                signal(15, 6);
                halt(7);
            }
        "#;
    let program = compile(source, CompilerOptions::default());
    let signals = mnemonics(&program)
        .iter()
        .filter(|m| m.starts_with("signal"))
        .count();
    assert_eq!(signals, 2, "{}", program.listing);
    assert!(has_prefix(&program, "halt"), "{}", program.listing);
    assert_eq!(run(source), 7);
}

#[test]
fn special_register_reads_lower_to_mfsr() {
    let source = r#"
            fn main() {
                let c = read_cseg();
                let d = read_dseg();
                signal(1, c + d);
                halt(c + d);
            }
        "#;
    let program = compile(source, CompilerOptions::default());
    assert_eq!(
        mnemonics(&program)
            .iter()
            .filter(|m| m.starts_with("mfsr"))
            .count(),
        2,
        "{}",
        program.listing
    );
    assert_eq!(run(source), 0);
}
