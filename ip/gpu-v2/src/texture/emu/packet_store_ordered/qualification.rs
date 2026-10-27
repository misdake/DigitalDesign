//! Actual selected Store ports and paid synchronous payload DO, bounded replay.
use super::*;
use std::{
    fmt::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
pub(crate) struct Replay {
    tb: String,
    edges: usize,
    fault: bool,
}
impl Replay {
    pub(crate) fn new() -> Self {
        Self { tb:String::from("module tb;reg clk=0,reset=0,ce=0,reserve_valid=0,write_valid=0,owner_request=0,consume=0;reg[5:0]reserve_owner=0;reg[71:0]write_payload=0;wire producer_ready,front_valid,fault,bad;wire[71:0]front_payload,returned_payload;wire[4:0]producer_count,unwritten_count;wire[5:0]group_count,row_count,allocate_row,write_row,transfer_row,read_row,reclaim_row,strobes,owner_value;wire[3:0]owner_address;wire[1:0]head_valid_out;wire pending_out;\n`ifdef NATIVE\nGSR GSR(.GSRI(1'b1));\n`endif\ngpu_v2_packet_store_ordered dut(.*);initial begin #8000000;$fatal(1,\"watchdog\");end initial begin\n"),edges:0,fault:false }
    }
    pub(crate) fn sample(
        &mut self,
        t: Tick,
        e: Result<Edge, String>,
        before: Option<u128>,
        after: (Control, Option<u128>, bool, u128),
    ) {
        let error = e.is_err();
        let e = e.unwrap_or_default();
        let b = |v| u8::from(v);
        writeln!(self.tb,"reset={};ce={};reserve_valid={};reserve_owner=6'd{};write_valid={};write_payload=72'h{:018x};owner_request={};consume={};#1;",b(t.reset),b(t.ce),b(t.reserve.is_some()),t.reserve.unwrap_or(0),b(t.write.is_some()),t.write.unwrap_or(0),b(t.owner_read),b(t.consume)).unwrap();
        let active = t.ce && !t.reset && !error;
        let strobes = if active {
            b(e.reserve.is_some())
                | b(e.write.is_some()) << 1
                | b(e.transfer.is_some()) << 2
                | b(e.read.is_some()) << 3
                | b(e.capture.is_some()) << 4
                | b(e.consume.is_some()) << 5
        } else {
            0
        };
        writeln!(
            self.tb,
            "if(bad!==1'b{} || strobes!==6'd{})$fatal(1,\"ports {}\");",
            b(error && t.ce && !self.fault),
            strobes,
            self.edges
        )
        .unwrap();
        if !t.reset {
            writeln!(
                self.tb,
                "if(front_valid!==1'b{})$fatal(1,\"old valid {}\");",
                b(before.is_some()),
                self.edges
            )
            .unwrap();
            if let Some(w) = before {
                writeln!(
                    self.tb,
                    "if(front_payload!==72'h{w:018x})$fatal(1,\"old payload {}\");",
                    self.edges
                )
                .unwrap();
            }
        }
        if active {
            if let Some((_, _, w)) = e.capture {
                writeln!(
                    self.tb,
                    "if(returned_payload!==72'h{w:018x})$fatal(1,\"capture DO {}\");",
                    self.edges
                )
                .unwrap();
            }
        }
        self.tb.push_str("clk=1;#1;\n");
        let (c, front, fault, hard) = after;
        for (name, v) in [
            ("alloc", u64::from(c.allocate)),
            ("wptr", c.write.into()),
            ("tptr", c.transfer.into()),
            ("rptr", c.read.into()),
            ("reclaim", c.reclaim.into()),
            ("pc", c.producer.into()),
            ("gc", c.groups.into()),
            ("rows", c.rows.into()),
            ("uw", c.unwritten.into()),
            ("hv", (b(c.head_valid[0]) | b(c.head_valid[1]) << 1).into()),
            ("rh", c.read_head.into()),
            ("ph", c.pop_head.into()),
            ("pending", b(c.pending).into()),
            ("prow", c.pending_row.into()),
            ("phead", c.pending_head.into()),
            ("fault", b(fault).into()),
        ] {
            writeln!(
                self.tb,
                "if(dut.{name}!=={v})$fatal(1,\"state {name} {}\");",
                self.edges
            )
            .unwrap();
        }
        writeln!(
            self.tb,
            "if(returned_payload!==72'h{hard:018x})$fatal(1,\"paid DO every wall edge {}\");",
            self.edges
        )
        .unwrap();
        if let Some(w) = front {
            writeln!(
                self.tb,
                "if(!front_valid || front_payload!==72'h{w:018x})$fatal(1,\"new payload {}\");",
                self.edges
            )
            .unwrap();
        }
        self.tb.push_str("clk=0;#1;\n");
        self.edges += 1;
        self.fault = fault;
    }
    pub(crate) fn finish(mut self, path: &Path) {
        std::fs::create_dir_all(path).unwrap();
        self.tb
            .push_str("$display(\"PASS packet store\");$finish;end\n`ifdef NATIVE\n");
        for b in 0..2 {
            writeln!(self.tb,"always @(posedge clk) if(dut.payload_hardware.banks[{b}].bank.CEA!==dut.pw || dut.payload_hardware.banks[{b}].bank.CEB!==dut.pr || dut.payload_hardware.banks[{b}].bank.DI!==write_payload[{}+:36] || dut.payload_hardware.banks[{b}].bank.ADA!=={{3'b0,dut.wptr,1'b0,4'b1111}} || dut.payload_hardware.banks[{b}].bank.ADB!=={{3'b0,dut.rptr,5'b0}})$fatal(1,\"payload pins\");",b*36).unwrap();
        }
        self.tb.push_str("`endif\nendmodule\n");
        std::fs::write(path.join("tb.v"), self.tb).unwrap();
        for (name, native) in [("portable", false), ("native", true)] {
            std::fs::write(
                path.join(format!("{name}.v")),
                crate::texture::rtl::packet_store_ordered::source(native),
            )
            .unwrap();
        }
        std::fs::write(path.join("receipt.json"),format!("{{\"edges\":{},\"scope\":\"actual selected pool model/ports/pins/paidDO versus portable/native RTL\"}}",self.edges)).unwrap();
    }
}
fn bounded(mut c: Command, path: &Path, label: &str) {
    c.stdout(Stdio::from(
        std::fs::File::create(path.join(format!("{label}.out"))).unwrap(),
    ))
    .stderr(Stdio::from(
        std::fs::File::create(path.join(format!("{label}.err"))).unwrap(),
    ));
    let mut child = c.spawn().unwrap();
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(
                status.success(),
                "{label}: {} {}",
                std::fs::read_to_string(path.join(format!("{label}.out"))).unwrap(),
                std::fs::read_to_string(path.join(format!("{label}.err"))).unwrap()
            );
            break;
        }
        if start.elapsed() > Duration::from_secs(60) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("packet RTL process watchdog");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
pub(crate) fn run(path: &Path) {
    std::thread::scope(|scope| {
        for name in ["portable", "native"] {
            scope.spawn(move || run_variant(path, name));
        }
    });
}
fn run_variant(path: &Path, name: &str) {
    let mut c = Command::new(std::env::var_os("IVERILOG_EXE").unwrap());
    if name != "portable" {
        c.arg("-DNATIVE");
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
    if name != "portable" {
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
            .contains("PASS packet store")
    );
}

fn root() -> PathBuf {
    std::env::var_os("GPU_ORDERED_PACKET_STORAGE_OUTPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ordered-pool"))
}
fn invariants(c: Control) {
    let d = |a: u8, b: u8| a.wrapping_sub(b) & 63;
    let heads = c.head_valid.iter().filter(|&&v| v).count() as u8;
    assert!(c.producer <= 16 && c.groups <= 32 && c.rows <= 48 && c.unwritten <= c.producer);
    assert!(heads + u8::from(c.pending) <= 2);
    assert_eq!(d(c.allocate, c.write), c.unwritten);
    assert_eq!(d(c.allocate, c.transfer), c.producer);
    assert_eq!(d(c.allocate, c.reclaim), c.rows);
    assert_eq!(c.rows, c.producer + c.groups - heads);
    assert_eq!(
        d(c.transfer, c.read),
        c.groups - heads - u8::from(c.pending)
    );
    assert_eq!(
        c.read != c.transfer,
        c.groups - heads - u8::from(c.pending) != 0
    );
}
fn word(i: u64) -> u128 {
    ((u128::from(i) * 0x123456789ab) & ((1_u128 << 66) - 1))
        | (u128::from((i * 7) % 16) << 66)
        | (u128::from((i * 3) % 4) << 70)
}
fn stress(path: &Path) {
    use std::collections::VecDeque;
    let mut s = Store::default();
    let mut replay = Replay::new();
    let mut ram = [None; 64];
    let mut hard = 0;
    let mut queue = VecDeque::new();
    let mut pipe = [None; 2];
    let mut issued = 0_u64;
    let mut consumed = 0;
    let mut rng = 0x39825161_u32;
    let (mut peakp, mut peakg, mut wraps, mut ce0, mut full_wait) = (0, 0, 0, 0, 0);
    let tick = Tick {
        reset: true,
        ..Default::default()
    };
    let e = s.tick(tick).unwrap();
    replay.sample(
        tick,
        Ok(e),
        None,
        (s.control, s.front(), s.faulted(), s.hard_output()),
    );
    for wall in 0..50_000 {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        let ce = rng % 7 > 1;
        let old = s.control;
        let before = s.front();
        let input = (ce && issued < 4096 && s.producer_ready()).then(|| word(issued));
        let key = |w: u128| ((((w >> 66) & 15) * 4) + ((w >> 70) & 3)) as u8;
        let t = Tick {
            ce,
            reserve: input.map(key),
            write: pipe[1],
            owner_read: pipe[0].is_some(),
            consume: ce && wall > 600 && rng % 13 > 2 && before.is_some(),
            ..Default::default()
        };
        let e = s.tick(t).unwrap();
        if ce {
            if let Some((_, w)) = e.consume {
                assert_eq!(Some(w), queue.pop_front());
                consumed += 1;
            }
            if let Some((_, _, w)) = e.capture {
                assert_eq!(w, hard);
            }
            if let Some((row, _)) = e.read {
                hard = ram[row as usize].expect("old written RAM only");
            }
            if let Some(row) = e.write {
                ram[row as usize] = t.write;
                wraps += usize::from(row == 0);
            }
            if let Some(w) = input {
                queue.push_back(w);
                issued += 1;
            }
            if old.groups == 32 {
                assert!(e.transfer.is_none());
                full_wait += 1;
            }
            if old.producer == 16 {
                assert!(e.reserve.is_none());
            }
            if e.write == Some(old.transfer) {
                assert!(e.transfer.is_none());
            }
            if e.transfer == Some(old.read) {
                assert!(e.read.is_none());
            }
            pipe = [input, pipe[0]];
        } else if old.pending {
            ce0 += 1;
        }
        invariants(s.control);
        assert_eq!(s.hard_output(), hard);
        peakp = peakp.max(s.control.producer);
        peakg = peakg.max(s.control.groups);
        replay.sample(
            t,
            Ok(e),
            before,
            (s.control, s.front(), s.faulted(), s.hard_output()),
        );
        if issued == 4096 && s.idle() {
            assert_eq!(consumed, 4096);
            break;
        }
        assert!(wall < 49_999, "stress watchdog");
    }
    assert_eq!((peakp, peakg), (16, 32));
    assert!(wraps >= 64 && ce0 > 0 && full_wait > 0);
    // Reset a genuinely live owned head; no stale payload may reappear.
    for i in 0..8 {
        let payload = word(i);
        let key = ((((payload >> 66) & 15) * 4) + ((payload >> 70) & 3)) as u8;
        let t = Tick {
            ce: true,
            reserve: Some(key),
            write: pipe[1],
            owner_read: pipe[0].is_some(),
            ..Default::default()
        };
        let before = s.front();
        let e = s.tick(t).unwrap();
        replay.sample(
            t,
            Ok(e),
            before,
            (s.control, s.front(), false, s.hard_output()),
        );
        pipe = [Some(payload), pipe[0]];
    }
    assert!(s.front().is_some());
    let t = Tick {
        reset: true,
        ..Default::default()
    };
    let before = s.front();
    let e = s.tick(t).unwrap();
    replay.sample(
        t,
        Ok(e),
        before,
        (s.control, s.front(), false, s.hard_output()),
    );
    assert!(s.idle() && s.front().is_none());
    replay.finish(path);
    println!("ORDERED_STRESS packets={consumed} P={peakp} G={peakg} wraps={wraps} CE0paid={ce0} fullWait={full_wait}");
}
fn faults(path: &Path) {
    let mut replay = Replay::new();
    for kind in 0..3 {
        let mut s = Store::default();
        let t = Tick {
            reset: true,
            ..Default::default()
        };
        let e = s.tick(t).unwrap();
        replay.sample(
            t,
            Ok(e),
            None,
            (s.control, s.front(), false, s.hard_output()),
        );
        if kind == 2 {
            let mut pipe = [None; 2];
            for i in 0..200 {
                let input = s.producer_ready().then(|| word(i));
                let key = |w: u128| ((((w >> 66) & 15) * 4) + ((w >> 70) & 3)) as u8;
                let t = Tick {
                    ce: true,
                    reserve: input.map(key),
                    write: pipe[1],
                    owner_read: pipe[0].is_some(),
                    ..Default::default()
                };
                let before = s.front();
                let e = s.tick(t).unwrap();
                replay.sample(
                    t,
                    Ok(e),
                    before,
                    (s.control, s.front(), false, s.hard_output()),
                );
                pipe = [input, pipe[0]];
                if s.control.producer == 16 && pipe == [None, None] {
                    break;
                }
                assert!(i < 199);
            }
            assert_eq!(s.control.producer, 16);
            assert!(s.front().is_some());
        }
        let before = (s.control, s.front(), s.hard_output());
        let t = match kind {
            0 => Tick {
                ce: true,
                consume: true,
                ..Default::default()
            },
            1 => Tick {
                ce: true,
                reserve: Some(0),
                write: Some(0),
                ..Default::default()
            },
            _ => Tick {
                ce: true,
                reserve: Some(0),
                consume: true,
                ..Default::default()
            },
        };
        let e = s.tick(t);
        assert!(e.is_err());
        assert_eq!((s.control, s.front(), s.hard_output()), before);
        replay.sample(
            t,
            e,
            before.1,
            (s.control, s.front(), true, s.hard_output()),
        );
        for ce in [false, true] {
            let t = Tick {
                ce,
                ..Default::default()
            };
            let e = s.tick(t);
            assert!(e.is_err());
            replay.sample(
                t,
                e,
                s.front(),
                (s.control, s.front(), true, s.hard_output()),
            );
        }
    }
    replay.finish(path);
}
#[test]
fn ordered_pool_model_full_credits_wrap_reset_and_atomic_faults() {
    stress(&root().join("directed-model"));
    faults(&root().join("faults-model"));
}
#[test]
fn generic_owner_diagnostic_remains_independent() {
    let mut generic = crate::texture::emu::packet_store::Store::default();
    let mut selected = Store::default();
    for t in [
        Tick {
            reset: true,
            ..Default::default()
        },
        Tick {
            ce: true,
            reserve: Some(0),
            ..Default::default()
        },
        Tick {
            ce: true,
            owner_read: true,
            ..Default::default()
        },
    ] {
        generic
            .tick(crate::texture::emu::packet_store::Tick {
                reset: t.reset,
                ce: t.ce,
                reserve: t.reserve,
                write: t.write,
                owner_read: t.owner_read,
                consume: t.consume,
            })
            .unwrap();
        selected.tick(t).unwrap();
    }
    let t = Tick {
        ce: true,
        write: Some(1_u128 << 66),
        ..Default::default()
    };
    assert!(generic
        .tick(crate::texture::emu::packet_store::Tick {
            ce: true,
            write: t.write,
            ..Default::default()
        })
        .unwrap_err()
        .contains("owner mismatch"));
    assert!(
        selected.tick(t).is_ok(),
        "trusted profile deliberately has no expected-key table"
    );
}
#[test]
#[ignore = "requires Icarus and native Gowin primitives"]
fn ordered_pool_directed_portable_native() {
    let p = root().join("directed-rtl");
    stress(&p);
    run(&p);
    let p = root().join("faults-rtl");
    faults(&p);
    run(&p);
}
