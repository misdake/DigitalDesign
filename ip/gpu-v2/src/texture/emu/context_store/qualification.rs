//! Independent wire concatenations, native port replay and same-instance reuse.
use super::*;
use std::{
    fmt::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
fn h(v: Header) -> u128 {
    (((((u128::from(v.filter) * 2 + u128::from(v.has_mip)) * 16 + u128::from(v.max_n)) * 16
        + u128::from(v.slot))
        * 16
        + u128::from(v.mask))
        * 16)
        + u128::from(v.quad)
}
fn rw(v: Raw) -> u128 {
    ((h(v.header) * 2 + u128::from(v.force_coarsest)) * 65536) + v.bias as u16 as u128
}
fn dw(v: lod::Input) -> u128 {
    ((h(v.header) * 65536 + v.bias as u16 as u128) * 262144) + u128::from(v.slope)
}
fn lw(v: lod::Output) -> u128 {
    let c = v.context;
    let mut w = u128::from(v.slot);
    for (width, value) in [
        (4, u128::from(v.mask)),
        (4, u128::from(v.quad)),
        (1, u128::from(c.last_fine)),
        (4, u128::from(c.levels[1])),
        (4, u128::from(c.levels[0])),
        (9, u128::from(c.parents[1])),
        (9, u128::from(c.parents[0])),
        (12, c.side[1] as u16 as u128 & 4095),
        (12, c.side[0] as u16 as u128 & 4095),
        (1, u128::from(c.halve)),
        (1, u128::from(c.nearest)),
        (18, c.shift as u32 as u128 & 0x3ffff),
    ] {
        w = (w << width) | value;
    }
    w
}
pub(crate) struct Replay {
    tb: String,
    edges: usize,
    capacity: usize,
    lod_ready: bool,
}
impl Replay {
    pub(crate) fn new(capacity: usize) -> Self {
        Self::with_format(capacity, false)
    }
    pub(crate) fn new_lod_ready(capacity: usize) -> Self {
        Self::with_format(capacity, true)
    }
    fn with_format(capacity: usize, lod_ready: bool) -> Self {
        let mut tb=String::from("module tb;reg clk=0,reset=1,ce=0,fault=0,control_bad=0;reg[2:0]phase=0;reg admit=0,d_issue=0,l_issue=0,c_read=0,d_return=0,l_return=0;reg[3:0]allocate=0,d_head=0,l_head=0,c_head=0,d_owner=0,l_owner=0;reg[35:0]raw_data=0;reg[52:0]derived_data=0;reg[82:0]ready_data=0;wire bad,advance;wire[2:0]writes,reads;wire[3:0]raw_ra,derived_ra,ready_ra;wire[35:0]raw_q;wire[55:0]derived_q;wire[83:0]ready_q;\n`ifdef CONTEXT_NATIVE\nGSR GSR(.GSRI(1'b1));\n`endif\ngpu_v2_context_store dut(.*);\n");
        tb.push_str("`ifdef CONTEXT_NATIVE\n");
        tb.push_str("wire[27:0] compact_expected={2'b0,ready_data[82:71],ready_data[61:53],ready_data[18],ready_data[65:62]};\n");
        for (bank, n, wa, ra, data) in [
            ("raw", 9, "allocate", "raw_ra", "raw_data"),
            ("derived", 14, "d_owner", "derived_ra", "dut.padded_derived"),
            (
                "ready",
                if lod_ready { 7 } else { 21 },
                "l_owner",
                "ready_ra",
                if lod_ready {
                    "compact_expected"
                } else {
                    "dut.padded_ready"
                },
            ),
        ] {
            let b = if bank == "raw" {
                0
            } else if bank == "derived" {
                1
            } else {
                2
            };
            for i in 0..n {
                let ram = if bank == "ready" {
                    if lod_ready {
                        format!("dut.sram.compact_ready.bank[{i}].ram")
                    } else {
                        format!("dut.sram.full_ready.ready_bank[{i}].ram")
                    }
                } else {
                    format!("dut.sram.{bank}_bank[{i}].ram")
                };
                writeln!(tb,"always @(posedge clk) if({ram}.WRE!==writes[{b}] || {ram}.WAD!=={wa} || {ram}.RAD!=={ra} || {ram}.DI!=={data}[{}+:4])$fatal(1,\"native {bank}{i} pins\");",i*4).unwrap();
            }
        }
        tb.push_str("`endif\ninitial begin #4000000;$fatal(1,\"watchdog\");end initial begin #1;clk=1;#1;clk=0;reset=0;#1;\n");
        Self {
            tb,
            edges: 0,
            capacity,
            lod_ready,
        }
    }
    pub(crate) fn edge(&mut self, t: Tick, e: Result<Edge, ()>) {
        let bad = e.is_err();
        let e = e.unwrap_or_default();
        let active = !t.reset && t.ce && !t.fault && !bad;
        let bit = |v| u8::from(v);
        let a = |v: Option<usize>| v.unwrap_or(0);
        writeln!(self.tb,"reset={};ce={};fault={};control_bad={};phase=3'd{};admit={};d_issue={};l_issue={};c_read={};d_return={};l_return={};allocate=4'd{};d_head=4'd{};l_head=4'd{};c_head=4'd{};d_owner=4'd{};l_owner=4'd{};raw_data=36'h{:09x};derived_data=53'h{:014x};ready_data=83'h{:021x};#1;",bit(t.reset),bit(t.ce),bit(t.fault),bit(t.control_bad),t.phase,bit(t.admission.is_some()),bit(t.raw_issue.is_some()),bit(t.derived_issue.is_some()),bit(t.ready_read.is_some()),bit(t.d_return.is_some()),bit(t.l_return.is_some()),a(t.admission.map(|x|x.0)),a(t.raw_issue),a(t.derived_issue),a(t.ready_read),a(t.d_return.map(|x|x.0)),a(t.l_return.map(|x|x.0)),t.admission.map_or(0,|x|rw(x.1)),t.d_return.map_or(0,|x|dw(x.1)),t.l_return.map_or(0,|x|lw(x.1))).unwrap();
        let mask = |p: [Option<(usize, u128)>; 3]| {
            p.iter()
                .enumerate()
                .fold(0, |v, (i, x)| v | (usize::from(x.is_some()) << i))
        };
        writeln!(self.tb,"if(bad!=={} || advance!=={} || reads!==3'd{} || writes!==3'd{})$fatal(1,\"control edge{}\");",bit(bad),bit(active),mask(e.reads),mask(e.writes),self.edges).unwrap();
        for (b, port) in ["raw_q", "derived_q", "ready_q"].into_iter().enumerate() {
            if let Some((addr, value)) = e.reads[b] {
                let width = [36, 56, 84][b];
                let ra = ["raw_ra", "derived_ra", "ready_ra"][b];
                writeln!(self.tb,"if({port}!=={width}'h{value:x} || {ra}!==4'd{addr})$fatal(1,\"{port} edge{}\");",self.edges).unwrap();
            }
        }
        self.tb.push_str("clk=1;#1;clk=0;#1;\n");
        self.edges += 1;
    }
    pub(crate) fn finish(mut self, path: &Path) {
        std::fs::create_dir_all(path).unwrap();
        self.tb
            .push_str("$display(\"PASS context banks\");$finish;end endmodule\n");
        std::fs::write(path.join("tb.v"), self.tb).unwrap();
        for (name, native, union) in [
            ("portable", false, false),
            ("native", true, false),
            ("union", false, true),
        ] {
            std::fs::write(
                path.join(format!("{name}.v")),
                crate::texture::rtl::context_store::source_with_lod_ready(
                    self.capacity,
                    native,
                    union,
                    self.lod_ready,
                )
                .unwrap(),
            )
            .unwrap();
        }
        std::fs::write(path.join("receipt.json"),format!("{{\"capacity\":{},\"edges\":{},\"ports\":\"three fixed1W1R banks; shared issue/identity R\"}}",self.capacity,self.edges)).unwrap();
    }
}
fn bounded(mut c: Command, dir: &Path, name: &str) {
    c.stdout(Stdio::from(
        std::fs::File::create(dir.join(format!("{name}.out"))).unwrap(),
    ))
    .stderr(Stdio::from(
        std::fs::File::create(dir.join(format!("{name}.err"))).unwrap(),
    ));
    let mut p = c.spawn().unwrap();
    let start = Instant::now();
    loop {
        if let Some(s) = p.try_wait().unwrap() {
            assert!(
                s.success(),
                "{name}: {} {}",
                std::fs::read_to_string(dir.join(format!("{name}.out"))).unwrap(),
                std::fs::read_to_string(dir.join(format!("{name}.err"))).unwrap()
            );
            break;
        }
        if start.elapsed() > Duration::from_secs(60) {
            let _ = p.kill();
            let _ = p.wait();
            panic!("context replay watchdog");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
pub(crate) fn run(path: &Path) {
    for name in ["portable", "native", "union"] {
        let mut c = Command::new(std::env::var_os("IVERILOG_EXE").unwrap());
        if name == "native" {
            c.arg("-DCONTEXT_NATIVE");
        }
        c.current_dir(path).args([
            "-g2012",
            "-s",
            "tb",
            "-o",
            &format!("{name}.vvp"),
            &format!("{name}.v"),
            "tb.v",
        ]);
        if name == "native" {
            c.arg(
                PathBuf::from(std::env::var_os("GOWIN_HOME").unwrap())
                    .join("IDE/simlib/gw2a/prim_sim.v"),
            );
        }
        bounded(c, path, &format!("compile-{name}"));
        let mut c = Command::new(std::env::var_os("VVP_EXE").unwrap());
        c.current_dir(path).arg(format!("{name}.vvp"));
        bounded(c, path, &format!("run-{name}"));
        assert!(
            std::fs::read_to_string(path.join(format!("run-{name}.out")))
                .unwrap()
                .contains("PASS context banks")
        );
    }
}
fn fixture(i: usize) -> (Raw, lod::Input, lod::Output) {
    let bias = [-8192, -1, 0, 8192][i % 4];
    let header = Header {
        quad: (i % 16) as u8,
        mask: ((i * 7) % 16) as u8,
        slot: ((i * 3) % 16) as u8,
        max_n: (i % 11) as u8,
        has_mip: i & 1 != 0,
        filter: (i % 3) as u8,
    };
    (
        Raw {
            bias,
            force_coarsest: i & 2 != 0,
            header,
        },
        lod::Input {
            slope: ((i * 1723) % 262144) as u64,
            bias,
            header,
        },
        lod::Output {
            quad: header.quad,
            mask: header.mask,
            slot: header.slot,
            context: lod::CoordinateContext {
                shift: [-131072, -1, 0, 131071][i % 4],
                nearest: i & 1 != 0,
                halve: i & 2 != 0,
                side: [
                    [-2048, -1, 0, 2047][i % 4],
                    [-2048, -1, 0, 2047][(i + 1) % 4],
                ],
                parents: [(i % 512) as u16, 511 - (i % 512) as u16],
                levels: [(i % 16) as u8, ((i * 3) % 16) as u8],
                last_fine: i & 1 != 0,
            },
        },
    )
}

fn canonical(n: u8, parent: u16, nearest: bool, identity: usize) -> lod::Output {
    let coarse = n.saturating_sub(1);
    lod::Output {
        quad: (identity & 15) as u8,
        mask: ((identity >> 4) & 15) as u8,
        slot: ((identity >> 8) & 15) as u8,
        context: lod::CoordinateContext {
            shift: i32::from(n.max(1)) - 8,
            nearest,
            halve: n >= 2,
            side: [
                2_i16.pow(u32::from(n.max(1))),
                2_i16.pow(u32::from(coarse.max(1))),
            ],
            parents: [parent ^ 511, parent],
            levels: [n, coarse],
            last_fine: parent == 0,
        },
    }
}

#[test]
fn canonical_ready_decision_exhausts_levels_parents_and_identity() {
    let mut count = 0;
    for n in 0..=10 {
        for parent in 0..=511 {
            for nearest in [false, true] {
                let v = canonical(n, parent, nearest, count % 4096);
                let decision = ReadyDecision::capture(v).unwrap();
                let id = (u32::from(v.slot) * 16 + u32::from(v.mask)) * 16 + u32::from(v.quad);
                let golden =
                    ((id * 512 + u32::from(parent)) * 2 + u32::from(nearest)) * 16 + u32::from(n);
                assert_eq!(decision.bits(), golden);
                assert_eq!(lw(decision.expand()), lw(v));
                count += 1;
            }
        }
    }
    for identity in 0..4096 {
        let v = canonical(
            (identity % 11) as u8,
            (identity % 512) as u16,
            identity & 1 != 0,
            identity,
        );
        assert_eq!(ReadyDecision::capture(v).unwrap().expand(), v);
        count += 1;
    }
    assert_eq!(count, 15_360);
}

fn canonical_directed(native: bool) {
    for capacity in 1..=8 {
        let mut store = Store::new_lod_ready(capacity).unwrap();
        let mut replay = Replay::new_lod_ready(capacity);
        for epoch in 0..3 {
            for row in 0..capacity {
                let (raw, derived, _) = fixture(row + epoch * 19);
                for n in 0..=10 {
                    for parent in [0, 1, 255, 256, 511] {
                        // Reuse follows the real Raw -> Derived -> Ready lifecycle.
                        // The union reference has only one payload per context.
                        drive(
                            &mut store,
                            &mut replay,
                            Tick {
                                ce: true,
                                admission: Some((row, raw)),
                                ..Default::default()
                            },
                        )
                        .unwrap();
                        drive(
                            &mut store,
                            &mut replay,
                            Tick {
                                ce: true,
                                phase: 3,
                                d_return: Some((row, derived)),
                                ..Default::default()
                            },
                        )
                        .unwrap();
                        let identity = usize::from(raw.header.quad)
                            + 16 * usize::from(raw.header.mask)
                            + 256 * usize::from(raw.header.slot);
                        let ready = canonical(n, parent, epoch & 1 != 0, identity);
                        let tick = Tick {
                            ce: true,
                            phase: 0,
                            l_return: Some((row, ready)),
                            ..Default::default()
                        };
                        let before = store.rows();
                        for mutation in 0..7 {
                            let mut bad = ready;
                            match mutation {
                                0 => bad.context.shift ^= 1,
                                1 => bad.context.side[0] ^= 1,
                                2 => bad.context.side[1] ^= 1,
                                3 => bad.context.parents[0] ^= 1,
                                4 => bad.context.levels[1] ^= 1,
                                5 => bad.context.halve = !bad.context.halve,
                                _ => bad.context.last_fine = !bad.context.last_fine,
                            }
                            assert!(drive(
                                &mut store,
                                &mut replay,
                                Tick {
                                    l_return: Some((row, bad)),
                                    ..tick
                                }
                            )
                            .is_err());
                            assert_eq!(store.rows(), before);
                            // CE0 discards even a malformed offered return.
                            drive(
                                &mut store,
                                &mut replay,
                                Tick {
                                    ce: false,
                                    l_return: Some((row, bad)),
                                    ..tick
                                },
                            )
                            .unwrap();
                        }
                        drive(&mut store, &mut replay, tick).unwrap();
                        let edge = drive(
                            &mut store,
                            &mut replay,
                            Tick {
                                ce: true,
                                ready_read: Some(row),
                                ..Default::default()
                            },
                        )
                        .unwrap();
                        assert_eq!(edge.reads[2], Some((row, lw(ready))));
                    }
                }
            }
            drive(
                &mut store,
                &mut replay,
                Tick {
                    reset: true,
                    ce: true,
                    ..Default::default()
                },
            )
            .unwrap();
        }
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/sampling-d-shared-20261009/canonical")
            .join(format!("{capacity}-{native}"));
        replay.finish(&path);
        if native {
            run(&path);
        }
    }
}

#[test]
fn canonical_ready_store_rejects_each_inconsistent_operand_before_commit() {
    canonical_directed(false);
}

#[test]
#[ignore = "requires Icarus and GOWIN_HOME; compact/general native fault and reuse"]
fn canonical_ready_store_native_faults_and_reuse() {
    canonical_directed(true);
}

fn drive(s: &mut Store, r: &mut Replay, t: Tick) -> Result<Edge, ()> {
    let before = s.rows();
    let edge = s.prepare(t);
    r.edge(t, edge.as_ref().copied().map_err(|_| ()));
    if let Ok(e) = edge {
        s.commit(e);
        if t.reset || !t.ce || t.fault {
            assert_eq!(before, s.rows());
        }
        Ok(e)
    } else {
        assert_eq!(before, s.rows());
        Err(())
    }
}
fn directed(replay: bool) -> Vec<PathBuf> {
    let mut paths = vec![];
    for cap in 1..=8 {
        let mut s = Store::new(cap).unwrap();
        let mut r = Replay::new(cap);
        let mut epochs = 0;
        for epoch in 0..3 {
            // Same Store object survives full contamination and reset. No RAM clear.
            for slot in 0..cap {
                let (a, d, l) = fixture(slot + epoch * 19);
                assert_eq!(a.pack().unwrap() as u128, rw(a));
                assert_eq!(Raw::unpack(rw(a) as u64), a);
                assert_eq!(pack_derived(d).unwrap() as u128, dw(d));
                assert_eq!(unpack_derived(dw(d) as u64), d);
                assert_eq!(pack_ready(l).unwrap(), lw(l));
                assert_eq!(unpack_ready(lw(l)), l);
                for t in [
                    Tick {
                        ce: true,
                        admission: Some((slot, a)),
                        ..Default::default()
                    },
                    Tick {
                        ce: true,
                        raw_issue: Some(slot),
                        ..Default::default()
                    },
                    Tick {
                        ce: true,
                        phase: 3,
                        d_return: Some((slot, d)),
                        ..Default::default()
                    },
                    Tick {
                        ce: true,
                        phase: 4,
                        derived_issue: Some(slot),
                        ..Default::default()
                    },
                    Tick {
                        ce: true,
                        phase: 0,
                        l_return: Some((slot, l)),
                        ..Default::default()
                    },
                    Tick {
                        ce: true,
                        ready_read: Some(slot),
                        ..Default::default()
                    },
                ] {
                    drive(&mut s, &mut r, t).unwrap();
                }
            }
            for slot in 0..cap {
                let e = drive(
                    &mut s,
                    &mut r,
                    Tick {
                        ce: true,
                        ready_read: Some(slot),
                        ..Default::default()
                    },
                )
                .unwrap();
                assert_eq!(e.reads[2], Some((slot, lw(fixture(slot + epoch * 19).2))));
            }
            let (a, d, l) = fixture(epoch * 19);
            for t in [
                Tick {
                    admission: Some((0, a)),
                    d_return: Some((0, d)),
                    l_return: Some((0, l)),
                    ..Default::default()
                },
                Tick {
                    reset: true,
                    ce: true,
                    control_bad: true,
                    admission: Some((0, a)),
                    ..Default::default()
                },
            ] {
                drive(&mut s, &mut r, t).unwrap();
            }
            // Populate a Raw row then reject wrong identity and an unrelated write atomically.
            drive(
                &mut s,
                &mut r,
                Tick {
                    ce: true,
                    admission: Some((0, a)),
                    ..Default::default()
                },
            )
            .unwrap();
            let mut bad = d;
            bad.header.quad ^= 1;
            assert!(drive(
                &mut s,
                &mut r,
                Tick {
                    ce: true,
                    phase: 3,
                    admission: Some((cap - 1, a)),
                    d_return: Some((0, bad)),
                    ..Default::default()
                }
            )
            .is_err());
            let mut bad_raw = a;
            bad_raw.bias = 8193;
            assert!(drive(
                &mut s,
                &mut r,
                Tick {
                    ce: true,
                    admission: Some((0, bad_raw)),
                    ..Default::default()
                }
            )
            .is_err());
            assert!(drive(
                &mut s,
                &mut r,
                Tick {
                    ce: true,
                    raw_issue: Some(0),
                    admission: Some((0, a)),
                    ..Default::default()
                }
            )
            .is_err());
            assert!(drive(
                &mut s,
                &mut r,
                Tick {
                    ce: true,
                    phase: 3,
                    control_bad: true,
                    d_return: Some((0, d)),
                    ..Default::default()
                }
            )
            .is_err());
            drive(
                &mut s,
                &mut r,
                Tick {
                    ce: true,
                    fault: true,
                    admission: Some((0, a)),
                    ..Default::default()
                },
            )
            .unwrap();
            drive(
                &mut s,
                &mut r,
                Tick {
                    reset: true,
                    ..Default::default()
                },
            )
            .unwrap();
            epochs += 1;
        }
        assert_eq!(epochs, 3);
        let family = if replay { "directed-rtl" } else { "directed" };
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../../target/sampling-d-shared-20261009/general/{family}-N{cap}"
        ));
        r.finish(&path);
        paths.push(path);
    }
    paths
}
#[test]
fn context_storage_signed_ports_poison_reset_and_faults() {
    directed(false);
}
#[test]
#[ignore = "requires Icarus and GOWIN_HOME"]
fn context_storage_directed_native_replay() {
    for p in directed(true) {
        run(&p);
    }
}
