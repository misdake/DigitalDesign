//! Trusted diagnostic specialization: real public owner, seal/replay and fail-stop.
use super::*;
use cpu_v3::{Device, StepOutcome};
use std::any::Any;
#[derive(Default)]
struct Observation {
    count: u16,
    publications: usize,
    commands: Vec<Vec<u32>>,
}
struct NativePeer(u8, Rc<RefCell<Observation>>);
impl Device for NativePeer {
    fn read(&mut self, _: &mut [u16], channel: u8) -> u16 {
        match (self.0, channel) {
            (3, 2) => 0x20,
            _ => 0,
        }
    }
    fn write(&mut self, memory: &mut [u16], channel: u8, value: u16) {
        let mut s = self.1.borrow_mut();
        match (self.0, channel) {
            (0, 1) => s.publications += 1,
            (4, 0) => s.count = value,
            (4, 1) => {
                let start = usize::from(value) * 64;
                let end = start + (usize::from(s.count) + 1) * 2;
                let code = memory[start..end]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|w| u32::from(w[0]) | (u32::from(w[1]) << 16))
                    .collect::<Vec<_>>();
                assert_eq!(code.iter().filter(|&&w| w >> 28 == 12).count(), 3);
                s.commands.push(code);
            }
            _ => {}
        }
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
#[test]
fn explicit_native_diagnostic_seals_once_replays_and_rejects_reserved_fields_before_send() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = root
        .join("../../target/native-profile-contract")
        .join(std::process::id().to_string());
    let source = r#"mod gpu_api;use gpu_api::*;mod gpu_demo_api;use gpu_demo_api::*;
 fn main(){let mut gpu=Gpu::init(demo_config());let mut commands=gpu.command_buffer();
 commands.invalidate_textures();gpu.request_snapshot();gpu.submit_capturable(commands);commands=gpu.wait();
 gpu.request_snapshot();gpu.submit_capturable(commands);commands=gpu.wait();
 commands.reset();commands.invalidate_textures();gpu.request_snapshot();gpu.submit_capturable(commands);commands=gpu.wait();halt(0x600d);}"#;
    for debug in [false, true] {
        for reserved in [false, true] {
            let folder = output.join(format!("{debug}-{reserved}"));
            let shared = folder.join("rcc");
            std::fs::create_dir_all(&folder).unwrap();
            let script="import importlib.util,sys;from pathlib import Path;p=Path(sys.argv[1]);s=importlib.util.spec_from_file_location('native_sources',p);m=importlib.util.module_from_spec(s);s.loader.exec_module(m);m.materialize(Path(sys.argv[2]),Path(sys.argv[3]),reserved=sys.argv[4]=='1',explicit_api=True)";
            let result = std::process::Command::new("python")
                .arg("-B")
                .arg("-c")
                .arg(script)
                .arg(root.join("examples/gpu_profiler_board/tools/native_sources.py"))
                .arg(root.join("rcc"))
                .arg(&shared)
                .arg(if reserved { "1" } else { "0" })
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            let gate = folder.join("profiler_gate.rs");
            std::fs::write(
                &gate,
                "use crate::dsl_rt::*;pub fn profiler_gate(d:Array<u16>)->u16{signal(14,0);0}",
            )
            .unwrap();
            let manifest = folder.join("build.json");
            std::fs::write(&manifest,serde_json::to_vec_pretty(&serde_json::json!({"schema":"digitaldesign.gpu-capture-build","version":3,"command_abi":8,"manifest_id":"42".repeat(32),"resources":[],"layout":resolver::RendererLayoutProfile::Column16.recorded_description(),"options":{}})).unwrap()).unwrap();
            let configuration =
                resolver::RendererCaptureConfiguration::from_manifest(gate, &manifest).unwrap();
            let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
            options.gpu_debug = debug;
            let ir = resolver::compile_explicit_gpu_program_checked(
                "trusted native diagnostic",
                source,
                &options,
                &folder,
                &shared,
                RENDERER_METADATA,
                &configuration,
                &rcc::frontend::rustc_check::RustcCheck::new(folder.join("checked")),
                &mut |_| {},
            )
            .unwrap();
            let program = rcc_backend::try_compile(ir, &options, "main").unwrap();
            let global = |name: &str| super::changes::global(&program, name);
            let state = Rc::new(RefCell::new(Observation::default()));
            let mut cpu = CpuV3Sim::default();
            cpu.set_boot_window(&program.words);
            for device in [0, 3, 4] {
                cpu.attach_device(device, Box::new(NativePeer(device, state.clone())));
            }
            let mut halted = None;
            let mut gates = 0;
            for _ in 0..3_000_000 {
                match cpu.step().unwrap() {
                    StepOutcome::Signaled(e) if e.signal_type == 14 => gates += 1,
                    StepOutcome::Halted { signal } => {
                        halted = Some(signal);
                        break;
                    }
                    _ => {}
                }
            }
            assert!(halted.is_some(), "bounded native diagnostic timed out");
            assert_eq!(cpu.data_segments(), [0, 1, 2, 3]);
            let memory = cpu.physical_memory_mut();
            let owner = global("GPU_EXECUTION");
            let capture = global("GPU_CAPTURE_CONTROL");
            if reserved {
                assert_ne!(halted, Some(0x600d));
                assert_eq!(state.borrow().publications, 1);
                assert!(state.borrow().commands.is_empty());
                assert_eq!(gates, 0);
                assert_eq!(memory[owner + 1], 1);
                assert_eq!((memory[capture], memory[capture + 1]), (1, 9));
            } else {
                assert_eq!(halted, Some(0x600d));
                assert_eq!(state.borrow().publications, 3);
                assert_eq!(gates, 3);
                let s = state.borrow();
                assert_eq!(s.commands.len(), 3);
                assert_eq!(s.commands[0], s.commands[1]);
                assert_eq!(s.commands[0], s.commands[2]);
                assert_eq!(memory[owner + 1], 0);
            }
        }
    }
}
