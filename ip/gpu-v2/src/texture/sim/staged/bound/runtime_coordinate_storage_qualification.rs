//! Independent ordered semantic scoreboard and actual Runtime command replay.
//! Selected Runtime uses physical owner24/result36; generic directed fixtures
//! retain owner38/result112. Wide owner pins are decoded observation wires.
use super::{qualification as q, *};
use crate::texture::{
    emu::{
        coefficient, coordinate,
        coordinate_store::{self as store, Owner},
    },
    ports::Filter,
    rtl,
};
use std::{
    collections::VecDeque,
    fmt::Write,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
fn input(o: Owner, v: coordinate::Output) -> coefficient::Input {
    coefficient::Input {
        parents: o.parents,
        fractions: v.fractions,
        nearest: o.nearest,
        metadata: coefficient::Metadata {
            coordinates: v.coordinates,
            levels: o.levels,
            slot: o.slot,
            key: o.key,
            last_fine: o.last_fine,
        },
    }
}
// Separate field concatenation for RTL pins and expected bits, not DUT packers.
fn owner_bits(o: Owner) -> u64 {
    (((((((u64::from(o.last_fine) * 64 + u64::from(o.key)) * 16 + u64::from(o.slot)) * 16
        + u64::from(o.levels[1]))
        * 16
        + u64::from(o.levels[0]))
        * 2
        + u64::from(o.nearest))
        * 512
        + u64::from(o.parents[1]))
        * 512)
        + u64::from(o.parents[0])
}
// Independent physical canonical concatenation: do not reinterpret an actual
// owner24 write as owner38, or use the DUT's codec to form expected storage.
fn canonical_owner_bits(o: Owner) -> u64 {
    (((u64::from(o.key) * 16 + u64::from(o.slot)) * 16 + u64::from(o.levels[0])) * 2
        + u64::from(o.nearest))
        * 512
        + u64::from(o.parents[1])
}
fn result_bits(v: coordinate::Output) -> u128 {
    let mut n = 0u128;
    for &x in v.coordinates.iter().flatten().rev() {
        n = n * 1024 + u128::from(x);
    }
    for &x in v.fractions.iter().flatten().rev() {
        n = n * 256 + u128::from(x);
    }
    n
}
fn fixture(i: usize) -> (Owner, coordinate::Output) {
    let p = (i * 173 % 512) as u16;
    (
        Owner {
            parents: [p, 511 - p],
            nearest: i & 1 != 0,
            levels: [(i % 11) as u8, ((i * 3) % 11) as u8],
            slot: (i % 16) as u8,
            key: (i % 64) as u8,
            last_fine: !i.is_multiple_of(3),
        },
        coordinate::Output {
            fractions: std::array::from_fn(|w| {
                std::array::from_fn(|a| ((i * 37 + w * 59 + a * 113) % 256) as u8)
            }),
            coordinates: std::array::from_fn(|w| {
                std::array::from_fn(|a| ((i * 271 + w * 97 + a * 503) % 1024) as u16)
            }),
        },
    )
}
struct Audit {
    compact: bool,
    capacity: usize,
    dut: store::Store,
    queue: VecDeque<(Owner, Option<coordinate::Output>)>,
    wall: usize,
    csv: String,
    tb: String,
    writes: [usize; 2],
    takes: usize,
    simultaneous: usize,
    held: usize,
    peak: usize,
    peak_ready: usize,
    rejected: usize,
    resets: usize,
}
impl Audit {
    fn new(cap: usize, compact: bool) -> Self {
        Self{compact,capacity:cap,dut:if compact {store::Store::new_canonical(cap).unwrap()} else {store::Store::new(cap).unwrap()},queue:VecDeque::new(),wall:0,
            csv:String::from("wall,CE,issue,returned,take,write,return,read,occupied,completed\n"),
            tb:String::from("module tb;reg clk=0,reset=1,ce=0,issue=0,returned=0,take=0;reg[37:0]owner_data=0;reg[111:0]result_data=0;wire in_ready,out_valid,bad,owner_write,result_write,consume;wire[37:0]out_owner;wire[111:0]out_result;wire[3:0]write_row,return_row,read_row;wire[4:0]occupied,completed;GSR global_reset(.GSRI(1'b1));gpu_v2_coordinate_store dut(.*);initial begin #1000000;$fatal(1,\"watchdog\");end initial begin #1;clk=1;#1;clk=0;reset=0;#1;\n").replace("[111:0]",if compact {"[35:0]"} else {"[111:0]"}),
            writes:[0;2],takes:0,simultaneous:0,held:0,peak:0,peak_ready:0,rejected:0,resets:0}
    }
    fn stored_owner_bits(&self, o: Owner) -> u64 {
        if self.compact {
            canonical_owner_bits(o)
        } else {
            owner_bits(o)
        }
    }
    fn result_bits(&self, v: coordinate::Output) -> u128 {
        if self.compact {
            (((u128::from(v.coordinates[0][2]) * 1024 + u128::from(v.coordinates[0][0])) * 256
                + u128::from(v.fractions[0][1]))
                * 256)
                + u128::from(v.fractions[0][0])
        } else {
            result_bits(v)
        }
    }
    fn edge(&mut self, t: store::Tick) -> store::Edge {
        let old = self.dut.state();
        let ready = self.dut.input_ready();
        let data = self.dut.input();
        let expected = self
            .queue
            .front()
            .and_then(|(o, v)| v.map(|v| input(*o, v)));
        assert_eq!(data, expected);
        assert_eq!(old.count, self.queue.len());
        assert_eq!(
            old.ready,
            self.queue.iter().filter(|(_, r)| r.is_some()).count()
        );
        let reject = !t.reset
            && t.ce
            && ((t.issue.is_some() && self.queue.len() == self.capacity)
                || (t.returned.is_some() && self.queue.iter().all(|(_, r)| r.is_some()))
                || (t.take && expected.is_none()));
        let result = self.dut.tick(t);
        let edge = if reject {
            assert!(result.is_err());
            assert_eq!(old, self.dut.state());
            assert_eq!(data, self.dut.input());
            self.rejected += 1;
            store::Edge::default()
        } else {
            result.unwrap()
        };
        if t.reset {
            self.queue.clear();
            self.resets += 1;
        } else if t.ce && !reject {
            if t.take {
                let (o, v) = self.queue.pop_front().unwrap();
                let v = v.unwrap();
                assert_eq!(
                    edge.consume,
                    Some((old.read, self.stored_owner_bits(o), self.result_bits(v)))
                );
                self.takes += 1;
            }
            if let Some(v) = t.returned {
                let (_, r) = self.queue.iter_mut().find(|(_, r)| r.is_none()).unwrap();
                *r = Some(v);
                assert_eq!(edge.result_write, Some((old.returned, self.result_bits(v))));
                self.writes[1] += 1;
            }
            if let Some(o) = t.issue {
                assert!(ready);
                self.queue.push_back((o, None));
                assert_eq!(
                    edge.owner_write,
                    Some((old.write, self.stored_owner_bits(o)))
                );
                self.writes[0] += 1;
            }
            self.simultaneous += usize::from(t.issue.is_some() && t.returned.is_some() && t.take);
            if t.take {
                assert!(edge.owner_write.is_none_or(|(r, _)| r != old.read));
                assert!(edge.result_write.is_none_or(|(r, _)| r != old.read));
            }
        } else if !t.ce {
            assert_eq!(old, self.dut.state());
            assert_eq!(data, self.dut.input());
            assert_eq!(edge, store::Edge::default());
            self.held += 1;
        }
        let post = self.dut.state();
        self.peak = self.peak.max(post.count);
        self.peak_ready = self.peak_ready.max(post.ready);
        let w = self.wall;
        writeln!(self.tb,"reset={};ce={};issue={};returned={};take={};owner_data=38'h{:010x};result_data=112'h{:028x};#1;",
            u8::from(t.reset),u8::from(t.ce),u8::from(t.issue.is_some()),u8::from(t.returned.is_some()),u8::from(t.take),t.issue.map_or(0,owner_bits),t.returned.map_or(0,|v|self.result_bits(v))).unwrap();
        writeln!(self.tb,"if(bad!=={} || in_ready!=={} || out_valid!=={} || occupied!==5'd{} || completed!==5'd{} || write_row!==4'd{} || return_row!==4'd{} || read_row!==4'd{}) $fatal(1,\"state wall{w}\");",
            u8::from(reject),u8::from(ready&&!t.reset),u8::from(data.is_some()&&!t.reset),old.count,old.ready,old.write,old.returned,old.read).unwrap();
        writeln!(self.tb,"if(owner_write!=={} || result_write!=={} || consume!=={}) $fatal(1,\"ports wall{w}\");",u8::from(edge.owner_write.is_some()),u8::from(edge.result_write.is_some()),u8::from(edge.consume.is_some())).unwrap();
        if let Some(v) = expected.filter(|_| !t.reset) {
            let o = Owner {
                parents: v.parents,
                nearest: v.nearest,
                levels: v.metadata.levels,
                slot: v.metadata.slot,
                key: v.metadata.key,
                last_fine: v.metadata.last_fine,
            };
            let result = coordinate::Output {
                fractions: v.fractions,
                coordinates: v.metadata.coordinates,
            };
            writeln!(self.tb,"if(out_owner!==38'h{:010x} || out_result!==112'h{:028x}) $fatal(1,\"payload wall{w}\");",owner_bits(o),self.result_bits(result)).unwrap();
            if self.compact {
                writeln!(self.tb,"`ifdef CANONICAL_NATIVE\nif(dut.sram.owner_read!==24'h{:06x}) $fatal(1,\"physical owner24 wall{w}\");\n`else\nif(dut.ff.owners[dut.cp]!==24'h{:06x}) $fatal(1,\"physical owner24 wall{w}\");\n`endif",canonical_owner_bits(o),canonical_owner_bits(o)).unwrap();
            }
        }
        self.tb.push_str("clk=1;#1;clk=0;#1;\n");
        writeln!(
            self.csv,
            "{w},{},{},{},{},{},{},{},{},{}",
            t.ce,
            t.issue.is_some(),
            t.returned.is_some(),
            t.take,
            old.write,
            old.returned,
            old.read,
            old.count,
            old.ready
        )
        .unwrap();
        self.wall += 1;
        edge
    }
    fn finish(mut self, cap: usize, name: &str) -> std::path::PathBuf {
        assert!(self.queue.is_empty());
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/c-owner-runtime-integration-20261010/coordinate-storage")
            .join(name);
        std::fs::create_dir_all(&dir).unwrap();
        self.tb
            .push_str("$display(\"PASS Coordinate SRAM calendar\");$finish;end endmodule\n");
        std::fs::write(
            dir.join("tb.v"),
            if self.compact {
                self.tb.replace("112\'h", "36\'h")
            } else {
                self.tb
            },
        )
        .unwrap();
        std::fs::write(dir.join("calendar.csv"), self.csv).unwrap();
        for native in [false, true] {
            std::fs::write(
                dir.join(format!("controller-{native}.v")),
                if self.compact {
                    rtl::coordinate_store::canonical_source_trusted(cap, native).unwrap()
                } else {
                    rtl::coordinate_store::source(cap, native).unwrap()
                },
            )
            .unwrap();
        }
        std::fs::write(dir.join("receipt.json"),format!("{{\"edges\":{},\"owner_writes\":{},\"result_writes\":{},\"consumes\":{},\"simultaneous_all_three\":{},\"CE_off_edges\":{},\"peak_occupied\":{},\"peak_ready\":{},\"rejected\":{},\"resets\":{},\"capacity\":{cap}}}\n",self.wall,self.writes[0],self.writes[1],self.takes,self.simultaneous,self.held,self.peak,self.peak_ready,self.rejected,self.resets)).unwrap();
        dir
    }
}
fn runtime(cap: usize, paused: bool) -> std::path::PathBuf {
    let (slots, bytes) = q::fixture();
    let mut r = q::runtime(
        &slots,
        super::super::control::Hardware {
            coordinate_credits: cap,
            ..Default::default()
        },
    );
    let mut mc = q::physical::Physical::new(4096, bytes, true, paused);
    let mut a = Audit::new(cap, true);
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
    let mut accepted = 0;
    let mut rng = 0x94af231du32;
    for wall in 0..40_000 {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        let step = r
            .step(
                &mut mc,
                inputs.get(accepted),
                timed::Control {
                    ce: !paused || rng & 7 > 1,
                    result_ready: !paused || wall > 1100 && rng % 13 > 2,
                },
            )
            .unwrap();
        accepted += usize::from(step.accepted);
        let t = &r.preparation.trace;
        assert_eq!(a.edge(t.coordinate_store_tick), t.coordinate_store_edge);
        got.extend(step.results);
        if accepted == inputs.len() && r.idle() {
            break;
        }
    }
    assert!(r.idle());
    assert_eq!(got, expected);
    a.finish(cap, &format!("runtime-C{cap}-paused{paused}"))
}
fn directed(cap: usize) -> std::path::PathBuf {
    let mut a = Audit::new(cap, false);
    let mut issue = 0;
    let mut returned = 0;
    let mut consumed = 0;
    for wall in 0..4000 {
        let old = a.dut.state();
        let ce = wall % 17 != 3;
        let t = store::Tick {
            reset: false,
            ce,
            issue: (issue < 128 && a.dut.input_ready()).then(|| fixture(issue).0),
            returned: (returned < issue && old.count > old.ready).then(|| fixture(returned).1),
            take: wall > 80 && old.ready > 0,
        };
        if ce {
            issue += usize::from(t.issue.is_some());
            returned += usize::from(t.returned.is_some());
            consumed += usize::from(t.take);
        }
        a.edge(t);
        if consumed == 128 {
            break;
        }
    }
    assert_eq!((issue, returned, consumed), (128, 128, 128));
    assert_eq!(a.peak, cap);
    assert_eq!(a.peak_ready, cap);
    if cap >= 3 {
        assert!(a.simultaneous > 0);
    }
    // Refuse borrowing this edge's issue/return/pop. Reset must invalidate
    // outstanding payload even with CE=0, and permit different data on reuse.
    let (o, v) = fixture(135);
    a.edge(store::Tick {
        ce: true,
        issue: Some(o),
        returned: Some(v),
        ..Default::default()
    });
    a.edge(store::Tick {
        ce: true,
        take: true,
        ..Default::default()
    });
    a.edge(store::Tick {
        ce: true,
        issue: Some(o),
        ..Default::default()
    });
    a.edge(store::Tick {
        ce: true,
        returned: Some(v),
        take: true,
        ..Default::default()
    });
    a.edge(store::Tick {
        ce: true,
        returned: Some(v),
        ..Default::default()
    });
    for i in 1..cap {
        let (o, v) = fixture(135 + i);
        a.edge(store::Tick {
            ce: true,
            issue: Some(o),
            ..Default::default()
        });
        a.edge(store::Tick {
            ce: true,
            returned: Some(v),
            ..Default::default()
        });
    }
    a.edge(store::Tick {
        ce: true,
        issue: Some(o),
        take: true,
        ..Default::default()
    });
    a.edge(store::Tick {
        ce: false,
        issue: Some(o),
        returned: Some(v),
        take: true,
        ..Default::default()
    });
    a.edge(store::Tick {
        reset: true,
        ce: false,
        issue: Some(o),
        returned: Some(v),
        take: true,
    });
    let (o, v) = fixture(211);
    a.edge(store::Tick {
        ce: true,
        issue: Some(o),
        ..Default::default()
    });
    a.edge(store::Tick {
        ce: true,
        returned: Some(v),
        ..Default::default()
    });
    a.edge(store::Tick {
        ce: true,
        take: true,
        ..Default::default()
    });
    a.edge(store::Tick {
        ce: true,
        ..Default::default()
    });
    assert_eq!(a.rejected, 4);
    assert_eq!(a.resets, 1);
    a.finish(cap, &format!("directed-C{cap}"))
}
#[test]
fn coordinate_storage_old_credit_and_split_fields() {
    for cap in [1, 2, 6, 16] {
        directed(cap);
    }
    let (o, v) = fixture(1);
    let mut s = store::Store::new(1).unwrap();
    s.tick(store::Tick {
        ce: true,
        issue: Some(o),
        ..Default::default()
    })
    .unwrap();
    assert!(s
        .tick(store::Tick {
            ce: true,
            returned: Some(v),
            take: true,
            ..Default::default()
        })
        .is_err());
    s.tick(store::Tick {
        ce: true,
        returned: Some(v),
        ..Default::default()
    })
    .unwrap();
    assert!(s
        .tick(store::Tick {
            ce: true,
            issue: Some(o),
            take: true,
            ..Default::default()
        })
        .is_err());
    assert_eq!(s.input(), Some(input(o, v)));
    s.tick(store::Tick {
        reset: true,
        ..Default::default()
    })
    .unwrap();
    assert_eq!(s.state(), store::State::default());
    assert!(s.input().is_none());
}
#[test]
fn coordinate_storage_runtime_matches_independent_numerics() {
    for (cap, paused) in [(1, true), (2, true), (6, false), (6, true), (16, true)] {
        runtime(cap, paused);
    }
}
fn bounded(mut command: Command, dir: &Path, name: &str) {
    command.stdout(Stdio::from(
        std::fs::File::create(dir.join(format!("{name}.out"))).unwrap(),
    ));
    command.stderr(Stdio::from(
        std::fs::File::create(dir.join(format!("{name}.err"))).unwrap(),
    ));
    let mut child = command.spawn().unwrap();
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(
                status.success(),
                "{name}: {} {}",
                std::fs::read_to_string(dir.join(format!("{name}.out"))).unwrap(),
                std::fs::read_to_string(dir.join(format!("{name}.err"))).unwrap()
            );
            break;
        }
        if start.elapsed() > Duration::from_secs(60) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("{name} watchdog");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
#[test]
#[ignore = "requires Icarus and Gowin RAM16SDP4 primitives"]
fn coordinate_storage_native_and_ff_replay_actual_calendar() {
    let dirs = [(1, true), (2, true), (6, false), (6, true), (16, true)]
        .map(|(c, p)| runtime(c, p))
        .into_iter()
        .chain([1, 2, 6, 16].map(directed));
    for dir in dirs {
        for native in [false, true] {
            let mut c = Command::new(std::env::var_os("IVERILOG_EXE").unwrap());
            if native {
                c.arg("-DCANONICAL_NATIVE");
            }
            c.current_dir(&dir)
                .args(["-g2012", "-s", "tb", "-o"])
                .arg(format!("test-{native}.vvp"))
                .arg(format!("controller-{native}.v"))
                .arg("tb.v")
                .arg(
                    Path::new(&std::env::var_os("GOWIN_HOME").unwrap())
                        .join("IDE/simlib/gw2a/prim_sim.v"),
                );
            bounded(c, &dir, &format!("compile-{native}"));
            let mut r = Command::new(std::env::var_os("VVP_EXE").unwrap());
            r.current_dir(&dir).arg(format!("test-{native}.vvp"));
            bounded(r, &dir, &format!("run-{native}"));
        }
    }
}
