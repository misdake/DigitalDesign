//! Color result RAM: independent integer semantics, owned rows and native ports.
use super::{qualification as q, *};
use crate::texture::{
    emu::color::{self as c, ColorEmu, Input, Output, Tick},
    ports::{Filter, Group4, TileKey},
    rtl,
};
use std::{
    collections::VecDeque,
    fmt::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
fn word(o: Output) -> u32 {
    u32::from(o.key) * 0x1000000
        + u32::from(o.rgb[2]) * 65536
        + u32::from(o.rgb[1]) * 256
        + u32::from(o.rgb[0])
}
fn fixture(i: usize) -> Input {
    let weights = if i & 1 == 0 {
        [200, 120, 120, 71]
    } else {
        [0, 511, 0, 0]
    };
    Input {
        payload: Group4 {
            key: TileKey {
                slot: 0,
                n: 5,
                x: 0,
                y: 0,
            },
            top_left_local: [0, 0],
            coefficients: weights,
            first: true,
            last: true,
            quad_id: (i % 16) as u8,
            lane: ((i / 16) % 4) as u8,
        }
        .pack72()
        .unwrap() as i128,
        texels: std::array::from_fn(|t| ((i * 1049 + t * 8191 + 19) % 65536) as u16),
    }
}
struct Audit {
    sum: [u32; 3],
    future: VecDeque<(u64, u32)>,
    rows: VecDeque<(u8, u32)>,
    head: Option<u32>,
    write: u8,
    pop: u8,
    enabled: u64,
    wall: usize,
    previous_write: bool,
    tb: String,
    csv: String,
    writes: usize,
    reads: usize,
    consumes: usize,
    write_visible_ce0: usize,
    both: usize,
    full_credit_push_pop: usize,
    isolated_replace: usize,
    peak: usize,
    resets: usize,
    faults: usize,
    commits: Vec<usize>,
}
impl Audit {
    fn new() -> Self {
        Self{sum:[0;3],future:VecDeque::new(),rows:VecDeque::new(),head:None,write:0,pop:0,enabled:0,wall:0,previous_write:false,
 tb:String::from("module tb;reg clk=0,reset=1,ce=0,in_valid=0,out_ready=0;reg[71:0]in_payload=0;reg[15:0]in_tex0=0,in_tex1=0,in_tex2=0,in_tex3=0;wire in_ready,out_valid,fault;wire[5:0]out_key;wire[7:0]out_r,out_g,out_b;`ifdef GPU_V2_GOWIN_DSP\nGSR GSR(.GSRI(1'b1));\n`endif\ngpu_v2_texture_color dut(.*);initial begin #2000000;$fatal(1,\"watchdog\");end initial begin #1;clk=1;#1;clk=0;reset=0;#1;\n"),
 csv:String::from("wall,CE,old_count,old_credits,old_head_valid,write_address,write_data,read_address,read_data,pop,write_visible_CE0\n"),writes:0,reads:0,consumes:0,write_visible_ce0:0,both:0,full_credit_push_pop:0,isolated_replace:0,peak:0,resets:0,faults:0,commits:vec![]}
    }
    fn stimulus(&mut self, t: Tick) {
        writeln!(
            self.tb,
            "reset=0;ce={};in_valid={};out_ready={};in_payload=72'h{:018x};",
            u8::from(t.ce),
            u8::from(t.input.is_some()),
            u8::from(t.output_ready),
            t.input.map_or(0, |i| i.payload)
        )
        .unwrap();
        for i in 0..4 {
            writeln!(
                self.tb,
                "in_tex{i}=16'h{:04x};",
                t.input.map_or(0, |p| p.texels[i])
            )
            .unwrap();
        }
        self.tb.push_str("#1;\n");
    }
    fn accepted(&mut self, t: Input) {
        if t.payload >> 64 & 1 != 0 {
            self.sum = [0; 3];
        }
        for tap in 0..4 {
            let w = ((t.payload >> (28 + tap * 9)) & 511) as u32;
            let v = u32::from(t.texels[tap]);
            let raw = [v / 2048, (v / 32) % 64, v % 32];
            let rgb = [
                raw[0] * 8 + raw[0] / 4,
                raw[1] * 4 + raw[1] / 16,
                raw[2] * 8 + raw[2] / 4,
            ];
            for (c, x) in rgb.into_iter().enumerate() {
                self.sum[c] += w * x;
            }
        }
        if t.payload >> 65 & 1 != 0 {
            let key = (((t.payload >> 66) & 15) * 4 + ((t.payload >> 70) & 3)) as u8;
            let out = Output {
                key,
                rgb: self.sum.map(|v| ((v + 255) / 511) as u8),
            };
            self.future.push_back((self.enabled, word(out)));
        }
    }
    fn edge(&mut self, t: Tick, before: c::Snapshot, s: &c::Step) {
        let w = self.wall;
        assert_eq!(s.output.map(word), self.head);
        assert_eq!(before.queued, self.rows.len());
        assert_eq!(before.result_pointers, [self.pop, self.write]);
        assert_eq!(before.result_head_valid, self.head.is_some());
        if s.accepted {
            self.accepted(t.input.unwrap());
        }
        let values: Vec<_> = s
            .events
            .iter()
            .filter_map(|e| {
                if let c::Event::Commit(o) = e {
                    Some(word(*o))
                } else {
                    None
                }
            })
            .collect();
        let push = s.events.iter().find_map(|e| {
            if let c::Event::Queued(o) = e {
                Some(word(*o))
            } else {
                None
            }
        });
        let pop = t.ce && t.output_ready && self.head.is_some();
        assert_eq!(values.len(), usize::from(pop));
        if pop {
            assert_eq!(values[0], self.rows.front().unwrap().1);
            self.consumes += 1;
            self.commits.push(w);
        }
        if let Some(raw) = push {
            let (age, expected) = self.future.pop_front().unwrap();
            assert_eq!(self.enabled - age, 2);
            assert_eq!(raw, expected);
        }
        let ports = s.result_access;
        assert_eq!(ports.write, push.map(|raw| (self.write, raw)));
        assert_eq!(ports.read, self.rows.front().copied());
        if let (Some((ra, _)), Some((wa, _))) = (ports.read, ports.write) {
            assert_ne!(ra, wa, "physical RAM collision");
        }
        let write_ce0 = self.previous_write && !t.ce;
        if write_ce0 {
            assert_eq!(s.output.map(word), self.head);
            assert!(s.output.is_some());
            self.write_visible_ce0 += 1;
        }
        self.previous_write = ports.write.is_some();
        self.both += usize::from(pop && push.is_some());
        self.full_credit_push_pop +=
            usize::from(before.result_credits == 16 && pop && push.is_some());
        self.isolated_replace += usize::from(before.queued == 1 && pop && push.is_some());
        if before.result_credits == 16 && t.input.is_some_and(|p| p.payload >> 65 & 1 != 0) {
            assert!(!s.accepted);
        }
        if pop {
            self.rows.pop_front();
            self.pop = (self.pop + 1) & 15;
            self.head = None;
        }
        if ports.read.is_some() {
            self.reads += 1;
        }
        if let Some((addr, raw)) = ports.write {
            self.rows.push_back((addr, raw));
            self.write = (self.write + 1) & 15;
            self.writes += 1;
        }
        self.head = self.rows.front().map(|row| row.1);
        assert_eq!(s.snapshot.queued, self.rows.len());
        assert_eq!(s.snapshot.result_pointers, [self.pop, self.write]);
        assert_eq!(s.snapshot.result_head_valid, self.head.is_some());
        assert_eq!(
            s.snapshot.result_credits,
            self.rows.len() + self.future.len()
        );
        self.peak = self.peak.max(self.rows.len());
        if !t.ce {
            assert!(ports.write.is_none());
            assert_eq!(
                s.snapshot,
                c::Snapshot {
                    wall: before.wall + 1,
                    ..before
                }
            );
        }
        self.stimulus(t);
        writeln!(self.tb,"if(fault!==0 || in_ready!=={} || out_valid!=={} || dut.f_count!==5'd{} || dut.credits!==5'd{} || dut.result_write!=={})$fatal(1,\"state wall{w}\");",u8::from(s.input_ready),u8::from(s.output.is_some()),before.queued,before.result_credits,u8::from(ports.write.is_some())).unwrap();
        if let Some(o) = s.output {
            writeln!(self.tb,"if({{out_key,out_b,out_g,out_r}}!==30'h{:08x} || dut.result_do[31:30]!==0)$fatal(1,\"old asynchronous head wall{w}\");",word(o)).unwrap();
        }
        if let Some((addr, raw)) = ports.write {
            writeln!(self.tb,"if(dut.f_tail!==4'd{addr} || dut.result_write_data!==32'h{raw:08x})$fatal(1,\"write wall{w}\");").unwrap();
        }
        if let Some((addr, _)) = ports.read {
            writeln!(
                self.tb,
                "if(dut.result_read_address!==4'd{addr})$fatal(1,\"read address wall{w}\");"
            )
            .unwrap();
        }
        self.tb.push_str("`ifdef GPU_V2_GOWIN_DSP\n");
        for bank in 0..8 {
            writeln!(self.tb,"if(dut.result_bank[{bank}].ram.WRE!==dut.result_write || dut.result_bank[{bank}].ram.WAD!==dut.f_tail || dut.result_bank[{bank}].ram.RAD!==dut.f_head || dut.result_bank[{bank}].ram.DI!==dut.result_write_data[{}+:4])$fatal(1,\"native bank{bank} wall{w}\");",bank*4).unwrap();
        }
        self.tb.push_str("`endif\n");
        self.tb.push_str("clk=1;#1;clk=0;#1;\n");
        writeln!(self.tb,"if(dut.f_count!==5'd{} || dut.credits!==5'd{} || out_valid!=={} || dut.f_head!==4'd{} || dut.f_tail!==4'd{})$fatal(1,\"post wall{w}\");",s.snapshot.queued,s.snapshot.result_credits,u8::from(s.snapshot.result_head_valid),self.pop,self.write).unwrap();
        writeln!(
            self.csv,
            "{w},{},{},{},{},{},{},{},{},{},{}",
            t.ce,
            before.queued,
            before.result_credits,
            before.result_head_valid,
            ports.write.map_or(99, |x| x.0),
            ports.write.map_or(0, |x| x.1),
            ports.read.map_or(99, |x| x.0),
            ports.read.map_or(0, |x| x.1),
            pop,
            write_ce0
        )
        .unwrap();
        self.enabled += u64::from(t.ce);
        self.wall += 1;
    }
    fn drive(&mut self, e: &mut ColorEmu, t: Tick) -> c::Step {
        let before = e.snapshot();
        let s = e.tick(t).unwrap();
        self.edge(t, before, &s);
        s
    }
    fn reset(&mut self, e: &mut ColorEmu) {
        e.reset();
        self.tb.push_str("reset=1;ce=0;in_valid=1;out_ready=1;#1;if(dut.result_write!==0)$fatal(1,\"reset write port\");clk=1;#1;clk=0;#1;reset=0;in_valid=0;#1;if(out_valid!==0||fault!==0||dut.f_count!==0||dut.credits!==0)$fatal(1,\"reset stale ownership\");\n");
        self.sum = [0; 3];
        self.future.clear();
        self.rows.clear();
        self.head = None;
        self.write = 0;
        self.pop = 0;
        self.enabled = 0;
        self.previous_write = false;
        self.resets += 1;
    }
    fn fault(&mut self, e: &mut ColorEmu, mut bad: Input) {
        bad.payload = (bad.payload & !(15 << 4)) | (15 << 4);
        let t = Tick {
            ce: true,
            input: Some(bad),
            output_ready: true,
        };
        let before = e.snapshot();
        assert!(e.tick(t).is_err());
        assert!(e.faulted());
        assert_eq!(e.snapshot().queued, before.queued);
        assert_eq!(e.snapshot().result_pointers, before.result_pointers);
        assert_eq!(e.snapshot().result_credits, before.result_credits);
        self.stimulus(t);
        self.tb.push_str("if(dut.packet_bad!==1 || dut.do_push!==1 || dut.do_pop!==1 || dut.f_count<2 || dut.result_write!==0)$fatal(1,\"fault must suppress write and ownership transfer\");clk=1;#1;clk=0;#1;if(fault!==1 || out_valid!==0 || dut.result_write!==0)$fatal(1,\"terminal fault\");\n");
        writeln!(self.tb,"if(dut.f_count!==5'd{} || dut.credits!==5'd{} || dut.f_head!==4'd{} || dut.f_tail!==4'd{})$fatal(1,\"fault changed ownership\");",before.queued,before.result_credits,before.result_pointers[0],before.result_pointers[1]).unwrap();
        assert!(e
            .tick(Tick {
                ce: false,
                input: None,
                output_ready: true
            })
            .is_err());
        self.tb.push_str("ce=0;#1;clk=1;#1;clk=0;#1;if(fault!==1 || dut.result_write!==0)$fatal(1,\"fault held\");\n");
        self.faults += 1;
    }
    fn finish(mut self, name: &str) -> PathBuf {
        assert!(self.rows.is_empty() && self.future.is_empty() && self.head.is_none());
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/sampling-color-result-storage-20261008/ssram-production")
            .join(name);
        std::fs::create_dir_all(&dir).unwrap();
        self.tb
            .push_str("$display(\"PASS Color result RAM\");$finish;end endmodule\n");
        std::fs::write(dir.join("tb.v"), self.tb).unwrap();
        std::fs::write(dir.join("calendar.csv"), self.csv).unwrap();
        std::fs::write(dir.join("color.v"), rtl::color::verilog()).unwrap();
        std::fs::write(dir.join("receipt.json"),format!("{{\"edges\":{},\"writes\":{},\"reads\":{},\"consumes\":{},\"write_visible_CE0\":{},\"push_pop\":{},\"old_full_credit_push_pop\":{},\"isolated_pop_push\":{},\"peak_stored\":{},\"CE0_resets\":{},\"faults\":{}}}\n",self.wall,self.writes,self.reads,self.consumes,self.write_visible_ce0,self.both,self.full_credit_push_pop,self.isolated_replace,self.peak,self.resets,self.faults)).unwrap();
        dir
    }
}
fn drain(a: &mut Audit, e: &mut ColorEmu) {
    for _ in 0..128 {
        if e.idle() {
            return;
        }
        a.drive(
            e,
            Tick {
                ce: true,
                input: None,
                output_ready: true,
            },
        );
    }
    panic!("drain watchdog");
}
fn directed() -> PathBuf {
    let mut a = Audit::new();
    let mut e = ColorEmu::new(20_000).unwrap();
    // Single first fill: write age2, public asynchronous head age3, even on CE0.
    for i in 0..3 {
        a.drive(
            &mut e,
            Tick {
                ce: true,
                input: (i == 0).then(|| fixture(1)),
                output_ready: true,
            },
        );
    }
    let s = a.drive(
        &mut e,
        Tick {
            ce: false,
            input: None,
            output_ready: true,
        },
    );
    assert!(s.output.is_some());
    drain(&mut a, &mut e);
    // One stored row consumed on the same edge as the next write: no unowned bypass.
    a.reset(&mut e);
    let start = a.wall;
    let n = a.commits.len();
    for i in 0..7 {
        a.drive(
            &mut e,
            Tick {
                ce: true,
                input: match i {
                    0 => Some(fixture(10)),
                    1 => Some(fixture(11)),
                    _ => None,
                },
                output_ready: true,
            },
        );
    }
    assert!(e.idle());
    assert_eq!(&a.commits[n..], &[start + 3, start + 4]);
    assert!(a.isolated_replace > 0);
    // Continuous standalone last groups sustain every-edge admission and output.
    a.reset(&mut e);
    let n = a.commits.len();
    let start = a.wall;
    for i in 0..128 {
        assert!(
            a.drive(
                &mut e,
                Tick {
                    ce: true,
                    input: Some(fixture(i + 100)),
                    output_ready: true
                }
            )
            .accepted
        );
    }
    drain(&mut a, &mut e);
    assert_eq!(a.commits.len() - n, 128);
    assert_eq!(a.commits[n], start + 3);
    assert!(a.commits[n..].windows(2).all(|v| v[1] - v[0] == 1));
    // Sixteen credits include the two pipeline owners; simultaneous push/pop on old-full credits.
    for i in 0..16 {
        assert!(
            a.drive(
                &mut e,
                Tick {
                    ce: true,
                    input: Some(fixture(i + 300)),
                    output_ready: false
                }
            )
            .accepted
        );
    }
    assert_eq!(e.snapshot().result_credits, 16);
    let blocked = fixture(400);
    let s = a.drive(
        &mut e,
        Tick {
            ce: true,
            input: Some(blocked),
            output_ready: true,
        },
    );
    assert!(!s.accepted);
    assert!(
        a.drive(
            &mut e,
            Tick {
                ce: true,
                input: Some(blocked),
                output_ready: true
            }
        )
        .accepted
    );
    drain(&mut a, &mut e);
    assert!(a.full_credit_push_pop > 0);
    // Truly fill all16 stored rows, hold CE0, reset same instance and reuse different data.
    for i in 0..16 {
        a.drive(
            &mut e,
            Tick {
                ce: true,
                input: Some(fixture(i + 500)),
                output_ready: false,
            },
        );
    }
    for _ in 0..4 {
        a.drive(
            &mut e,
            Tick {
                ce: true,
                input: None,
                output_ready: false,
            },
        );
    }
    assert_eq!(e.snapshot().queued, 16);
    for _ in 0..3 {
        a.drive(
            &mut e,
            Tick {
                ce: false,
                input: Some(fixture(600)),
                output_ready: true,
            },
        );
    }
    a.reset(&mut e);
    a.drive(
        &mut e,
        Tick {
            ce: true,
            input: Some(fixture(777)),
            output_ready: true,
        },
    );
    drain(&mut a, &mut e);
    // Fault would otherwise pop and write an older result. Neither may commit.
    for i in 0..4 {
        a.drive(
            &mut e,
            Tick {
                ce: true,
                input: Some(fixture(i + 800)),
                output_ready: false,
            },
        );
    }
    a.fault(&mut e, fixture(900));
    a.reset(&mut e);
    a.drive(
        &mut e,
        Tick {
            ce: true,
            input: Some(fixture(991)),
            output_ready: true,
        },
    );
    drain(&mut a, &mut e);
    assert!(a.write_visible_ce0 > 0 && a.peak == 16 && a.faults == 1);
    a.finish("directed")
}
fn runtime(paused: bool) -> PathBuf {
    let (slots, bytes) = q::fixture();
    let mut r = q::runtime(&slots, super::super::control::Hardware::default());
    let mut mc = q::physical::Physical::new(4096, bytes, true, paused);
    let mut a = Audit::new();
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
    let mut sent = 0;
    let mut got = vec![];
    let mut rng = 0x168fafedu32;
    for wall in 0..40_000 {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        let control = timed::Control {
            ce: !paused || rng & 7 > 1,
            result_ready: !paused || wall > 1300 && rng % 13 > 2,
        };
        let before = r.color.snapshot();
        let t = Tick {
            ce: control.ce,
            input: r.color_input,
            output_ready: control.result_ready,
        };
        let s = r.step(&mut mc, inputs.get(sent), control).unwrap();
        sent += usize::from(s.accepted);
        got.extend(s.results);
        a.edge(t, before, &s.color);
        if sent == inputs.len() && r.idle() {
            break;
        }
    }
    assert!(r.idle());
    assert_eq!(got, expected);
    if paused {
        assert!(a.write_visible_ce0 > 0 && a.peak == 16);
    }
    a.finish(&format!("runtime-paused{paused}"))
}
#[test]
fn color_storage_semantics_lifetimes_and_runtime_ports() {
    directed();
    runtime(false);
    runtime(true);
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
#[ignore = "requires Gowin and Icarus"]
fn color_storage_portable_native_replay() {
    for dir in [directed(), runtime(false), runtime(true)] {
        for native in [false, true] {
            let mut cmd = Command::new(std::env::var_os("IVERILOG_EXE").unwrap());
            cmd.current_dir(&dir)
                .args(["-g2012", "-s", "tb", "-o"])
                .arg(format!("test-{native}.vvp"));
            if native {
                cmd.arg("-DGPU_V2_GOWIN_DSP").arg(
                    Path::new(&std::env::var_os("GOWIN_HOME").unwrap())
                        .join("IDE/simlib/gw2a/prim_sim.v"),
                );
            }
            cmd.args(["color.v", "tb.v"]);
            bounded(cmd, &dir, &format!("compile-{native}"));
            let mut cmd = Command::new(std::env::var_os("VVP_EXE").unwrap());
            cmd.current_dir(&dir).arg(format!("test-{native}.vvp"));
            bounded(cmd, &dir, &format!("run-{native}"));
        }
    }
}
