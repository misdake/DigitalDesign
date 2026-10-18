//! Single-application cold entry through the independent architectural model.
use cpu_v3::{
    rcc_backend::{self, CompilerOptions},
    RunOutcome,
};
use cpu_v3_tang_nano_20k::{
    boot::*,
    system_sim::{CpuV3SystemSim, VBlankMode},
};
fn fixture() -> Vec<u8> {
    let source = include_str!("../rcc/boot-contract.rs");
    let mut options = CompilerOptions::for_separate_code_and_data_segments(0);
    options.stack_init = SINGLE_BOOT_STACK;
    let parsed =
        rcc::frontend::compile_program_named("boot-contract.rs", source, &options, &mut |name| {
            Err(format!("unexpected module {name}"))
        })
        .unwrap();
    let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
    let bytes: Vec<u8> = program.words.iter().flat_map(|x| x.to_le_bytes()).collect();
    pack_single_application(&bytes).unwrap()
}
#[test]
fn cold_flash_clears_only_statics_and_executes_nonzero_initializers() {
    let package = fixture();
    let mut system =
        CpuV3SystemSim::from_single_flash(&package, 0xd00d, VBlankMode::Manual).unwrap();
    assert_eq!(system.cpu().code_segment(), 7);
    assert_eq!(system.cpu().data_segments(), [0, 1, 2, 3]);
    assert_eq!(system.cpu().retired_words(), 0);
    assert!(
        system.cpu_mut().physical_memory_mut()[..SINGLE_BOOT_STATIC_WORDS]
            .iter()
            .all(|x| *x == 0)
    );
    assert_eq!(
        system.cpu_mut().physical_memory_mut()[SINGLE_BOOT_STATIC_WORDS],
        0xd00d
    );
    let result = system.run(2000).unwrap();
    assert!(
        matches!(result, RunOutcome::Halted { signal: 0x5a35, .. }),
        "{result:?}"
    );
    assert_eq!(system.cpu().register(13), Some(0xc000));
    assert_eq!(system.cpu().code_segment(), 7);
    assert_eq!(system.cpu().data_segments(), [0, 1, 2, 3]);
}
#[test]
fn malformed_or_truncated_flash_never_constructs_a_running_system() {
    let good = fixture();
    for index in (0..12).chain(16..32) {
        let mut bad = good.clone();
        bad[index] ^= 1;
        assert!(CpuV3SystemSim::from_single_flash(&bad, 0xd00d, VBlankMode::Manual).is_err());
    }
    for len in [0, 31, 127, good.len() - 1] {
        assert!(
            CpuV3SystemSim::from_single_flash(&good[..len], 0xd00d, VBlankMode::Manual).is_err()
        );
    }
}
#[test]
fn generic_cpu_entry_remains_segment_zero() {
    assert_eq!(cpu_v3::CpuV3Sim::default().code_segment(), 0);
}
