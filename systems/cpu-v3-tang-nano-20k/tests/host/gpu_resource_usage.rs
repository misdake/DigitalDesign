//! Actual approved resource facade: ownership, cleanup and source isolation.
use super::*;
const APP: &str = include_str!("../fixtures/gpu_renderer_minimal.rs");
fn parse(source: &str) -> Result<rcc::frontend::Program, rcc::frontend::CompileError> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("rcc");
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    resolver::compile_renderer_program(
        "renderer scope caller",
        source,
        &options,
        &root,
        &root,
        RENDERER_METADATA,
        &mut |_| {},
    )
}
fn execute(
    source: &str,
    frames: usize,
) -> (CpuV3Sim, Rc<RefCell<State>>, rcc_backend::CpuV3Program) {
    execute_signal(source, frames, 0x600d, false)
}
fn execute_signal(
    source: &str,
    frames: usize,
    signal: u16,
    fail_publication: bool,
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
    state.borrow_mut().expected_depth = Some(65535);
    state.borrow_mut().fail_publication = fail_publication;
    let result = cpu.run(5_000_000).unwrap();
    assert!(
        matches!(result, RunOutcome::Halted { signal: actual, .. } if actual==signal),
        "{result:?}"
    );
    assert_eq!(cpu.data_segments(), [0, 1, 2, 3]);
    (cpu, state, program)
}

#[test]
fn duplicate_renderer_never_constructs_a_second_owner_of_outstanding_work() {
    let source = r#"mod gpu_pipeline;
fn main(){let mut r=renderer_open();let mut field=renderer_text(&mut r,0,0,8);let mut f=renderer_frame(&mut r,0x1111);
let mut p=frame_ui_pass(&mut f);pass_text(&mut p,&mut field);pass_complete(p);frame_complete(f);
let mut next=renderer_frame(&mut r,0x1112);let mut q=frame_ui_pass(&mut next);
pass_text(&mut q,&mut field);pass_complete(q);frame_complete(next);
let duplicate=renderer_open();halt(0x600d);}"#;
    let (mut cpu, state, program) = execute_signal(source, 1, 0xe022, false);
    assert_eq!(state.borrow().starts.len(), 1);
    assert_eq!(state.borrow().swaps, 0);
    let refs = record(&mut cpu, &program, "GPU_PUI_PAGES", 8);
    assert_eq!((refs[0], refs[4]), (3, 2));
    assert_eq!(record(&mut cpu, &program, "GPU_PIPE_FAULT", 5)[0], 34);
}

#[test]
fn failed_complete_consumes_the_frame_and_preserves_its_unpublished_input() {
    let source = r#"mod gpu_pipeline;
fn main(){let mut r=renderer_open();let mut field=renderer_text(&mut r,0,0,8);let mut f=renderer_frame(&mut r,0x1111);
let mut p=frame_ui_pass(&mut f);pass_text(&mut p,&mut field);pass_complete(p);frame_complete(f);
halt(0x600d);}"#;
    let (mut cpu, state, program) = execute_signal(source, 0, 0xe01f, true);
    assert_eq!(state.borrow().counts, 0);
    assert!(state.borrow().starts.is_empty());
    assert_eq!(state.borrow().swaps, 0);
    let refs = record(&mut cpu, &program, "GPU_PUI_PAGES", 8);
    assert_eq!(refs[0], 2);
    assert_eq!(record(&mut cpu, &program, "GPU_PIPE", 6)[0], 3);
    assert_eq!(record(&mut cpu, &program, "GPU_RENDERER", 2)[0], 5);
}

#[test]
fn approved_production_facade_rejects_raw_helpers_aliases_and_legacy_bridges() {
    for source in [
        "mod gpu_pipeline;fn main(){let p=GPU_DRIVER.as_array();}",
        "mod gpu_pipeline;fn main(){let p=GPU_SLOT_OWNERS.as_array();}",
        "mod gpu_pipeline;fn main(){let p=GPU_BUILD.as_array();}",
        "mod gpu_pipeline;fn main(){let p=addr_of(&GPU_DRIVER);}",
        "mod gpu_pipeline;fn main(){let f=gpu_driver_owned_buffer;}",
        "mod gpu_pipeline;fn main(){gpu_high_slot_init(0,0);}",
        "mod gpu_pipeline;mod gpu_device;use gpu_device::*;fn main(){gpu_driver_init();}",
        "mod gpu_pipeline;mod gpu;fn main(){gpu_init();}",
        "mod gpu_device;fn main(){gpu_driver_owned_buffer();}",
        "fn main(){}",
        "mod gpu_pipeline;const GPU_DRIVER:u16=0;fn main(){let p=addr_of(&GPU_DRIVER);}",
    ] {
        assert!(parse(source).is_err(), "accepted {source}");
    }
    let source = r#"mod gpu_pipeline;
fn cancel(mut r:&mut Renderer,mut field:&mut TextField){let mut f=renderer_frame(&mut r,0x1111);
let mut p=frame_ui_pass(&mut f);pass_text(&mut p,&mut field);return;}
fn main(){let mut r=renderer_open();let mut field=renderer_text(&mut r,0,0,8);cancel(&mut r,&mut field);renderer_close(r);halt(0x600d);}"#;
    let (_, state, _) = execute(source, 0);
    assert!(state.borrow().starts.is_empty());
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
    assert_eq!(record(&mut cpu, &program, "GPU_RENDERER", 2), [4, 2]);
    let refs = record(&mut cpu, &program, "GPU_PUI_PAGES", 8);
    assert_eq!(refs[0], 0);
    assert_eq!(refs[4], 0);
    let pipe = record(&mut cpu, &program, "GPU_PIPE", 6);
    assert_eq!(&pipe[1..4], &[0, 0, 0]);
    drop(s);
    let early = r#"mod gpu_pipeline;
fn unfinished(){let mut r=renderer_open();let mut field=renderer_text(&mut r,0,0,8);let mut f=renderer_frame(&mut r,0x1111);
 frame_number(&mut f,&mut field,42);let mut p=frame_ui_pass(&mut f);pass_text(&mut p,&mut field);return;}
fn main(){unfinished();halt(0x600d);}"#;
    let (mut cpu, state, program) = execute(early, 0);
    assert!(state.borrow().starts.is_empty());
    assert_eq!(state.borrow().maintenance, 0);
    assert_eq!(state.borrow().swaps, 0);
    assert_eq!(record(&mut cpu, &program, "GPU_RENDERER", 2), [4, 3]);
    let refs = record(&mut cpu, &program, "GPU_PUI_PAGES", 8);
    assert_eq!(refs[0], 0);
    assert_eq!(refs[4], 0);
}
#[test]
fn implicit_renderer_exit_freezes_async_work_without_drain_or_successor_submission() {
    let source = r#"mod gpu_pipeline;
fn run(){let mut r=renderer_open();let mut field=renderer_text(&mut r,0,0,8);let mut f=renderer_frame(&mut r,0x1111);
 let mut p=frame_ui_pass(&mut f);pass_text(&mut p,&mut field);pass_complete(p);frame_complete(f);
 let mut next=renderer_frame(&mut r,0x1112);let mut q=frame_ui_pass(&mut next);
 pass_text(&mut q,&mut field);pass_complete(q);frame_complete(next);}
fn main(){run();halt(0x600d);}"#;
    let (mut cpu, state, program) = execute(source, 1);
    let s = state.borrow();
    assert_eq!(s.starts.len(), 1);
    assert_eq!(s.swaps, 0);
    assert_eq!(s.maintenance, 1);
    assert_eq!(record(&mut cpu, &program, "GPU_RENDERER", 2)[0], 5);
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
        "let mut r=renderer_open();let mut field=renderer_text(&mut r,0,0,8);let mut f=renderer_frame(&mut r,0);let p=frame_ui_pass(&mut f);frame_complete(f);",
        "let mut r=renderer_open();let mut field=renderer_text(&mut r,0,0,8);let mut f=renderer_frame(&mut r,0);frame_complete(f);frame_number(&mut f,&mut field,1);",
        "let mut r=renderer_open();let mut field=renderer_text(&mut r,0,0,8);let mut f=renderer_frame(&mut r,0);let p=frame_ui_pass(&mut f);pass_complete(p);pass_complete(p);",
        "let mut r=renderer_open();let mut field=renderer_text(&mut r,0,0,8);let f=renderer_frame(&mut r,0);renderer_close(r);",
    ] {
        assert!(parse(&format!("mod gpu_pipeline;fn main(){{{body}halt(0x600d);}}")).is_err(),"accepted {body}");
    }
}

#[test]
fn owned_text_fields_erase_tails_cache_hidden_edits_and_release_only_metadata() {
    let source = r#"mod gpu_pipeline;
fn main(){let mut r=renderer_open();{let mut field=renderer_text(&mut r,0,0,8);
{let mut f=renderer_frame(&mut r,0x1111);frame_number(&mut f,&mut field,12345);signal(1,1);
frame_number(&mut f,&mut field,7);signal(1,2);text_visible(&mut field,false);signal(1,3);
frame_number(&mut f,&mut field,42);signal(1,4);text_visible(&mut field,true);signal(1,5);
text_visible(&mut field,true);frame_number(&mut f,&mut field,42);signal(1,6);}
signal(1,7);}renderer_close(r);halt(0x600d);}"#;
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let program = rcc_backend::try_compile(parse(source).unwrap(), &options, "main").unwrap();
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    let state = attach(&mut cpu, Some(vec![]));
    let mut observations = Vec::new();
    let mut halted = false;
    for _ in 0..2_000_000 {
        match cpu.step().unwrap() {
            cpu_v3::StepOutcome::Signaled(event) => {
                assert_eq!(event.signal_type, 1);
                assert_eq!(usize::from(event.value), observations.len() + 1);
                observations.push((
                    record(&mut cpu, &program, "GPU_PUI_DESIRED", 4),
                    record(&mut cpu, &program, "GPU_PUI_CANVAS", 4)[1],
                ));
            }
            cpu_v3::StepOutcome::Halted { signal } => {
                assert_eq!(signal, 0x600d);
                halted = true;
                break;
            }
            _ => {}
        }
    }
    assert!(halted);
    assert_eq!(observations.len(), 7);
    assert_eq!(observations[0].0, [0x3231, 0x3433, 0xff35, 65535]);
    assert_eq!(observations[1].0, [0xff37, 65535, 65535, 65535]);
    assert_eq!(observations[2].0, [65535; 4]);
    assert_eq!(
        observations[2], observations[3],
        "hidden edit touched desired"
    );
    assert_eq!(observations[4].0, [0x3234, 65535, 65535, 65535]);
    assert_eq!(
        observations[4], observations[5],
        "unchanged edit invalidated canvas"
    );
    assert_eq!(
        observations[5], observations[6],
        "cancel erased desired text"
    );
    assert!(state.borrow().starts.is_empty());
    assert_eq!(record(&mut cpu, &program, "GPU_RENDERER_FIELDS", 1), [0]);
    assert_eq!(
        record(&mut cpu, &program, "GPU_PUI_DESIRED", 4),
        observations[6].0,
        "field cleanup erased desired text"
    );
    for (body,signal) in [
      ("let a=renderer_text(&mut r,49,0,2);",0xe033),
      ("let a=renderer_text(&mut r,0,0,8);let b=renderer_text(&mut r,7,0,8);",0xe034),
      ("let a=renderer_text(&mut r,0,0,1);let b=renderer_text(&mut r,0,1,1);let c=renderer_text(&mut r,0,2,1);let d=renderer_text(&mut r,0,3,1);let e=renderer_text(&mut r,0,4,1);",0xe035),
    ] {
      execute_signal(&format!("mod gpu_pipeline;fn main(){{let mut r=renderer_open();{body}halt(0x600d);}}"),0,signal,false);
    }
}
