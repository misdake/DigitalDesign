//! Digital TMDS: production encoder + actual old-pipe lane wiring + raw RGB miter.
#[path = "support/display_test_output.rs"]
mod test_output;
use cpu_v3_tang_nano_20k::hardware::display_cycle::{
    ColorPrecode, PixelPipe, Precode, Timing, Tmds, TmdsInput, TmdsOutput,
};
use std::{
    fmt::Write as _,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
// Reuse production packing; this leaf does not call its RAM-table exporters.
#[allow(dead_code)]
#[path = "../src/hardware/display/precode_table.rs"]
mod precode;

fn colors(bytes: [u8; 3]) -> ColorPrecode {
    ColorPrecode {
        channels: bytes.map(|v| Precode {
            value: precode::tmds_precode(v) as u16,
            known: true,
            collision: false,
        }),
    }
}
#[derive(Clone, Copy)]
struct Stimulus {
    edge: bool,
    reset: bool,
    de: bool,
    hs: bool,
    vs: bool,
    rgb: [u8; 3],
}
#[derive(Default)]
struct Fixture {
    tmds: Tmds,
    pipe: PixelPipe,
    text: String,
    expected: Vec<Vec<Option<u64>>>,
    active: usize,
}
fn values(out: TmdsOutput) -> Vec<Option<u64>> {
    out.lanes
        .iter()
        .map(|l| l.symbol_defined.then_some(u64::from(l.symbol)))
        .chain(out.lanes.iter().map(|l| {
            l.disparity_defined
                .then_some(u64::from(l.disparity as u8 & 63))
        }))
        .collect()
}
impl Fixture {
    fn step(&mut self, s: Stimulus) {
        let index = self.expected.len();
        assert!(index < 32768);
        let old = self.pipe.outputs();
        let before = self.tmds.outputs();
        assert_eq!(before, self.tmds.outputs());
        let mut expected = values(before);
        if s.edge {
            self.active += usize::from(!s.reset && old.de);
            self.tmds.clock(&TmdsInput::from_video(s.reset, old));
            let mut t = Timing::default().outputs();
            t.pixel_reset = s.reset;
            t.active = s.de;
            t.hsync = s.hs;
            t.vsync = s.vs;
            t.started = true;
            t.scan_valid = true;
            t.scan_line_ready = true;
            self.pipe.clock(&t, colors(s.rgb));
        }
        expected.extend(old.precode.channels.map(|c| Some(u64::from(c.value))));
        expected.extend(old.rgb888.map(|c| Some(u64::from(c))));
        expected.extend(values(self.tmds.outputs()));
        writeln!(
            self.text,
            "{index} {} {} {} {} {} {:02x} {:02x} {:02x} {:03x} {:03x} {:03x}",
            u8::from(s.edge),
            u8::from(s.reset),
            u8::from(s.de),
            u8::from(s.hs),
            u8::from(s.vs),
            s.rgb[0],
            s.rgb[1],
            s.rgb[2],
            precode::tmds_precode(s.rgb[0]),
            precode::tmds_precode(s.rgb[1]),
            precode::tmds_precode(s.rgb[2])
        )
        .unwrap();
        self.expected.push(expected);
    }
}
fn vectors() -> Fixture {
    let mut f = Fixture::default();
    let mut s = Stimulus {
        edge: false,
        reset: false,
        de: false,
        hs: false,
        vs: false,
        rgb: [0; 3],
    };
    f.step(s);
    s.reset = true;
    f.step(s);
    s.edge = true;
    f.step(s);
    s.reset = false;
    for control in 0..4 {
        s.de = false;
        s.hs = control & 1 != 0;
        s.vs = control & 2 != 0;
        f.step(s);
        f.step(s);
        for byte in 0u8..=255 {
            s.de = true;
            s.rgb = [byte, byte.rotate_left(3), !byte];
            f.step(s);
        }
    }
    let mut seed = 0x8e41_23bdu32;
    for index in 0..4096 {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        s.de = index % 127 != 0;
        s.hs = index & 1 != 0;
        s.vs = index & 2 != 0;
        s.rgb = [(seed >> 24) as u8, (seed >> 16) as u8, index as u8];
        f.step(s);
        if index % 257 == 0 {
            let mut probe = s;
            probe.edge = false;
            probe.reset = true;
            f.step(probe);
        }
    }
    s.edge = true;
    s.reset = false;
    s.de = false;
    s.hs = true;
    s.vs = true;
    f.step(s);
    f.step(s);
    s.reset = true;
    f.step(s);
    s.reset = false;
    s.de = true;
    s.rgb = [17, 83, 129];
    f.step(s);
    f.step(s);
    s.reset = true;
    f.step(s);
    s.reset = true;
    s.edge = false;
    f.step(s);
    s.edge = true;
    f.step(s);
    s.reset = false;
    s.de = false;
    f.step(s);
    f.step(s);
    assert!(f.active > 4000);
    f
}
#[test]
fn concrete_inputs_and_lane_diagnostics() {
    let _ = vectors();
    let mut first_active = Tmds::default();
    first_active.clock(&TmdsInput {
        reset: Some(false),
        de: Some(true),
        hsync: None,
        vsync: None,
        precode: colors([0; 3]),
    });
    assert!(first_active
        .outputs()
        .lanes
        .iter()
        .all(|l| l.symbol_defined && l.symbol == 0x100 && l.disparity == -8));
    let mut e = Tmds::default();
    assert!(e
        .outputs()
        .lanes
        .iter()
        .all(|l| !l.symbol_defined && l.disparity_defined && l.disparity == 0));
    let mut i = TmdsInput {
        reset: Some(true),
        de: None,
        hsync: None,
        vsync: None,
        precode: colors([11, 73, 129]),
    };
    e.clock(&i);
    assert!(e
        .outputs()
        .lanes
        .iter()
        .all(|l| l.symbol_defined && l.symbol == 0x354));
    i.reset = Some(false);
    i.de = Some(true);
    i.precode.channels[0].known = false;
    e.clock(&i);
    assert!(!e.outputs().lanes[0].disparity_defined);
    assert!(e.outputs().lanes[1..].iter().all(|l| l.symbol_defined));
    i.precode = colors([21, 83, 139]);
    e.clock(&i);
    assert!(!e.outputs().lanes[0].symbol_defined);
    i.de = Some(false);
    e.clock(&i);
    let o = e.outputs();
    assert!(o
        .lanes
        .iter()
        .all(|l| l.disparity_defined && l.disparity == 0));
    assert!(o.lanes[0].symbol_defined && o.lanes[1].symbol_defined && !o.lanes[2].symbol_defined);
    i.de = Some(true);
    i.precode.channels[2].collision = true;
    e.clock(&i);
    assert!(e.outputs().lanes[2].tainted && !e.outputs().lanes[0].tainted);
    i.precode = colors([0; 3]);
    e.clock(&i);
    assert!(e.outputs().lanes[2].tainted);
    i.de = None;
    e.clock(&i);
    assert!(e
        .outputs()
        .lanes
        .iter()
        .all(|l| !l.symbol_defined && !l.disparity_defined));
    i.reset = None;
    i.de = Some(false);
    e.clock(&i);
    assert!(e.outputs().lanes.iter().all(|l| !l.disparity_defined));
    i.reset = Some(false);
    i.hsync = Some(true);
    i.vsync = Some(false);
    e.clock(&i);
    assert!(e
        .outputs()
        .lanes
        .iter()
        .all(|l| l.symbol_defined && l.disparity_defined && !l.tainted));
    assert_eq!(e.outputs().lanes[2].symbol, 0x0ab);
}
fn run(mut command: Command, out: &Path, name: &str) -> (bool, String) {
    let args = std::iter::once(command.get_program())
        .chain(command.get_args())
        .map(|a| a.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    std::fs::write(
        out.join(format!("{name}.command.json")),
        serde_json::to_string_pretty(&args).unwrap(),
    )
    .unwrap();
    command.stdout(Stdio::from(
        std::fs::File::create(out.join(format!("{name}.stdout"))).unwrap(),
    ));
    command.stderr(Stdio::from(
        std::fs::File::create(out.join(format!("{name}.stderr"))).unwrap(),
    ));
    let mut child = command.spawn().unwrap();
    let start = Instant::now();
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        if start.elapsed() > Duration::from_secs(30) {
            child.kill().unwrap();
            let _ = child.wait();
            panic!("{name} 30s cap");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    (
        status.success(),
        std::fs::read_to_string(out.join(format!("{name}.stdout"))).unwrap(),
    )
}
fn compare(trace: &str, f: &Fixture) -> Result<(), String> {
    let mut count = 0;
    for line in trace.lines().filter(|line| line.starts_with("R ")) {
        let v = line.split_whitespace().collect::<Vec<_>>();
        if count >= f.expected.len() || v.len() != 20 {
            return Err(format!("TMDS_TRACE {count}: {line}"));
        }
        for (col, (actual, expected)) in v[2..].iter().zip(&f.expected[count]).enumerate() {
            let value = u64::from_str_radix(actual, 16).ok();
            if value != *expected {
                return Err(format!(
                    "TMDS_VALUE row={count} col={col} got={actual} expected={expected:?}"
                ));
            }
        }
        count += 1;
    }
    if count != f.expected.len() {
        return Err(format!("TMDS_TRACE missing {count}/{}", f.expected.len()));
    }
    Ok(())
}
#[test]
#[ignore = "bounded production digital encoder, old pipe, raw RGB miter and actual DUT mutations"]
fn production_tmds_and_finite_mutants() {
    let out = test_output::fresh_output("display-cycle-tmds");
    std::fs::create_dir_all(&out).unwrap();
    let f = vectors();
    std::fs::write(out.join("vectors.txt"), &f.text).unwrap();
    let source = include_str!("../src/hardware/display/display_hdmi.v");
    let encoder = format!(
        "module HdmiTmdsEncoder{}",
        source
            .split_once("module HdmiTmdsEncoder")
            .unwrap()
            .1
            .split_once("`ifndef __ICARUS__")
            .unwrap()
            .0
    );
    let lanes = source
        .split_once("wire [9:0] blue_symbol,green_symbol,red_symbol;")
        .unwrap()
        .1
        .split_once("wire [3:0] serialized;")
        .unwrap()
        .0;
    let reference = format!(
        "module DisplayTmdsReference{}endmodule",
        include_str!("../src/hardware/display/display_hdmi_tb.v")
            .split_once("module DisplayTmdsReference")
            .unwrap()
            .1
            .split_once("endmodule")
            .unwrap()
            .0
    );
    let tb = include_str!("display_cycle_tmds_tb.v").replace("__LANES__", lanes) + &reference;
    let cases = [
        ("production", encoder.clone(), tb.clone(), true),
        (
            "initialized_symbol",
            encoder.replace(
                "reg signed [5:0] disparity=0;",
                "reg signed [5:0] disparity=0; initial symbol=10'b0;",
            ),
            tb.clone(),
            false,
        ),
        (
            "new_de",
            encoder.clone(),
            tb.replacen(".de(visible_pipe3)", ".de(active)", 3),
            false,
        ),
        (
            "async_reset",
            encoder.replace(
                "always @(posedge clk)",
                "always @(posedge clk or posedge reset)",
            ),
            tb.clone(),
            false,
        ),
        (
            "reset_priority",
            encoder.replace("if(reset) begin", "if(reset && de) begin"),
            tb.clone(),
            false,
        ),
        (
            "no_blank_clear",
            encoder.replace(
                "else if(!de) begin disparity<=0;",
                "else if(!de) begin disparity<=disparity;",
            ),
            tb.clone(),
            false,
        ),
        (
            "wrong_qm8",
            encoder.replace("symbol[9]<=~qm[8]", "symbol[9]<=qm[8]"),
            tb.clone(),
            false,
        ),
        (
            "wrong_delta",
            encoder.replace(
                "disparity-qm_delta+(qm[8]?6'sd2:0)",
                "disparity+qm_delta+(qm[8]?6'sd2:0)",
            ),
            tb.clone(),
            false,
        ),
        (
            "wrong_signed",
            encoder
                .replace("disparity>0", "$unsigned(disparity)>0")
                .replace("disparity<0", "$unsigned(disparity)<0"),
            tb.clone(),
            false,
        ),
        (
            "blue_sync",
            encoder.clone(),
            tb.replacen(
                ".control({vsync_pipe3,hsync_pipe3})",
                ".control({hsync_pipe3,vsync_pipe3})",
                1,
            ),
            false,
        ),
        (
            "swap_rb",
            encoder.clone(),
            tb.replace(".precode(blue)", ".precode(tmp)")
                .replace(".precode(red)", ".precode(blue)")
                .replace(".precode(tmp)", ".precode(red)"),
            false,
        ),
        (
            "new_pipe",
            encoder,
            tb.replace(".precode(blue)", ".precode(next_b)")
                .replace(".precode(green)", ".precode(next_g)")
                .replace(".precode(red)", ".precode(next_r)"),
            false,
        ),
    ];
    let mut results = Vec::new();
    let mut coverage = serde_json::Value::Null;
    for (name, encoder, tb, positive) in cases {
        std::fs::write(out.join(format!("{name}.v")), encoder).unwrap();
        std::fs::write(out.join(format!("{name}-tb.v")), tb).unwrap();
        let exe = out.join(format!("{name}.vvp"));
        let mut c =
            Command::new(std::env::var("IVERILOG_EXE").unwrap_or_else(|_| "iverilog".into()));
        c.args(["-g2012", "-s", "tb", "-o"])
            .arg(&exe)
            .arg(out.join(format!("{name}.v")))
            .arg(out.join(format!("{name}-tb.v")));
        assert!(run(c, &out, &format!("{name}-compile")).0, "compile {name}");
        let mut c = Command::new(std::env::var("VVP_EXE").unwrap_or_else(|_| "vvp".into()));
        c.arg(exe)
            .arg(format!("+vectors={}", out.join("vectors.txt").display()));
        let (success, trace) = run(c, &out, name);
        let compared = compare(&trace, &f);
        if positive {
            let mut min = 0i16;
            let mut max = 0i16;
            for line in trace.lines().filter(|l| l.starts_with("R ")) {
                let v = line.split_whitespace().collect::<Vec<_>>();
                for index in [5, 6, 7, 17, 18, 19] {
                    let bits = i16::from_str_radix(v[index], 16).unwrap();
                    let d = if bits & 32 != 0 { bits - 64 } else { bits };
                    min = min.min(d);
                    max = max.max(d);
                }
            }
            let counts = trace
                .lines()
                .filter(|l| l.starts_with("COV "))
                .map(|l| {
                    l.split_whitespace()
                        .skip(1)
                        .map(|v| v.parse::<u64>().unwrap())
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            coverage = serde_json::json!({"actual_rtl_counts":counts,"fields":["lane","d_neg","d_zero","d_pos","n_four","qm8_zero","qm8_one","balanced","same_sign","other"],"disparity_min":min,"disparity_max":max,"all_256_bytes_each_lane":true,"no_forced_disparity":true,"signed6_overflow_boundary_reached":false});
        }
        std::fs::write(out.join(format!("{name}.diff")), format!("{compared:?}")).unwrap();
        if positive {
            assert!(
                success && trace.contains("PASS TMDS") && compared.is_ok(),
                "{name} {compared:?}: {trace}"
            );
        } else {
            assert!(
                (!success && (trace.contains("TMDS_MITER") || trace.contains("TMDS_INITIAL")))
                    || compared
                        .as_ref()
                        .is_err_and(|e| e.starts_with("TMDS_VALUE")),
                "undetected {name} {compared:?}"
            );
        }
        results.push(serde_json::json!({"case":name,"positive":positive,"success":success,"difference":compared.err(),"miter_failure":trace.lines().find(|l|l.contains("TMDS_MITER")||l.contains("TMDS_INITIAL"))}));
    }
    std::fs::write(
        out.join("results.json"),
        serde_json::to_string_pretty(
            &serde_json::json!({"rows":f.expected.len(),"active":f.active,"coverage":coverage,"results":results}),
        )
        .unwrap(),
    )
    .unwrap();
}
