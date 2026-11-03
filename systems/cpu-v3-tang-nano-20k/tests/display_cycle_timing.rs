//! Timing/control qualification with fresh tracked Fill inputs and real RTL edges.
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

#[path = "support/display_fill_qualification.rs"]
mod fill_qualification;
#[path = "support/display_test_output.rs"]
mod test_output;

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
    run_bounded(command, out, name, 30)
}
fn run_bounded(command: &mut Command, out: &Path, name: &str, seconds: u64) -> bool {
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
        if start.elapsed() > Duration::from_secs(seconds) {
            child.kill().unwrap();
            let _ = child.wait();
            panic!("{name} {seconds}s wall cap");
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
    let out = test_output::fresh_output("display-cycle-timing");
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
    video_expected: std::collections::VecDeque<(u64, u64)>,
    video_samples: usize,
    video_source: usize,
    tmds_expected: std::collections::VecDeque<(u64, [u16; 3], [u8; 3])>,
    tmds_samples: usize,
}
impl Replay {
    fn new(phase: u64) -> Self {
        Self {
            control: DisplayControl::default(),
            pins: ControlInput::default(),
            next_logic: 37,
            next_pixel: 60 + phase,
            phase,
            video_expected: Default::default(),
            video_samples: 0,
            video_source: 0,
            tmds_expected: Default::default(),
            tmds_samples: 0,
        }
    }
    fn commit(&mut self, edges: Edges, t: u64) {
        if edges.pixel_rise && self.video_expected.front().is_some_and(|v| v.0 <= t) {
            let (expected_t, expected) = self.video_expected.pop_front().unwrap();
            assert_eq!(expected_t, t, "video clock calendar");
            let video = self.control.outputs(&self.pins).video;
            assert!(
                video
                    .precode
                    .channels
                    .iter()
                    .all(|v| v.known && !v.collision),
                "effective video provenance at {t}"
            );
            let codes = video
                .precode
                .channels
                .into_iter()
                .fold(0u64, |v, c| (v << 9) | u64::from(c.value));
            let rgb = video
                .rgb888
                .into_iter()
                .fold(0u64, |v, c| (v << 8) | u64::from(c));
            let flags = (u64::from(video.de) << 3)
                | (u64::from(video.framebuffer) << 2)
                | (u64::from(video.hsync) << 1)
                | u64::from(video.vsync);
            let actual = (codes << 28) | (rgb << 4) | flags;
            assert_eq!(
                actual, expected,
                "VIDEO_COLD tick={t} sample={}",
                self.video_samples
            );
            self.video_samples += 1;
            self.video_source += usize::from(video.effective_source);
        }
        if edges.pixel_rise && self.tmds_expected.front().is_some_and(|v| v.0 <= t) {
            let (expected_t, symbols, disparities) = self.tmds_expected.pop_front().unwrap();
            assert_eq!(expected_t, t, "symbol clock calendar");
            let out = self.control.outputs(&self.pins).tmds;
            assert!(
                out.lanes
                    .iter()
                    .all(|v| v.symbol_defined && v.disparity_defined && !v.tainted),
                "TMDS_COLD definedness {t}"
            );
            assert_eq!(
                out.lanes.map(|v| v.symbol),
                symbols,
                "TMDS_COLD symbols tick={t}"
            );
            assert_eq!(
                out.lanes.map(|v| v.disparity as u8 & 63),
                disparities,
                "TMDS_COLD disparity tick={t}"
            );
            self.tmds_samples += 1;
        }
        self.control.clock(edges, &self.pins);
    }
    fn pixels_before(&mut self, t: u64) {
        while self.next_pixel < t {
            self.commit(
                Edges {
                    logic_rise: false,
                    pixel_rise: true,
                },
                self.next_pixel,
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
        self.commit(edges, t);
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
    replay_observed(path, phase, Default::default(), Default::default())
}
fn replay_observed(
    path: &Path,
    phase: u64,
    video: std::collections::VecDeque<(u64, u64)>,
    tmds: std::collections::VecDeque<(u64, [u16; 3], [u8; 3])>,
) -> Replay {
    let mut r = Replay::new(phase);
    r.video_expected = video;
    r.tmds_expected = tmds;
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
                    r.commit(
                        Edges {
                            logic_rise: true,
                            pixel_rise: together,
                        },
                        rise,
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
#[ignore = "generates eight bounded production Fill cases, then cold-replays every edge"]
fn cold_control_replay_and_underflow_failure_feedback() {
    let out = test_output::fresh_output("display-cycle-timing");
    fs::create_dir_all(&out).unwrap();
    let producer = out.join("producer");
    assert!(
        !producer.exists(),
        "fresh replay cannot reuse a prior producer"
    );
    fill_qualification::qualification(&producer, "normal");
    assert_eq!(
        fs::read_to_string(producer.join("production.v")).unwrap(),
        source()
    );
    assert_eq!(
        fs::read_to_string(producer.join("line.v")).unwrap(),
        DisplayLineBuffer::verilog_source().unwrap()
    );
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(producer.join("manifest-normal.json")).unwrap()).unwrap();
    let cases = manifest["results"].as_array().unwrap();
    assert_eq!(cases.len(), 8);
    assert!(cases.iter().all(|case| case["pass"] == true));
    let mut names = vec![
        "production.v".to_string(),
        "line.v".into(),
        "tb.v".into(),
        "manifest-normal.json".into(),
    ];
    for case in cases {
        let label = case["case"].as_str().unwrap();
        names.extend([format!("{label}.trace"), format!("{label}.stdout")]);
    }
    let mut hashes = serde_json::Map::new();
    for name in names {
        let hash = Command::new("git")
            .args(["hash-object", "--no-filters"])
            .arg(producer.join(&name))
            .output()
            .unwrap();
        assert!(hash.status.success());
        hashes.insert(name, String::from_utf8(hash.stdout).unwrap().trim().into());
    }
    fs::write(
        out.join("fresh-inputs.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "kind":"fresh production Fill qualification from tracked generator/testbench",
            "producer":producer,"cases":cases,"input_git_blobs":hashes,
            "historical_bundle_used":false,
        }))
        .unwrap(),
    )
    .unwrap();
    let mut results = Vec::new();
    for case in cases {
        let label = case["case"].as_str().unwrap();
        let phase = label.rsplit('-').next().unwrap().parse().unwrap();
        let r = replay(&producer.join(format!("{label}.trace")), phase);
        if label == "production-2-17" {
            failure_feedback(r.clone());
        }
        results
            .push(serde_json::json!({"case":label,"logic_edges":(r.next_logic-37)/74,"pass":true}));
    }
    fs::write(out.join("replay-manifest.json"),serde_json::to_string_pretty(&serde_json::json!({
        "scope":"cold control-cone replay of freshly generated real RTL pin tapes, no native MC or color/TMDS",
        "input_generation":"shared tracked Fill qualification, all eight cases",
        "each_logic_and_pixel_edge_executed":true,"max_logic_edges_per_tape":4000000,
        "feedback_assertions":"underflow only stays clear; boot/raw error two pixel stages; coincident raw error reads old sticky",
        "results":results
    })).unwrap()).unwrap();
}

#[test]
#[ignore = "verbatim production failure crossing and underflow-only DUT mutation"]
fn production_failure_feedback_leaf() {
    let out = test_output::fresh_output("display-cycle-timing");
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

#[test]
#[ignore = "bounded Display-only fresh cold video window and same-input Rust replay"]
fn production_cold_video_window_replays_through_real_control() {
    let out = test_output::fresh_output("display-cycle-pipeline-cold");
    fs::create_dir_all(&out).unwrap();
    fs::write(
        out.join("line.v"),
        DisplayLineBuffer::verilog_source().unwrap(),
    )
    .unwrap();
    let observer = r#"
integer vf,tf,video_pixels=0,video_samples=0,video_real=0;
reg [2047:0] video_path,tmds_path;
wire [63:0] video_view={dut.precode_pipe,dut.rgb_pipe,dut.visible_pipe3,dut.framebuffer_pipe3,dut.hsync_pipe3,dut.vsync_pipe3};
initial begin
 if(!$value$plusargs("video=%s",video_path))$fatal(1,"missing video path");
 vf=$fopen(video_path,"w");if(!vf)$fatal(1,"video open");
 if(!$value$plusargs("tmds=%s",tmds_path))$fatal(1,"tmds path");
 tf=$fopen(tmds_path,"w");if(!tf)$fatal(1,"tmds open");
end
always @(posedge pixel_clock) begin
 #0;
 video_pixels=video_pixels+1;
 if(video_pixels>4*1056*525)$fatal(1,"video cycle cap");
 // Fixed raw H/V windows include preroll, first/last active pixels and postroll.
 // They are not selected by observed pixel value or scan readiness.
 if(dut.v_count>=32&&dut.v_count<34&&dut.h_count>=212&&dut.h_count<=1020)begin
  $fwrite(vf,"V %0d %016h\n",video_pixels,video_view);
  $fwrite(tf,"T %0d %h %h %h %h %h %h\n",video_pixels,dut.red_symbol,dut.green_symbol,dut.blue_symbol,
   dut.u_red.disparity,dut.u_green.disparity,dut.u_blue.disparity);
  video_samples=video_samples+1;
  if(dut.scan_valid&&dut.scan_line_ready&&dut.h_count>=217&&dut.h_count<=1016)video_real=video_real+1;
 end
end
"#;
    let tb = include_str!("display_cycle_fill_tb.v");
    let marker = "  finish_trace;\n  $display(\"PASS case=";
    assert!(tb.contains(marker));
    let tb=tb.replace(marker,"  if(video_samples<3000||video_real<1600)$fatal(1,\"video coverage\");\n  $fclose(vf);$fclose(tf);\n  finish_trace;\n  $display(\"PASS case=");
    let tb = tb.replacen(
        "always @(posedge clk) begin",
        &format!("{observer}\nalways @(posedge clk) begin"),
        1,
    );
    let exe = compile(&out, "production-cold", &tb, &source());
    let pin_trace = out.join("pins.trace");
    let video_trace = out.join("video.trace");
    let tmds_trace = out.join("tmds.trace");
    let mut command = Command::new(std::env::var("VVP_EXE").unwrap_or_else(|_| "vvp".into()));
    command
        .arg(exe)
        .arg("+case=0")
        .arg("+phase=17")
        .arg(format!("+trace={}", pin_trace.display()))
        .arg(format!("+video={}", video_trace.display()))
        .arg(format!("+tmds={}", tmds_trace.display()));
    assert!(run_bounded(&mut command, &out, "production-cold", 90));
    let mut expected = std::collections::VecDeque::new();
    for line in BufReader::new(fs::File::open(&video_trace).unwrap()).lines() {
        let line = line.unwrap();
        let fields = line.split_whitespace().collect::<Vec<_>>();
        assert_eq!(fields.len(), 3);
        let ordinal = fields[1].parse::<u64>().unwrap();
        expected.push_back((
            77 + (ordinal - 1) * 120,
            u64::from_str_radix(fields[2], 16).unwrap(),
        ));
    }
    let mut symbols = std::collections::VecDeque::new();
    for line in BufReader::new(fs::File::open(&tmds_trace).unwrap()).lines() {
        let line = line.unwrap();
        let v = line.split_whitespace().collect::<Vec<_>>();
        assert_eq!(v.len(), 8);
        let ordinal = v[1].parse::<u64>().unwrap();
        symbols.push_back((
            77 + (ordinal - 1) * 120,
            std::array::from_fn(|i| u16::from_str_radix(v[i + 2], 16).unwrap()),
            std::array::from_fn(|i| u8::from_str_radix(v[i + 5], 16).unwrap()),
        ));
    }
    let replay = replay_observed(&pin_trace, 17, expected, symbols);
    assert!(replay.tmds_expected.is_empty() && replay.tmds_samples == replay.video_samples);
    assert!(
        replay.video_expected.is_empty()
            && replay.video_samples >= 3000
            && replay.video_source >= 1600
    );
    fs::write(out.join("results.json"),serde_json::to_string_pretty(&serde_json::json!({
        "scope":"fresh production Display-only video windows and cold Rust Control replay; controlled MC response peer",
        "video_samples":replay.video_samples,"real_source_samples":replay.video_source,"tmds_samples":replay.tmds_samples,
        "max_logic_edges":4000000,"max_frames":4,"wall_seconds":90,"phase":17
    })).unwrap()).unwrap();
}
