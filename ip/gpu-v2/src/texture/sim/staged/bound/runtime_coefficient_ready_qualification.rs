//! Ready weights: independent semantic lifetime scoreboard and actual Runtime replay.
use super::{qualification as q, *};
use crate::texture::{
    emu::coefficient::{self as c, fixed_stage::FixedCoefficientEmu, Input, Output, Tick},
    ports::Filter,
    rtl,
};
use std::{
    collections::VecDeque,
    fmt::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
fn golden(v: Input) -> Output {
    Output {
        metadata: v.metadata,
        weights: std::array::from_fn(|p| {
            let fu = if v.nearest {
                0
            } else {
                u32::from(v.fractions[p][0])
            };
            let fv = if v.nearest {
                0
            } else {
                u32::from(v.fractions[p][1])
            };
            let b = u32::from(v.parents[p]) * fv / 256;
            let t = u32::from(v.parents[p]) - b;
            let br = b * fu / 256;
            let tr = t * fu / 256;
            [(t - tr) as u16, tr as u16, (b - br) as u16, br as u16]
        }),
    }
}
// Independent low-to-high field writer, shared only by the checker/RTL pins.
fn words(fields: impl IntoIterator<Item = (u64, usize)>) -> [u64; 3] {
    let mut out = [0u64; 3];
    let mut at = 0;
    for (v, width) in fields {
        for b in 0..width {
            out[at / 64] |= ((v >> b) & 1) << (at % 64);
            at += 1;
        }
    }
    out
}
fn packed(o: Output) -> [u64; 3] {
    words(
        o.weights
            .into_iter()
            .flatten()
            .map(|v| (u64::from(v), 9))
            .chain(
                o.metadata
                    .coordinates
                    .into_iter()
                    .flatten()
                    .map(|v| (u64::from(v), 10)),
            )
            .chain([
                (u64::from(o.metadata.levels[0]), 4),
                (u64::from(o.metadata.levels[1]), 4),
                (u64::from(o.metadata.slot), 4),
                (u64::from(o.metadata.key), 6),
                (u64::from(o.metadata.last_fine), 1),
            ]),
    )
}
fn weights(o: Output) -> [u64; 3] {
    words(o.weights.into_iter().flatten().map(|v| (u64::from(v), 9)))
}
fn metadata(o: Output) -> u128 {
    let p = packed(o);
    u128::from(p[1] >> 8) | (u128::from(p[2]) << 56)
}
fn pins(i: Input) -> [u64; 3] {
    words(
        [
            (u64::from(i.nearest), 1),
            (u64::from(i.parents[0]), 10),
            (u64::from(i.parents[1]), 10),
        ]
        .into_iter()
        .chain(i.fractions.into_iter().flatten().map(|v| (u64::from(v), 8)))
        .chain(
            i.metadata
                .coordinates
                .into_iter()
                .flatten()
                .map(|v| (u64::from(v), 11)),
        )
        .chain([
            (u64::from(i.metadata.levels[0]), 4),
            (u64::from(i.metadata.levels[1]), 4),
            (u64::from(i.metadata.slot), 5),
            (u64::from(i.metadata.key), 7),
            (u64::from(i.metadata.last_fine), 1),
        ]),
    )
}
fn literal(v: [u64; 3], width: usize) -> String {
    format!("{width}'h{:x}{:016x}{:016x}", v[2], v[1], v[0])
}
struct Flight {
    compact_meta: u128,
    out: Output,
    age: u64,
    written: bool,
    published: bool,
}
struct Audit {
    compact: bool,
    queue: VecDeque<Flight>,
    tb: String,
    csv: String,
    wall: usize,
    read: u8,
    write: u8,
    enabled: u64,
    writes: usize,
    publishes: usize,
    takes: usize,
    epoch_takes: usize,
    both: usize,
    held_pending: usize,
    full_pop: usize,
    resets: usize,
    peak: u8,
}
impl Audit {
    fn new(compact: bool) -> Self {
        Self {
            compact,
            queue: VecDeque::new(),
            tb: String::from(
                "module tb; reg clk=0,reset=1,ce=0,in_valid=0,out_ready=0;reg[161:0]in_payload=0;reg[7:0]work_available=16;wire in_ready,in_accept,out_valid,fault;wire[71:0]out_weights;wire[98:0]out_meta;wire[1:0]work_reserved;GSR GSR(.GSRI(1'b1));gpu_v2_texture_coefficient dut(.*);initial begin #2000000;$fatal(1,\"watchdog\");end initial begin #1;clk=1;#1;clk=0;reset=0;#1;\n",
            ).replace("wire[98:0]out_meta",if compact {"wire[35:0]out_meta"} else {"wire[98:0]out_meta"}),
            csv: String::from(
                "wall,CE,advance,old_queued,pending,write,publish,consume,read_row,write_row\n",
            ),
            wall: 0,
            read: 0,
            write: 0,
            enabled: 0,
            writes: 0,
            publishes: 0,
            takes: 0,
            epoch_takes: 0,
            both: 0,
            held_pending: 0,
            full_pop: 0,
            resets: 0,
            peak: 0,
        }
    }
    fn edge(&mut self, t: Tick, s: &c::Step) {
        let old_count = self.queue.iter().filter(|x| x.published).count();
        // Address ownership comes from accepted/consumed totals, independently
        // of emulator pointers. Reset starts a fresh metadata-ring epoch.
        let admitted = self.queue.len() + self.epoch_takes;
        let ma = &s.metadata_access;
        assert_eq!(
            ma.write,
            s.accepted.then(|| (
                (admitted & 15) as u8,
                if self.compact {
                    compact_metadata(t.input.unwrap())
                } else {
                    metadata(golden(t.input.unwrap()))
                }
            ))
        );
        assert_eq!(
            ma.read_address,
            (s.ready_access.publish || s.consumed)
                .then_some(((self.epoch_takes + usize::from(s.consumed)) & 15) as u8)
        );
        writeln!(
            self.tb,
            "reset=0;ce={};in_valid={};in_payload={};out_ready={};work_available=8'd{};#1;",
            u8::from(t.ce),
            u8::from(t.input.is_some()),
            literal(t.input.map_or([0; 3], pins), 162),
            u8::from(t.output_ready),
            t.work_available
        )
        .unwrap();
        writeln!(
            self.tb,
            "if(dut.meta_we!=={} || dut.meta_re!=={}) $fatal(1,\"metadata enable\");",
            u8::from(ma.write.is_some()),
            u8::from(ma.read_address.is_some())
        )
        .unwrap();
        if let Some((a, d)) = ma.write {
            writeln!(
                self.tb,
                "if(dut.cwr!==4'd{a} || dut.meta_data!==108'h{d:x}) $fatal(1,\"metadata write\");"
            )
            .unwrap();
        }
        if let Some(a) = ma.read_address {
            writeln!(
                self.tb,
                "if(dut.meta_ra!==4'd{a}) $fatal(1,\"metadata read\");"
            )
            .unwrap();
        }
        let expected_meta = self
            .queue
            .front()
            .filter(|x| x.published)
            .map_or(0, |x| x.compact_meta);
        let expected = self.queue.front().filter(|x| x.published).map(|x| x.out);
        assert_eq!(s.output, expected);
        let advance = t.ce && old_count < 2;
        assert_eq!(s.snapshot.enabled, self.enabled + u64::from(advance));
        let old_pending = self.queue.iter().any(|x| x.written && !x.published);
        if old_pending && !advance {
            self.held_pending += 1;
        }
        if old_count == 2 && s.consumed {
            self.full_pop += 1;
            assert!(!s.accepted);
        }
        assert_eq!(s.ready_access.read_address, self.read);
        assert_eq!(s.consumed, t.ce && t.output_ready && expected.is_some());
        let write = s.ready_access.write;
        let mut write_expected = None;
        if s.consumed {
            let f = self.queue.pop_front().unwrap();
            assert!(self.enabled - f.age >= 6);
            self.read ^= 1;
            self.takes += 1;
            self.epoch_takes += 1;
        }
        if let Some((row, raw)) = write {
            assert!(advance);
            assert_eq!(row, self.write);
            if expected.is_some() {
                assert_ne!(row, s.ready_access.read_address);
            }
            let f = self.queue.iter_mut().find(|x| !x.written).unwrap();
            assert_eq!(self.enabled - f.age, 4);
            assert_eq!(raw, weights(f.out));
            assert_eq!(raw[1] >> 8, 0);
            assert_eq!(raw[2], 0);
            write_expected = Some(f.out);
            f.written = true;
            self.writes += 1;
        }
        if s.ready_access.publish {
            assert!(advance);
            assert!(write.is_none(), "II2 separates write and publication");
            let f = self.queue.iter_mut().find(|x| !x.published).unwrap();
            assert!(f.written);
            assert_eq!(self.enabled - f.age, 5);
            f.published = true;
            self.write ^= 1;
            self.publishes += 1;
        }
        if s.accepted {
            self.queue.push_back(Flight {
                compact_meta: compact_metadata(t.input.unwrap()),
                out: golden(t.input.unwrap()),
                age: self.enabled,
                written: false,
                published: false,
            });
        }
        assert_eq!(
            usize::from(s.snapshot.queued),
            self.queue.iter().filter(|x| x.published).count()
        );
        self.both += usize::from(write.is_some() && s.consumed);
        self.peak = self.peak.max(s.snapshot.queued);
        let w = self.wall;
        writeln!(
            self.tb,
            "reset=0;ce={};in_valid={};in_payload={};out_ready={};work_available=8'd{};#1;",
            u8::from(t.ce),
            u8::from(t.input.is_some()),
            literal(t.input.map_or([0; 3], pins), 162),
            u8::from(t.output_ready),
            t.work_available
        )
        .unwrap();
        writeln!(self.tb,"if(fault!==0 || in_ready!=={} || in_accept!=={} || work_reserved!==2'd{} || out_valid!=={} || dut.queued!==2'd{} || dut.ready_we!=={} || dut.do_capture!=={}) $fatal(1,\"handshake wall{w}\");",u8::from(s.input_ready),u8::from(s.accepted),s.work_reserved,u8::from(expected.is_some()),old_count,u8::from(write.is_some()),u8::from(s.ready_access.publish)).unwrap();
        if let Some(o) = expected {
            writeln!(
                self.tb,
                "if({{out_meta,out_weights}}!=={}) $fatal(1,\"old payload wall{w}\");",
                if self.compact {
                    literal(compact_packed(o, expected_meta), 108)
                } else {
                    literal(packed(o), 171)
                }
            )
            .unwrap();
        }
        if let Some(o) = write_expected {
            writeln!(
                self.tb,
                "if(dut.out_w!=={} || dut.rwr!=={}) $fatal(1,\"write payload wall{w}\");",
                literal(weights(o), 72),
                write.unwrap().0
            )
            .unwrap();
        }
        self.tb.push_str("clk=1;#1;clk=0;#1;\n");
        writeln!(self.tb,"if(dut.queued!==2'd{} || dut.rrd!=={} || dut.rwr!=={} || dut.valid!==5'd{} || dut.ph!==1'd{}) $fatal(1,\"post wall{w}\");",s.snapshot.queued,self.read,self.write,s.snapshot.valid,s.snapshot.phase.trailing_zeros()).unwrap();
        writeln!(
            self.csv,
            "{w},{},{},{old_count},{old_pending},{},{},{},{},{}",
            t.ce,
            advance,
            write.is_some(),
            s.ready_access.publish,
            s.consumed,
            s.ready_access.read_address,
            write.map_or(9, |v| v.0)
        )
        .unwrap();
        self.enabled = s.snapshot.enabled;
        self.wall += 1;
    }
    fn reset(&mut self) {
        // Retain payload rows in RTL; the independent queue loses ownership.
        self.tb.push_str("reset=1;ce=0;in_valid=1;out_ready=1;#1;if(dut.ready_we!==0)$fatal(1,\"reset write\");clk=1;#1;clk=0;#1;reset=0;ce=0;in_valid=0;#1;if(out_valid!==0 || fault!==0 || dut.queued!==0)$fatal(1,\"reset stale row\");\n");
        self.queue.clear();
        self.epoch_takes = 0;
        self.read = 0;
        self.write = 0;
        self.enabled = 0;
        self.resets += 1;
    }
    fn finish(mut self, name: &str) -> PathBuf {
        assert!(self.queue.is_empty());
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/sampling-compact-coordinate-20261009/coefficient-runtime")
            .join(name);
        std::fs::create_dir_all(&dir).unwrap();
        self.tb
            .push_str("$display(\"PASS ready weights\");$finish;end endmodule\n");
        std::fs::write(dir.join("tb.v"), self.tb).unwrap();
        std::fs::write(dir.join("calendar.csv"), self.csv).unwrap();
        std::fs::write(
            dir.join("coefficient.v"),
            format!(
                "`define GPU_V2_GOWIN_DSP\n{}",
                if self.compact {
                    rtl::compact_coefficient::source()
                } else {
                    rtl::coefficient::source()
                }
            ),
        )
        .unwrap();
        std::fs::write(dir.join("receipt.json"),format!("{{\"edges\":{},\"writes\":{},\"publishes\":{},\"consumes\":{},\"write_and_pop\":{},\"held_unpublished\":{},\"old_full_pop\":{},\"CE0_resets\":{},\"peak_ready\":{}}}\n",self.wall,self.writes,self.publishes,self.takes,self.both,self.held_pending,self.full_pop,self.resets,self.peak)).unwrap();
        dir
    }
}
fn runtime(paused: bool, work: usize) -> PathBuf {
    let (slots, bytes) = q::fixture();
    let mut r = q::runtime(
        &slots,
        super::super::control::Hardware {
            work_credits: work,
            ..Default::default()
        },
    );
    let mut mc = q::physical::Physical::new(4096, bytes, true, paused);
    let mut a = Audit::new(true);
    let inputs: Vec<_> = (0..64)
        .map(|i| {
            q::input(
                (i % 16) as u8,
                if paused {
                    [15, 0, 1, 10, 6, 15, 3, 0][i % 8]
                } else {
                    15
                },
                if paused {
                    [-0.03125 + (i % 7) as f64 / 32., 0.46875]
                } else {
                    [0.13, 0.07]
                },
                Filter::Trilinear,
            )
        })
        .collect();
    let expected: Vec<_> = inputs.iter().flat_map(q::golden).collect();
    let mut got = vec![];
    let mut sent = 0;
    let mut rng = 0x391c25u32;
    for wall in 0..40_000 {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        let s = r
            .step(
                &mut mc,
                inputs.get(sent),
                timed::Control {
                    ce: !paused || rng & 7 > 1,
                    result_ready: !paused || wall > 1000 && rng % 13 > 3,
                },
            )
            .unwrap();
        sent += usize::from(s.accepted);
        got.extend(s.results);
        let trace = &r.preparation.trace;
        if let Some(step) = &trace.coefficient {
            a.edge(trace.coefficient_tick.unwrap(), step);
        }
        if sent == inputs.len() && r.idle() {
            break;
        }
    }
    assert!(r.idle());
    assert_eq!(got, expected);
    // Runtime phase alignment need not coincide write and pop. The directed
    // calendar proves the simultaneous port case without changing live phases.
    assert!(a.writes > 0 && a.takes == a.writes);
    a.finish(&format!("runtime-paused{paused}-W{work}"))
}
fn fixture(i: usize) -> Input {
    let p = [0, 1, 255, 256, 510, 511][i % 6];
    Input {
        parents: [p, 511 - p],
        fractions: [[((i * 31) % 256) as u8, 255], [128, ((i * 63) % 256) as u8]],
        nearest: i % 6 == 5,
        metadata: c::Metadata {
            coordinates: std::array::from_fn(|p| {
                std::array::from_fn(|v| ((i * 271 + p * 91 + v * 501) % 1024) as u16)
            }),
            levels: [(i % 11) as u8, ((i * 3) % 11) as u8],
            slot: (i % 16) as u8,
            key: (i % 64) as u8,
            last_fine: p == 511,
        },
    }
}
fn directed() -> PathBuf {
    let mut a = Audit::new(false);
    let mut e = FixedCoefficientEmu::new(20_000).unwrap();
    // CE0 holds a written, unpublished row, then reset it and reuse different data.
    for i in 0..6 {
        let t = Tick {
            input: (i == 0).then(|| fixture(1)),
            output_ready: false,
            ..Default::default()
        };
        let s = e.tick(t).unwrap();
        a.edge(t, &s);
        if s.ready_access.write.is_some() {
            break;
        }
    }
    assert!(e.snapshot().pending_capture);
    for _ in 0..5 {
        let t = Tick {
            ce: false,
            ..Default::default()
        };
        let s = e.tick(t).unwrap();
        a.edge(t, &s);
    }
    a.reset();
    e = FixedCoefficientEmu::new(20_000).unwrap();
    // Full ready freezes arithmetic even on the edge that pops the old head.
    let mut sent = 0;
    for wall in 0..4000 {
        let t = Tick {
            ce: wall % 17 != 4,
            input: (sent < 128).then(|| fixture(sent + 43)),
            output_ready: wall > 90,
            ..Default::default()
        };
        let s = e.tick(t).unwrap();
        sent += usize::from(s.accepted);
        a.edge(t, &s);
        if sent == 128 && e.idle() {
            break;
        }
    }
    assert!(e.idle());
    assert_eq!(sent, 128);
    assert!(a.full_pop > 0 && a.both > 0 && a.held_pending > 0);
    // Reset a fully published held queue and confirm later reuse with new fields.
    let mut accepted = false;
    for _ in 0..10 {
        let t = Tick {
            input: (!accepted).then(|| fixture(200)),
            output_ready: false,
            ..Default::default()
        };
        let s = e.tick(t).unwrap();
        accepted |= s.accepted;
        a.edge(t, &s);
    }
    assert!(accepted && e.output().is_some());
    a.reset();
    e = FixedCoefficientEmu::new(20_000).unwrap();
    accepted = false;
    for _ in 0..12 {
        let t = Tick {
            input: (!accepted).then(|| fixture(211)),
            ..Default::default()
        };
        let s = e.tick(t).unwrap();
        accepted |= s.accepted;
        a.edge(t, &s);
    }
    assert!(e.idle());
    a.finish("directed")
}
fn compact_directed() -> PathBuf {
    let mut a = Audit::new(true);
    let mut e = c::compact_stage::CompactCoefficientEmu::new(20_000).unwrap();
    // CE0 holds a written, unpublished row, then reset it and reuse different data.
    for i in 0..6 {
        let t = Tick {
            input: (i == 0).then(|| compact_fixture(1)),
            output_ready: false,
            ..Default::default()
        };
        let s = e.tick(t).unwrap();
        a.edge(t, &s);
        if s.ready_access.write.is_some() {
            break;
        }
    }
    assert!(e.snapshot().pending_capture);
    for _ in 0..5 {
        let t = Tick {
            ce: false,
            ..Default::default()
        };
        let s = e.tick(t).unwrap();
        a.edge(t, &s);
    }
    a.reset();
    e = c::compact_stage::CompactCoefficientEmu::new(20_000).unwrap();
    // Full ready freezes arithmetic even on the edge that pops the old head.
    let mut sent = 0;
    for wall in 0..4000 {
        let t = Tick {
            ce: wall % 17 != 4,
            input: (sent < 128).then(|| compact_fixture(sent + 43)),
            output_ready: wall > 90,
            ..Default::default()
        };
        let s = e.tick(t).unwrap();
        sent += usize::from(s.accepted);
        a.edge(t, &s);
        if sent == 128 && e.idle() {
            break;
        }
    }
    assert!(e.idle());
    assert_eq!(sent, 128);
    assert!(a.full_pop > 0 && a.both > 0 && a.held_pending > 0);
    // Reset a fully published held queue and confirm later reuse with new fields.
    let mut accepted = false;
    for _ in 0..10 {
        let t = Tick {
            input: (!accepted).then(|| compact_fixture(200)),
            output_ready: false,
            ..Default::default()
        };
        let s = e.tick(t).unwrap();
        accepted |= s.accepted;
        a.edge(t, &s);
    }
    assert!(accepted && e.output().is_some());
    a.reset();
    e = c::compact_stage::CompactCoefficientEmu::new(20_000).unwrap();
    accepted = false;
    for _ in 0..12 {
        let t = Tick {
            input: (!accepted).then(|| compact_fixture(211)),
            ..Default::default()
        };
        let s = e.tick(t).unwrap();
        accepted |= s.accepted;
        a.edge(t, &s);
    }
    assert!(e.idle());
    a.finish("directed-compact")
}
#[test]
fn coefficient_ready_lifetimes_and_actual_runtime_golden() {
    directed();
    compact_directed();
    for (p, w) in [(false, 16), (true, 16), (true, 2)] {
        runtime(p, w);
    }
}
fn bounded(mut command: Command, dir: &Path, name: &str) {
    command
        .stdout(Stdio::from(
            std::fs::File::create(dir.join(format!("{name}.out"))).unwrap(),
        ))
        .stderr(Stdio::from(
            std::fs::File::create(dir.join(format!("{name}.err"))).unwrap(),
        ));
    let mut child = command.spawn().unwrap();
    let start = Instant::now();
    loop {
        if let Some(s) = child.try_wait().unwrap() {
            assert!(
                s.success(),
                "{name}: {} {}",
                std::fs::read_to_string(dir.join(format!("{name}.out"))).unwrap(),
                std::fs::read_to_string(dir.join(format!("{name}.err"))).unwrap()
            );
            break;
        }
        if start.elapsed() > Duration::from_secs(60) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("watchdog {name}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
#[test]
#[ignore = "requires Gowin and Icarus"]
fn coefficient_ready_native_replay() {
    let dirs = [
        directed(),
        compact_directed(),
        runtime(false, 16),
        runtime(true, 16),
        runtime(true, 2),
    ];
    for d in dirs {
        let mut c = Command::new(std::env::var_os("IVERILOG_EXE").unwrap());
        c.current_dir(&d)
            .args([
                "-g2012",
                "-s",
                "tb",
                "-o",
                "test.vvp",
                "coefficient.v",
                "tb.v",
            ])
            .arg(
                Path::new(&std::env::var_os("GOWIN_HOME").unwrap())
                    .join("IDE/simlib/gw2a/prim_sim.v"),
            );
        bounded(c, &d, "compile");
        let mut v = Command::new(std::env::var_os("VVP_EXE").unwrap());
        v.current_dir(&d).arg("test.vvp");
        bounded(v, &d, "run");
    }
}

fn compact_metadata(i: Input) -> u128 {
    let x = i.metadata.coordinates[0][0];
    let y = i.metadata.coordinates[0][2];
    u128::from(x)
        | (u128::from(y) << 10)
        | (u128::from(x & 1 == 0 && i.fractions[0][0] & 128 == 0) << 20)
        | (u128::from(y & 1 == 0 && i.fractions[0][1] & 128 == 0) << 21)
        | (u128::from(i.metadata.levels[0]) << 22)
        | (u128::from(i.metadata.slot) << 26)
        | (u128::from(i.metadata.key) << 30)
}
fn compact_packed(o: Output, meta: u128) -> [u64; 3] {
    let w = weights(o);
    [w[0], w[1] | ((meta as u64) << 8), 0]
}

fn compact_fixture(i: usize) -> Input {
    let (n, uv, nearest, p1) = match i % 7 {
        0 => (2, [12288u16, 40960u16], true, 0u16),
        1 => (8, [255, 512], false, 256),
        2 => (8, [639, 128], false, 383),
        _ => (
            (i % 11) as u8,
            [(i * 127 % 65536) as u16, (i * 113 % 65536) as u16],
            i.is_multiple_of(6),
            (i % 512) as u16,
        ),
    };
    let p1 = if nearest { 0 } else { p1 };
    let phys = n.max(1);
    let mask = (1i32 << phys) - 1;
    let axes = uv.map(|v| {
        let f = ((i32::from(v) << 2) >> (10 - phys)) - if nearest { 0 } else { 128 };
        [
            (f, mask),
            (
                if n > 1 { (f - 128) >> 1 } else { f },
                if n > 1 { (1 << (n - 1)) - 1 } else { mask },
            ),
        ]
    });
    Input {
        parents: [511 - p1, p1],
        nearest,
        fractions: std::array::from_fn(|p| std::array::from_fn(|a| axes[a][p].0 as u8)),
        metadata: c::Metadata {
            coordinates: std::array::from_fn(|p| {
                let (x, m) = axes[0][p];
                let (y, _) = axes[1][p];
                [
                    (x >> 8) & m,
                    ((x >> 8) + 1) & m,
                    (y >> 8) & m,
                    ((y >> 8) + 1) & m,
                ]
                .map(|v| v as u16)
            }),
            levels: [n, n.saturating_sub(1)],
            slot: (i % 16) as u8,
            key: (i % 64) as u8,
            last_fine: p1 == 0,
        },
    }
}
