//! Host-side qualification of the actual RCC camera and frame program.
#![allow(dead_code)]
#[path = "../../build_support/rcc_modules.rs"]
mod resolver;
use cpu_v3::rcc_backend::CpuV3Program;
use cpu_v3::{rcc_backend, CpuV3Sim, PhysicalWordAddress, StepOutcome};
use std::path::Path;

pub fn compile(source: &str, constants: &str, entry: &'static str) -> CpuV3Program {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    options.stack_init = cpu_v3_tang_nano_20k::boot::SINGLE_BOOT_STACK;
    let parsed = rcc::frontend::compile_program_named("GPU demo", source, &options, &mut |n| {
        resolver::load_module(
            &root.join("examples/gpu_demo/rcc"),
            &root.join("rcc"),
            constants,
            n,
            &mut |_| {},
        )
    })
    .unwrap();
    rcc_backend::try_compile(parsed, &options, entry).unwrap()
}
pub fn address(program: &CpuV3Program, name: &str) -> u32 {
    program
        .debug
        .globals
        .iter()
        .find_map(|v| {
            if v.name == name {
                if let rcc::VarLoc::Global(a) = v.loc {
                    return Some(u32::from(a));
                }
            }
            None
        })
        .unwrap()
}
pub fn orbit_matrices() -> Vec<Vec<i32>> {
    let code = compile(
        r#"
mod camera;
static MATRIX:Buf<u16,42>=Buf::new([0;42]);
fn main(){let mut i:u16=0;let mut phase:u16=0;
while i<512{demo_camera(MATRIX.as_array(),phase);signal(1,phase);phase+=128;i+=1;}halt(0x600d);}
"#,
        "",
        "main",
    );
    let base = address(&code, "MATRIX");
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&code.words);
    let mut matrices = Vec::new();
    for _ in 0..5_000_000 {
        match cpu.step().unwrap() {
            StepOutcome::Signaled(event) => {
                assert_eq!(event.signal_type, 1);
                assert_eq!(usize::from(event.value), matrices.len() * 128);
                let matrix: Vec<i32> = (0..21)
                    .map(|i| {
                        let lo = cpu.physical_memory(PhysicalWordAddress::new(base + i * 2));
                        let hi = cpu.physical_memory(PhysicalWordAddress::new(base + i * 2 + 1));
                        (u32::from(lo) | u32::from(hi) << 16) as i32
                    })
                    .collect();
                let (s, c) = cpu_v3::sincos_q16(cpu_v3::fix32_mul(i32::from(event.value), 411775));
                assert_eq!(matrix[0], cpu_v3::fix32_mul(-c, 51200));
                assert_eq!(matrix[2], cpu_v3::fix32_mul(s, 51200));
                assert_eq!(&matrix[8..12], &[-s, 0, -c, 524288]);
                assert_eq!(&matrix[12..], &[-c, 0, s, 0, 65536, 0, s, 0, c]);
                matrices.push(matrix);
            }
            StepOutcome::Halted { signal } => {
                assert_eq!(signal, 0x600d);
                assert_eq!(matrices.len(), 512);
                return matrices;
            }
            StepOutcome::Running => {}
        }
    }
    panic!("bounded camera probe exceeded 5000000 CPU steps");
}
pub fn write_matrices(path: &Path) {
    let matrices = orbit_matrices();
    let json = format!(
        "[{}]\n",
        matrices
            .iter()
            .map(|m| format!(
                "[{}]",
                m.iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            ))
            .collect::<Vec<_>>()
            .join(",\n")
    );
    std::fs::write(path, json).unwrap();
}
