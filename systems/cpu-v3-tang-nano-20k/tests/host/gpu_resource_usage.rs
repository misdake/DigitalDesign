//! Test-only caller model: lexical resources over the accepted native engine.
//! Helper isolation and the full camera/mesh/stats usage round remain pending.
use super::*;
const MODEL: &str = include_str!("../fixtures/gpu_renderer_scope_model.rs");
const APP: &str = include_str!("../fixtures/gpu_renderer_minimal.rs");
fn parse(source: &str) -> Result<rcc::frontend::Program, rcc::frontend::CompileError> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("rcc");
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    rcc::frontend::compile_program_named("renderer scope model", source, &options, &mut |name| {
        if name == "renderer_model" {
            Ok(MODEL.to_owned())
        } else {
            std::fs::read_to_string(root.join(format!("{name}.rs"))).map_err(|e| e.to_string())
        }
    })
}
fn execute(
    source: &str,
    frames: usize,
) -> (CpuV3Sim, Rc<RefCell<State>>, rcc_backend::CpuV3Program) {
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let program = rcc_backend::try_compile(
        parse(source).unwrap_or_else(|e| panic!("{e}")),
        &options,
        "main",
    )
    .unwrap();
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    let state = attach(&mut cpu, Some(vec![vec![(0, 8)]; frames]));
    state.borrow_mut().automatic = true;
    let result = cpu.run(5_000_000).unwrap();
    assert!(
        matches!(result, RunOutcome::Halted { signal: 0x600d, .. }),
        "{result:?}"
    );
    assert_eq!(cpu.data_segments(), [0, 1, 2, 3]);
    (cpu, state, program)
}
fn record(
    cpu: &mut CpuV3Sim,
    program: &rcc_backend::CpuV3Program,
    name: &str,
    n: usize,
) -> Vec<u16> {
    let location = program
        .debug
        .globals
        .iter()
        .find(|v| v.name == name)
        .unwrap();
    let rcc::VarLoc::Global(address) = location.loc else {
        panic!("not global")
    };
    cpu.physical_memory_mut()[usize::from(address)..usize::from(address) + n].to_vec()
}
#[test]
fn compiled_scope_caller_cancels_unfinished_pass_and_transfers_completed_frames() {
    let (mut cpu, state, program) = execute(APP, 3);
    let s = state.borrow();
    assert_eq!(s.starts.len(), 3);
    assert_eq!(s.swaps, 3);
    assert_eq!(s.maintenance, 3);
    for (i, page) in s.ui_snapshots.iter().enumerate() {
        assert_eq!(page[0], 0xff00 | (48 + i as u16));
        assert!(page[1..384].iter().all(|&w| w == 65535));
    }
    assert_eq!(record(&mut cpu, &program, "GPU_SCOPE_MODEL", 2), [4, 2]);
    let refs = record(&mut cpu, &program, "GPU_PUI_PAGES", 8);
    assert_eq!(refs[0], 0);
    assert_eq!(refs[4], 0);
    let pipe = record(&mut cpu, &program, "GPU_PIPE", 6);
    assert_eq!(&pipe[1..4], &[0, 0, 0]);
    drop(s);
    let early = r#"mod renderer_model;
fn unfinished(){let mut r=renderer_open();let mut f=renderer_frame(&mut r,0x1111);
 frame_number(&mut f,42);let mut p=frame_pass(&mut f);pass_text(&mut p);return;}
fn main(){unfinished();halt(0x600d);}"#;
    let (mut cpu, state, program) = execute(early, 0);
    assert!(state.borrow().starts.is_empty());
    assert_eq!(state.borrow().maintenance, 0);
    assert_eq!(state.borrow().swaps, 0);
    assert_eq!(record(&mut cpu, &program, "GPU_SCOPE_MODEL", 2), [4, 3]);
    let refs = record(&mut cpu, &program, "GPU_PUI_PAGES", 8);
    assert_eq!(refs[0], 0);
    assert_eq!(refs[4], 0);
}
#[test]
fn implicit_renderer_exit_freezes_async_work_without_drain_or_successor_submission() {
    let source = r#"mod renderer_model;
fn run(){let mut r=renderer_open();let mut f=renderer_frame(&mut r,0x1111);
 let mut p=frame_pass(&mut f);pass_text(&mut p);pass_complete(p);frame_complete(f);
 let mut next=renderer_frame(&mut r,0x1112);let mut q=frame_pass(&mut next);
 pass_text(&mut q);pass_complete(q);frame_complete(next);}
fn main(){run();halt(0x600d);}"#;
    let (mut cpu, state, program) = execute(source, 1);
    let s = state.borrow();
    assert_eq!(s.starts.len(), 1);
    assert_eq!(s.swaps, 0);
    assert_eq!(s.maintenance, 1);
    assert_eq!(record(&mut cpu, &program, "GPU_SCOPE_MODEL", 2)[0], 5);
    let refs = record(&mut cpu, &program, "GPU_PUI_PAGES", 8);
    assert_eq!(refs[0], 3);
    assert_eq!(refs[4], 2);
    assert_eq!(record(&mut cpu, &program, "GPU_PIPE", 6)[0], 3);
}
#[test]
fn concrete_caller_rejects_resource_misuse_before_lowering() {
    for body in [
        "let r:Renderer=1;",
        "let r=Renderer{word:1};",
        "let mut r=renderer_open();let mut f=renderer_frame(&mut r,0);let p=frame_pass(&mut f);frame_complete(f);",
        "let mut r=renderer_open();let mut f=renderer_frame(&mut r,0);frame_complete(f);frame_number(&mut f,1);",
        "let mut r=renderer_open();let mut f=renderer_frame(&mut r,0);let p=frame_pass(&mut f);pass_complete(p);pass_complete(p);",
        "let mut r=renderer_open();let f=renderer_frame(&mut r,0);renderer_close(r);",
    ] {
        assert!(parse(&format!("mod renderer_model;fn main(){{{body}halt(0x600d);}}")).is_err(),"accepted {body}");
    }
}
