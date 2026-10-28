//! Real RCC capture producer fixtures; controlled peers are not GPU/board proof.
use cpu_v3::{rcc_backend, CpuV3Sim, Device, StepOutcome};
use serde_json::{json, Value};
use std::{
    any::Any,
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
};
#[path = "../build_support/rcc_modules.rs"]
#[allow(dead_code)]
mod resolver;
const APP: &str = r#"mod gpu_pipeline;
fn main(){let mut r=renderer_open();let mut t=renderer_text(&mut r,0,0,8);
let mut f=renderer_frame(&mut r,0x1234);frame_number(&mut f,&mut t,7);
let mut p=frame_ui_pass(&mut f);pass_text(&mut p,&mut t);pass_complete(p);frame_complete(f);
renderer_close(r);if gpu_capture_read_word(0,0,0)!=0{halt(0xbad);}if gpu_capture_error()!=1{halt(0xbad);}halt(0x600d);}"#;
const GATE: &str = r#"use crate::dsl_rt::*;mod gpu_pipeline;use gpu_pipeline::*;mod gpu_capture_abi;use gpu_capture_abi::*;
pub fn profiler_gate(d:Array<u16>)->u16{
let mut i:u16=0;while i<64{signal(2,d[i]);i+=1;}
let mut r:u16=0;while r<d[GPU_CAPTURE_H_RANGE_COUNT]{
 i=0;while i<8{signal(3,gpu_capture_range_word(r,i));i+=1;}
 signal(4,gpu_capture_read_word(r,0,0));
 let low=gpu_capture_range_word(r,GPU_CAPTURE_R_BYTES);let high=gpu_capture_range_word(r,GPU_CAPTURE_R_BYTES+1);
 let mut last=(low>>1)|(high<<15);let mut upper=high>>1;
 if last==0{last=65535;upper-=1;}else{last-=1;}
 signal(5,gpu_capture_read_word(r,last,upper));r+=1;
}
signal(6,gpu_capture_error());RETURN_BODY
}"#;
#[derive(Default)]
struct State {
    active: u32,
    shadow: u32,
    format: u16,
    shadow_format: u16,
    pending: bool,
    gpu: u16,
    counts: usize,
    starts: usize,
    swaps: usize,
}
struct Peer {
    kind: u8,
    state: Rc<RefCell<State>>,
}
impl Device for Peer {
    fn read(&mut self, _: &mut [u16], c: u8) -> u16 {
        let mut s = self.state.borrow_mut();
        match (self.kind, c) {
            (3, 1) => s.active as u16,
            (3, 2) => (s.active >> 16) as u16,
            (3, 4) => s.format,
            (3, 3) => {
                if s.pending {
                    s.pending = false;
                    s.active = s.shadow;
                    s.format = s.shadow_format;
                    1
                } else {
                    0
                }
            }
            (4, 2) => {
                let value = s.gpu;
                s.gpu = 0;
                value
            }
            _ => 0,
        }
    }
    fn write(&mut self, _: &mut [u16], c: u8, v: u16) {
        let mut s = self.state.borrow_mut();
        match (self.kind, c) {
            (4, 0) => {
                assert_eq!(s.gpu, 0);
                s.counts += 1;
            }
            (4, 1) => {
                assert!(!s.pending);
                s.starts += 1;
                s.gpu = 1;
            }
            (3, 1) => s.shadow = (s.shadow & 0xffff0000) | u32::from(v),
            (3, 2) => s.shadow = (s.shadow & 65535) | (u32::from(v) << 16),
            (3, 4) => s.shadow_format = v,
            (3, 3) => {
                assert_eq!(s.gpu, 0);
                assert!(!s.pending);
                s.pending = true;
                s.swaps += 1;
            }
            _ => {}
        }
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
fn call(root: &Path, args: &[&str]) {
    let p = std::process::Command::new("python")
        .arg("-B")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        p.status.success(),
        "{}{}",
        String::from_utf8_lossy(&p.stdout),
        String::from_utf8_lossy(&p.stderr)
    );
}
fn global(cpu: &mut CpuV3Sim, p: &rcc_backend::CpuV3Program, name: &str, n: usize) -> Vec<u16> {
    let a = p
        .debug
        .globals
        .iter()
        .find_map(|g| match g.loc {
            rcc::VarLoc::Global(a) if g.name == name => Some(usize::from(a)),
            _ => None,
        })
        .unwrap();
    cpu.physical_memory_mut()[a..a + n].to_vec()
}
fn run(label: &str, profile: &str, legacy: bool, finish: &str, wrong_root: bool) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo = root.join("../..");
    let out = std::env::var_os("GPU_CAPTURE_LAYOUT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo.join("target/capture-layout"))
        .join(label);
    let assets = out.join("assets");
    std::fs::create_dir_all(&out).unwrap();
    call(
        &repo,
        &[
            "systems/cpu-v3-tang-nano-20k/examples/gpu_renderer/tools/assets.py",
            "--profile",
            profile,
            "--output",
            assets.to_str().unwrap(),
        ],
    );
    std::fs::write(out.join("main.rs"), APP).unwrap();
    std::fs::write(
        out.join("profiler_gate.rs"),
        GATE.replace("RETURN_BODY", finish),
    )
    .unwrap();
    let version = if profile == "column16" || label == "row-v2" {
        "2"
    } else {
        "1"
    };
    call(
        &repo,
        &[
            "systems/cpu-v3-tang-nano-20k/tools/capture_fixture.py",
            "--output",
            out.to_str().unwrap(),
            "--assets",
            assets.to_str().unwrap(),
            "--version",
            version,
        ],
    );
    let manifest = out.join("build.json");
    let catalog: Value =
        serde_json::from_slice(&std::fs::read(assets.join("assets.json")).unwrap()).unwrap();
    let mut config = String::from("format 2\nrenderer\napplication main.rs\n");
    for r in catalog["resources"].as_array().unwrap() {
        config.push_str(&format!(
            "copy GPU_RENDERER_{} {} {}\n",
            r["file"]
                .as_str()
                .unwrap()
                .trim_end_matches(".bin")
                .to_uppercase(),
            r["address"].as_u64().unwrap(),
            r["file"].as_str().unwrap()
        ));
    }
    let project = cpu_v3_tang_nano_20k::boot::parse_boot_project(&config).unwrap();
    let mut constants =
        project.constants() + &std::fs::read_to_string(assets.join("renderer_assets.rs")).unwrap();
    if wrong_root {
        constants = constants.replace(
            "GPU_RENDERER_TEXTURE:u16=39968;",
            "GPU_RENDERER_TEXTURE:u16=39970;",
        );
    }
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let shared = root.join("rcc");
    let mut dependencies = vec![];
    // Unselected local constants must not override the actual shared profile origin.
    std::fs::write(
        out.join("gpu_layout.rs"),
        "invalid application layout override",
    )
    .unwrap();
    let parsed = if legacy {
        let c =
            resolver::CaptureConfiguration::from_manifest(out.join("profiler_gate.rs"), &manifest)
                .unwrap();
        resolver::compile_renderer_program_with_capture(
            "capture fixture",
            APP,
            &options,
            &out,
            &shared,
            &constants,
            &[],
            Some(&c),
            &mut |p| dependencies.push(p.to_path_buf()),
        )
        .map_err(|e| e.to_string())
    } else {
        let c = resolver::RendererCaptureConfiguration::from_manifest(
            out.join("profiler_gate.rs"),
            &manifest,
        )
        .unwrap();
        assert_eq!(
            c.profile(),
            if profile == "column16" {
                resolver::RendererLayoutProfile::Column16
            } else {
                resolver::RendererLayoutProfile::Row
            }
        );
        resolver::compile_renderer_program_with_capture_profile(
            "capture fixture",
            APP,
            &options,
            &out,
            &shared,
            &constants,
            &[],
            &c,
            &out.join("profile-rcc"),
            &mut |p| dependencies.push(p.to_path_buf()),
        )
    }
    .unwrap_or_else(|e| panic!("{label}: {e}"));
    if profile == "column16" {
        let generated = out.join("profile-rcc");
        assert_eq!(
            std::fs::read(generated.join("gpu_layout.rs")).unwrap(),
            std::fs::read(shared.join("gpu_layout_column16.rs")).unwrap()
        );
        let origins: Value =
            serde_json::from_slice(&std::fs::read(generated.join("source-origins.json")).unwrap())
                .unwrap();
        assert!(origins["gpu_layout.rs"]
            .as_str()
            .unwrap()
            .ends_with("gpu_layout_column16.rs"));
        assert!(dependencies
            .iter()
            .any(|p| std::fs::canonicalize(p).unwrap()
                == std::fs::canonicalize(generated.join("gpu_pipeline.rs")).unwrap()));
        assert!(dependencies
            .iter()
            .any(|p| std::fs::canonicalize(p).unwrap()
                == std::fs::canonicalize(generated.join("gpu_layout.rs")).unwrap()));
    }
    let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
    std::fs::write(
        out.join("producer.v3bin"),
        program
            .words
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    // External plane preimages include physical padding and untouched allocation tails.
    for at in 0x400000usize / 2..0x4c0000 / 2 {
        cpu.physical_memory_mut()[at] = ((at as u32 * 17) ^ (at as u32 >> 8)) as u16;
    }
    for r in catalog["resources"].as_array().unwrap() {
        let base = r["address"].as_u64().unwrap() as usize / 2;
        let data = std::fs::read(assets.join(r["file"].as_str().unwrap())).unwrap();
        for (i, b) in data.as_chunks::<2>().0.iter().enumerate() {
            cpu.physical_memory_mut()[base + i] = u16::from_le_bytes(*b);
        }
    }
    let state = Rc::new(RefCell::new(State {
        active: 0x200000,
        ..Default::default()
    }));
    for kind in [0, 3, 4, 5, 7] {
        cpu.attach_device(
            kind,
            Box::new(Peer {
                kind,
                state: state.clone(),
            }),
        );
    }
    let mut header = vec![];
    let mut ranges = vec![];
    let mut first = vec![];
    let mut last = vec![];
    let mut samples = 0;
    let mut signal = 0;
    for _ in 0..3_000_000 {
        match cpu.step().unwrap() {
            StepOutcome::Signaled(e) => match e.signal_type {
                2 => header.push(e.value),
                3 => ranges.push(e.value),
                4 => first.push(e.value),
                5 => last.push(e.value),
                6 => {
                    assert_eq!(e.value, 0);
                    assert_eq!(
                        (
                            state.borrow().counts,
                            state.borrow().starts,
                            state.borrow().swaps
                        ),
                        (0, 0, 0)
                    );
                    assert_eq!(header.len(), 64);
                    assert_eq!(ranges.len(), usize::from(header[36]) * 8);
                    for (i, r) in ranges.as_chunks::<8>().0.iter().enumerate() {
                        let base = (u32::from(r[2]) | (u32::from(r[3]) << 16)) as usize;
                        let bytes = (u32::from(r[4]) | (u32::from(r[5]) << 16)) as usize;
                        let words = &cpu.physical_memory_mut()[base / 2..(base + bytes) / 2];
                        assert_eq!(first[i], words[0]);
                        assert_eq!(last[i], words[words.len() - 1]);
                        std::fs::write(
                            out.join(format!("range-{i}.bin")),
                            words
                                .iter()
                                .flat_map(|w| w.to_le_bytes())
                                .collect::<Vec<_>>(),
                        )
                        .unwrap();
                    }
                    samples += 1;
                }
                _ => {}
            },
            StepOutcome::Halted { signal: s } => {
                signal = s;
                break;
            }
            _ => {}
        }
    }
    assert_ne!(signal, 0, "producer watchdog");
    if finish == "0" {
        assert_eq!(signal, 0x600d);
        assert_eq!((state.borrow().starts, state.borrow().swaps), (1, 1));
        assert_eq!(global(&mut cpu, &program, "GPU_CAPTURE_CONTROL", 13)[0], 0);
    } else {
        assert!(signal >= 0xe000, "{label}: {signal:04x}");
        assert_eq!(
            (
                state.borrow().counts,
                state.borrow().starts,
                state.borrow().swaps
            ),
            (0, 0, 0)
        );
        assert_eq!(global(&mut cpu, &program, "GPU_CAPTURE_CONTROL", 13)[0], 1);
    }
    assert_eq!(cpu.data_segments(), [0, 1, 2, 3]);
    let control = global(&mut cpu, &program, "GPU_CAPTURE_CONTROL", 13);
    if wrong_root {
        assert_eq!(control[1], 6);
        assert_eq!(samples, 0);
    } else {
        assert_eq!(samples, 1);
    }
    if !header.is_empty() {
        std::fs::write(
            out.join("header.bin"),
            header
                .iter()
                .flat_map(|w| w.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        std::fs::write(
            out.join("ranges.bin"),
            ranges
                .iter()
                .flat_map(|w| w.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
    }
    std::fs::write(out.join("producer.json"),serde_json::to_vec_pretty(&json!({"status":"pass","profile":profile,"header_words":header,"range_words":ranges,"signal":signal,"control":control,"dependencies":dependencies,"samples":samples,"boundary":"Actual RCC/CPU producer and public read bridges. Host copied immutable/preimage bytes at the frozen gate; controlled GPU/Display peers, no UART or board proof."})).unwrap()).unwrap();
    out
}
#[test]
fn real_headers_keep_legacy_row_and_bind_column_physical_ranges() {
    let a = run("row-legacy", "default", true, "2", false);
    let b = run("row", "default", false, "2", false);
    let c = run("column", "column16", false, "2", false);
    run("row-v2", "default", false, "2", false);
    for name in ["header.bin", "ranges.bin"] {
        assert_eq!(
            std::fs::read(a.join(name)).unwrap(),
            std::fs::read(b.join(name)).unwrap(),
            "legacy row {name}"
        );
    }
    let d: Value =
        serde_json::from_slice(&std::fs::read(c.join("producer.json")).unwrap()).unwrap();
    let h = d["header_words"].as_array().unwrap();
    assert_eq!(
        (
            h[39].as_u64(),
            h[40].as_u64(),
            h[42].as_u64(),
            h[43].as_u64()
        ),
        (Some(2), Some(1), Some(8192), Some(3))
    );
    for r in d["range_words"].as_array().unwrap().as_chunks::<8>().0 {
        if [7, 8, 9].contains(&r[1].as_u64().unwrap()) {
            assert_eq!((r[4].as_u64(), r[5].as_u64()), (Some(8192), Some(3)));
        }
    }
}

#[test]
fn column_identity_bounds_and_lease_lifetime() {
    run("column-continue", "column16", false, "0", false);
    run("column-wrong-root", "column16", false, "2", true);
    // The first word after the physical range is allocation tail, not captured.
    run("column-end", "column16", false,
        "let i:u16=6;let low=gpu_capture_range_word(i,4);let high=gpu_capture_range_word(i,5);if gpu_capture_read_word(i,(low>>1)|(high<<15),high>>1)!=0{halt(0xbad);}if gpu_capture_error()!=2{halt(0xbad);}0", false);
}

#[test]
fn strict_profile_manifest_selection() {
    let out = run("column-manifest", "column16", false, "2", false);
    let valid: Value =
        serde_json::from_slice(&std::fs::read(out.join("build.json")).unwrap()).unwrap();
    let path = out.join("negative.json");
    let parse = |v: &Value| {
        std::fs::write(&path, serde_json::to_vec(v).unwrap()).unwrap();
        resolver::RendererCaptureConfiguration::from_manifest(out.join("profiler_gate.rs"), &path)
    };
    for field in ["physical_bytes", "allocation_bytes", "macro_pitch", "abi"] {
        let mut v = valid.clone();
        v["layout"][field] = json!(0);
        assert!(parse(&v).is_err(), "{field}");
    }
    for command in [json!(2), json!(null), json!("1")] {
        let mut v = valid.clone();
        v["command_abi"] = command;
        assert!(parse(&v).is_err());
    }
    for version in [json!(1), json!(3), json!("2")] {
        let mut v = valid.clone();
        v["version"] = version;
        assert!(parse(&v).is_err());
    }
    let mut v = valid.clone();
    v["schema"] = json!("unknown");
    assert!(parse(&v).is_err());
    let mut v = valid.clone();
    v["options"]["capture_allocation_guards"] = json!(true);
    assert!(parse(&v).is_err());
    for (field, value) in [
        ("address", json!(0x400000)),
        ("bytes", json!(0)),
        ("kind", json!("mystery")),
    ] {
        let mut v = valid.clone();
        v["resources"][0][field] = value;
        assert!(parse(&v).is_err());
    }
    let texture = valid["resources"]
        .as_array()
        .unwrap()
        .iter()
        .position(|r| r["kind"] == "texture")
        .unwrap();
    for (field, value) in [
        ("layout", json!("pot-row-macro8x8")),
        ("bytes", json!(128)),
        ("address", json!(0x4e1080)),
    ] {
        let mut v = valid.clone();
        v["resources"][texture][field] = value;
        assert!(parse(&v).is_err());
    }
    let mut v = valid.clone();
    v["resources"][texture]["address"] = v["resources"][0]["address"].clone();
    assert!(parse(&v).is_err());
    std::fs::write(&path, serde_json::to_vec(&valid).unwrap()).unwrap();
    assert!(
        resolver::CaptureConfiguration::from_manifest(out.join("profiler_gate.rs"), &path).is_err()
    );
}

#[test]
fn generated_profile_root_cannot_overwrite_authority() {
    let shared = Path::new(env!("CARGO_MANIFEST_DIR")).join("rcc");
    let original = std::fs::read(shared.join("gpu_layout.rs")).unwrap();
    assert!(resolver::materialize_renderer_sources(
        &shared,
        &shared,
        resolver::RendererLayoutProfile::Column16,
        &mut |_| {}
    )
    .is_err());
    assert_eq!(
        std::fs::read(shared.join("gpu_layout.rs")).unwrap(),
        original
    );
}
