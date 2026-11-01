//! Real public caller/CPU recording with finite, control-only device peers.
use crate::shared_build_support_rcc_modules as resolver;
use cpu_v3::{rcc_backend, CpuV3Sim, Device, StepOutcome};
use std::{
    any::Any,
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
};

fn assets(test_name: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = root
        .join("../../target/public-handheld/test-assets")
        .join(test_name);
    let result = std::process::Command::new("python")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .arg(root.join("examples/gpu_demo/tools/public_assets.py"))
        .arg("--output")
        .arg(&out)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    out
}
fn compile(source: &str, out: &Path, debug: bool) -> rcc_backend::CpuV3Program {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("assets.json")).unwrap()).unwrap();
    let mut project =
        String::from("format 2\ngpu-api\napplication examples/gpu_demo/rcc/public_main.rs\n");
    for resource in manifest["resources"].as_array().unwrap() {
        let file = resource["file"].as_str().unwrap();
        project.push_str(&format!(
            "copy GPU_RENDERER_{} {} {}\n",
            file.trim_end_matches(".bin").to_uppercase(),
            resource["address"],
            file
        ));
    }
    let constants = cpu_v3_tang_nano_20k::boot::parse_boot_project(&project)
        .unwrap()
        .constants()
        + &std::fs::read_to_string(out.join("renderer_assets.rs")).unwrap();
    let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    options.gpu_debug = debug;
    let config = resolver::RendererCaptureConfiguration::without_capture(
        resolver::RendererLayoutProfile::Column16,
    );
    let parsed = resolver::compile_explicit_gpu_program_checked(
        "Public classic controller",
        source,
        &options,
        &root.join("examples/gpu_demo/rcc"),
        &root.join("rcc"),
        &constants,
        &config,
        &rcc::frontend::rustc_check::RustcCheck::new(out.join(format!("check-{debug}"))),
        &mut |_| {},
    )
    .unwrap_or_else(|e| panic!("{e}"));
    rcc_backend::try_compile(parsed, &options, "main").unwrap()
}

#[derive(Clone, Debug)]
struct Recorded {
    triangles: usize,
    draws: usize,
    ui: usize,
    matrix: Vec<u16>,
    global: Vec<u16>,
}
fn recorded(memory: &[u16], sector: u16, count: u16) -> Recorded {
    let base = usize::from(sector) * 64;
    let mut local = [0u16; 2048];
    let mut mesh_triangles = [0; 2];
    let mut control = 0;
    let mut texture_word = 0;
    let mut result = Recorded {
        triangles: 0,
        draws: 0,
        ui: 0,
        matrix: Vec::new(),
        global: Vec::new(),
    };
    let mut ended = false;
    let mut clears = [false; 2];
    for w in memory[base..base + (usize::from(count) + 1) * 2]
        .as_chunks::<2>()
        .0
        .iter()
    {
        let code = u32::from(w[0]) | (u32::from(w[1]) << 16);
        match code >> 28 {
            1 => {
                let source = ((code >> 10) & 0x3ffff) as usize * 16;
                let dest = (((code >> 3) & 127) * 16) as usize;
                let length = (32usize << ((code >> 1) & 3)) / 2;
                local[dest..dest + length].copy_from_slice(&memory[source..source + length]);
            }
            2 => {
                let record = ((code >> 12) & 65535) as usize * 64;
                mesh_triangles[(code & 1) as usize] = usize::from(memory[record] >> 8);
            }
            3 => {
                assert_eq!(
                    texture_word,
                    if result.draws == 6 { 0 } else { 0x56 },
                    "only colored RGB is untextured; room/rabbit remain full-mip trilinear"
                );
                assert_eq!(
                    control,
                    if result.draws < 7 { 41 } else { 9 },
                    "classic material/depth policy"
                );
                result.triangles += mesh_triangles[(code & 1) as usize];
                result.draws += 1;
                if result.matrix.is_empty() {
                    result.matrix.extend_from_slice(&local[1280..1322]);
                }
            }
            5 => {
                let at = ((code & 127) * 16) as usize;
                assert_eq!(local[at + 10], 0x33, "shininess exponent32/code12");
                assert_eq!(
                    &local[at + 8..at + 10],
                    &[64, 192],
                    "original material responses"
                );
                if result.global.is_empty() {
                    result.global.extend_from_slice(&local[at..at + 16]);
                }
            }
            6 => {
                let at = ((code & 127) * 16) as usize;
                texture_word = local[at + 1];
                control = local[at + 4] & 63;
            }
            8 => {
                let depth = code & 0x10000 != 0;
                clears[usize::from(depth)] = true;
                assert_eq!(
                    code & 65535,
                    if depth { 65535 } else { 0 },
                    "original color/depth clear"
                );
            }
            11 => result.ui += 1,
            15 => {
                ended = true;
                break;
            }
            _ => {}
        }
    }
    assert!(ended);
    assert_eq!(clears, [true, true]);
    assert_eq!((result.draws, result.triangles), (24, 1141));
    result
}

#[derive(Default)]
struct State {
    us: u64,
    clear: u64,
    last_us_sample: u16,
    prep_ticks: u64,
    phase: u16,
    uart_until: u64,
    active: u32,
    shadow: u32,
    format: u16,
    shadow_format: u16,
    pending: bool,
    present_until: u64,
    present_delay: u64,
    gpu_until: u64,
    gpu_delay: u64,
    command_count: u16,
    frames: Vec<Recorded>,
    swaps: Vec<u32>,
    retired: Vec<u64>,
    observed: Vec<u16>,
    input_reads: Vec<(usize, u64)>,
    telemetry: Vec<u8>,
    uart_phases: Vec<u16>,
    analog: bool,
    digital: bool,
    input_latched: bool,
    hide: bool,
}
struct Peer(u8, Rc<RefCell<State>>);
impl Device for Peer {
    fn read(&mut self, _: &mut [u16], channel: u8) -> u16 {
        let mut s = self.1.borrow_mut();
        if self.0 == 3 && s.pending && s.us >= s.present_until {
            s.active = s.shadow;
            s.format = s.shadow_format;
            s.pending = false;
            let now = s.us;
            s.retired.push(now);
        }
        match (self.0, channel) {
            (0, 3) => u16::from(s.us < s.uart_until),
            (0, 8) => u16::from(s.hide && !s.swaps.is_empty()),
            (0, 10) => (s.us / 1000) as u16,
            (0, 11) => {
                let sample = (s.us - s.clear).min(65535) as u16;
                s.last_us_sample = sample;
                sample
            }
            (3, 0) => (s.us / 1000) as u16, // Can change before old reads retire.
            (3, 1) => s.active as u16,
            (3, 2) => (s.active >> 16) as u16,
            (3, 3) => u16::from(s.pending),
            (3, 4) => s.format,
            (4, 2) => u16::from(!s.frames.is_empty() && s.us < s.gpu_until),
            (5, 8) => {
                let frame = s.swaps.len();
                let now = s.us;
                if s.input_reads.last().is_none_or(|(f, _)| *f != frame) {
                    s.input_reads.push((frame, now));
                }
                if !s.analog && !s.digital {
                    0
                } else if s.input_latched {
                    1
                } else {
                    3
                }
            }
            (5, 2) => {
                if s.digital {
                    0x41
                } else {
                    0x73
                }
            }
            (5, 3) => 0x5a,
            (5, 4) => 255,
            (5, 5) => 247, // Active-low R1.
            (5, 6) => 255,
            (5, 7) => 0,
            (5, 0) => 255,
            (5, 1) => 128,
            _ => 0,
        }
    }
    fn write(&mut self, memory: &mut [u16], channel: u8, value: u16) {
        let mut s = self.1.borrow_mut();
        match (self.0, channel) {
            (0, 11) => {
                s.clear = s.us;
                s.prep_ticks = 0;
            }
            (0, 3) => {
                assert!(s.us >= s.uart_until);
                s.uart_until = s.us + 87;
                let phase = s.phase;
                s.uart_phases.push(phase);
                s.telemetry.push(value as u8);
            }
            (4, 0) => s.command_count = value,
            (4, 1) => {
                assert!(!s.pending, "next START before Display retirement");
                let r = recorded(memory, value, s.command_count);
                s.frames.push(r);
                s.gpu_until = s.us + s.gpu_delay;
            }
            (3, 1) => s.shadow = (s.shadow & 0xffff0000) | u32::from(value),
            (3, 2) => s.shadow = (s.shadow & 65535) | (u32::from(value) << 16),
            (3, 4) => {
                assert_eq!(value, 1);
                s.shadow_format = value;
            }
            (3, 3) => {
                assert!(!s.pending);
                assert!(s.us >= s.gpu_until);
                assert_eq!(s.frames.len(), s.swaps.len() + 1);
                let target = s.shadow;
                s.swaps.push(target);
                s.pending = true;
                s.present_until = s.us + s.present_delay;
            }
            (5, 8) => s.input_latched = true,
            (7, 1) => s.phase = value,
            (7, 4) => {
                assert!(s.us >= s.gpu_until && !s.pending);
                assert_eq!(value, s.last_us_sample, "rendering interval is locked at the Device0 read, not later trace transmission");
                s.observed.push(value);
            }
            _ => {}
        }
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn run(program: &rcc_backend::CpuV3Program, assets: &Path, mut state: State, signal: u16) -> State {
    state.active = 0x200000; // Device3 addresses count halfwords.
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(assets.join("assets.json")).unwrap()).unwrap();
    for r in manifest["resources"].as_array().unwrap() {
        let base = r["address"].as_u64().unwrap() as usize / 2;
        let data = std::fs::read(assets.join(r["file"].as_str().unwrap())).unwrap();
        for (i, b) in data.as_chunks::<2>().0.iter().enumerate() {
            cpu.physical_memory_mut()[base + i] = u16::from_le_bytes([b[0], b[1]]);
        }
    }
    let shared = Rc::new(RefCell::new(state));
    for kind in [0, 3, 4, 5, 7] {
        cpu.attach_device(kind, Box::new(Peer(kind, shared.clone())));
    }
    let mut halted = false;
    for _ in 0..8_000_000 {
        {
            let mut s = shared.borrow_mut();
            if s.phase == 13 || s.phase == 14 {
                s.us += 10;
            } else {
                s.prep_ticks += 1;
                if s.prep_ticks == 100 {
                    s.us += 1;
                    s.prep_ticks = 0;
                }
            }
        }
        if let StepOutcome::Halted { signal: got } = cpu.step().unwrap() {
            assert_eq!(
                got,
                signal,
                "pc={} phase={} packets={:?}",
                cpu.pc(),
                shared.borrow().phase,
                shared.borrow().telemetry
            );
            halted = true;
            break;
        }
    }
    assert!(
        halted,
        "bounded public demo timed out at pc={} phase={}",
        cpu.pc(),
        shared.borrow().phase
    );
    drop(cpu);
    Rc::try_unwrap(shared).ok().unwrap().into_inner()
}

#[test]
fn actual_public_classic_input_recording_retirement_and_deadlines_both_profiles() {
    let out = assets("caller");
    let reference: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/public_handheld_phase0.json")).unwrap();
    // The accepted static caller uses S16 for unlit and S32 for the rabbit.
    // This controller instead preserves the native caller's S32 throughout.
    assert_eq!(reference["unlit_global"][10], 0x23);
    assert_eq!(reference["lit_global"][10], 0x33);
    for code in [8, 12] {
        use gpu_v2::lighting::{ports::*, sim::counted};
        let result = counted::evaluate(
            PixelInput {
                normal: [0, 0, 16384],
                ndc: [0, 0],
            },
            Material {
                unlit: true,
                shininess_code: code,
                specular_color: [160; 3],
            },
            Light::default(),
            Projection::default(),
            100_000,
        )
        .unwrap();
        assert_eq!(result.output, LightingOutput { g: 256, h: 0 });
    }
    let source = include_str!("../../examples/gpu_demo/rcc/public_main.rs")
        .replace("demo_run(0, 2000);", "demo_run(3, 2000);");
    assert!(source.contains("demo_run(3, 2000);"));
    let source = source.replace(
        "let mut display = Display::open_with_format(DisplayFormat::LinearRgb565);",
        "let mut display = Display::open_with_format(DisplayFormat::LinearRgb565);if !display.try_wait_swap(){halt(0xbad);}",
    );
    for debug in [false, true] {
        let program = compile(&source, &out, debug);
        let baseline = run(
            &program,
            &out,
            State {
                gpu_delay: 5000,
                present_delay: 40000,
                ..State::default()
            },
            0x600d,
        );
        assert_eq!(
            baseline.frames[0].matrix,
            serde_json::from_value::<Vec<u16>>(reference["phase0_instance"].clone()).unwrap(),
            "exact accepted public static phase0 instance"
        );
        std::fs::write(out.join(format!("neutral-{debug}.json")), serde_json::to_vec_pretty(&serde_json::json!({"matrix":baseline.frames[0].matrix,"global":baseline.frames[0].global,"observed":baseline.observed,"swaps":baseline.swaps})).unwrap()).unwrap();
        assert_eq!(baseline.swaps, vec![0x220000, 0x200000, 0x220000]);
        assert_eq!(
            baseline.frames.iter().map(|f| f.ui).collect::<Vec<_>>(),
            vec![0, 1, 1]
        );
        for pair in baseline.input_reads.windows(2) {
            assert!(pair[1].1 >= baseline.retired[pair[0].0]);
        }
        assert!(baseline
            .uart_phases
            .iter()
            .take(22)
            .all(|p| *p == 13 || *p == 14));
        let slow = run(
            &program,
            &out,
            State {
                gpu_delay: 5000,
                present_delay: 90000,
                ..State::default()
            },
            0x600d,
        );
        assert_eq!(
            baseline.observed, slow.observed,
            "presentation excluded from interval"
        );
        let moved = run(
            &program,
            &out,
            State {
                gpu_delay: 5000,
                present_delay: 40000,
                analog: true,
                ..State::default()
            },
            0x600d,
        );
        assert_ne!(
            moved.frames[0].matrix, moved.frames[1].matrix,
            "held input without another packet integrates"
        );
        assert_ne!(
            moved.frames[0].global, moved.frames[1].global,
            "camera-space light follows view"
        );
        let digital = run(
            &program,
            &out,
            State {
                gpu_delay: 5000,
                present_delay: 40000,
                digital: true,
                ..State::default()
            },
            0x600d,
        );
        assert_ne!(
            digital.frames[0].matrix, digital.frames[1].matrix,
            "digital R1 still moves vertically"
        );
        assert_eq!(
            digital.frames[1].global, baseline.frames[1].global,
            "digital axes remain neutral"
        );
        let hidden = run(
            &program,
            &out,
            State {
                gpu_delay: 5000,
                present_delay: 40000,
                hide: true,
                ..State::default()
            },
            0x600d,
        );
        assert_eq!(
            hidden.frames.iter().map(|f| f.ui).collect::<Vec<_>>(),
            vec![0, 1, 0]
        );
        let saturated = run(
            &program,
            &out,
            State {
                gpu_delay: 70000,
                present_delay: 40000,
                ..State::default()
            },
            0x600d,
        );
        assert!(saturated.frames.iter().all(|f| f.ui == 0));
        let gpu_timeout = run(
            &program,
            &out,
            State {
                gpu_delay: 3_000_000,
                ..State::default()
            },
            0xd00a,
        );
        assert!(gpu_timeout.swaps.is_empty());
        let display_timeout = run(
            &program,
            &out,
            State {
                gpu_delay: 5000,
                present_delay: 3_000_000,
                ..State::default()
            },
            0xd00c,
        );
        assert_eq!(display_timeout.frames.len(), 1);
        assert!(display_timeout.retired.is_empty());
    }
}

#[test]
fn real_rcc_motion_preserves_classic_initial_pose_controls_and_limits() {
    let out = assets("motion");
    let source = r#"
mod gpu_api;use gpu_api::*;mod gamepad;use gamepad::*;mod motion;use motion::*;
static OUT:Buf<u16,120>=Buf::new([0;120]);
fn save(row:u16,p:DemoMotion){let at=row*10;
OUT[at]=p.eye.x.lo_bits();OUT[at+1]=p.eye.x.hi_bits();
OUT[at+2]=p.eye.y.lo_bits();OUT[at+3]=p.eye.y.hi_bits();
OUT[at+4]=p.eye.z.lo_bits();OUT[at+5]=p.eye.z.hi_bits();
OUT[at+6]=p.yaw.lo_bits();OUT[at+7]=p.yaw.hi_bits();
OUT[at+8]=p.pitch.lo_bits();OUT[at+9]=p.pitch.hi_bits();}
fn pad(x:fix32,y:fix32,rx:fix32,ry:fix32,flags:u16)->GamepadFrame{
GamepadFrame{connected:flags&1,id:115,buttons:flags&3072,pressed:0,released:0,
left_x:x,left_y:y,right_x:rx,right_y:ry}}
fn main(){let z=fix32::zero();let one=fix32::from_int(1);
let mut p=demo_motion_init(65530);save(0,p);
let mut n:u16=1;while n<12{
let mut x=z;let mut y=z;let mut rx=z;let mut ry=z;let mut flags:u16=1;
let mut now=(n-1)*100+4;
if n==2{y=-one;}if n==3{x=one;}if n==4{flags=1025;}
if n==5{flags=3073;}if n==6{rx=one;ry=-one;}
if n==7{x=one;y=-one;}if n==8{x=one;y=one;rx=one;ry=one;flags=2048;}
if n==9{p.yaw=fix32::from_words(8585,3);p.pitch=fix32::from_words(32113,1);
rx=one;ry=-one;now=1704;}
if n==10{p.eye=Vec3{x:fix32::from_int(16),y:fix32::from_int(16),z:fix32::from_int(16)};
flags=2049;now=1804;}
if n==11{p=demo_motion_init(65530);y=-one;now=4;}
p=demo_motion_update(p,pad(x,y,rx,ry,flags),now);save(n,p);n+=1;}
halt(0x600d);}
"#;
    for debug in [false, true] {
        let p = compile(source, &out, debug);
        let address = p
            .debug
            .globals
            .iter()
            .find_map(|g| {
                if g.name == "OUT" {
                    if let rcc::VarLoc::Global(at) = g.loc {
                        return Some(usize::from(at));
                    }
                }
                None
            })
            .unwrap();
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&p.words);
        assert!(matches!(
            cpu.run(2_000_000).unwrap(),
            cpu_v3::RunOutcome::Halted { signal: 0x600d, .. }
        ));
        let word = &cpu.physical_memory_mut()[address..address + 120];
        let values: Vec<[f64; 5]> = word
            .as_chunks::<10>()
            .0
            .iter()
            .map(|row| {
                std::array::from_fn(|j| {
                    let bits = u32::from(row[2 * j]) | u32::from(row[2 * j + 1]) << 16;
                    f64::from(bits as i32) / 65536.
                })
            })
            .collect();
        assert_eq!(
            values[0], values[1],
            "neutral input keeps exact initial codes"
        );
        let initial = [
            0.,
            14418. / 65536.,
            2. + 52429. / 65536.,
            0.,
            -5604. / 65536.,
        ];
        assert_eq!(values[0], initial);
        let near = |got: f64, want: f64| assert!((got - want).abs() < 0.0005, "{got} vs {want}");
        near(values[2][2], initial[2] - 0.2);
        near(values[3][0], -0.2);
        near(values[4][1], initial[1] - 0.2);
        assert_eq!(values[4], values[5], "both shoulders cancel");
        near(values[6][3], 0.3);
        near(values[6][4], initial[4] + 0.3);
        near(
            values[7][0],
            values[6][0] - 0.2 * 0.3f64.cos() - 0.2 * 0.3f64.sin(),
        );
        near(
            values[7][2],
            values[6][2] + 0.2 * 0.3f64.sin() - 0.2 * 0.3f64.cos(),
        );
        assert_eq!(values[7], values[8], "disconnect stops motion");
        near(
            values[9][3],
            3. + 8585. / 65536. + 0.3 - 2. * std::f64::consts::PI,
        );
        assert_eq!(values[9][4], 1.5);
        assert_eq!(values[10][1], 16.);
        near(values[11][2], initial[2] - 0.02);
    }
}

#[test]
fn public_camera_view_uses_the_expected_moving_pose_basis() {
    let out = assets("view");
    let source = r#"
mod gpu_api;use gpu_api::*;mod gpu_demo_api;use gpu_demo_api::*;
static OUT:Buf<u16,600>=Buf::new([0;600]);
fn row(at:u16,v:Vec4){OUT[at]=v.x.lo_bits();OUT[at+1]=v.x.hi_bits();
OUT[at+2]=v.y.lo_bits();OUT[at+3]=v.y.hi_bits();
OUT[at+4]=v.z.lo_bits();OUT[at+5]=v.z.hi_bits();
OUT[at+6]=v.w.lo_bits();OUT[at+7]=v.w.hi_bits();}
fn main(){let gpu=Gpu::init(demo_config());let mut camera=demo_camera();
let eye=demo_point(fix32::zero(),fix32::from_words(14418,0),fix32::from_words(52429,2));
let mut i:u16=0;while i<5{let mut j:u16=0;while j<5{
let yaw=fix32::from_int((i as i16)-2)*fix32::from_words(32768,0);
let pitch=fix32::from_int((j as i16)-2)*fix32::from_words(16384,0);
camera.set_pose(eye,demo_point(pitch,yaw,fix32::zero()));let view=camera.view();
let at=(i*5+j)*24;row(at,view.row0);row(at+8,view.row1);row(at+16,view.row2);
j+=1;}i+=1;}halt(0x600d);}
"#;
    for debug in [false, true] {
        let p = compile(source, &out, debug);
        let at = p
            .debug
            .globals
            .iter()
            .find_map(|g| match (&*g.name, &g.loc) {
                ("OUT", rcc::VarLoc::Global(at)) => Some(usize::from(*at)),
                _ => None,
            })
            .unwrap();
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&p.words);
        let shared = Rc::new(RefCell::new(State {
            active: 0x200000,
            ..State::default()
        }));
        for kind in [0, 3, 4] {
            cpu.attach_device(kind, Box::new(Peer(kind, shared.clone())));
        }
        assert!(matches!(
            cpu.run(4_000_000).unwrap(),
            cpu_v3::RunOutcome::Halted { signal: 0x600d, .. }
        ));
        let words = &cpu.physical_memory_mut()[at..at + 600];
        for i in 0..5 {
            for j in 0..5 {
                let yaw = (i as f64 - 2.) * 0.5;
                let pitch = (j as f64 - 2.) * 0.25;
                let (sy, cy) = yaw.sin_cos();
                let (sp, cp) = pitch.sin_cos();
                // Camera view uses positive right; native projected X reverses it.
                let basis = [
                    [cy, 0., -sy],
                    [sy * sp, cp, cy * sp],
                    [sy * cp, -sp, cy * cp],
                ];
                let eye = [0., 14418. / 65536., 2. + 52429. / 65536.];
                for (r, basis_row) in basis.iter().enumerate() {
                    let translation = -(0..3).map(|k| basis_row[k] * eye[k]).sum::<f64>();
                    let expected_row = [basis_row[0], basis_row[1], basis_row[2], translation];
                    for (c, &expected) in expected_row.iter().enumerate() {
                        let index = (i * 5 + j) * 24 + r * 8 + c * 2;
                        let got = f64::from(
                            (u32::from(words[index]) | u32::from(words[index + 1]) << 16) as i32,
                        ) / 65536.;
                        assert!(
                            (got - expected).abs() < 0.0005,
                            "pose({i},{j})[{r},{c}]: {got} vs {expected}"
                        );
                    }
                }
            }
        }
        assert!(
            shared.borrow().frames.is_empty(),
            "math qualification never submits GPU"
        );
    }
}
