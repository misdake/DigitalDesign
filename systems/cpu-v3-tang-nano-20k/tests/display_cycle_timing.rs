//! Timing/control qualification: short production RTL plus preserved pre-candidate
//! fill pin tapes, replayed from declaration cold state with every real edge.
use cpu_v3_tang_nano_20k::hardware::display_cycle::{
    ControlInput, DisplayControl, Edges, FillOutput, Timing, TimingInput, TimingOutput,
};
use cpu_v3_tang_nano_20k::{DisplayLineBuffer, FramebufferHdmi};
use digital_design_hardware::{HardwareIdentity, Module};
use std::{
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn decode(word: u128) -> ControlInput {
    let mut shift = 97u32;
    let mut take = |width: u32| {
        shift -= width;
        (word >> shift) & ((1u128 << width) - 1u128)
    };
    ControlInput {
        reset: take(1) != 0,
        video_locked: take(1) != 0,
        boot_enable: take(1) != 0,
        boot_failed: take(1) != 0,
        memory_request_ready: take(1) != 0,
        memory_data_valid: take(1) != 0,
        memory_read_data: take(64) as u64,
        memory_last: take(1) != 0,
        memory_error: take(1) != 0,
        device_index: take(3) as u8,
        device_channel: take(4) as u8,
        device_read_enable: take(1) != 0,
        device_write_enable: take(1) != 0,
        device_write_data: take(16) as u16,
    }
}
fn from_fill(word: u128) -> ControlInput {
    decode(((word >> 100) << 96) | (1u128 << 95) | ((word >> 5) & ((1u128 << 95) - 1)))
}
fn logic(o: FillOutput) -> u128 {
    [
        (u128::from(o.memory_request_valid), 1),
        (u128::from(o.memory_urgent), 1),
        (u128::from(o.memory_address), 22),
        (u128::from(o.line_write), 1),
        (u128::from(o.line_write_address), 9),
        (
            u128::from(if o.line_write { o.line_write_data } else { 0 }),
            64,
        ),
        (u128::from(o.published), 2),
        (u128::from(o.group_valid), 2),
        (u128::from(o.group_linear), 2),
        (u128::from(o.device_read_data), 16),
        (u128::from(o.underflow), 1),
        (u128::from(o.started_ack), 1),
        (u128::from(o.startup_error), 1),
        (u128::from(o.frame_event), 1),
    ]
    .into_iter()
    .fold(0, |v, (p, w)| (v << w) | p)
}
fn pixel(o: TimingOutput) -> u64 {
    [
        (u64::from(o.h_count), 11),
        (u64::from(o.v_count), 10),
        (u64::from(o.hsync), 1),
        (u64::from(o.vsync), 1),
        (u64::from(o.active), 1),
        (u64::from(o.released), 2),
        (u64::from(o.frame_toggle), 1),
        (u64::from(o.started), 1),
        (u64::from(o.underflow), 1),
        (u64::from(o.slot), 1),
        (u64::from(o.second), 1),
        (u64::from(o.vertical_repeat), 1),
        (u64::from(o.scan_valid), 1),
        (u64::from(o.scan_line_ready), 1),
        (u64::from(o.scan_linear), 1),
        (u64::from(o.pixel_reset), 1),
        (u64::from(o.publish_sync), 2),
        (u64::from(o.enable_sync), 2),
        (u64::from(o.failure_sync), 2),
        (u64::from(o.reset_sync), 3),
    ]
    .into_iter()
    .fold(0, |v, (p, w)| (v << w) | p)
}
fn run(command: &mut Command, out: &Path, name: &str) -> bool {
    let argv: Vec<_> = std::iter::once(command.get_program())
        .chain(command.get_args())
        .map(|s| s.to_string_lossy().into_owned())
        .collect();
    fs::write(
        out.join(format!("{name}.command.json")),
        serde_json::to_string_pretty(&argv).unwrap(),
    )
    .unwrap();
    command
        .stdout(Stdio::from(
            fs::File::create(out.join(format!("{name}.stdout"))).unwrap(),
        ))
        .stderr(Stdio::from(
            fs::File::create(out.join(format!("{name}.stderr"))).unwrap(),
        ));
    let mut child = command.spawn().unwrap();
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status.success();
        }
        if start.elapsed() > Duration::from_secs(30) {
            child.kill().unwrap();
            let _ = child.wait();
            panic!("{name} 30s wall cap");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn source() -> String {
    FramebufferHdmi::verilog_source().unwrap().replace(
        &DisplayLineBuffer::verilog_identity().module_name(),
        "DisplayLineBuffer",
    )
}
fn compile(out: &Path, name: &str, tb: &str, dut: &str) -> PathBuf {
    fs::write(out.join(format!("{name}-tb.v")), tb).unwrap();
    fs::write(out.join(format!("{name}.v")), dut).unwrap();
    let exe = out.join(format!("{name}.vvp"));
    let mut c = Command::new(std::env::var("IVERILOG_EXE").unwrap_or_else(|_| "iverilog".into()));
    c.args(["-g2012", "-s", "tb", "-o"])
        .arg(&exe)
        .arg(out.join(format!("{name}-tb.v")))
        .arg(out.join(format!("{name}.v")))
        .arg(out.join("line.v"));
    assert!(run(&mut c, out, &format!("{name}-compile")));
    exe
}
fn short_compare(path: &Path) -> Result<usize, String> {
    let mut control = DisplayControl::default();
    let mut events = 0;
    for row in BufReader::new(fs::File::open(path).unwrap()).lines() {
        let row = row.unwrap();
        let w: Vec<_> = row.split_whitespace().collect();
        let mask = w[1].parse::<u8>().unwrap();
        let input = decode(u128::from_str_radix(w[2], 16).unwrap());
        let expected = u128::from_str_radix(w[3], 16).unwrap();
        let expected_pixel = u64::from_str_radix(w[4], 16).unwrap();
        let out = control.outputs(&input);
        if logic(out.logic) != expected || pixel(out.pixel) != expected_pixel {
            return Err(format!("TIMING_PORT event={events} mask={mask} expected={expected:032x}/{expected_pixel:016x} actual={:032x}/{:016x} state={control:?}",logic(out.logic),pixel(out.pixel)));
        }
        if w[0] == "E" {
            control.clock(
                Edges {
                    logic_rise: mask & 1 != 0,
                    pixel_rise: mask & 2 != 0,
                },
                &input,
            );
            events += 1;
        }
        if events > 4000 {
            return Err("event cap".into());
        }
    }
    Ok(events)
}
#[test]
fn no_edges_leave_control_state_and_cold_ram_unchanged() {
    let mut control = DisplayControl::default();
    let input = ControlInput::default();
    let before = control.outputs(&input);
    for _ in 0..4 {
        control.clock(Edges::default(), &input);
        assert_eq!(control.outputs(&input), before);
    }
    assert!(!control.line_word(0, 0).known);
    assert!(control.line_word(1, 416).known);
}
#[test]
#[ignore = "bounded full production pixel timing/CDC and same-edge error RTL"]
fn production_timing_short_edges_and_mutants() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = root.join("target/display-cycle-timing");
    fs::create_dir_all(&out).unwrap();
    fs::write(
        out.join("line.v"),
        DisplayLineBuffer::verilog_source().unwrap(),
    )
    .unwrap();
    let source = source();
    let variants = [
        ("production", source.clone(), true),
        (
            "reset_too_early",
            source.replace(
                "wire pixel_reset = ~pixel_reset_sync[2];",
                "wire pixel_reset = ~pixel_reset_sync[1];",
            ),
            false,
        ),
        (
            "short_publish_cdc",
            source.replace("publish_sync<=publish_meta;", "publish_sync<=published;"),
            false,
        ),
        (
            "raw_error_same_edge",
            source.replace(
                "boot_failed | memory_error_sticky",
                "boot_failed | memory_error",
            ),
            false,
        ),
    ];
    let mut results = Vec::new();
    for (name, source, positive) in variants {
        let exe = compile(
            &out,
            name,
            include_str!("display_cycle_timing_tb.v"),
            &source,
        );
        for phase in if positive { vec![0, 17] } else { vec![17] } {
            let label = format!("{name}-{phase}");
            let trace = out.join(format!("{label}.trace"));
            let mut c = Command::new(std::env::var("VVP_EXE").unwrap_or_else(|_| "vvp".into()));
            c.arg(&exe)
                .arg(format!("+phase={phase}"))
                .arg(format!("+trace={}", trace.display()));
            assert!(run(&mut c, &out, &label));
            let result = short_compare(&trace);
            if positive {
                let n = result.unwrap();
                assert!(n > 2500);
                results.push(serde_json::json!({"case":label,"events":n}));
            } else {
                let diff = result.expect_err("DUT mutation escaped");
                assert!(diff.starts_with("TIMING_PORT"));
                fs::write(out.join(format!("{label}.diff")), &diff).unwrap();
                results.push(serde_json::json!({"case":label,"detected":diff}));
            }
        }
    }
    fs::write(
        out.join("short-manifest.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "scope":"timing/control cone, no color reads/TMDS/MC","pixel_edges":1200,
            "max_events":4000,"wall_seconds_per_process":30,"results":results
        }))
        .unwrap(),
    )
    .unwrap();
}

#[derive(Clone)]
struct Replay {
    control: DisplayControl,
    pins: ControlInput,
    next_logic: u64,
    next_pixel: u64,
    phase: u64,
}
impl Replay {
    fn new(phase: u64) -> Self {
        Self {
            control: DisplayControl::default(),
            pins: ControlInput::default(),
            next_logic: 37,
            next_pixel: 60 + phase,
            phase,
        }
    }
    fn pixels_before(&mut self, t: u64) {
        while self.next_pixel < t {
            self.control.clock(
                Edges {
                    logic_rise: false,
                    pixel_rise: true,
                },
                &self.pins,
            );
            self.next_pixel += 120;
        }
    }
    fn tick(&mut self) -> Edges {
        let t = self.next_logic.min(self.next_pixel);
        let edges = Edges {
            logic_rise: t == self.next_logic,
            pixel_rise: t == self.next_pixel,
        };
        self.control.clock(edges, &self.pins);
        if edges.logic_rise {
            self.next_logic += 74;
        }
        if edges.pixel_rise {
            self.next_pixel += 120;
        }
        edges
    }
}
fn feedback(control: &DisplayControl, pins: &ControlInput, raw: u128, expected: u128, n: usize) {
    let o = control.outputs(pins);
    assert_eq!(
        (
            o.pixel.released,
            o.pixel.frame_toggle,
            o.pixel.underflow,
            o.pixel.started
        ),
        (
            ((raw >> 3) & 3) as u8,
            raw & 4 != 0,
            raw & 2 != 0,
            raw & 1 != 0
        ),
        "pixel feedback logic edge {n}"
    );
    assert_eq!(logic(o.logic), expected, "control ports logic edge {n}");
    if pins.device_read_enable && pins.device_index == 3 && pins.device_channel == 3 {
        assert_eq!(
            control.memory_error_sticky(),
            expected & (1 << 8) != 0,
            "raw sticky accessor edge {n}"
        );
    }
}
fn replay(path: &Path, phase: u64) -> Replay {
    let mut r = Replay::new(phase);
    let mut n = 0usize;
    let mut cells = 0;
    let mut finals = 0;
    let mut summary = false;
    for row in BufReader::new(fs::File::open(path).unwrap()).lines() {
        let row = row.unwrap();
        let w: Vec<_> = row.split_whitespace().collect();
        match w[0] {
            "E" => {
                let count = w[1].parse::<usize>().unwrap();
                let raw = u128::from_str_radix(w[2], 16).unwrap();
                let expected = u128::from_str_radix(w[3], 16).unwrap();
                for _ in 0..count {
                    let rise = r.next_logic;
                    let fall = rise - 37;
                    r.pixels_before(fall + 1);
                    r.pins = from_fill(raw);
                    r.pixels_before(rise);
                    feedback(&r.control, &r.pins, raw, expected, n);
                    let together = r.next_pixel == rise;
                    r.control.clock(
                        Edges {
                            logic_rise: true,
                            pixel_rise: together,
                        },
                        &r.pins,
                    );
                    if together {
                        r.next_pixel += 120;
                    }
                    r.next_logic += 74;
                    n += 1;
                    assert!(n <= 4_000_000);
                }
            }
            "F" => {
                r.pixels_before(r.next_logic - 37);
                let raw = u128::from_str_radix(w[1], 16).unwrap();
                r.pins = from_fill(raw);
                feedback(
                    &r.control,
                    &r.pins,
                    raw,
                    u128::from_str_radix(w[2], 16).unwrap(),
                    n,
                );
                finals += 1;
            }
            "M" => {
                let address = w[1].parse::<u16>().unwrap();
                for bank in 0..2 {
                    let expected = u32::from_str_radix(w[bank + 2], 16).ok();
                    let actual = r.control.line_word(bank, address);
                    assert_eq!(
                        actual.known.then_some(actual.value),
                        expected,
                        "RAM {address} bank {bank}"
                    );
                }
                cells += 1;
            }
            "S" => {
                assert_eq!(w[1].parse::<usize>().unwrap(), n);
                summary = true;
            }
            _ => panic!("bad trace row"),
        }
    }
    assert!(summary && cells == 400 && finals == 5);
    r
}
fn next_pixel(r: &mut Replay) -> TimingOutput {
    for _ in 0..4 {
        if r.tick().pixel_rise {
            return r.control.outputs(&r.pins).pixel;
        }
    }
    panic!("pixel event bound");
}
fn failure_feedback(mut r: Replay) {
    let initial = r.control.outputs(&r.pins);
    assert!(initial.pixel.underflow && initial.logic.startup_error);
    assert!(!r.control.memory_error_sticky() && !r.pins.boot_failed);
    for _ in 0..16 {
        assert_eq!(next_pixel(&mut r).failure_sync, 0);
    }
    let mut boot = r.clone();
    boot.pins.boot_failed = true;
    assert_eq!(next_pixel(&mut boot).failure_sync, 1);
    assert_eq!(next_pixel(&mut boot).failure_sync, 3);
    // Accept a real pending request before a terminal response error.
    let mut error = r;
    error.pins.memory_request_ready = true;
    let mut accepted = false;
    for _ in 0..4 {
        let requesting = error
            .control
            .outputs(&error.pins)
            .logic
            .memory_request_valid;
        if error.tick().logic_rise && requesting {
            accepted = true;
            break;
        }
    }
    assert!(accepted);
    error.pins.memory_request_ready = false;
    let mut common = error.next_logic + 148;
    let residue = (60 + error.phase) % 120;
    for _ in 0..60 {
        if common % 120 == residue {
            break;
        }
        common += 74;
    }
    assert_eq!(common % 120, residue);
    for _ in 0..200 {
        if error.next_logic.min(error.next_pixel) >= common {
            break;
        }
        error.tick();
    }
    assert_eq!(error.next_logic, common);
    assert_eq!(error.next_pixel, common);
    error.pins.memory_error = true;
    let edges = error.tick();
    assert!(edges.logic_rise && edges.pixel_rise);
    assert!(error.control.memory_error_sticky());
    assert_eq!(
        error.control.outputs(&error.pins).pixel.failure_sync,
        0,
        "coincident pixel must sample old sticky"
    );
    error.pins.memory_error = false;
    assert_eq!(next_pixel(&mut error).failure_sync, 1);
    assert_eq!(next_pixel(&mut error).failure_sync, 3);
}
#[test]
#[ignore = "requires DISPLAY_CYCLE_FILL_EVIDENCE qualified eight-case source bundle"]
fn cold_control_replay_and_underflow_failure_feedback() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = root.join("target/display-cycle-timing");
    fs::create_dir_all(&out).unwrap();
    let origin = PathBuf::from(
        std::env::var("DISPLAY_CYCLE_FILL_EVIDENCE")
            .expect("point to qualified fill target directory"),
    );
    let imported = out.join("imported");
    let mut c = Command::new("python");
    c.arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/display_cycle_timing_import.py"))
        .arg("--source")
        .arg(&origin)
        .arg("--out")
        .arg(&imported);
    assert!(run(&mut c, &out, "import"));
    assert_eq!(
        fs::read_to_string(imported.join("production.v")).unwrap(),
        source()
    );
    assert_eq!(
        fs::read_to_string(imported.join("line.v")).unwrap(),
        DisplayLineBuffer::verilog_source().unwrap()
    );
    let binding: serde_json::Value =
        serde_json::from_slice(&fs::read(imported.join("binding.json")).unwrap()).unwrap();
    for (name, value) in binding["files"].as_object().unwrap() {
        let hash = Command::new("git")
            .args(["hash-object", "--no-filters"])
            .arg(imported.join(name))
            .output()
            .unwrap();
        assert!(hash.status.success());
        assert_eq!(
            String::from_utf8(hash.stdout).unwrap().trim(),
            value["git_blob"].as_str().unwrap()
        );
    }
    let mut results = Vec::new();
    for case in binding["cases"].as_array().unwrap() {
        let label = case["case"].as_str().unwrap();
        let phase = label.rsplit('-').next().unwrap().parse().unwrap();
        let r = replay(&imported.join(format!("{label}.trace")), phase);
        if label == "production-2-17" {
            failure_feedback(r.clone());
        }
        results
            .push(serde_json::json!({"case":label,"logic_edges":(r.next_logic-37)/74,"pass":true}));
    }
    fs::write(out.join("replay-manifest.json"),serde_json::to_string_pretty(&serde_json::json!({
        "scope":"cold control-cone replay of preserved real RTL pin tapes, no native MC or color/TMDS",
        "each_logic_and_pixel_edge_executed":true,"max_logic_edges_per_tape":4000000,
        "feedback_assertions":"underflow only stays clear; boot/raw error two pixel stages; coincident raw error reads old sticky",
        "results":results
    })).unwrap()).unwrap();
}

#[test]
#[ignore = "verbatim production failure crossing and underflow-only DUT mutation"]
fn production_failure_feedback_leaf() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = root.join("target/display-cycle-timing");
    fs::create_dir_all(&out).unwrap();
    fs::write(
        out.join("line.v"),
        DisplayLineBuffer::verilog_source().unwrap(),
    )
    .unwrap();
    let source = source().replace("\r\n", "\n");
    let start = source
        .find("always @(posedge pixel_clock) begin\n    enable_sync")
        .unwrap();
    let end = start + source[start..].find("\nend").unwrap() + 4;
    let block = &source[start..end];
    let wrong = block.replace(
        "boot_failed | memory_error_sticky",
        "boot_failed | memory_error_sticky | underflow_sync",
    );
    assert_ne!(wrong, block);
    for (name, body, positive) in [
        ("failure_production", block.to_owned(), true),
        ("startup_feedback", wrong, false),
    ] {
        let dut=format!("module FailureLeaf(input pixel_clock,boot_enable,boot_failed,memory_error_sticky,underflow_sync,output reg [1:0] enable_sync=0,failure_sync=0);\n{body}\nendmodule\n");
        let exe = compile(
            &out,
            name,
            include_str!("display_cycle_timing_failure_tb.v"),
            &dut,
        );
        let mut c = Command::new(std::env::var("VVP_EXE").unwrap_or_else(|_| "vvp".into()));
        c.arg(exe);
        let success = run(&mut c, &out, name);
        let stdout = fs::read_to_string(out.join(format!("{name}.stdout"))).unwrap();
        if positive {
            assert!(success && stdout.contains("PASS FAILURE"), "{stdout}");
        } else {
            assert!(
                !success && stdout.contains("FAILURE_PIPE"),
                "underflow route mutation escaped: {stdout}"
            );
        }
    }
    let mut timing = Timing::default();
    let golden = [0u8, 0, 0, 1, 3, 2, 0, 1, 3, 2, 0];
    for (edge, expected) in golden.into_iter().enumerate() {
        timing.clock(&TimingInput {
            video_locked: true,
            boot_failed: edge == 3 || edge == 4,
            memory_error: edge == 7 || edge == 8,
            ..Default::default()
        });
        assert_eq!(
            timing.outputs().failure_sync,
            expected,
            "pixel stage edge {edge}"
        );
    }
    fs::write(out.join("failure-manifest.json"),serde_json::to_string_pretty(&serde_json::json!({
        "source":"verbatim production enable/failure always block",
        "cases":["underflow-only excluded","boot_failed two pixel stages","raw memory error two pixel stages"],
        "real_dut_mutation":"OR underflow_sync into failure input: rejected at edge0",
        "max_pixel_edges":11,"wall_seconds_per_process":30
    })).unwrap()).unwrap();
}
