//! Actual RTT application and bounded presentation tests; peers do not render pixels.
use super::changes::{global, preload};
use super::*;
use cpu_v3::{Device, StepOutcome};
use std::{any::Any, path::PathBuf};
#[allow(dead_code)]
#[path = "../../examples/bloxorz/geometry.rs"]
mod geometry;
fn check_block_pose(memory: &[u16], program: &rcc_backend::CpuV3Program, world: usize) {
    let m = global(program, "MOTION");
    let g = global(program, "GAME");
    let s = global(program, "SESSION");
    let moving = memory[m + 5] != 0;
    let orient = memory[if moving { m + 2 } else { g + 2 }];
    let x = f64::from(memory[if moving { m } else { g }] as i16);
    let z = f64::from(memory[if moving { m + 1 } else { g + 1 }] as i16);
    let d = if moving { memory[m + 3] } else { 0 };
    // FRECIP returns the Q16 reciprocal; retain that specified quantization
    // before comparing the independently derived rolling geometry and matrix.
    let fraction = |time: u16, duration: u16| {
        if time >= duration {
            1.0
        } else {
            f64::from(time) * (65536.0 / f64::from(duration)).floor() / 65536.0
        }
    };
    let f = if moving {
        fraction(memory[m + 4], 220)
    } else {
        0.
    };
    let reference = geometry::roll(orient as u8, d as u8, f, 0.);
    let mode = if memory[s] >= 9 {
        memory[s + 5]
    } else {
        memory[s]
    };
    let t = f64::from(if memory[s] >= 9 {
        memory[s + 8]
    } else {
        memory[s + 1]
    });
    let mut y = 0.;
    let mut squeeze = 1.;
    if mode == 3 {
        let remain = 1. - fraction(t as u16, 380);
        y = -2.08 * (0.4 * remain + 0.6 * remain * remain);
        squeeze = 1. - 0.08 * remain;
    }
    if (5..=7).contains(&mode) {
        let fraction = if mode == 5 {
            fraction(t as u16, 420)
        } else {
            1.
        };
        y = -2.08 * (0.4 * fraction + 0.6 * fraction * fraction);
        squeeze = 1. - 0.08 * fraction;
    }
    if mode == 8 {
        y = -4. * fraction(t as u16, 600).powi(2);
    }
    let angle = std::f64::consts::FRAC_PI_2 * f * if d == 0 || d == 3 { -1. } else { 1. };
    let dimensions = match orient {
        0 => [1., 2., 1.],
        1 => [2., 1., 1.],
        _ => [1., 1., 2.],
    };
    let expected = [
        reference[0][3] + x - 4.,
        reference[1][3] + y,
        reference[2][3] + z - 1.5,
        if d >= 2 { angle } else { 0. },
        0.,
        if d < 2 { angle } else { 0. },
        dimensions[0] * squeeze,
        dimensions[1],
        dimensions[2] * squeeze,
    ];
    let rotation = scene::rotation([expected[3], expected[4], expected[5]]);
    for row in 0..3 {
        for column in 0..4 {
            let want = if column == 3 {
                expected[row]
            } else {
                rotation[row][column] * expected[6 + column]
            };
            assert!(
                (scene::fixed(&memory[world..world + 24], 2 * (row * 4 + column)) - want).abs()
                    < 0.005,
                "mode{mode} world[{row},{column}] actual={} expected={want}",
                scene::fixed(&memory[world..world + 24], 2 * (row * 4 + column))
            );
        }
    }
}

fn application(
    name: &str,
    probe: Option<&str>,
) -> (rcc_backend::CpuV3Program, Vec<(usize, Vec<u8>)>, PathBuf) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let example = root.join("examples/gpu_renderer_bloxorz");
    let out = root.join("../../target/bloxorz-rtt").join(name);
    assert!(std::process::Command::new("python")
        .arg("-B")
        .arg(example.join("tools/assets.py"))
        .arg("--output")
        .arg(&out)
        .status()
        .unwrap()
        .success());
    let catalog: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("assets.json")).unwrap()).unwrap();
    assert_eq!(catalog["profile"], "production");
    let mut resources = vec![];
    let mut config = String::from("format 2\ngpu-api\napplication main.rs\n");
    for r in catalog["resources"].as_array().unwrap() {
        let file = r["file"].as_str().unwrap();
        let base = r["address"].as_u64().unwrap() as usize;
        config.push_str(&format!(
            "copy GPU_RENDERER_{} {base} {file}\n",
            file.trim_end_matches(".bin").to_uppercase()
        ));
        resources.push((base, std::fs::read(out.join(file)).unwrap()));
    }
    let constants = cpu_v3_tang_nano_20k::boot::parse_boot_project(&config)
        .unwrap()
        .constants()
        + &std::fs::read_to_string(out.join("renderer_assets.rs")).unwrap();
    let mut source = std::fs::read_to_string(example.join("rcc/main.rs")).unwrap();
    let entry = if let Some(body) = probe {
        source += &format!(
            "\nfn check(ok:bool){{if !ok{{halt(0xbad);}}}}\npub fn probe(){{{body}halt(0x600d);}}"
        );
        "probe"
    } else if name == "automatic-capture-full-application" {
        "renderer_game_capture_full"
    } else if name == "automatic-capture-application" {
        "renderer_game_capture"
    } else {
        "renderer_game_frames"
    };
    // Test-only readonly instrumentation uses the public getter, never arena slot layout.
    // Original marker12 is preserved for the independent input-transition trials.
    source = source.replace(
        "block_pose(model);",
        "block_pose(model);world_probe(model);",
    );
    source += "\nfn world_probe(model:&Model){let world=model.world();let words:Buf<u16,24>=Buf::new([world.row0.x.lo_bits(),world.row0.x.hi_bits(),world.row0.y.lo_bits(),world.row0.y.hi_bits(),world.row0.z.lo_bits(),world.row0.z.hi_bits(),world.row0.w.lo_bits(),world.row0.w.hi_bits(),world.row1.x.lo_bits(),world.row1.x.hi_bits(),world.row1.y.lo_bits(),world.row1.y.hi_bits(),world.row1.z.lo_bits(),world.row1.z.hi_bits(),world.row1.w.lo_bits(),world.row1.w.hi_bits(),world.row2.x.lo_bits(),world.row2.x.hi_bits(),world.row2.y.lo_bits(),world.row2.y.hi_bits(),world.row2.z.lo_bits(),world.row2.z.hi_bits(),world.row2.w.lo_bits(),world.row2.w.hi_bits()]);signal(13,words.as_ptr().addr());}\n";
    let source = crate::shared_application_entry::select(&source, entry).unwrap();
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let mut dependencies = vec![];
    let ir = resolver::compile_explicit_gpu_program_checked_with_sources(
        "Bloxorz RTT",
        &source,
        &options,
        &example.join("rcc"),
        &root.join("rcc"),
        &constants,
        &[root.join("examples/bloxorz/rcc")],
        &resolver::RendererCaptureConfiguration::without_capture(
            resolver::RendererLayoutProfile::Column16,
        ),
        &rcc::frontend::rustc_check::RustcCheck::new(out.join("checked")),
        &mut |p| dependencies.push(p.to_owned()),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert!(dependencies.contains(&root.join("examples/bloxorz/rcc/bloxorz_core.rs")));
    (
        rcc_backend::try_compile(ir, &options, "main").unwrap(),
        resources,
        out,
    )
}

#[test]
fn transitions_reset_only_after_black_presentation_and_ignore_locked_input() {
    let body = r#"
session_init();let mut n:u16=0;while n<15{session_tick(32,40);n+=1;}
check(SESSION[0u16]==PLAY && GAME[5u16]==0);
session_tick(32,0);n=0;while n<6{session_tick(0,40);n+=1;}
check(GAME[5u16]==1 && GAME[0u16]==1);
session_tick(32,0);session_tick(0,40);session_tick(8,40);
check(SESSION[0u16]==FADE_OUT && SESSION[3u16]==1 && MOTION[4u16]==40);
session_captured();n=0;while n<20{session_tick(8|4096|128,50);n+=1;}
check(tint()==0 && SESSION[0u16]==FADE_OUT && GAME[5u16]==1 && MOTION[4u16]==40);
session_presented();check(SESSION[0u16]==HOLD && GAME[5u16]==0 && MOTION[6u16]==0 && SESSION[3u16]==1);
session_captured();n=0;while n<20{session_tick(8|4096|32,50);n+=1;}
check(SESSION[0u16]==FADE_IN && tint()==255 && GAME[5u16]==0 && MOTION[6u16]==0);
session_presented();check(SESSION[0u16]==ENTRY_OPEN);n=0;while n<15{session_tick(32,40);n+=1;}
check(SESSION[0u16]==PLAY && GAME[5u16]==0);
// Four real +X rules reach this fixture's goal; animations cannot skip to fade.
n=0;while n<4{session_tick(32,0);let mut k:u16=0;while k<6{session_tick(0,40);k+=1;}n+=1;}
check(GAME[4u16]==1 && SESSION[0u16]==GOAL_OPEN);
session_tick(0,1000);check(SESSION[1u16]==50);
n=0;while n<30{session_tick(0,40);n+=1;}
check(SESSION[0u16]==FADE_OUT && SESSION[2u16]==1 && MOTION[6u16]==0);
session_captured();n=0;while n<10{session_tick(8,40);n+=1;}
check(MOTION[6u16]==0);session_presented();check(MOTION[6u16]==1 && GAME[5u16]==0);
// Fall is a separate serial animation; it does not fade during its live pose.
level_reset(0);SESSION[0u16]=PLAY;SESSION[1u16]=0;SESSION[3u16]=0;SESSION[9u16]=0;
session_tick(128,0);n=0;while n<6{session_tick(0,40);n+=1;}
check(GAME[4u16]==2 && SESSION[0u16]==FALL);n=0;while n<16{session_tick(0,40);n+=1;}
check(SESSION[0u16]==FADE_OUT && SESSION[2u16]==0);
"#;
    let (program, resources, _) = application("session", Some(body));
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    preload(cpu.physical_memory_mut(), &resources);
    let result = cpu.run(3_000_000).unwrap();
    assert!(
        matches!(result, cpu_v3::RunOutcome::Halted { signal: 0x600d, .. }),
        "{result:?}"
    );
}

#[derive(Default)]
struct Observed {
    boundary_button: u16,
    boundary_hold: bool,
    boundary_step: u8,
    active: u32,
    staged: u32,
    pending: bool,
    gpu_left: usize,
    display_left: usize,
    count: u16,
    now: u16,
    buttons: u16,
    connected: bool,
    fresh: bool,
    format: u16,
    staged_format: u16,
    session: usize,
    execution: usize,
    mode_at_launch: Vec<u16>,
    views: Vec<(u16, u16, u16)>,
    snapshots: Vec<(u16, Vec<u32>, Vec<u16>)>,
    surfaces: Vec<u32>,
    captures: Vec<(u16, u16)>,
    swaps: usize,
}
struct Ports {
    kind: u8,
    state: Rc<RefCell<Observed>>,
}
impl Device for Ports {
    fn read(&mut self, m: &mut [u16], channel: u8) -> u16 {
        let mut s = self.state.borrow_mut();
        match (self.kind, channel) {
            (0, 10) => s.now,
            (0, 5) => 0,
            (5, 8) => u16::from(s.connected) | if s.fresh { 2 } else { 0 },
            (5, 0) | (5, 1) | (5, 6) | (5, 7) => 128,
            (5, 2) => 0x73,
            (5, 3) => 0x5a,
            (5, 4) => (!s.buttons) & 255,
            (5, 5) => (!s.buttons) >> 8,
            (3, 1) => s.active as u16,
            (3, 2) => (s.active >> 16) as u16,
            (3, 4) => s.format,
            (3, 3) => {
                if s.pending {
                    if s.boundary_button != 0
                        && s.boundary_step < 3
                        && m[s.session] == 11
                        && m[s.session + 1] == 380
                    {
                        let button = s.boundary_button;
                        if s.boundary_step == 0 {
                            s.buttons = button;
                            s.fresh = true;
                            s.boundary_step = 1;
                        } else if s.boundary_step == 1 {
                            if s.boundary_hold {
                                s.boundary_step = 3;
                            } else {
                                s.buttons = 0;
                                s.fresh = true;
                                s.boundary_step = 2;
                            }
                        } else if s.boundary_step == 2 {
                            s.boundary_step = 3;
                        }
                        // Wire reports advance independently: Display.wait_vblank
                        // does not own or implicitly capture application input.
                    }
                    s.display_left -= 1;
                    if s.display_left == 0 {
                        s.active = s.staged;
                        s.format = s.staged_format;
                        s.pending = false;
                    }
                }
                u16::from(s.pending)
            }
            (4, 2) if s.gpu_left > 0 => {
                s.gpu_left -= 1;
                assert_eq!(m[s.execution + 1], 1, "driver released CB while GPU busy");
                if let Some((sector, commands, push)) = s.snapshots.last() {
                    let base = usize::from(*sector) * 64;
                    for (i, word) in commands.iter().enumerate() {
                        assert_eq!(
                            &m[base + i * 2..base + i * 2 + 2],
                            &[*word as u16, (*word >> 16) as u16]
                        );
                    }
                    assert_eq!(&m[0x4d0000 / 2..0x4d0000 / 2 + push.len()], push);
                }
                1
            }
            _ => 0,
        }
    }
    fn write(&mut self, m: &mut [u16], channel: u8, value: u16) {
        let mut s = self.state.borrow_mut();
        match (self.kind, channel) {
            (5, 8) => s.fresh = false,
            (4, 0) => {
                assert_eq!(s.gpu_left, 0);
                s.count = value;
            }
            (3, 1) => s.staged = (s.staged & 0xffff0000) | u32::from(value),
            (3, 2) => s.staged = (s.staged & 65535) | (u32::from(value) << 16),
            (3, 4) => s.staged_format = value,
            (3, 3) => {
                assert_eq!(value, 1);
                assert_eq!(s.gpu_left, 0);
                assert!(!s.pending);
                s.pending = true;
                s.display_left = 5;
                s.swaps += 1;
            }
            (4, 1) => {
                assert_eq!(s.gpu_left, 0);
                let at = usize::from(value) * 64;
                let commands = m[at..at + (usize::from(s.count) + 1) * 2]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|w| u32::from(w[0]) | (u32::from(w[1]) << 16))
                    .collect::<Vec<_>>();
                assert_eq!(&commands[..2], &[0x40000003, 0x90000000]);
                assert_eq!(
                    &commands[commands.len() - 3..],
                    &[0x40000003, 0x70000000, 0xf0000000]
                );
                let objects = if value == 0x9800 {
                    0x4d0000
                } else {
                    assert_eq!(value, 0xfc00);
                    0x7f0000
                };
                let obj = m[objects / 2..objects / 2 + 32768].to_vec();
                let mut local = vec![0u16; 2048];
                let mut render = 0;
                let mut color = 0;
                let mut view_draws = 0;
                let mode = m[s.session];
                s.mode_at_launch.push(mode);
                let launch = s.mode_at_launch.len() as u16;
                for &c in &commands {
                    match c >> 28 {
                        1 => {
                            let src = ((c >> 10) & 0x3ffff) as usize * 16;
                            let dst = ((c >> 3) & 127) as usize * 16;
                            let n = 16 << ((c >> 1) & 3);
                            local[dst..dst + n].copy_from_slice(&m[src..src + n]);
                        }
                        6 => render = (c & 127) as usize * 16,
                        10 => {
                            let q = (c & 127) as usize * 16;
                            let w = u64::from(local[q])
                                | (u64::from(local[q + 1]) << 16)
                                | (u64::from(local[q + 2]) << 32)
                                | (u64::from(local[q + 3]) << 48);
                            color = ((w & 0x3fff) << 9) as u32;
                        }
                        3 => {
                            assert_eq!(local[render + 4] & 16, 0, "blend enabled");
                            if local[render + 1] & 128 != 0 {
                                view_draws += 1;
                                assert_eq!(local[render], 0xc000);
                                assert_eq!(local[render + 1], 137);
                                assert_eq!(m[s.execution + 1], 1);
                                let rgba = u32::from(local[1324]) | (u32::from(local[1325]) << 16);
                                let shade = rgba as u8;
                                assert_eq!(rgba, (u32::from(shade) * 0x010101) | 0xff000000);
                                let case = scene::Case {
                                    eye: [0.; 3],
                                    camera: [0.; 3],
                                    position: [0., 0., -1.],
                                    angles: [0.; 3],
                                    scale: [1., 39322. / 65536., 1.],
                                    focal: 200.,
                                };
                                let (expected, _) = scene::reference(case);
                                for (row, values) in expected.iter().enumerate() {
                                    for (col, want) in values.iter().enumerate() {
                                        assert!(
                                            (scene::fixed(&local[1280..1344], 2 * (row * 4 + col))
                                                - want)
                                                .abs()
                                                < 0.001
                                        );
                                    }
                                }
                                let epoch = m[s.session + 4];
                                s.views.push((u16::from(shade), epoch, launch));
                            }
                        }
                        _ => {}
                    }
                }
                if color == 0x600000 {
                    assert_eq!(view_draws, 0);
                    assert_eq!(m[s.execution + 1], 1);
                } else {
                    assert!([0x400000, 0x440000].contains(&color));
                    assert_ne!(color / 2, s.active);
                    assert_eq!(view_draws, usize::from(mode >= 9));
                }
                s.snapshots.push((value, commands, obj));
                s.surfaces.push(color);
                s.gpu_left = 9;
            }
            _ => {}
        }
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Clone, Copy)]
enum Trial {
    Restart,
    Goal,
    NoController,
    CaptureCard,
    CaptureCardFull,
}
fn actual_caller(trial: Trial) {
    let full = matches!(trial, Trial::CaptureCardFull);
    let automatic = full || matches!(trial, Trial::CaptureCard);
    let goal = matches!(trial, Trial::Goal);
    let no_controller = matches!(trial, Trial::NoController);
    let (program, resources, out) = application(
        if full {
            "automatic-capture-full-application"
        } else if automatic {
            "automatic-capture-application"
        } else if no_controller {
            "no-controller-application"
        } else if goal {
            "goal-application"
        } else {
            "application"
        },
        None,
    );
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    preload(cpu.physical_memory_mut(), &resources);
    cpu.physical_memory_mut()[0x600000 / 2..0x680000 / 2].fill(0xa55a);
    let session = global(&program, "SESSION");
    let execution = global(&program, "GPU_EXECUTION");
    let state = Rc::new(RefCell::new(Observed {
        active: 0x200000,
        connected: true,
        fresh: true,
        session,
        execution,
        ..Observed::default()
    }));
    for kind in [0, 3, 4, 5] {
        cpu.attach_device(
            kind,
            Box::new(Ports {
                kind,
                state: state.clone(),
            }),
        );
    }
    let mut halted = false;
    let mut moves_issued = 0;
    for _ in 0..40_000_000 {
        match cpu.step().unwrap() {
            StepOutcome::Signaled(e) if e.signal_type == 4 => {
                let mut s = state.borrow_mut();
                s.now = e.value * 40;
                s.fresh = true;
                s.connected = !automatic && !no_controller && (goal || e.value != 24);
                let mode = cpu.physical_memory_mut()[session];
                s.buttons = if automatic || no_controller {
                    0
                } else if mode >= 9 {
                    if e.value % 2 == 0 {
                        4096
                    } else {
                        8 | 32
                    }
                } else if goal {
                    if mode == 0 && moves_issued < 4 {
                        moves_issued += 1;
                        32
                    } else {
                        0
                    }
                } else {
                    match e.value {
                        16 => 32,
                        18 => 8,
                        64 => 4096,
                        _ => 0,
                    }
                };
            }
            StepOutcome::Signaled(e) if e.signal_type == 11 => {
                let m = cpu.physical_memory_mut();
                assert_eq!(
                    m[execution + 1],
                    0,
                    "offscreen capture announced before real retirement"
                );
                assert_eq!(m[session + 4], e.value);
                state.borrow_mut().captures.push((e.value, m[session + 5]));
            }
            StepOutcome::Signaled(e) if e.signal_type == 13 => {
                check_block_pose(cpu.physical_memory_mut(), &program, usize::from(e.value))
            }
            StepOutcome::Halted { signal } => {
                assert_eq!(signal, 0x600d);
                halted = true;
                break;
            }
            _ => {}
        }
    }
    assert!(halted, "bounded application did not halt");
    let s = state.borrow();
    if automatic {
        let m = cpu.physical_memory_mut();
        assert_eq!(
            (m[session], m[session + 1]),
            (9, if full { 0 } else { 160 })
        );
        assert_eq!(m[global(&program, "GAME") + 5], 1);
        assert_eq!(m[global(&program, "CAPTURE_STIMULUS")], 3);
        assert_eq!(s.captures.len(), 1);
        assert_eq!(s.views.last().unwrap().0, if full { 255 } else { 128 });
        assert_eq!(m[execution + 1], 0);
        assert!(m[0x600000 / 2..0x680000 / 2].iter().all(|v| *v == 0xa55a));
        std::fs::write(out.join("observed.json"), serde_json::to_vec_pretty(&serde_json::json!({"captures":s.captures,"views":s.views,"submissions":s.snapshots.len(),"presents":s.swaps,"halt":24589,"mode":9,"elapsed":if full {0} else {160},"moves":1,"boundary":"Actual checked automatic capture caller; bounded peers, no rendered pixels"})).unwrap()).unwrap();
        return;
    }
    if no_controller {
        assert!(s.captures.is_empty() && s.views.is_empty());
        assert_eq!((s.snapshots.len(), s.swaps), (120, 120));
        assert!(
            s.mode_at_launch.contains(&2)
                && s.mode_at_launch.contains(&3)
                && s.mode_at_launch.contains(&0)
        );
        assert_eq!(cpu.physical_memory_mut()[session], 0);
        assert_eq!(
            cpu.physical_memory_mut()[global(&program, "GAMEPAD_STATE")],
            0
        );
        return;
    }
    assert_eq!(s.captures.len(), if goal { 2 } else { 4 });
    assert_eq!(
        s.captures.iter().map(|v| v.0).collect::<Vec<_>>(),
        (1..=s.captures.len() as u16).collect::<Vec<_>>()
    );
    if goal {
        for mode in 4..=7 {
            assert!(s.mode_at_launch.contains(&mode));
        }
    }
    assert!(
        s.views.iter().any(|v| v.0 == 0)
            && s.views.iter().any(|v| v.0 == 255)
            && s.views.iter().any(|v| v.0 == 128)
    );
    assert!(s.views.iter().all(|v| (1..=4).contains(&v.1)));
    assert_eq!(cpu.physical_memory_mut()[global(&program, "MOTION") + 6], 1);
    assert!(
        cpu.physical_memory_mut()[0x600000 / 2..0x680000 / 2]
            .iter()
            .all(|v| *v == 0xa55a),
        "CPU wrote pixels"
    );
    assert_eq!(cpu.data_segments(), [0, 1, 2, 3]);
    let grey = s.views.iter().find(|v| v.0 == 128).unwrap();
    let producer = s
        .surfaces
        .iter()
        .enumerate()
        .filter(|(_, c)| **c == 0x600000)
        .nth(usize::from(grey.1) - 1)
        .unwrap()
        .0;
    let consumer = usize::from(grey.2) - 1;
    let mut submissions = vec![];
    for (job, index) in [producer, consumer].into_iter().enumerate() {
        let (base, commands, objects) = &s.snapshots[index];
        let command_file = format!("grey-{job}-commands.bin");
        let object_file = format!("grey-{job}-objects.bin");
        std::fs::write(
            out.join(&command_file),
            commands
                .iter()
                .flat_map(|w| w.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        std::fs::write(
            out.join(&object_file),
            objects
                .iter()
                .flat_map(|w| w.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        submissions.push(serde_json::json!({"command_base_bytes":u32::from(*base)*128,"object_base_bytes":if *base==0x9800{0x4d0000}else{0x7f0000},"command_file":command_file,"object_file":object_file,"valid_instructions":commands.len(),"color_base_bytes":s.surfaces[index],"depth_base_bytes":if job==0{0x640000}else{0x480000}}));
    }
    std::fs::write(out.join("grey-rtl-inputs.json"),serde_json::to_vec_pretty(&serde_json::json!({"profile":"production","surface":"column-major-16x16-pad16","assets":"assets.json","submissions":submissions,"source":"actual checked Bloxorz application; grey uniform128, opaque nearest"})).unwrap()).unwrap();
    std::fs::write(out.join("observed.json"),serde_json::to_vec_pretty(&serde_json::json!({"captures":s.captures,"views":s.views,"submissions":s.snapshots.len(),"presents":s.swaps,"boundary":"Actual checked RCC caller on CPU V3 with bounded GPU/Display peers; no rendering or board claim"})).unwrap()).unwrap();
}

#[test]
fn actual_rtt_caller_refreshes_each_transition_and_keeps_opaque_sources_pinned() {
    actual_caller(Trial::Restart);
}
#[test]
fn actual_goal_sequence_finishes_before_fade_and_next_level_entry() {
    actual_caller(Trial::Goal);
}

#[test]
fn automatic_entry_draws_without_controller_input() {
    actual_caller(Trial::NoController);
}

#[test]
fn capture_card_diagnostic_moves_then_freezes_presented_half_brightness_without_input() {
    actual_caller(Trial::CaptureCard);
}

#[test]
fn capture_card_full_brightness_uses_the_same_frozen_scene_and_stops_without_input() {
    actual_caller(Trial::CaptureCardFull);
}

#[test]
fn final_fade_wait_discards_start_and_triangle_edges_but_accepts_new_presses() {
    let (program, resources, out) = application("fade-wait-input", None);
    let session = global(&program, "SESSION");
    let mut results = vec![];
    for (button, held) in [(8, false), (4096, false), (8, true), (4096, true)] {
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&program.words);
        preload(cpu.physical_memory_mut(), &resources);
        let state = Rc::new(RefCell::new(Observed {
            active: 0x200000,
            connected: true,
            fresh: true,
            session,
            execution: global(&program, "GPU_EXECUTION"),
            boundary_button: button,
            boundary_hold: held,
            ..Observed::default()
        }));
        for kind in [0, 3, 4, 5] {
            cpu.attach_device(
                kind,
                Box::new(Ports {
                    kind,
                    state: state.clone(),
                }),
            );
        }
        let mut started = false;
        let mut boundary_seen = false;
        let mut entry_presented = false;
        let mut release_presented = !held;
        let mut new_press = false;
        let mut accepted = false;
        for _ in 0..40_000_000 {
            match cpu.step().unwrap() {
                StepOutcome::Signaled(e) if e.signal_type == 4 => {
                    let mode = cpu.physical_memory_mut()[session];
                    let mut s = state.borrow_mut();
                    s.now = e.value * 40;
                    s.fresh = true;
                    if s.boundary_step < 3 {
                        if mode == 0 && !started {
                            s.buttons = 8;
                            started = true;
                        } else if s.boundary_step == 0 {
                            s.buttons = 0;
                        }
                    } else if new_press {
                        assert_eq!(mode, 9, "fresh post-transition press was ignored");
                        assert_eq!(
                            cpu.physical_memory_mut()[session + 2],
                            u16::from(button == 4096)
                        );
                        accepted = true;
                        break;
                    } else {
                        assert_eq!(
                            mode, 2,
                            "button {button}, held={held}: fade-wait input leaked into entry"
                        );
                        boundary_seen = true;
                        if !entry_presented {
                            s.buttons = if held { button } else { 0 };
                        } else if !release_presented {
                            s.buttons = 0;
                        } else {
                            s.buttons = button;
                            new_press = true;
                        }
                    }
                }
                StepOutcome::Signaled(e) if e.signal_type == 12 && boundary_seen && !new_press => {
                    assert_eq!(e.value, 2, "entry did not survive a discarded or held edge");
                    if entry_presented {
                        release_presented = true;
                    }
                    entry_presented = true;
                }
                StepOutcome::Halted { signal } => {
                    panic!("boundary test halted early: {signal:04x}")
                }
                _ => {}
            }
        }
        assert!(
            started && boundary_seen && entry_presented && accepted,
            "bounded input trial incomplete"
        );
        assert_eq!(
            state.borrow().boundary_step,
            3,
            "wire stimulus was not captured during the wait"
        );
        results.push(serde_json::json!({"button":button,"held_across_boundary":held,"entry_presented":entry_presented,"fresh_press_accepted":accepted}));
    }
    std::fs::write(
        out.join("boundary-results.json"),
        serde_json::to_vec_pretty(&results).unwrap(),
    )
    .unwrap();
}
