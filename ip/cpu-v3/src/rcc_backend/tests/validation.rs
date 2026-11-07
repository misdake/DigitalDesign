use super::*;

#[test]
fn invalid_constant_arguments_are_source_diagnostics() {
    // Invalid device/channel constants are rejected by the frontend with a
    // source diagnostic instead of reaching the lowering asserts.
    for (source, expected) in [
        (
            "const D: u16 = 8; fn main() { dev_recv(D, 0); halt(0); }",
            "device index 8",
        ),
        (
            "const C: u16 = 20; fn main() { dev_recv(0, C); halt(0); }",
            "channel 20",
        ),
        ("fn main() { dev_send(0, 16, 1); halt(0); }", "channel 16"),
    ] {
        let error = parse_source_with(source, 0)
            .err()
            .expect("the invalid constant must be rejected");
        assert!(
            error.to_string().contains(expected),
            "expected {expected:?} in:\n{error}"
        );
    }
}

#[test]
fn fallible_compile_succeeds_for_valid_programs() {
    let options = CompilerOptions::default();
    let program = parse_source_with("fn main() { halt(1); }", options.data_base).unwrap();
    let compiled = super::super::try_compile(program, &options, "main")
        .expect("a well-formed program compiles");
    assert_eq!(compiled.words.last(), Some(&cpu_v3::halt()));
    assert_eq!(
        validate_program(&compiled.words, compiled.code_base),
        Ok(())
    );
}

#[test]
fn backend_error_display_is_a_compiler_message() {
    assert_eq!(
        BackendError::Validation(ProgramValidationError::InvalidInstruction {
            address: 0x10,
            word: 0xe800,
        })
        .to_string(),
        "illegal instruction 0xe800 at 0x0010"
    );
    assert_eq!(
        BackendError::UnknownFunction("ghost".to_string()).to_string(),
        "unknown function `ghost`"
    );
}

#[test]
fn program_validator_accepts_compiled_output() {
    let source = r#"
            fn main() {
                let a: u16 = 0x1234;
                let b = a + 5;
                signal(1, b);
                halt(b);
            }
        "#;
    let program = compile(source, CompilerOptions::default());
    assert_eq!(validate_program(&program.words, program.code_base), Ok(()));
}

#[test]
fn program_validator_rejects_illegal_and_malformed_images() {
    // the reserved revision-0.7 HALT word
    assert_eq!(
        validate_program(&[0xe800], 0),
        Err(ProgramValidationError::InvalidInstruction {
            address: 0,
            word: 0xe800
        })
    );
    // a prefix followed by a non-consumer, and a trailing prefix
    assert_eq!(
        validate_program(&[cpu_v3::prefix12(0), cpu_v3::halt()], 0),
        Err(ProgramValidationError::DanglingPrefix {
            address: 0,
            next: Some(cpu_v3::halt())
        })
    );
    assert_eq!(
        validate_program(&[cpu_v3::prefix12(0)], 0),
        Err(ProgramValidationError::DanglingPrefix {
            address: 0,
            next: None
        })
    );
    // relative consumers must zero payload12[11:8]
    assert_eq!(
        validate_program(
            &[
                cpu_v3::prefix12(0x100),
                cpu_v3::branch(cpu_v3::TestCondition::Equal, 0)
            ],
            0
        ),
        Err(ProgramValidationError::RelativePrefixHighBits {
            address: 0,
            payload: 0x100
        })
    );
    assert_eq!(
        validate_program(
            &[
                cpu_v3::prefix12(0x001),
                cpu_v3::branch(cpu_v3::TestCondition::Equal, 0)
            ],
            0
        ),
        Ok(())
    );
}
