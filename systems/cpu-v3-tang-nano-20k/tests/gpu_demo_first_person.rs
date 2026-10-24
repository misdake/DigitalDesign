//! Bounded production RCC control and independent floating camera reference.
use cpu_v3::{rcc_backend, CpuV3Sim, PhysicalWordAddress, StepOutcome};
use std::path::Path;
#[path = "../build_support/rcc_modules.rs"]
mod resolver;

fn compile(source: &str, constants: &str, entry: &'static str) -> rcc_backend::CpuV3Program {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let ir =
        rcc::frontend::compile_program_named("First-person demo", source, &options, &mut |name| {
            resolver::load_module(
                &root.join("examples/gpu_demo/rcc"),
                &root.join("rcc"),
                constants,
                name,
                &mut |_| {},
            )
        })
        .unwrap();
    rcc_backend::try_compile(ir, &options, entry).unwrap()
}
fn address(p: &rcc_backend::CpuV3Program, name: &str) -> u32 {
    p.debug
        .globals
        .iter()
        .find_map(|v| match v.loc {
            rcc::VarLoc::Global(a) if v.name == name => Some(u32::from(a)),
            _ => None,
        })
        .unwrap()
}
fn read(cpu: &CpuV3Sim, at: u32) -> u16 {
    cpu.physical_memory(PhysicalWordAddress::new(at))
}
fn pair(cpu: &CpuV3Sim, at: u32, index: u32) -> i32 {
    (u32::from(read(cpu, at + 2 * index)) | u32::from(read(cpu, at + 2 * index + 1)) << 16) as i32
}
fn camera_source(body: &str) -> String {
    let scene: serde_json::Value =
        serde_json::from_str(include_str!("../examples/gpu_demo/assets/scene.json")).unwrap();
    let c = &scene["cameras"][0];
    let mut words = Vec::new();
    for v in c["matrix"].as_array().unwrap() {
        let v = v.as_i64().unwrap() as u32;
        words.extend([v as u16, (v >> 16) as u16]);
    }
    words.extend(
        c["light"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_i64().unwrap() as u16),
    );
    format!(
        r#"mod camera;
static C:Buf<u16,12>=Buf::new([0;12]);static I:Buf<u16,10>=Buf::new([0;10]);
static M:Buf<u16,45>=Buf::new({words:?});
fn check(ok:bool){{if !ok{{halt(0xbad);}}}}
fn reset(now:u16){{demo_camera_reset(C.as_array(),now);I[0u16]=1;I[1u16]=115;I[2u16]=0;
I[3u16]=128;I[4u16]=128;I[5u16]=128;I[6u16]=128;}}
fn step(now:u16){{check(demo_camera_update(M.as_array(),C.as_array(),I.as_array(),now));}}
fn main(){{{body} halt(0x600d);}}"#
    )
}
#[test]
fn controls_preserve_phase_zero_move_horizontally_and_bound_time() {
    let source = camera_source(
        r#"
reset(0);let x=gpu_fload(M.as_array(),0);let z=gpu_fload(M.as_array(),11);
step(20);check(gpu_fload(M.as_array(),0)==x&&gpu_fload(M.as_array(),11)==z);
// Full forward, then strafe: pitch does not change world height.
reset(0);I[6u16]=0;step(100);check(gpu_fload(C.as_array(),2)<fix32::from_words(52429,2));
check(gpu_fload(C.as_array(),1)==fix32::from_words(14418,0));
let capped=gpu_fload(C.as_array(),2);reset(0);I[6u16]=0;step(1000);check(gpu_fload(C.as_array(),2)==capped);
reset(0);I[5u16]=255;step(100);check(gpu_fload(C.as_array(),0)<fix32::zero());
reset(0);I[5u16]=0;step(100);check(gpu_fload(C.as_array(),0)>fix32::zero());
reset(0);I[6u16]=255;step(100);check(gpu_fload(C.as_array(),2)>fix32::from_words(52429,2));
reset(0);I[2u16]=3072;step(100);check(gpu_fload(C.as_array(),1)==fix32::from_words(14418,0));
I[2u16]=1024;step(200);check(gpu_fload(C.as_array(),1)<fix32::from_words(14418,0));
reset(0);I[2u16]=2048;step(100);check(gpu_fload(C.as_array(),1)>fix32::from_words(14418,0));
// Disconnected/digital sticks cannot move; reconnect uses the updated time.
reset(0);I[0u16]=0;I[6u16]=0;step(60000);I[0u16]=1;step(60001);
check(gpu_fload(C.as_array(),2)>fix32::from_words(50000,2));
reset(0);I[1u16]=65;I[6u16]=0;I[3u16]=255;step(100);check(gpu_fload(C.as_array(),3)==fix32::zero());
reset(65530);I[6u16]=0;step(44);check(gpu_fload(C.as_array(),2)<fix32::from_words(52429,2));
// Yaw/pitch controls and limits, including long wall time and diagonal input.
reset(0);I[3u16]=255;I[4u16]=0;let mut t:u16=100;
while t<=3000{step(t);t+=100;}
check(gpu_fload(C.as_array(),4)==fix32::from_words(32768,1));
check(gpu_fload(C.as_array(),3).abs()<=fix32::from_words(9279,3));
I[4u16]=255;while t<=6000{step(t);t+=100;}
check(gpu_fload(C.as_array(),4)==-fix32::from_words(32768,1));
I[3u16]=128;I[4u16]=128;I[6u16]=0;let height=gpu_fload(C.as_array(),1);step(6100);
check(gpu_fload(C.as_array(),1)==height);
reset(0);I[5u16]=255;I[6u16]=0;step(100);let px=gpu_fload(C.as_array(),0);let pz=gpu_fload(C.as_array(),2)-fix32::from_words(52429,2);
check(px.abs()>fix32::from_words(8500,0)&&px.abs()<fix32::from_words(10000,0));
check(pz.abs()>fix32::from_words(8500,0)&&pz.abs()<fix32::from_words(10000,0));
"#,
    );
    let p = compile(&source, "pub const DEMO_CAMERA_FIRST:u16=0;", "main");
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&p.words);
    assert!(matches!(
        cpu.run(3_000_000).unwrap(),
        cpu_v3::RunOutcome::Halted { signal: 0x600d, .. }
    ));
}

#[test]
fn matrix_and_light_match_classic_basis_across_yaw_pitch_grid() {
    let source = camera_source(
        r#"
reset(0);I[3u16]=255;
let mut i:u16=0;while i<64{
gpu_fstore(C.as_array(),3,fix32::from_int((i as i16)-32)*fix32::from_words(6434,0));
gpu_fstore(C.as_array(),4,fix32::from_int(((i&7) as i16)-4)*fix32::from_words(19661,0));
step((i+1)*100);signal(1,i);i+=1;}
"#,
    );
    let p = compile(&source, "pub const DEMO_CAMERA_FIRST:u16=0;", "main");
    let (c, m) = (address(&p, "C"), address(&p, "M"));
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&p.words);
    let mut count = 0;
    for _ in 0..5_000_000 {
        match cpu.step().unwrap() {
            StepOutcome::Signaled(_) => {
                let yaw = f64::from(pair(&cpu, c, 3)) / 65536.;
                let pitch = f64::from(pair(&cpu, c, 4)) / 65536.;
                let (sy, cy) = yaw.sin_cos();
                let (sp, cp) = pitch.sin_cos();
                let basis = [
                    [-cy, 0., sy],
                    [sy * sp, cp, cy * sp],
                    [sy * cp, -sp, cy * cp],
                ];
                let eye: [f64; 3] =
                    std::array::from_fn(|j| f64::from(pair(&cpu, c, j as u32)) / 65536.);
                for row in 0..3 {
                    for col in 0..3 {
                        let got = f64::from(pair(&cpu, m, 12 + row * 3 + col)) / 65536.;
                        let expected = basis[row as usize][col as usize];
                        assert!(
                            (got - expected).abs() < 0.0005,
                            "basis {count} {row} {col}: {got} {expected}"
                        );
                        let k = if row < 2 { 200. / 256. } else { 1. };
                        let sign = if row == 0 { 1. } else { -1. };
                        assert!(
                            (f64::from(pair(&cpu, m, row * 4 + col)) / 65536.
                                - expected * k * sign)
                                .abs()
                                < 0.0005
                        );
                    }
                    let dot: f64 = (0..3).map(|j| basis[row as usize][j] * eye[j]).sum();
                    let expected = dot
                        * if row == 0 {
                            -200. / 256.
                        } else if row == 1 {
                            200. / 256.
                        } else {
                            1.
                        };
                    assert!(
                        (f64::from(pair(&cpu, m, row * 4 + 3)) / 65536. - expected).abs() < 0.001
                    );
                }
                let norm = (0.45f64.powi(2) + 0.8f64.powi(2) + 1.1f64.powi(2)).sqrt();
                let world = [-0.45 / norm, 0.8 / norm, 1.1 / norm];
                let mut squared = 0i64;
                for row in 0..3 {
                    let got = read(&cpu, m + 42 + row) as i16;
                    let expected = (0..3)
                        .map(|j| basis[row as usize][j] * world[j])
                        .sum::<f64>()
                        * 16384.;
                    assert!(
                        (f64::from(got) - expected).abs() < 8.,
                        "light {count}: {got} {expected}"
                    );
                    squared += i64::from(got).pow(2);
                }
                assert!((squared - (1 << 28)).abs() <= 32768);
                count += 1;
            }
            StepOutcome::Halted { signal } => {
                assert_eq!(signal, 0x600d);
                assert_eq!(count, 64);
                return;
            }
            _ => {}
        }
    }
    panic!("bounded first-person grid did not finish");
}

#[derive(Default)]
struct TimingState {
    us: u64,
    clear: u64,
    active: u32,
    shadow: u32,
    format: u16,
    shadow_format: u16,
    gpu_until: u64,
    present_until: u64,
    pending: bool,
    gpu_delay: u64,
    present_delay: u64,
    phase: u16,
    fail_gpu: bool,
    starts: usize,
    swaps: usize,
    captured: Vec<u16>,
    telemetry: Vec<u8>,
}
struct TimingPeer {
    kind: u8,
    state: std::rc::Rc<std::cell::RefCell<TimingState>>,
}
impl cpu_v3::Device for TimingPeer {
    fn read(&mut self, _: &mut [u16], ch: u8) -> u16 {
        let mut s = self.state.borrow_mut();
        if self.kind == 3 && s.pending && s.us >= s.present_until {
            s.active = s.shadow;
            s.format = s.shadow_format;
            s.pending = false;
        }
        match (self.kind, ch) {
            (0, 10) => (s.us / 1000) as u16,
            (0, 11) => (s.us - s.clear).min(65535) as u16,
            (3, 0) => (s.us / 16667) as u16,
            (3, 1) => s.active as u16,
            (3, 2) => (s.active >> 16) as u16,
            (3, 3) => u16::from(s.pending),
            (3, 4) => s.format,
            (4, 2) => {
                if s.us < s.gpu_until {
                    1
                } else if s.fail_gpu && s.starts > 0 {
                    2
                } else {
                    0
                }
            }
            // Input is disconnected; this control test has no serial timing claim.
            _ => 0,
        }
    }
    fn write(&mut self, _: &mut [u16], ch: u8, value: u16) {
        let mut s = self.state.borrow_mut();
        match (self.kind, ch) {
            (0, 11) if value & 1 != 0 => s.clear = s.us,
            (0, 3) => s.telemetry.push(value as u8),
            (4, 1) => {
                s.starts += 1;
                s.gpu_until = s.us + s.gpu_delay;
            }
            (3, 1) => s.shadow = (s.shadow & 0xffff0000) | u32::from(value),
            (3, 2) => s.shadow = (s.shadow & 65535) | (u32::from(value) << 16),
            (3, 4) => s.shadow_format = value,
            (3, 3) => {
                assert!(!s.pending);
                assert_eq!(s.captured.len(), s.swaps + 1);
                s.pending = true;
                s.swaps += 1;
                s.present_until = s.us + s.present_delay;
            }
            (7, 1) => s.phase = value,
            (7, 4) => {
                assert!(s.us >= s.gpu_until && !s.pending);
                assert_eq!(value, (s.us - s.clear).min(65535) as u16);
                s.captured.push(value);
            }
            _ => {}
        }
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn timing_run(present_delay: u64, gpu_delay: u64, fail: bool) -> TimingState {
    static ASSETS: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    let assets = ASSETS.get_or_init(|| {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let out = root.join("../../target/gpu-first-person-assets");
        let result = std::process::Command::new("python")
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .arg(root.join("examples/gpu_demo/tools/assets.py"))
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
    });
    let constants = std::fs::read_to_string(assets.join("demo_assets.rs"))
        .unwrap()
        .replace("DEMO_ROOM_COUNT: u16 = 6;", "DEMO_ROOM_COUNT: u16 = 1;")
        .replace("DEMO_BUNNY_COUNT: u16 = 17;", "DEMO_BUNNY_COUNT: u16 = 2;");
    let p = compile(
        include_str!("../examples/gpu_demo/rcc/main.rs"),
        &constants,
        "demo_native_main",
    );
    let stats = address(&p, "STATS");
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&p.words);
    for (name, base) in [
        ("room.bin", 0x4c0000u32),
        ("atlas.bin", 0x4b0000),
        ("texture.bin", 0x4d0000),
    ] {
        let bytes = std::fs::read(assets.join(name)).unwrap();
        for (i, b) in bytes.chunks(2).enumerate() {
            cpu.physical_memory_mut()[base as usize / 2 + i] =
                u16::from(b[0]) | (u16::from(*b.get(1).unwrap_or(&0)) << 8);
        }
    }
    let state = std::rc::Rc::new(std::cell::RefCell::new(TimingState {
        active: 0x200000,
        present_delay,
        gpu_delay,
        fail_gpu: fail,
        ..TimingState::default()
    }));
    for kind in [0, 3, 4, 5, 7] {
        cpu.attach_device(
            kind,
            Box::new(TimingPeer {
                kind,
                state: state.clone(),
            }),
        );
    }
    let mut done = false;
    for step in 0..6_000_000 {
        state.borrow_mut().us = step / 10;
        if let StepOutcome::Halted { signal } = cpu.step().unwrap() {
            assert_eq!(signal, if fail { 0xd00a } else { 0x600d });
            done = true;
            break;
        }
    }
    assert!(done,"bounded demo control run did not halt pc={} us={} starts={} swaps={} phase={} captured={:?}",cpu.pc(),state.borrow().us,state.borrow().starts,state.borrow().swaps,state.borrow().phase,state.borrow().captured);
    if fail || gpu_delay >= 65535 {
        assert_eq!(read(&cpu, stats + 3), 0);
    } else {
        assert_eq!(read(&cpu, stats + 1), state.borrow().captured[0]);
        assert_eq!(read(&cpu, stats + 3), 1);
    }
    drop(cpu);
    std::rc::Rc::try_unwrap(state).ok().unwrap().into_inner()
}

#[test]
fn actual_frame_timer_excludes_present_and_rejects_failure_or_saturation() {
    let fast = timing_run(500, 2000, false);
    let slow = timing_run(25000, 2000, false);
    assert_eq!(
        (fast.starts, fast.swaps, slow.starts, slow.swaps),
        (2, 2, 2, 2)
    );
    assert_eq!(fast.captured[0], slow.captured[0]);
    assert!(fast
        .captured
        .iter()
        .zip(&slow.captured)
        .all(|(&a, &b)| a.abs_diff(b) <= 2));
    for s in [&fast, &slow] {
        assert_eq!(s.format, 1);
        assert_eq!(s.telemetry.len(), 22);
        for (packet, elapsed) in s.telemetry.chunks(11).zip(&s.captured) {
            assert_eq!(&packet[..5], b"DDGD\x03");
            assert_eq!(u16::from_le_bytes([packet[8], packet[9]]), *elapsed);
            assert_eq!(packet[10], packet[4..10].iter().fold(0, |a, b| a ^ b));
        }
    }
    let saturated = timing_run(500, 70000, false);
    assert_eq!(saturated.captured, [65535, 65535]);
    let failed = timing_run(500, 2000, true);
    assert_eq!(failed.swaps, 0);
    assert!(failed.captured.is_empty());
}
