//! Single-application cold entry through the independent architectural model.
use cpu_v3::{
    rcc_backend::{self, CompilerOptions},
    RunOutcome,
};
use cpu_v3_tang_nano_20k::{
    boot::*,
    system_sim::{CpuV3SystemSim, VBlankMode},
};
#[test]
fn explicit_gpu_project_mode_is_host_policy_and_retired_renderer_is_rejected() {
    let native = parse_boot_project("format 2\napplication app.rs\n").unwrap();
    let explicit = parse_boot_project("format 2\ngpu-api\napplication app.rs\n").unwrap();
    assert!(!native.gpu_api && explicit.gpu_api);
    assert_eq!(native.application, explicit.application);
    assert_eq!(native.constants(), explicit.constants());
    for directives in [
        "renderer",
        "gpu-api\ngpu-api",
        "renderer\ngpu-api",
        "gpu-api\nrenderer",
    ] {
        assert!(
            parse_boot_project(&format!("format 2\n{directives}\napplication app.rs\n")).is_err()
        );
    }
}

#[path = "../../support/boot_contract.rs"]
mod contract;
use contract::fixture;
#[test]
fn cold_flash_clears_only_allocated_statics_and_executes_initializers() {
    let package = fixture();
    let mut system =
        CpuV3SystemSim::from_single_flash(&package, 0xd00d, VBlankMode::Manual).unwrap();
    assert_eq!(system.cpu().code_segment(), 7);
    assert_eq!(system.cpu().data_segments(), [0, 1, 2, 3]);
    assert_eq!(system.cpu().retired_words(), 0);
    assert!(system.cpu_mut().physical_memory_mut()[..17]
        .iter()
        .all(|x| *x == 0));
    assert!(
        system.cpu_mut().physical_memory_mut()[17..SINGLE_BOOT_STATIC_WORDS]
            .iter()
            .all(|x| *x == 0xd00d)
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
    for index in 0..4 {
        let mut bad = good.clone();
        bad[index] ^= 1;
        assert!(CpuV3SystemSim::from_single_flash(&bad, 0xd00d, VBlankMode::Manual).is_err());
    }
    for len in [0, 3, 11, good.len() - 1] {
        assert!(
            CpuV3SystemSim::from_single_flash(&good[..len], 0xd00d, VBlankMode::Manual).is_err()
        );
    }
}
#[test]
fn generic_cpu_entry_remains_segment_zero() {
    assert_eq!(cpu_v3::CpuV3Sim::default().code_segment(), 0);
}

#[test]
fn generated_resource_constants_compile_before_the_image_is_packed() {
    let project =
        parse_boot_project("format 2\napplication app.rs\ncopy MESH 0x100000 mesh.bin").unwrap();
    let constants = project.constants();
    let mut options = CompilerOptions::for_separate_code_and_data_segments(0);
    options.stack_init = SINGLE_BOOT_STACK;
    let parsed = rcc::frontend::compile_program_named(
        "resources.rs",
        "mod boot_resources; use boot_resources::*; fn main() { halt(MESH_WORD_ADDRESS_HIGH); }",
        &options,
        &mut |name| {
            if name == "boot_resources" {
                Ok(constants.clone())
            } else {
                Err(name.into())
            }
        },
    )
    .unwrap();
    let static_words = parsed.debug.static_data_end;
    let compiled = rcc_backend::try_compile(parsed, &options, "main").unwrap();
    let code: Vec<u8> = compiled
        .words
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    let image = pack_application_resources(
        &code,
        static_words,
        &[BootResource::copy(0x100000, b"asset".to_vec())],
    )
    .unwrap();
    let mut system = CpuV3SystemSim::from_single_flash(&image, 0xd00d, VBlankMode::Manual).unwrap();
    assert_eq!(
        system.cpu_mut().physical_memory_mut()[0x80000],
        u16::from_le_bytes(*b"as")
    );
    assert!(matches!(
        system.run(2000).unwrap(),
        RunOutcome::Halted { signal: 8, .. }
    ));
}
