//! Real approved graphics bodies -> rustc -> RCC -> bounded CPU V3 execution.
use cpu_v3::{rcc_backend, CpuV3Sim, Device, StepOutcome};
use rcc::frontend::rustc_check::RustcCheck;
use serde_json::json;
use std::{
    any::Any,
    cell::RefCell,
    collections::BTreeMap,
    path::{Path, PathBuf},
    rc::Rc,
};
#[path = "../../build_support/rcc_modules.rs"]
#[allow(dead_code)]
mod resolver;
const SETUP: &str = r#"let mut r=renderer_open();let z=vec3::zero();let one=fix32::from_int(1);
let mut c=renderer_camera(&mut r,z,z,fix32::from_int(200),fix32::from_int(199));
let mut mesh=renderer_mesh(&mut r);let mut model=renderer_model(&mut r,vec3::new(fix32::zero(),fix32::zero(),fix32::from_int(-3)),z,vec3::new(one,one,one));
let mut material=renderer_material(&mut r,255,255,255,false);let mut label=renderer_text(&mut r,0,0,8);"#;
const RECORD: &str = r#"let mut f=r.frame(0x1111);frame_number(&mut f,&mut label,7);let mut p=f.pass(&mut c);
p.draw(&mut mesh,&mut model,&mut material);p.draw(&mut mesh,&mut model,&mut material);pass_text(&mut p,&mut label);"#;
fn output() -> PathBuf {
    std::env::var_os("RCC_RUSTC_TEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("rcc-graphics-{}", std::process::id()))
        })
}
struct Fixture {
    out: PathBuf,
    assets: PathBuf,
    catalog: serde_json::Value,
    constants: String,
    profile: resolver::RendererLayoutProfile,
}
impl Fixture {
    fn new(label: &str) -> Self {
        Self::profile(label, resolver::RendererLayoutProfile::Row)
    }
    fn profile(label: &str, profile: resolver::RendererLayoutProfile) -> Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let out = output().join(label);
        std::fs::create_dir_all(&out).unwrap();
        let assets = out.join("assets");
        let packed = std::process::Command::new("python")
            .arg("-B")
            .arg(root.join("examples/gpu_renderer/tools/assets.py"))
            .args([
                "--profile",
                if profile == resolver::RendererLayoutProfile::Column16 {
                    "column16"
                } else {
                    "default"
                },
            ])
            .arg("--output")
            .arg(&assets)
            .output()
            .unwrap();
        assert!(
            packed.status.success(),
            "{}",
            String::from_utf8_lossy(&packed.stderr)
        );
        let catalog: serde_json::Value =
            serde_json::from_slice(&std::fs::read(assets.join("assets.json")).unwrap()).unwrap();
        let mut config = String::from("format 2\nrenderer\napplication main.rs\n");
        for resource in catalog["resources"].as_array().unwrap() {
            let file = resource["file"].as_str().unwrap();
            config.push_str(&format!(
                "copy GPU_RENDERER_{} {} {}\n",
                file.trim_end_matches(".bin").to_uppercase(),
                resource["address"].as_u64().unwrap(),
                file
            ));
        }
        let constants = cpu_v3_tang_nano_20k::boot::parse_boot_project(&config)
            .unwrap()
            .constants()
            + &std::fs::read_to_string(assets.join("renderer_assets.rs")).unwrap();
        Self {
            out,
            assets,
            catalog,
            constants,
            profile,
        }
    }
    fn parse(&self, label: &str, source: &str) -> Result<rcc::frontend::Program, String> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let shared = root.join("rcc");
        let dir = self.out.join(label);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("caller.rs"), source).unwrap();
        resolver::compile_renderer_program_checked(
            "graphics-main.rs",
            source,
            &rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0),
            &dir,
            &shared,
            &self.constants,
            &resolver::RendererCaptureConfiguration::without_capture(self.profile),
            &RustcCheck::new(dir.join("rustc")),
            &mut |_| {},
        )
    }
}
#[derive(Default)]
struct State {
    active: u32,
    shadow: u32,
    format: u16,
    shadow_format: u16,
    pending: bool,
    hold_gpu: bool,
    hold_display: bool,
    auto_display: usize,
    gpu_live: bool,
    gpu_fault: bool,
    counts: Vec<u16>,
    starts: Vec<u16>,
    swaps: usize,
    publications: usize,
    display_reads: usize,
}
struct Peer {
    kind: u8,
    state: Rc<RefCell<State>>,
}
impl Device for Peer {
    fn read(&mut self, _: &mut [u16], channel: u8) -> u16 {
        let mut s = self.state.borrow_mut();
        match (self.kind, channel) {
            (4, 2) => {
                if s.gpu_fault {
                    2
                } else if s.gpu_live && s.hold_gpu {
                    1
                } else {
                    0
                }
            }
            (3, 1) => s.active as u16,
            (3, 2) => (s.active >> 16) as u16,
            (3, 4) => s.format,
            (3, 3) => {
                s.display_reads += 1;
                if s.pending {
                    if s.auto_display > 0 {
                        s.auto_display -= 1;
                        if s.auto_display == 0 {
                            s.hold_display = false;
                        }
                    }
                    if !s.hold_display {
                        s.active = s.shadow;
                        s.format = s.shadow_format;
                        s.pending = false;
                    }
                }
                u16::from(s.pending)
            }
            _ => 0,
        }
    }
    fn write(&mut self, _: &mut [u16], channel: u8, value: u16) {
        let mut s = self.state.borrow_mut();
        match (self.kind, channel) {
            (0, 1) => s.publications += 1,
            (4, 0) => {
                assert!(!s.gpu_live || !s.hold_gpu);
                s.counts.push(value);
            }
            (4, 1) => {
                assert_eq!(s.counts.len(), s.starts.len() + 1);
                s.starts.push(value);
                s.gpu_live = true;
            }
            (3, 1) => s.shadow = (s.shadow & 0xffff0000) | u32::from(value),
            (3, 2) => s.shadow = (s.shadow & 65535) | (u32::from(value) << 16),
            (3, 4) => s.shadow_format = value,
            (3, 3) => {
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
fn global(cpu: &mut CpuV3Sim, p: &rcc_backend::CpuV3Program, name: &str, count: usize) -> Vec<u16> {
    let address = p
        .debug
        .globals
        .iter()
        .find_map(|g| match g.loc {
            rcc::VarLoc::Global(a) if g.name == name => Some(usize::from(a)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing {name}"));
    cpu.physical_memory_mut()[address..address + count].to_vec()
}
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Barriers,
    All,
    GpuTimeout,
    DisplayTimeout,
    Fault,
    Cancel,
    Freeze,
    Demo,
    Queued,
}
struct Run {
    calls: BTreeMap<String, usize>,
    starts: usize,
    swaps: usize,
    renderer: u16,
    pages: Vec<u16>,
    pipe: Vec<u16>,
}
fn execute(f: &Fixture, label: &str, source: &str, entry: &'static str, mode: Mode) -> Run {
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let parsed = f
        .parse(label, source)
        .unwrap_or_else(|e| panic!("{label}: {e}"));
    let program = rcc_backend::try_compile(parsed, &options, entry).unwrap();
    let dir = f.out.join(label);
    std::fs::write(
        dir.join("program.v3bin"),
        program
            .words
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    std::fs::write(dir.join("program.lst"), &program.listing).unwrap();
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    for resource in f.catalog["resources"].as_array().unwrap() {
        let a = resource["address"].as_u64().unwrap() as usize / 2;
        let data = std::fs::read(f.assets.join(resource["file"].as_str().unwrap())).unwrap();
        for (i, b) in data.as_chunks::<2>().0.iter().enumerate() {
            cpu.physical_memory_mut()[a + i] = u16::from_le_bytes(*b);
        }
    }
    let state = Rc::new(RefCell::new(State {
        active: 0x200000,
        hold_gpu: mode != Mode::Demo,
        hold_display: mode != Mode::Demo,
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
    let watched = [
        "pass_cleanup",
        "frame_cleanup",
        "renderer_cleanup",
        "gpu_persistent_ui_cancel",
        "gpu_persistent_ui_retire",
        "gpu_driver_start_prepared",
        "scene_camera_release",
        "scene_model_release",
        "scene_mesh_release",
        "scene_material_release",
    ];
    let entries: BTreeMap<_, _> = program
        .debug
        .functions
        .iter()
        .filter(|v| watched.contains(&v.name.as_str()))
        .map(|v| (v.addr.0 as u16, v.name.clone()))
        .collect();
    let mut calls = BTreeMap::new();
    let mut markers = vec![];
    let mut steps = 0;
    let mut halt = 0;
    let mut captured = 0;
    let limit = if matches!(mode, Mode::GpuTimeout | Mode::DisplayTimeout) {
        100_000_000
    } else {
        5_000_000
    };
    for step in 0..limit {
        steps = step + 1;
        if let Some(name) = entries.get(&cpu.pc()) {
            *calls.entry(name.clone()).or_insert(0) += 1;
        }
        match cpu.step().unwrap() {
            StepOutcome::Halted { signal } => {
                halt = signal;
                break;
            }
            StepOutcome::Signaled(e) if e.signal_type == 10 => {
                let pipe = global(&mut cpu, &program, "GPU_PIPE", 6);
                let pages = global(&mut cpu, &program, "GPU_PUI_PAGES", 8);
                let display = global(&mut cpu, &program, "GPU_DISPLAY", 8);
                let s = state.borrow();
                markers.push(json!({"marker":e.value,"steps":steps,"starts":s.starts.len(),"counts":s.counts.len(),"publications":s.publications,"swaps":s.swaps,"pipe":pipe,"pages":pages,"display":display}));
                if e.value <= 3 {
                    assert!(s.starts.is_empty() && s.counts.is_empty() && s.publications == 0);
                }
                if e.value == 2 || e.value == 3 {
                    assert_eq!(pages[0], 2);
                    assert_eq!(pipe[2], 1);
                }
                if e.value == 4 {
                    assert_eq!((s.starts.len(), s.counts.len(), s.publications), (1, 1, 1));
                    assert_eq!(pages[0], 3);
                    assert_eq!(pipe[3], 1);
                }
                if e.value == 5 {
                    assert_eq!(pipe[3], 0);
                    assert_eq!(pages[0], 0);
                    assert_ne!(display[2], 0);
                    assert!(s.pending);
                }
                if e.value == 6 {
                    assert_eq!(display[2], 0);
                    assert!(!s.pending);
                }
                if e.value == 7 && mode == Mode::GpuTimeout {
                    assert_eq!(pipe[3], 1);
                    assert_eq!(pages[0], 3);
                }
                if e.value == 7 && mode == Mode::DisplayTimeout {
                    assert_eq!(pipe[3], 0);
                    assert_eq!(pages[0], 0);
                    assert_ne!(display[2], 0);
                    assert!(s.pending);
                }
                if e.value == 7 && mode == Mode::Fault {
                    assert_eq!(pipe[0], 3);
                    assert_eq!(pages[0], 3);
                }
                if e.value == 21 {
                    assert_eq!(s.starts.len(), 1);
                    assert_eq!((pipe[2], pipe[3]), (2, 1));
                    assert_eq!((pages[0], pages[4]), (3, 2));
                }
                if e.value == 22 || e.value == 23 {
                    assert_eq!(s.starts.len(), 1);
                    assert_eq!((pipe[2], pipe[3]), (2, 0));
                    assert_eq!((pages[0], pages[4]), (0, 2));
                    assert!(s.pending);
                    assert_eq!(global(&mut cpu, &program, "GPU_RENDERER_LAST", 9)[0], 1);
                }
                if e.value == 24 {
                    assert_eq!(s.starts.len(), 2);
                    assert_eq!((pipe[2], pipe[3]), (0, 0));
                    assert_eq!((pages[0], pages[4]), (0, 0));
                    assert!(!s.pending);
                }
                drop(s);
                if e.value == 4 {
                    let mut s = state.borrow_mut();
                    if mode != Mode::GpuTimeout {
                        s.hold_gpu = false;
                    }
                    if mode == Mode::All {
                        s.auto_display = 30;
                    }
                    if mode == Mode::Fault {
                        s.gpu_fault = true;
                    }
                }
                if e.value == 5 && mode == Mode::Barriers {
                    state.borrow_mut().hold_display = false;
                }
                if e.value == 21 {
                    state.borrow_mut().hold_gpu = false;
                }
                if e.value == 23 {
                    state.borrow_mut().hold_display = false;
                }
            }
            _ => {}
        }
        let s = state.borrow();
        if s.starts.len() > captured {
            let base = usize::from(s.starts[captured]) * 64;
            let count = usize::from(s.counts[captured]) + 1;
            let commands: Vec<_> = cpu.physical_memory_mut()[base..base + count * 2]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|w| u32::from(w[0]) | (u32::from(w[1]) << 16))
                .collect();
            assert_eq!(commands.iter().filter(|c| *c >> 28 == 3).count(), 2);
            std::fs::write(
                dir.join(format!("commands-{captured}.bin")),
                commands
                    .iter()
                    .flat_map(|c| c.to_le_bytes())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
            captured += 1;
        }
    }
    assert_eq!(
        halt,
        0x600d,
        "{label}: watchdog/failure after {steps} pc={}",
        cpu.pc()
    );
    assert_eq!(cpu.data_segments(), [0, 1, 2, 3]);
    let result = Run {
        calls,
        starts: state.borrow().starts.len(),
        swaps: state.borrow().swaps,
        renderer: global(&mut cpu, &program, "GPU_RENDERER", 3)[0],
        pages: global(&mut cpu, &program, "GPU_PUI_PAGES", 8),
        pipe: global(&mut cpu, &program, "GPU_PIPE", 6),
    };
    std::fs::write(dir.join("execution.json"),serde_json::to_vec_pretty(&json!({"steps":steps,"bound":limit,"signal":halt,"program_words":program.words.len(),"calls":result.calls,"markers":markers,"starts":result.starts,"swaps":result.swaps,"renderer":result.renderer,"pages":result.pages,"pipe":result.pipe,"boundary":"Actual checked RCC/CPU library; controlled GPU/Display retirement. No pixel/RTL/cache-contention/physical timing or board claim."})).unwrap()).unwrap();
    result
}
fn app(body: &str) -> String {
    format!("mod gpu_pipeline;use gpu_pipeline::*;use crate::dsl_rt::*;fn main(){{{SETUP}{body}}}")
}
fn calls(r: &Run, n: &str) -> usize {
    r.calls.get(n).copied().unwrap_or(0)
}
#[test]
fn actual_member_finish_submit_wait_and_retirement_are_separate() {
    let f = Fixture::new("members");
    let prefix=format!("{RECORD}p.finish();signal(10,1);f.finish();signal(10,2);if r.wait(0)!=RENDERER_COMPLETED||r.wait(8)!=RENDERER_REJECTED||r.wait(WAIT_ALL)!=RENDERER_COMPLETED{{halt(0xbad);}}signal(10,3);if r.submit()!=RENDERER_PENDING{{halt(0xbad);}}signal(10,4);");
    let end = "if r.submit()!=RENDERER_REJECTED{halt(0xbad);}r.close();drop(label);drop(material);drop(model);drop(mesh);drop(c);halt(0x600d);";
    for(label,tail,mode)in[
  ("barriers","if r.wait(WAIT_GPU)!=RENDERER_COMPLETED{halt(0xbad);}signal(10,5);if r.wait(WAIT_DISPLAY)!=RENDERER_COMPLETED||r.wait(WAIT_ALL)!=RENDERER_COMPLETED{halt(0xbad);}signal(10,6);",Mode::Barriers),
  ("all","if r.wait(WAIT_ALL)!=RENDERER_COMPLETED{halt(0xbad);}signal(10,6);",Mode::All),
  ("gpu-timeout","if r.wait(WAIT_GPU)!=RENDERER_TIMEOUT{halt(0xbad);}signal(10,7);halt(0x600d);",Mode::GpuTimeout),
  ("display-timeout","if r.wait(WAIT_DISPLAY)!=RENDERER_TIMEOUT{halt(0xbad);}signal(10,7);halt(0x600d);",Mode::DisplayTimeout),
  ("fault","if r.wait(WAIT_ALL)!=RENDERER_FAULT{halt(0xbad);}signal(10,7);halt(0x600d);",Mode::Fault),
 ]{let r=execute(&f,label,&app(&(prefix.clone()+tail+if matches!(mode,Mode::Barriers|Mode::All){end}else{""})),"main",mode);assert_eq!(r.starts,1);assert_eq!(calls(&r,"pass_cleanup"),0);assert_eq!(calls(&r,"frame_cleanup"),0);if matches!(mode,Mode::Barriers|Mode::All){assert_eq!(r.swaps,1);assert_eq!(r.renderer,4);assert_eq!(calls(&r,"gpu_persistent_ui_retire"),1);assert_eq!(calls(&r,"renderer_cleanup"),0);assert_eq!(calls(&r,"scene_mesh_release"),1);}}
}
#[test]
fn actual_early_return_cancel_and_consumed_drop_are_once() {
    let f = Fixture::new("cleanup");
    let early=format!("mod gpu_pipeline;use gpu_pipeline::*;use crate::dsl_rt::*;fn early(r:&mut Renderer,c:&mut Camera,mesh:&mut Mesh,model:&mut Model,material:&mut Material){{let mut f=r.frame(0);let mut p=f.pass(c);p.draw(mesh,model,material);return;}}fn main(){{{SETUP}early(&mut r,&mut c,&mut mesh,&mut model,&mut material);if r.wait(WAIT_ALL)!=RENDERER_COMPLETED{{halt(0xbad);}}drop(r);halt(0x600d);}}");
    let r = execute(&f, "return", &early, "main", Mode::Cancel);
    assert_eq!(r.starts, 0);
    for n in [
        "pass_cleanup",
        "frame_cleanup",
        "renderer_cleanup",
        "gpu_persistent_ui_cancel",
    ] {
        assert_eq!(calls(&r, n), 1, "{n}");
    }
    assert_eq!(r.pages[0], 0);
    assert_eq!(r.renderer, 4);
    let r=execute(&f,"unfinished",&app(&format!("{RECORD}drop(p);f.finish();if r.submit()!=RENDERER_REJECTED{{halt(0xbad);}}drop(r);halt(0x600d);")),"main",Mode::Cancel);
    assert_eq!(r.starts, 0);
    assert_eq!(calls(&r, "pass_cleanup"), 1);
    assert_eq!(calls(&r, "frame_cleanup"), 1);
    assert_eq!(calls(&r, "gpu_persistent_ui_cancel"), 1);
    let r = execute(
        &f,
        "finished-drop",
        &app(&format!(
            "{RECORD}p.finish();f.finish();drop(r);halt(0x600d);"
        )),
        "main",
        Mode::Freeze,
    );
    assert_eq!(r.starts, 0);
    assert_eq!(calls(&r, "pass_cleanup"), 0);
    assert_eq!(calls(&r, "frame_cleanup"), 0);
    assert_eq!(calls(&r, "renderer_cleanup"), 1);
    assert_eq!(calls(&r, "gpu_persistent_ui_cancel"), 1);
    assert_eq!(r.pages[0], 0);
    assert_eq!(r.pipe[2], 0);
    assert_eq!(r.renderer, 4);
    let r = execute(
        &f,
        "finished-close",
        &app(&format!(
            "{RECORD}p.finish();f.finish();r.close();halt(0x600d);"
        )),
        "main",
        Mode::Cancel,
    );
    assert_eq!(r.starts, 0);
    assert_eq!(calls(&r, "gpu_persistent_ui_cancel"), 1);
    assert_eq!(r.renderer, 4);
    let body=format!("{{{RECORD}p.finish();f.finish();}}if r.submit()!=RENDERER_PENDING{{halt(0xbad);}}{{{RECORD}p.finish();f.finish();}}drop(r);halt(0x600d);");
    let r = execute(
        &f,
        "unsubmitted-drop-with-live-predecessor",
        &app(&body),
        "main",
        Mode::Freeze,
    );
    assert_eq!(r.starts, 1);
    assert_eq!((r.pages[0], r.pages[4]), (3, 0));
    assert_eq!(r.pipe[3], 1);
    assert_eq!(r.pipe[2], 0);
    assert_eq!(calls(&r, "gpu_persistent_ui_cancel"), 1);
    assert_eq!(calls(&r, "gpu_persistent_ui_retire"), 0);
    assert_eq!(r.renderer, 5);
}
#[test]
fn real_library_borrows_moves_and_parent_escape_fail_in_rustc() {
    let f = Fixture::new("negative");
    let cases=[("alias",format!("mod gpu_pipeline;fn alias(a:&mut Pass,b:&mut Pass){{}}fn main(){{{SETUP}{RECORD}alias(&mut p,&mut p);}}"),"E0499"),
 ("finished",app(&format!("{RECORD}p.finish();p.draw(&mut mesh,&mut model,&mut material);")),"E0382"),
 ("escape","mod gpu_pipeline;fn bad(r:&mut Renderer)->Frame{let mut local=renderer_open();local.frame(0)}fn main(){}".to_owned(),"E0515"),
 ("raw-unsafe","mod gpu_pipeline;fn main(){let p=Ptr::from_addr(0);p.read(0);}".to_owned(),"E0133")];
    for (label, source, code) in cases {
        let error = match f.parse(label, &source) {
            Ok(_) => panic!("{label} accepted"),
            Err(e) => e,
        };
        assert!(
            error.to_string().contains(&format!("rustc {code}")),
            "{label}: {error}"
        );
        assert!(error.contains("graphics-main.rs"));
    }
}
#[test]
fn actual_minimal_member_demo_runs_three_frames() {
    for (label, profile) in [
        ("demo", resolver::RendererLayoutProfile::Row),
        ("demo-column", resolver::RendererLayoutProfile::Column16),
    ] {
        let f = Fixture::profile(label, profile);
        let source = include_str!("../../examples/gpu_renderer/rcc/members.rs");
        let r = execute(
            &f,
            "three-frames",
            source,
            "renderer_member_three_frames",
            Mode::Demo,
        );
        assert_eq!((r.starts, r.swaps), (3, 3));
        assert_eq!(calls(&r, "gpu_persistent_ui_retire"), 3);
        assert_eq!(calls(&r, "pass_cleanup"), 0);
        assert_eq!(calls(&r, "frame_cleanup"), 0);
        assert_eq!(r.renderer, 4);
    }
}
#[test]
fn waiting_old_work_cannot_submit_the_unsubmitted_successor() {
    let f = Fixture::new("queued");
    let body=format!("{{{RECORD}p.finish();f.finish();}}if r.submit()!=RENDERER_PENDING{{halt(0xbad);}}signal(10,20);\
        {{{RECORD}p.finish();f.finish();}}signal(10,21);\
        if r.wait(WAIT_GPU)!=RENDERER_COMPLETED{{halt(0xbad);}}signal(10,22);\
        if r.submit()!=RENDERER_PENDING{{halt(0xbad);}}signal(10,23);\
        if r.wait(WAIT_ALL)!=RENDERER_COMPLETED{{halt(0xbad);}}signal(10,24);r.close();halt(0x600d);");
    let r = execute(&f, "two-real-slots", &app(&body), "main", Mode::Queued);
    assert_eq!((r.starts, r.swaps), (2, 2));
    assert_eq!(calls(&r, "gpu_persistent_ui_retire"), 2);
}
