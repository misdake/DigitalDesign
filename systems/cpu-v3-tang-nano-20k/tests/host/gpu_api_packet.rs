//! Native ABI4 byte oracle for the independent Target/Load/Clear assembler.
use cpu_v3::{rcc_backend, CpuV3Sim, RunOutcome};
use rcc::frontend::{compile_program_named_checked, rustc_check::RustcCheck};
use std::path::Path;

#[test]
fn pass_prefix_bytes_match_native_abi_for_each_independent_load_op() {
    let source = r#"mod gpu_packet;
static STEP:u16=0;
fn check(ok:bool){if !ok{halt(0xd000+STEP);}unsafe{addr_of(&STEP).write(0,STEP+1);}}
fn main(){gpu_record_init();check(gpu_record_configure(0,0x9800,1,0x9a00,1));
 let mut c=gpu_record_data(0);
 check(!gpu_packet_begin(0,0x8001,0x9000,0,0,0));
 check(!gpu_packet_begin(0,0x8000,0x8000,0,0,0));
 check(!gpu_packet_begin(0,0x9800,0x9000,0,0,0));
 check(!gpu_packet_begin(0,0x8000,0x9000,4,0,0));
 c[GPU_RECORD_COMMAND_USED]=56;check(!gpu_packet_begin(0,0x8000,0x9000,0,0,0));
 check(c[GPU_RECORD_COMMAND_USED]==56&&c[GPU_RECORD_PUSH_USED]==0&&c[GPU_RECORD_PHASE]==GPU_RECORD_READY);
 c[GPU_RECORD_COMMAND_USED]=0;c[GPU_RECORD_PUSH_USED]=64;
 check(!gpu_packet_begin(0,0x8000,0x9000,0,0,0));check(c[GPU_RECORD_COMMAND_USED]==0);
 let mut mask:u16=0;while mask<4{
 check(gpu_record_configure(0,0x9800+mask*2,1,0x9a00+mask*2,1));
 check(gpu_packet_begin(0,0x8000,0x9000,mask,0x1234,0xffff));
 check(c[GPU_RECORD_COLOR]==0x8000&&c[GPU_RECORD_DEPTH]==0x9000);
 check(gpu_record_close_pass(0));check(gpu_record_seal(0));mask+=1;
 }halt(0x600d);}"#;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("rcc");
    let output = std::env::var_os("RCC_RUSTC_TEST_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("../../../target/graphics-api-g0"));
    for enabled in [false, true] {
        let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
        options.gpu_debug = enabled;
        let parsed = compile_program_named_checked(
            "api-pass-packet",
            source,
            &options,
            &RustcCheck::new(output.join(format!("packet-{enabled}"))),
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
        for mask in 0..4usize {
            let command_sector = 0x9800 + mask * 2;
            let push_sector = 0x9a00 + mask * 2;
            let dma = 0x10000000u32 | (((push_sector as u32 * 128) / 32) << 10) | (69 << 3);
            let mut words = vec![0x40000003u32, 0x70000000, dma, 0x40000001, 0xa0000045];
            if mask & 1 != 0 {
                words.push(0x80001234);
            }
            if mask & 2 != 0 {
                words.push(0x8001ffff);
            }
            words.extend([0x40000003, 0x70000000, 0xf0000000]);
            let mut expected = words
                .iter()
                .flat_map(|w| [*w as u16, (*w >> 16) as u16])
                .collect::<Vec<_>>();
            expected.resize(64, 0);
            assert_eq!(
                &memory[command_sector * 64..command_sector * 64 + 64],
                expected,
                "load mask {mask}/{enabled}"
            );
            let mut target = vec![0x2000, 0x0900];
            target.resize(16, 0);
            assert_eq!(
                &memory[push_sector * 64..push_sector * 64 + 16],
                target,
                "Target32 {mask}/{enabled}"
            );
        }
    }
}
