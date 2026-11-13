//! Native END permission for maintenance-only and post-Pass command streams.
use cpu_v3::{rcc_backend, CpuV3Sim, RunOutcome};
use rcc::frontend::rustc_check::RustcCheck;
use std::path::Path;

#[test]
fn maintenance_only_seal_adds_missing_terminal_fence_without_changing_closed_passes() {
    let source = r#"mod gpu_packet;
static STEP:u16=0;
fn check(ok:bool){if !ok{halt(0xd000+STEP);}unsafe{addr_of(&STEP).write(0,STEP+1);}}
fn main(){gpu_record_init();check(gpu_record_configure(0,0x9800,1,0x9a00,1));
 check(!gpu_record_seal(0));check(gpu_packet_invalidate_textures(0));
 check(gpu_record_seal(0));check(gpu_record_seal(0));let a=gpu_record_data(0);
 check(a[GPU_RECORD_COMMAND_USED]==10&&a[GPU_RECORD_PADDED]==64);
 check(gpu_record_configure(1,0xfc00,2,0xfe00,1));let mut b=gpu_record_data(1);
 // A synthetic NOP prefix makes the terminal tail span two fetch sectors.
 b[GPU_RECORD_COMMAND_USED]=56;check(gpu_packet_invalidate_textures(1));check(gpu_record_seal(1));
 check(b[GPU_RECORD_COMMAND_USED]==66&&b[GPU_RECORD_PADDED]==128);
 check(gpu_record_configure(1,0xfb00,1,0xfd00,1));b[GPU_RECORD_COMMAND_USED]=56;
 check(!gpu_packet_invalidate_textures(1));check(b[GPU_RECORD_COMMAND_USED]==56&&b[GPU_RECORD_TERMINAL]==0);
 check(gpu_record_configure(0,0x9900,1,0x9b00,1));check(gpu_record_begin_pass(0));
 check(gpu_record_close_pass(0));check(gpu_packet_invalidate_textures(0));check(gpu_record_seal(0));
 check(a[GPU_RECORD_COMMAND_USED]==10);halt(0x600d);}"#;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("rcc");
    let output = std::env::var_os("RCC_RUSTC_TEST_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("../../../target/graphics-api-g0"));
    for enabled in [false, true] {
        let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
        options.gpu_debug = enabled;
        let parsed = crate::checked_flat_fixture::compile(
            "maintenance-seal",
            source,
            &options,
            &RustcCheck::new(output.join(format!("maintenance-{enabled}"))),
            &mut |name| {
                std::fs::read_to_string(root.join(format!("{name}.rs"))).map_err(|e| e.to_string())
            },
        )
        .unwrap();
        let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&program.words);
        assert!(matches!(
            cpu.run(500_000).unwrap(),
            RunOutcome::Halted { signal: 0x600d, .. }
        ));
        let memory = cpu.physical_memory_mut();
        for (base, code) in [
            (
                0x9800,
                [
                    0x40000003u32,
                    0x90000000,
                    0x40000003,
                    0x70000000,
                    0xf0000000,
                ],
            ),
            (
                0x9900,
                [
                    0x40000003u32,
                    0x70000000,
                    0x40000003,
                    0x90000000,
                    0xf0000000,
                ],
            ),
        ] {
            let mut expected = code
                .iter()
                .flat_map(|w| [*w as u16, (*w >> 16) as u16])
                .collect::<Vec<_>>();
            expected.resize(64, 0);
            assert_eq!(&memory[base * 64..base * 64 + 64], expected);
        }
        let base = 0xfc00 * 64;
        assert_eq!(
            &memory[base + 56..base + 66],
            &[3, 0x4000, 0, 0x9000, 3, 0x4000, 0, 0x7000, 0, 0xf000]
        );
        assert_eq!(&memory[base + 66..base + 128], &[0; 62]);
        assert_eq!(&memory[0xfb00 * 64..0xfb00 * 64 + 64], &[0; 64]);
    }
}
