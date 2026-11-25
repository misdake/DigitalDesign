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
    assets_with_motion(test_name, "manual")
}

fn assets_with_motion(test_name: &str, motion: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = root
        .join("../../target/public-handheld/test-assets")
        .join(test_name);
    let result = std::process::Command::new("python")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("BOX_RABBIT_MOTION", motion)
        .arg(root.join("examples/demo_box_rabbit/tools/public_assets.py"))
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
fn qualification_assets(kind: &str) -> PathBuf {
    let run = std::env::var("STAGE2_QUALIFICATION_RUN").unwrap_or_else(|_| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            .to_string()
    });
    let name = format!("{kind}-{run}");
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/public-handheld/test-assets")
        .join(&name);
    assert!(
        !root.exists(),
        "qualification output already exists; choose a fresh run identity"
    );
    assets(&name)
}
fn compile(source: &str, out: &Path, debug: bool) -> rcc_backend::CpuV3Program {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("assets.json")).unwrap()).unwrap();
    let mut project = String::from(
        "format 2\ngpu-api\napplication examples/demo_box_rabbit/rcc/public_main.rs\n",
    );
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
    // Production's low asset bank, disjoint from the classic fixed payloads.
    options.asset_banks = vec![rcc::frontend::static_assets::AssetBank {
        bytes: 0x10_0000..0x11_0000,
    }];
    let parsed = resolver::compile_explicit_gpu_program_checked(
        "Public classic controller",
        source,
        &options,
        &root.join("examples/demo_box_rabbit/rcc"),
        &root.join("rcc"),
        &constants,
        &rcc::frontend::rustc_check::RustcCheck::new(out.join(format!("check-{debug}"))),
        &mut |_| {},
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let static_words = parsed.debug.static_data_end;
    let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
    std::fs::write(out.join(format!("budget-{debug}.json")), serde_json::to_vec_pretty(&serde_json::json!({
        "code_words": program.words.len(), "static_words": static_words,
        "frames": program.debug.functions.iter().map(|f| (&f.name, f.frame_size)).collect::<std::collections::BTreeMap<_,_>>(),
    })).unwrap()).unwrap();
    program
}

#[derive(Clone, Debug)]
struct Recorded {
    triangles: usize,
    draws: usize,
    ui: usize,
    matrix: Vec<u16>,
    world: Vec<u16>,
    global: Vec<u16>,
    rabbit_matrix: Vec<u16>,
    target: u32,
    trace: Vec<Vec<u16>>,
    ui_descriptors: Vec<Vec<u16>>,
    invalidates: usize,
}
// 0=startup maintenance, 1=static, 2=dynamic, 3=unsplit reference.
fn recorded(memory: &[u16], sector: u16, count: u16, role: u16) -> Recorded {
    let base = usize::from(sector) * 64;
    let mut local = [0u16; 2048];
    let mut mesh_triangles = [0; 2];
    let mut mesh_records = [0u16; 2];
    let mut global_at = 0;
    let mut render_at = 0;
    let mut control = 0;
    let mut texture_word = 0;
    let mut result = Recorded {
        triangles: 0,
        draws: 0,
        ui: 0,
        matrix: Vec::new(),
        world: Vec::new(),
        global: Vec::new(),
        rabbit_matrix: Vec::new(),
        target: 0,
        trace: Vec::new(),
        ui_descriptors: Vec::new(),
        invalidates: 0,
    };
    let mut ended = false;
    let mut clears = [false; 2];
    let mut invalidates = 0;
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
                mesh_records[(code & 1) as usize] = ((code >> 12) & 65535) as u16;
            }
            3 => {
                let draw = result.draws + if role == 2 { 7 } else { 0 };
                assert_eq!(
                    texture_word,
                    if draw == 6 { 0 } else { 0x56 },
                    "only colored RGB is untextured; room/rabbit remain full-mip trilinear"
                );
                assert_eq!(
                    control,
                    if draw < 7 { 41 } else { 9 },
                    "classic material/depth policy"
                );
                result.triangles += mesh_triangles[(code & 1) as usize];
                result.draws += 1;
                let mut draw = vec![mesh_records[(code & 1) as usize]];
                draw.extend_from_slice(&local[global_at..global_at + 8]);
                draw.extend_from_slice(&local[render_at..render_at + 12]);
                draw.extend_from_slice(&local[1472..1536]);
                draw.extend_from_slice(&local[1280..1344]);
                draw.push((result.target >> 8) as u16);
                result.trace.push(draw);
                if result.matrix.is_empty() {
                    result.matrix.extend_from_slice(&local[1472..1514]);
                    result.world.extend_from_slice(&local[1280..1304]);
                    result.world.extend_from_slice(&local[1312..1330]);
                }
            }
            5 => {
                let at = ((code & 127) * 16) as usize;
                global_at = at;
                if result.global.is_empty() {
                    result.global.extend_from_slice(&local[at..at + 8]);
                }
            }
            6 => {
                let at = ((code & 127) * 16) as usize + if code & 128 != 0 { 4 } else { 0 };
                render_at = at;
                assert_eq!(local[at + 10], 0x33, "shininess exponent32/code12");
                assert_eq!(
                    &local[1306..1310],
                    &[16384, 0, 49152, 0],
                    "raw material responses"
                );
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
            11 => {
                result.ui += 1;
                let mut descriptor = vec![code as u16, (code >> 16) as u16];
                descriptor.extend_from_slice(&local[69 * 16..70 * 16]);
                let ui = usize::from(resolver::layout::GPU_UI_PAGE0_SECTOR) * 64;
                descriptor.extend_from_slice(&memory[ui..ui + 512]);
                result.ui_descriptors.push(descriptor);
            }
            9 => invalidates += 1,
            10 => {
                let at = ((code & 127) * 16) as usize;
                result.target = u32::from(local[at] & 16383) << 8;
                assert_eq!(local[at + 1] << 4, 0x9000, "depth target");
                assert!(local[at + 2..at + 16].iter().all(|word| *word == 0));
            }
            15 => {
                ended = true;
                break;
            }
            _ => {}
        }
    }
    assert!(ended);
    assert_eq!(
        clears,
        if role == 1 || role == 3 {
            [true, true]
        } else {
            [false, false]
        }
    );
    assert_eq!(
        (result.draws, result.triangles),
        match role {
            0 => (0, 0),
            1 => (7, 193),
            2 => (17, 948),
            3 => (24, 1141),
            _ => unreachable!(),
        }
    );
    if role != 3 {
        assert_eq!(invalidates, usize::from(role == 0));
    }
    result.invalidates = invalidates;
    result
}

#[derive(Default)]
struct State {
    us: u64,
    clear: u64,
    last_us_sample: u16,
    counter_samples: Vec<[u64; 5]>,
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
    submissions: usize,
    pending_static: Option<Recorded>,
    static_commands: Option<Vec<u16>>,
    static_generation: Option<(u16, u16)>,
    record_base: usize,
    dynamic_prepared_while_busy: Vec<bool>,
    unsplit: bool,
    record_calls: usize,
    append_calls: usize,
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
                let trace = [1, s.us, s.prep_ticks, u64::from(s.phase), u64::from(sample)];
                s.counter_samples.push(trace);
                sample
            }
            (3, 0) => (s.us / 1000) as u16, // Can change before old reads retire.
            (3, 1) => s.active as u16,
            (3, 2) => (s.active >> 16) as u16,
            (3, 3) => u16::from(s.pending),
            (3, 4) => s.format,
            (4, 2) => u16::from(s.submissions != 0 && s.us < s.gpu_until),
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
                let trace = [0, s.us, s.prep_ticks, u64::from(s.phase), 0];
                s.counter_samples.push(trace);
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
                assert!(
                    s.us >= s.gpu_until,
                    "next START before previous GPU completion"
                );
                let role = if s.unsplit {
                    3
                } else if s.submissions == 0 {
                    0
                } else if value == 0x9800 {
                    1
                } else {
                    2
                };
                let r = recorded(memory, value, s.command_count, role);
                if role == 0 {
                    assert_eq!(value, 0xfc00, "maintenance uses dynamic slot");
                } else if role == 1 {
                    assert!(s.pending_static.is_none());
                    let base = usize::from(value) * 64;
                    let words = ((usize::from(s.command_count) + 1) * 2 + 63) & !63;
                    let commands = memory[base..base + words].to_vec();
                    if let Some(old) = &s.static_commands {
                        assert_eq!(&commands, old, "persistent commands/END/padding");
                    }
                    s.static_commands = Some(commands);
                    let generation = (memory[s.record_base + 20], memory[s.record_base + 21]);
                    if let Some(old) = s.static_generation {
                        assert_ne!(generation, old, "each frame records a new Pass");
                    }
                    s.static_generation = Some(generation);
                    s.pending_static = Some(r);
                } else if role == 2 {
                    assert_eq!(value, 0xfc00);
                    let mut prefix = s
                        .pending_static
                        .take()
                        .expect("dynamic requires completed static prefix");
                    assert_eq!(
                        prefix.global, r.global,
                        "prepared static light matches current dynamic view"
                    );
                    assert_eq!(prefix.target, r.target, "both streams write the same Back");
                    prefix.draws += r.draws;
                    prefix.triangles += r.triangles;
                    prefix.ui += r.ui;
                    prefix.rabbit_matrix = r.world;
                    prefix.trace.extend(r.trace);
                    prefix.ui_descriptors.extend(r.ui_descriptors);
                    assert_eq!((prefix.draws, prefix.triangles), (24, 1141));
                    s.frames.push(prefix);
                } else {
                    assert_eq!(r.invalidates, usize::from(s.frames.is_empty()));
                    s.frames.push(r);
                }
                s.submissions += 1;
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
                assert_eq!(
                    s.frames.last().unwrap().target,
                    target,
                    "present uses completed Back"
                );
                s.swaps.push(target);
                s.pending = true;
                s.present_until = s.us + s.present_delay;
            }
            (5, 8) => s.input_latched = true,
            (7, 1) => {
                if value == 12 && !s.unsplit {
                    assert!(s.pending_static.is_some());
                    let busy = s.us < s.gpu_until;
                    s.dynamic_prepared_while_busy.push(busy);
                }
                s.phase = value;
            }
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
    state.record_base = match program
        .debug
        .globals
        .iter()
        .find(|g| g.name == "GPU_CB_RECORDS")
        .unwrap()
        .loc
    {
        rcc::VarLoc::Global(at) => usize::from(at),
        _ => unreachable!(),
    };
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
    let static_entry = program
        .debug
        .functions
        .iter()
        .find(|f| f.name == "demo_record_static")
        .map(|f| f.addr.0 as u16);
    let append_entry = program
        .debug
        .functions
        .iter()
        .find(|f| f.name == "gpu_group_part_append")
        .map(|f| f.addr.0 as u16);
    for _ in 0..8_000_000 {
        {
            let mut s = shared.borrow_mut();
            if Some(cpu.pc()) == static_entry {
                s.record_calls += 1;
            }
            if Some(cpu.pc()) == append_entry {
                s.append_calls += 1;
            }
            if matches!(s.phase, 9 | 13 | 14 | 16) {
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
        serde_json::from_str(include_str!("../../fixtures/public_handheld_phase0.json")).unwrap();
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
    let source = include_str!("../../../examples/demo_box_rabbit/rcc/public_main.rs").replace(
        "demo_run(0, 2000, GPU_DEMO_AUTO_ORBIT != 0);",
        "demo_run(3, 2000, false);",
    );
    assert!(source.contains("demo_run(3, 2000, false);"));
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
            baseline.submissions, 7,
            "one startup plus two STARTs per frame"
        );
        assert_eq!(baseline.dynamic_prepared_while_busy, [true; 3]);
        assert_eq!(baseline.record_calls, 1);
        assert_eq!(baseline.append_calls, 3);
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
            .all(|p| matches!(*p, 13 | 14 | 16)));
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
        std::fs::write(
            out.join(format!("presentation-clock-{debug}.json")),
            serde_json::to_vec_pretty(&serde_json::json!({
                "baseline":baseline.observed,"slow":slow.observed,
                "baseline_counter_samples":baseline.counter_samples,
                "slow_counter_samples":slow.counter_samples,
                "clock":"1us per100 preparation steps;10us per waiting step",
            }))
            .unwrap(),
        )
        .unwrap();
        for sample in baseline.counter_samples.iter().chain(&slow.counter_samples) {
            if sample[0] == 1 {
                assert_eq!(
                    sample[3], 13,
                    "timer sampled before the presentation phase14"
                );
            }
        }
        assert_eq!(baseline.observed.len(), slow.observed.len());
        // Present polling has discrete10us steps. Changed absolute phase can
        // move the following1us-quantized endpoint by one tick. Exact Device0
        // sample/report equality is independently checked by Peer::write.
        for (normal, delayed) in baseline.observed.iter().zip(&slow.observed) {
            assert!(
                normal.abs_diff(*delayed) <= 1,
                "presentation excluded from interval: {normal} vs {delayed}"
            );
        }
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

// Qualification consumes the accepted caller outside the candidate source tree.
// It is opt-in so normal tests do not depend on Git history or preserved target files.
#[test]
#[ignore = "requires STAGE2_UNSPLIT_SOURCE frozen accepted caller; bounded CPU/CB comparison only"]
fn stage2_frame2_matches_frozen_unsplit_inputs_and_rotates_only_rabbit() {
    let frozen = std::env::var_os("STAGE2_UNSPLIT_SOURCE").expect("frozen accepted source path");
    let identity = std::process::Command::new("git")
        .args(["hash-object", "--no-filters"])
        .arg(&frozen)
        .output()
        .unwrap();
    assert!(identity.status.success());
    assert_eq!(
        String::from_utf8(identity.stdout).unwrap().trim(),
        "a03812f24174b793d25e90d94d19cc9013902039",
        "accepted89b caller blob"
    );
    let reference = std::fs::read_to_string(frozen).unwrap();
    let candidate = include_str!("../../../examples/demo_box_rabbit/rcc/public_main.rs");
    let helpers = r#"
fn stage2_camera(camera:&mut Camera){camera.set_pose(
 demo_point(fix32::from_words(16384,0),fix32::from_words(14418,0),fix32::from_words(52429,2)),
 demo_point(fix32::from_words(3277,0),fix32::from_words(24576,3),fix32::zero()));}
fn stage2_rabbit(model:&mut Transform){model.set_pose(demo_pose(demo_zero(),
 demo_point(fix32::zero(),fix32::from_words(8192,0),fix32::zero()),demo_scale()));}
fn stage2_identity()->Transform{Transform::new(demo_pose(demo_zero(),demo_zero(),demo_scale()))}
"#;
    let camera = "        if frame == 1 { stage2_camera(&mut camera); }\n";
    let rabbit = "        if frame == 1 { stage2_rabbit(&mut rabbit_model); }\n";
    fn fixed_ui(source: &str) -> String {
        let first = source.find("fn prepare_ui(").unwrap();
        let end = first + source[first..].find("\n}").unwrap() + 2;
        let stats = if source[first..end].contains("stats: Array<u16>") {
            ",_stats:Array<u16>"
        } else {
            ""
        };
        format!(
            "{}fn prepare_ui(grid:&mut UiGrid{stats})->u16{{grid.fill(0);demo_glyph(grid,2,1,3);1}}{}",
            &source[..first],
            &source[end..]
        )
    }
    let mut reference = fixed_ui(&reference)
        .replace("demo_run(0, 2000);", "demo_run(2, 2000);")
        .replace(
            "let model = Model::new(demo_pose(demo_zero(), demo_zero(), demo_scale()));",
            "let model=stage2_identity();let mut rabbit_model=stage2_identity();",
        )
        .replace(
            "pass.draw(&rabbit, &model, &lit, &view);",
            "pass.draw(rabbit_model.ptr(), rabbit.ptr(), lit.ptr(), &view);",
        );
    assert_eq!(reference.matches("let mut rabbit_model=").count(), 1);
    reference = reference.replace(
        "        let mut glyphs: u16 = 0;",
        &format!("{camera}{rabbit}        let mut glyphs: u16 = 0;"),
    );
    reference.push_str(helpers);
    let mut candidate = fixed_ui(candidate)
        .replace(
            "demo_run(0, 2000, GPU_DEMO_AUTO_ORBIT != 0);",
            "demo_run(2, 2000, false);",
        )
        .replace(
            "let rabbit_model = Transform::new(Pose::identity());",
            "let mut rabbit_model = Transform::new(Pose::identity());",
        );
    let anchor =
        "        // The immutable part supplies World/material, the new Pass supplies View.";
    assert_eq!(candidate.matches(anchor).count(), 1);
    candidate = candidate.replace(anchor, &format!("{camera}{anchor}"));
    let anchor = "        // Only dynamic CPU storage is touched while the static GPU work runs.";
    candidate = candidate.replace(anchor, &format!("{anchor}\n{rabbit}"));
    candidate.push_str(helpers);
    let out = assets("stage2-equivalence");
    std::fs::write(out.join("reference.rs"), &reference).unwrap();
    std::fs::write(out.join("candidate.rs"), &candidate).unwrap();
    let mut profile_gold = None;
    for debug in [false, true] {
        let old = compile(&reference, &out, debug);
        std::fs::copy(
            out.join(format!("budget-{debug}.json")),
            out.join(format!("budget-reference-{debug}.json")),
        )
        .unwrap();
        let new = compile(&candidate, &out, debug);
        std::fs::copy(
            out.join(format!("budget-{debug}.json")),
            out.join(format!("budget-candidate-{debug}.json")),
        )
        .unwrap();
        let old = run(
            &old,
            &out,
            State {
                unsplit: true,
                gpu_delay: 5000,
                present_delay: 40000,
                ..State::default()
            },
            0x600d,
        );
        let new = run(
            &new,
            &out,
            State {
                gpu_delay: 5000,
                present_delay: 40000,
                ..State::default()
            },
            0x600d,
        );
        assert_eq!(old.frames.len(), 2);
        assert_eq!(new.frames.len(), 2);
        assert_eq!(new.record_calls, 1);
        assert_eq!(new.append_calls, 2);
        assert_eq!(new.submissions, 5);
        assert_eq!(old.submissions, 2);
        assert_eq!(new.swaps, old.swaps);
        for frame in 0..2 {
            assert_eq!(
                new.frames[frame].trace, old.frames[frame].trace,
                "complete ordered DRAW native state frame{frame}/{debug}"
            );
            assert_eq!(
                new.frames[frame].ui_descriptors, old.frames[frame].ui_descriptors,
                "full fixed-input UI state/data, without a pixel mask"
            );
        }
        assert_ne!(
            new.frames[0].matrix, new.frames[1].matrix,
            "frame2 Camera moves and turns"
        );
        assert_ne!(
            new.frames[0].global, new.frames[1].global,
            "frame2 view-space Global light changes"
        );
        assert_ne!(
            new.frames[1].world, new.frames[1].rabbit_matrix,
            "independent frame2 rabbit Y transform"
        );
        let healthy: Vec<_> = new
            .frames
            .iter()
            .map(|f| (&f.trace, &f.ui_descriptors))
            .collect();
        let serialized = serde_json::to_vec(&healthy).unwrap();
        if let Some(gold) = &profile_gold {
            assert_eq!(
                &serialized, gold,
                "both debug profiles emit equal healthy state"
            );
        }
        profile_gold = Some(serialized.clone());
        std::fs::write(out.join(format!("native-state-{debug}.json")), serialized).unwrap();
    }
}

fn functional_planes(
    program: &rcc_backend::CpuV3Program,
    out: &Path,
    debug: bool,
    label: &str,
) -> Vec<Vec<u16>> {
    use cpu_v3_tang_nano_20k::{
        boot,
        system_sim::{CpuV3SystemSim, VBlankMode},
    };
    let budget: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join(format!("budget-{debug}.json"))).unwrap())
            .unwrap();
    let static_words = budget["static_words"].as_u64().unwrap() as usize;
    let catalog: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("assets.json")).unwrap()).unwrap();
    let resources: Vec<_> = catalog["resources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            boot::BootResource::copy(
                r["address"].as_u64().unwrap() as usize,
                std::fs::read(out.join(r["file"].as_str().unwrap())).unwrap(),
            )
        })
        .collect();
    let code: Vec<u8> = program.words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let image = boot::pack_application_resources(&code, static_words, &resources).unwrap();
    let mut sim =
        CpuV3SystemSim::from_single_flash(&image, 0x5aa5, VBlankMode::PauseOnFrameIndexWait)
            .unwrap()
            .with_functional_gpu();
    let mut halted = false;
    let mut retired_planes = Vec::new();
    for _ in 0..8_000_000 {
        assert!(
            sim.gpu().last_error().is_none(),
            "functional GPU {label}: {:?}",
            sim.gpu().last_error()
        );
        if sim.waiting_for_vblank() && sim.advance_vblank() {
            let frame = retired_planes.len() / 2 + 1;
            let color = sim.display_state().active_base as usize;
            let memory = sim.cpu_mut().physical_memory_mut();
            for (name, base) in [("color", color), ("depth", 0x240000)] {
                let words = memory[base..base + 131072].to_vec();
                assert!(words[102400..].iter().all(|w| *w == 0x5aa5));
                std::fs::write(
                    out.join(format!("{label}-frame{frame}-{name}-{debug}.bin")),
                    words
                        .iter()
                        .flat_map(|w| w.to_le_bytes())
                        .collect::<Vec<_>>(),
                )
                .unwrap();
                retired_planes.push(words);
            }
        }
        if let StepOutcome::Halted { signal } = sim.step().unwrap() {
            assert_eq!(signal, 0x600d);
            halted = true;
            break;
        }
    }
    assert!(halted, "8M instruction functional renderer bound");
    assert_eq!(
        retired_planes.len(),
        4,
        "capture each completed frame's color and depth"
    );
    assert_eq!(
        sim.display_state().active_base,
        0x200000,
        "two completed frames"
    );
    let memory = sim.cpu_mut().physical_memory_mut();
    let mut planes = Vec::new();
    for (name, base) in [
        ("color-a", 0x200000),
        ("color-b", 0x220000),
        ("depth", 0x240000),
    ] {
        let plane = memory[base..base + 131072].to_vec();
        assert!(
            plane[102400..].iter().all(|v| *v == 0x5aa5),
            "full allocation guard {name}"
        );
        std::fs::write(
            out.join(format!("{label}-{name}-{debug}.bin")),
            plane
                .iter()
                .flat_map(|w| w.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        planes.push(plane);
    }
    planes.extend(retired_planes);
    planes
}

#[test]
fn rolling_rabbit_slices_match_flat_draws_across_wrap() {
    let out = assets("rabbit-slice-wrap");
    let source = include_str!("../../../examples/demo_box_rabbit/rcc/public_main.rs")
        .replace(
            "demo_run(0, 2000, GPU_DEMO_AUTO_ORBIT != 0);",
            "demo_run(2, 2000, false);",
        )
        .replace(
            "let visible = demo_buttons_update(",
            "let visible = false && demo_buttons_update(",
        );
    let mode = "let slice_demo = GPU_DEMO_RABBIT_SLICES != 0;";
    let sliced = source
        .replace(mode, "let slice_demo = true;")
        .replace(
            "const RABBIT_SLICE_HOLD: u16 = 16;",
            "const RABBIT_SLICE_HOLD: u16 = 1;",
        )
        .replace(
            "let mut slice_start: u16 = 0;",
            "let mut slice_start: u16 = 5;",
        );
    let flat = source.replace(mode, "let slice_demo = false;").replace(
        "pass.draw(rabbit_model.ptr(), rabbit, lit.ptr(), &view);",
        r#"if frame == 0 {
            let tail = rabbit.slice(15, 2);
            let head = rabbit.slice(0, 6);
            pass.draw(rabbit_model.ptr(), tail, lit.ptr(), &view);
            pass.draw(rabbit_model.ptr(), head, lit.ptr(), &view);
        } else {
            let head = rabbit.slice(0, 9);
            pass.draw(rabbit_model.ptr(), head, lit.ptr(), &view);
        }"#,
    );
    let listed = functional_planes(&compile(&sliced, &out, false), &out, false, "list");
    let reference = functional_planes(&compile(&flat, &out, false), &out, false, "flat");
    for (index, (actual, expected)) in listed.iter().zip(&reference).enumerate() {
        let different = actual.iter().zip(expected).filter(|(a, b)| a != b).count();
        assert_eq!(different, 0, "framebuffer/depth plane {index}");
    }
    assert!(
        listed[3] != listed[5],
        "the selected rabbit geometry must change"
    );
}

#[test]
fn rolling_ring_slices_light_both_sides_of_the_seam() {
    use cpu_v3_tang_nano_20k::framebuffer_word_at;

    let out = assets_with_motion("ring-slice-wrap", "ring");
    let source = include_str!("../../../examples/demo_box_rabbit/rcc/public_main.rs")
        .replace(
            "demo_run(0, 2000, GPU_DEMO_AUTO_ORBIT != 0);",
            "demo_run(2, 2000, false);",
        )
        .replace(
            "const RING_SLICE_HOLD: u16 = 8;",
            "const RING_SLICE_HOLD: u16 = 1;",
        )
        .replace(
            "let mut slice_start: u16 = 0;",
            "let mut slice_start: u16 = GPU_DEMO_RING_SECTIONS - 1;",
        );
    let planes = functional_planes(&compile(&source, &out, false), &out, false, "ring");
    let pixel = |frame: usize, x, y| planes[3 + frame * 2][framebuffer_word_at(0, x, y) as usize];
    let luminance = |color: u16| (color >> 11) + ((color >> 5) & 63) + (color & 31);
    for x in 192..=208 {
        assert!(
            luminance(pixel(0, x, 37)) > 45,
            "bright across the seam at x={x}"
        );
    }
    assert!(
        (1..30).contains(&luminance(pixel(1, 208, 37))),
        "tail returns to track"
    );
    assert!(luminance(pixel(1, 192, 37)) > 45, "head remains bright");
}

#[test]
#[ignore = "requires STAGE2_PREPARED_SOURCE/controlled frozen reference; raw functional color/depth qualification"]
fn stage2_functional_raw_color_depth_match_same_inputs_without_masks() {
    let path = PathBuf::from(
        std::env::var_os("STAGE2_PREPARED_SOURCE").expect("controlled candidate source"),
    );
    let candidate = std::fs::read_to_string(&path).unwrap();
    let reference = std::fs::read_to_string(path.parent().unwrap().join("reference.rs")).unwrap();
    assert!(candidate.contains("stage2_rabbit(&mut rabbit_model)"));
    assert!(reference.contains("stage2_rabbit(&mut rabbit_model)"));
    let out = qualification_assets("stage2-functional-raw");
    for debug in [false, true] {
        let old = compile(&reference, &out, debug);
        let old = functional_planes(&old, &out, debug, "reference");
        let new = compile(&candidate, &out, debug);
        let new = functional_planes(&new, &out, debug, "candidate");
        assert_eq!(
            old, new,
            "all raw colorA/colorB/depth words and tails {debug}; no ROI mask"
        );
        assert!(new[0][..102400].iter().any(|v| *v != 0 && *v != 0x5aa5));
        std::fs::write(out.join(format!("PASS-{debug}.json")),serde_json::to_vec_pretty(&serde_json::json!({
            "planes":["color-a","color-b","depth"],"words_each":131072,"physical_words":102400,
            "tail_words":28672,"final_allocation_words_equal":393216,"retired_frame_allocation_words_equal":524288,
            "frames":2,"depth_checked_each_frame":true,"controlled_frame2_camera_and_rabbit_y":true,
            "model":"functional GPU, no native hardware timing or display scanout qualification",
        })).unwrap()).unwrap();
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
let mut p=DemoMotion::new(65530);save(0,p);
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
if n==11{p=DemoMotion::new(65530);y=-one;now=4;}
let input=pad(x,y,rx,ry,flags);p.update(&input,now);save(n,p);n+=1;}
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
mod gpu_api;use gpu_api::*;mod gpu_demo_api;
mod gpu_board;use gpu_board::*;use gpu_demo_api::*;
static OUT:Buf<u16,600>=Buf::new([0;600]);
fn row(at:u16,v:Vec4){OUT[at]=v.x.lo_bits();OUT[at+1]=v.x.hi_bits();
OUT[at+2]=v.y.lo_bits();OUT[at+3]=v.y.hi_bits();
OUT[at+4]=v.z.lo_bits();OUT[at+5]=v.z.hi_bits();
OUT[at+6]=v.w.lo_bits();OUT[at+7]=v.w.hi_bits();}
fn main(){let gpu=Gpu::init(board_gpu_config());let mut camera=demo_camera();
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
