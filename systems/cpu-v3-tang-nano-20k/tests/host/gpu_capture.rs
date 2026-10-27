//! Actual approved renderer calls with a synchronous, read-only capture consumer.
use super::*;
use cpu_v3::StepOutcome;

const APP: &str = r#"mod gpu_pipeline;
fn main(){let mut r=renderer_open();let mut field=renderer_text(&mut r,0,0,8);
{let mut cancelled=renderer_frame(&mut r,0);let unfinished=frame_ui_pass(&mut cancelled);}
let mut i:u16=0;while i<3{let mut f=renderer_frame(&mut r,0x1111+i);
if frame_elapsed_ms(&mut f)!=0 {halt(0xbad);}
frame_number(&mut f,&mut field,i);let mut p=frame_ui_pass(&mut f);pass_text(&mut p,&mut field);
pass_complete(p);frame_complete(f);i+=1;}renderer_close(r);
if gpu_capture_error()!=1 {halt(0xbad);}halt(0x600d);}"#;

fn compile_capture(label: &str, gate: &str) -> rcc_backend::CpuV3Program {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("rcc");
    let out = root.join(format!("../../../target/gpu-capture-tests/{label}"));
    std::fs::create_dir_all(&out).unwrap();
    let gate_path = out.join("profiler_gate.rs");
    std::fs::write(&gate_path, gate).unwrap();
    let capture = resolver::CaptureConfiguration {
        gate_path,
        manifest_id: [0x3412; 16],
    };
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let parsed = resolver::compile_renderer_program_with_capture(
        "capture renderer",
        APP,
        &options,
        &root,
        &root,
        RENDERER_METADATA,
        &[],
        Some(&capture),
        &mut |_| {},
    )
    .unwrap_or_else(|e| panic!("{label}: {e}"));
    rcc_backend::try_compile(parsed, &options, "main").unwrap()
}

const CONSUMER: &str = r#"use crate::dsl_rt::*;mod gpu_pipeline;use gpu_pipeline::*;mod gpu_capture_abi;use gpu_capture_abi::*;
static GATE_CALLS:u16=0;
fn gate_check(ok:bool){if !ok{halt(0xca00+GATE_CALLS);}}
pub fn profiler_gate(d:Array<u16>)->u16{
gate_check(d[GPU_CAPTURE_H_VERSION]==1 && d[GPU_CAPTURE_H_WORDS]==64);
gate_check(d[GPU_CAPTURE_H_TOKEN]==GATE_CALLS+1 && d[GPU_CAPTURE_H_TOKEN+1]==0);
gate_check(d[GPU_CAPTURE_H_SUBMISSION]==GATE_CALLS+1);
gate_check(d[GPU_CAPTURE_H_FRAME]==GATE_CALLS+2);
gate_check(d[GPU_CAPTURE_H_SLOT]==(GATE_CALLS&1));
gate_check(d[GPU_CAPTURE_H_COMMAND_BYTES]==(d[GPU_CAPTURE_H_COUNT]+1)*4);
gate_check(d[GPU_CAPTURE_H_COMMAND_PADDED_BYTES]==128);
gate_check(d[GPU_CAPTURE_H_RANGE_COUNT]==9 && d[GPU_CAPTURE_H_RANGE_WORDS]==8);
gate_check(d[GPU_CAPTURE_H_UI_REFERENCE]==2 && d[GPU_CAPTURE_H_UI_BYTES]==1024);
gate_check(d[GPU_CAPTURE_H_MANIFEST]==0x3412 && gpu_capture_error()==0);
let mut r:u16=0;let mut color:u16=65535;
while r<d[GPU_CAPTURE_H_RANGE_COUNT]{
 let tag=gpu_capture_range_word(r,GPU_CAPTURE_R_TAG);
 if tag==GPU_CAPTURE_TAG_COLOR{color=r;}
 r+=1;
}
gate_check(color!=65535);
let first=gpu_capture_read_word(0,0,0);let pad=gpu_capture_read_word(0,63,0);
gate_check(first==3 && pad==0);
gate_check(gpu_capture_read_word(1,15,0)==0);
gate_check(gpu_capture_read_word(color,0,1)==0xbeef);
gate_check(gpu_capture_read_word(color,30463,1)==0xdead);
let token=d[GPU_CAPTURE_H_TOKEN];signal(2,token);
// A second dump reads exactly the same sealed inputs and descriptor identity.
gate_check(gpu_capture_read_word(0,0,0)==first && gpu_capture_read_word(0,63,0)==pad);
gate_check(gpu_capture_read_word(color,0,1)==0xbeef && d[GPU_CAPTURE_H_TOKEN]==token);
gate_check(gpu_capture_error()==0);signal(3,token);
unsafe{addr_of(&GATE_CALLS).write(0,GATE_CALLS+1);}
RETURN_VALUE
}"#;

#[test]
fn capture_reads_both_slots_and_repeated_inputs_without_starting_or_presenting() {
    for resumed in [false, true] {
        let program = compile_capture(
            if resumed { "resumed" } else { "continuous" },
            &CONSUMER.replace("RETURN_VALUE", if resumed { "1" } else { "0" }),
        );
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&program.words);
        let state = attach(&mut cpu, Some(vec![vec![(0, 8)]; 3]));
        state.borrow_mut().automatic = true;
        state.borrow_mut().expected_depth = Some(65535);
        for base in [0x400000, 0x430000] {
            cpu.physical_memory_mut()[base / 2 + 65536] = 0xbeef;
            cpu.physical_memory_mut()[base / 2 + 95999] = 0xdead;
        }
        let mut markers = Vec::new();
        let mut halted = false;
        for _ in 0..5_000_000 {
            match cpu.step().unwrap() {
                StepOutcome::Signaled(e) if e.signal_type == 2 || e.signal_type == 3 => {
                    let s = state.borrow();
                    let prior = usize::from(e.value) - 1;
                    assert_eq!(s.counts, prior);
                    assert_eq!(s.starts.len(), prior);
                    assert_eq!(s.swaps, prior);
                    assert_eq!(s.gpu, 0);
                    assert!(!s.pending);
                    assert_eq!(
                        s.maintenance,
                        usize::from(e.value) + usize::from(resumed) * prior
                    );
                    markers.push((e.signal_type, e.value));
                    drop(s);
                    if resumed && e.signal_type == 2 {
                        let now = state.borrow().now_ms;
                        state.borrow_mut().now_ms = now.wrapping_add(40000);
                    }
                }
                StepOutcome::Halted { signal } => {
                    assert_eq!(signal, 0x600d);
                    halted = true;
                    break;
                }
                _ => {}
            }
        }
        assert!(halted);
        assert_eq!(markers, [(2, 1), (3, 1), (2, 2), (3, 2), (2, 3), (3, 3)]);
        let s = state.borrow();
        assert_eq!(s.starts.len(), 3);
        assert_eq!(s.swaps, 3);
        assert_eq!(s.maintenance, if resumed { 6 } else { 3 });
        assert_eq!(cpu.data_segments(), [0, 1, 2, 3]);
        let last = global(&mut cpu, &program, "GPU_RENDERER_LAST", 9);
        assert_eq!(last[8], u16::from(resumed));
        if resumed {
            assert_eq!(&last[4..8], &[0, 0, 0, 0]);
        }
        let refs = global(&mut cpu, &program, "GPU_PUI_PAGES", 8);
        assert_eq!(refs[0], 0);
        assert_eq!(refs[4], 0);
    }
}

fn global(
    cpu: &mut CpuV3Sim,
    program: &rcc_backend::CpuV3Program,
    name: &str,
    n: usize,
) -> Vec<u16> {
    let address = match program
        .debug
        .globals
        .iter()
        .find(|g| g.name == name)
        .unwrap()
        .loc
    {
        rcc::VarLoc::Global(address) => address,
        _ => panic!("global"),
    };
    cpu.physical_memory_mut()[address as usize..address as usize + n].to_vec()
}

#[test]
fn consumer_and_bridge_failures_retain_the_prepared_lease_and_send_nothing() {
    for (label, body) in [
        ("fatal", "2"),
        ("bounds", "gpu_capture_read_word(0,65535,65535);1"),
        ("end-boundary", "gpu_capture_read_word(6,30464,1);1"),
        ("cache", "dev_send(7,0,3);1"),
        ("gpu", "dev_send(7,0,4);1"),
        ("display", "dev_send(7,0,5);1"),
    ] {
        let gate =
            format!("use crate::dsl_rt::*;pub fn profiler_gate(d:Array<u16>)->u16{{{body}}}");
        let program = compile_capture(label, &gate);
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&program.words);
        let state = attach(&mut cpu, Some(vec![vec![(0, 8)]; 3]));
        state.borrow_mut().automatic = true;
        let result = cpu.run(5_000_000).unwrap();
        assert!(
            matches!(result,RunOutcome::Halted{signal,..} if signal>=0xe000),
            "{label}: {result:?}"
        );
        assert_eq!(state.borrow().starts.len(), 0);
        assert_eq!(state.borrow().counts, 0);
        assert_eq!(state.borrow().swaps, 0);
        assert_eq!(global(&mut cpu, &program, "GPU_DRIVER", 2)[0], 3);
        assert_eq!(global(&mut cpu, &program, "GPU_CAPTURE_CONTROL", 13)[0], 1);
        assert_eq!(global(&mut cpu, &program, "GPU_PIPE_A", 12)[9], 5);
        assert_eq!(global(&mut cpu, &program, "GPU_PUI_PAGES", 8)[0], 2);
        assert_eq!(cpu.data_segments(), [0, 1, 2, 3]);
    }
}

#[test]
fn diagnostic_origin_and_manifest_are_explicit_and_do_not_override_normal_apps() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("rcc");
    let local = root.join("../../../target/gpu-capture-tests/origins");
    std::fs::create_dir_all(&local).unwrap();
    for name in ["profiler_gate", "gpu_capture_abi", "gpu_capture_config"] {
        std::fs::write(local.join(format!("{name}.rs")), "invalid local override").unwrap();
    }
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let parsed = resolver::compile_renderer_program(
        "normal renderer",
        APP,
        &options,
        &local,
        &root,
        RENDERER_METADATA,
        &mut |_| {},
    )
    .unwrap();
    rcc_backend::try_compile(parsed, &options, "main").unwrap();
    assert!(resolver::compile_renderer_program(
        "config constant shadow",
        &format!("const GPU_CAPTURE_CONFIG_ENABLED:u16=1;{APP}"),
        &options,
        &local,
        &root,
        RENDERER_METADATA,
        &mut |_| {},
    )
    .is_err());

    let manifest = local.join("manifest.json");
    std::fs::write(
        &manifest,
        serde_json::json!({"manifest_id":"1234".repeat(16)}).to_string(),
    )
    .unwrap();
    let gate_path = local.join("profiler_gate.rs");
    let configuration =
        resolver::CaptureConfiguration::from_manifest(gate_path.clone(), &manifest).unwrap();
    assert_eq!(configuration.manifest_id, [0x3412; 16]);
    // The approved host selects this actual consumer, without granting access
    // to the driver's private state. Only the public lease bridges are callable.
    std::fs::write(
        &gate_path,
        "use crate::dsl_rt::*;pub fn profiler_gate(d:Array<u16>)->u16{GPU_DRIVER[0u16]}",
    )
    .unwrap();
    assert!(resolver::compile_renderer_program_with_capture(
        "unprivileged consumer",
        APP,
        &options,
        &local,
        &root,
        RENDERER_METADATA,
        &[],
        Some(&configuration),
        &mut |_| {}
    )
    .is_err());
    for id in ["1234".repeat(15), "gg".repeat(32), "é".repeat(32)] {
        std::fs::write(&manifest, serde_json::json!({"manifest_id":id}).to_string()).unwrap();
        assert!(
            resolver::CaptureConfiguration::from_manifest(gate_path.clone(), &manifest).is_err()
        );
    }
    std::fs::write(&manifest, "{}").unwrap();
    assert!(resolver::CaptureConfiguration::from_manifest(gate_path, &manifest).is_err());
}
