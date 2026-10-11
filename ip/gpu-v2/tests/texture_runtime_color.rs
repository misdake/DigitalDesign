//! Closed Color/public-owner qualification. Numerical sums use ordinary integer
//! products/division; the independent diagnostic owner retains remaining masks.
use gpu_v2::texture::{
    emu::color::{ColorEmu, Input, Output, Tick},
    ports::{Filter, Group4, QuadInput, Slot, TileKey},
    rtl::color,
    sim::counted,
};
#[path = "../src/texture/rtl/runtime_color.rs"]
mod runtime_color;
use std::{
    collections::VecDeque,
    fmt::Write,
    fs::{self, File},
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
const BOUND: usize = 20000;
const DIR: &str = "../../target/runtime-color-public-20261010";

#[derive(Clone, Copy)]
struct Offer {
    reset: bool,
    ce: bool,
    allow: bool,
    check: u8,
    publish: Option<(u8, u8)>,
    input: Option<Input>,
    ready: bool,
}
impl Default for Offer {
    fn default() -> Self {
        Self {
            reset: false,
            ce: true,
            allow: true,
            check: 0,
            publish: None,
            input: None,
            ready: true,
        }
    }
}
#[derive(Clone, Copy)]
struct Expected {
    free: bool,
    publish_ready: bool,
    publish_fire: bool,
    input_ready: bool,
    input_fire: bool,
    output: Option<Output>,
    last: bool,
    pop: bool,
    eligible: bool,
    advance: bool,
    bad: bool,
    fault: bool,
    live: u16,
    last_lane: [u8; 16],
}
#[derive(Default)]
struct Golden {
    d: Option<Input>,
    p: Option<Input>,
    accumulator: Option<(u8, [u32; 3])>,
    stream: Option<u8>,
    queue: VecDeque<Output>,
    credits: usize,
    live: u16,
    last: [u8; 16],
    terminal: bool,
}
fn key(p: Input) -> u8 {
    ((p.payload >> 66 & 15) * 4 + (p.payload >> 70 & 3)) as u8
}
fn first(p: Input) -> bool {
    p.payload >> 64 & 1 != 0
}
fn last(p: Input) -> bool {
    p.payload >> 65 & 1 != 0
}
fn contribution(p: Input) -> [u32; 3] {
    let mut sum = [0; 3];
    for tap in 0..4 {
        let word = u32::from(p.texels[tap]);
        let rgb = [
            ((word >> 11) & 31) * 8 + ((word >> 13) & 7),
            ((word >> 5) & 63) * 4 + ((word >> 9) & 3),
            (word & 31) * 8 + ((word >> 2) & 7),
        ];
        let w = (p.payload >> (28 + 9 * tap) & 511) as u32;
        for c in 0..3 {
            sum[c] += rgb[c] * w;
        }
    }
    sum
}
impl Golden {
    fn step(&mut self, t: Offer) -> Expected {
        let attempted = t.ce && !t.reset && !self.terminal;
        let raw_ready = attempted && (t.input.is_some_and(|p| !last(p)) || self.credits < 16);
        let incoming = t.input.filter(|_| raw_ready);
        let old_output = self.queue.front().copied();
        let (pub_id, pub_mask) = t.publish.unwrap_or((0, 1));
        let pub_ready =
            !t.reset && !self.terminal && pub_mask != 0 && self.live & (1 << pub_id) == 0;
        let pub_bad = t.publish.is_some() && !pub_ready;
        let input_bad = incoming.is_some_and(|p| self.live & (1 << (key(p) / 4)) == 0);
        let stream_bad = incoming.is_some_and(|p| {
            if first(p) {
                self.stream.is_some()
            } else {
                self.stream != Some(key(p))
            }
        });
        let header_bad = incoming.is_some_and(|p| p.payload >> 4 & 15 > 10);
        let mut feedback_bad = false;
        let mut sum = None;
        if let Some(p) = self.p {
            feedback_bad = if first(p) {
                self.accumulator.is_some()
            } else {
                self.accumulator.is_none_or(|(k, _)| k != key(p))
            };
            let mut v = contribution(p);
            if !first(p) {
                if let Some((_, prior)) = self.accumulator {
                    for c in 0..3 {
                        v[c] += prior[c];
                    }
                }
            }
            sum = Some(v);
        }
        let sum_bad = sum.is_some_and(|v| v.into_iter().any(|c| c > 255 * 511));
        let pop = old_output.is_some() && t.ready;
        let output_bad = pop && old_output.is_some_and(|o| self.live & (1 << (o.key / 4)) == 0);
        let push = self.p.is_some_and(last);
        let overflow = push && self.queue.len() - usize::from(pop) == 16;
        let credit_bad = self.credits
            != self.queue.len() + usize::from(self.d.is_some_and(last)) + usize::from(push);
        let bad = attempted
            && (pub_bad
                || input_bad
                || stream_bad
                || header_bad
                || feedback_bad
                || sum_bad
                || output_bad
                || overflow
                || credit_bad);
        let fault = self.terminal;
        let eligible = attempted && !bad;
        let advance = eligible && t.allow;
        let output = old_output.filter(|_| !t.reset && !fault);
        let quad_last = output.is_some_and(|o| {
            self.live & (1 << (o.key / 4)) != 0 && self.last[usize::from(o.key / 4)] == o.key % 4
        });
        let e = Expected {
            free: !t.reset && !self.terminal && self.live & (1 << t.check) == 0,
            publish_ready: pub_ready,
            publish_fire: advance && t.publish.is_some() && pub_ready,
            input_ready: raw_ready && !fault,
            input_fire: advance && incoming.is_some(),
            output,
            last: quad_last,
            pop: advance && output.is_some() && t.ready,
            eligible,
            advance,
            bad,
            fault,
            live: self.live,
            last_lane: self.last,
        };
        if t.reset {
            *self = Self::default();
        } else {
            self.terminal |= bad;
            if advance {
                if pop {
                    self.queue.pop_front();
                    if quad_last {
                        self.live &= !(1 << (old_output.unwrap().key / 4));
                    }
                }
                if let Some((id, mask)) = t.publish {
                    self.live |= 1 << id;
                    self.last[usize::from(id)] = 7 - mask.leading_zeros() as u8;
                }
                if let Some(p) = self.p {
                    let value = sum.unwrap();
                    self.accumulator = (!last(p)).then_some((key(p), value));
                    if last(p) {
                        self.queue.push_back(Output {
                            key: key(p),
                            rgb: value.map(|c| ((c + 255) / 511) as u8),
                        });
                    }
                }
                self.credits += usize::from(incoming.is_some_and(last));
                self.credits -= usize::from(pop);
                self.p = self.d;
                self.d = incoming;
                if let Some(p) = incoming {
                    self.stream = (!last(p)).then_some(key(p));
                }
            }
        }
        e
    }
    fn idle(&self) -> bool {
        self.d.is_none() && self.p.is_none() && self.queue.is_empty() && self.live == 0
    }
}
#[derive(Default)]
struct Metrics {
    edges: usize,
    publications: usize,
    inputs: usize,
    results: usize,
    finals: usize,
    ce0: usize,
    denied: usize,
    stalls: usize,
    full: usize,
    resets: usize,
    faults: usize,
    public_peak: u32,
}
struct Replay {
    model: Golden,
    color: ColorEmu,
    remaining: [u8; 16],
    unpassed: [usize; 16],
    edges: Vec<(Offer, Expected)>,
    metrics: Metrics,
}
impl Replay {
    fn new() -> Self {
        Self {
            model: Golden::default(),
            color: ColorEmu::new(BOUND as u64).unwrap(),
            remaining: [0; 16],
            unpassed: [0; 16],
            edges: vec![],
            metrics: Metrics::default(),
        }
    }
    fn edge(&mut self, t: Offer) -> Expected {
        assert!(self.edges.len() < BOUND);
        let old_output = self.model.queue.front().copied();
        let e = self.model.step(t);
        if t.reset {
            self.color.reset();
            self.remaining.fill(0);
            self.unpassed.fill(0);
        } else {
            let c = self
                .color
                .tick(Tick {
                    ce: e.advance,
                    input: t.input,
                    output_ready: t.ready,
                })
                .unwrap();
            assert_eq!(
                c.output, old_output,
                "independent numerical/calendar golden"
            );
            assert_eq!(c.accepted, e.input_fire);
            if e.input_fire {
                let id = usize::from(key(t.input.unwrap()) / 4);
                self.unpassed[id] -= 1;
            }
            if e.pop {
                let o = e.output.unwrap();
                let id = usize::from(o.key / 4);
                let lane = o.key % 4;
                assert_ne!(
                    self.remaining[id] & (1 << lane),
                    0,
                    "wide owner duplicate/uncovered result"
                );
                self.remaining[id] &= !(1 << lane);
                assert_eq!(
                    e.last,
                    self.remaining[id] == 0,
                    "minimal owner vs independent remaining mask"
                );
                if e.last {
                    assert_eq!(
                        self.unpassed[id], 0,
                        "last Color pop proves producer groups done"
                    );
                }
            }
            if e.publish_fire {
                let (id, mask) = t.publish.unwrap();
                assert_eq!(self.remaining[usize::from(id)], 0);
                self.remaining[usize::from(id)] = mask;
            }
        }
        let m = &mut self.metrics;
        m.edges += 1;
        m.publications += usize::from(e.publish_fire);
        m.inputs += usize::from(e.input_fire);
        m.results += usize::from(e.pop);
        m.finals += usize::from(e.pop && e.last);
        m.ce0 += usize::from(!t.ce);
        m.denied += usize::from(t.ce && !t.allow && e.eligible);
        m.stalls += usize::from(e.output.is_some() && !t.ready);
        m.full += usize::from(self.model.credits == 16);
        m.resets += usize::from(t.reset);
        m.faults += usize::from(e.bad);
        m.public_peak = m.public_peak.max(e.live.count_ones());
        self.edges.push((t, e));
        e
    }
}
struct Quad {
    id: u8,
    mask: u8,
    groups: Vec<Input>,
}
fn fixture() -> Vec<Quad> {
    let slots = [Slot {
        base_address: 0,
        has_full_mip: true,
        max_size_log2: 5,
        valid: true,
    }];
    (0..96)
        .map(|i| {
            let base = [
                [0.11, 0.13],
                [7.0 / 32.0, 7.0 / 32.0],
                [31.0 / 32.0, 31.0 / 32.0],
            ][i % 3];
            let d = 1.0 / 32.0;
            let q = QuadInput {
                quad_id: (i % 16) as u8,
                mask: (i % 15 + 1) as u8,
                uv: [
                    base,
                    [base[0] + d, base[1]],
                    [base[0], base[1] + d],
                    [base[0] + d, base[1] + d],
                ],
                slot: 0,
                material_size_log2: 5,
                filter: [Filter::Nearest, Filter::Bilinear, Filter::Trilinear][(i / 3) % 3],
                lod_bias: if i % 2 == 0 { 0.5 } else { 0.0 },
                force_coarsest: i % 19 == 0,
            };
            let prep = counted::prepare(&q, &slots).unwrap();
            assert!(prep.groups.windows(2).all(|g| g[0].lane <= g[1].lane));
            let groups = prep
                .groups
                .iter()
                .enumerate()
                .map(|(j, g)| Input {
                    payload: g.pack72().unwrap() as i128,
                    texels: std::array::from_fn(|t| (i * 613 + j * 997 + t * 4193) as u16),
                })
                .collect();
            Quad {
                id: q.quad_id,
                mask: q.mask,
                groups,
            }
        })
        .collect()
}
fn ordinary(r: &mut Replay, quads: &[Quad], pause: bool) {
    let mut published = 0;
    let mut pending = VecDeque::new();
    let start = r.edges.len();
    for wall in 0..12000 {
        let ce = !pause || wall % 13 > 2;
        let allow = !pause || wall % 17 > 3;
        let ready = wall > 600 && (!pause || wall % 11 > 4);
        let next = quads.get(published);
        let pub_offer = next
            .filter(|q| r.model.live & (1 << q.id) == 0)
            .map(|q| (q.id, q.mask));
        let t = Offer {
            ce,
            allow,
            ready,
            publish: pub_offer,
            check: next.map_or(0, |q| q.id),
            input: pending.front().copied(),
            ..Default::default()
        };
        let e = r.edge(t);
        if e.input_fire {
            pending.pop_front();
        }
        if e.publish_fire {
            let q = next.unwrap();
            r.unpassed[usize::from(q.id)] = q.groups.len();
            pending.extend(q.groups.iter().copied());
            published += 1;
        }
        if published == quads.len() && pending.is_empty() && r.model.idle() {
            break;
        }
    }
    assert_eq!(published, quads.len());
    assert!(r.model.idle());
    assert!(r.edges.len() - start < 12000);
}
/// Independent sole-tail parent: row0 consumes old public-free credit, then
/// exactly three continuation beats follow on coalition advancing edges.
/// The descriptor is held privately by this test driver until row3 publishes.
/// Actual UV/Runtime parent integration remains the joint-top qualifier's job.
fn ordinary_rows(r: &mut Replay, quads: &[Quad], pause: bool) {
    let mut published = 0;
    let mut row = 0;
    let mut pending = VecDeque::new();
    let mut beats = 0;
    for wall in 0..12000 {
        let ce = !pause || wall % 13 > 2;
        let allow = !pause || wall % 17 > 3;
        let ready = wall > 600 && (!pause || wall % 11 > 4);
        let next = quads.get(published);
        let t = Offer {
            ce,
            allow,
            ready,
            publish: next.filter(|_| row == 3).map(|q| (q.id, q.mask)),
            check: next.map_or(0, |q| q.id),
            input: pending.front().copied(),
            ..Default::default()
        };
        let e = r.edge(t);
        if t.publish.is_some() {
            assert!(e.publish_ready, "row0 reservation guarantees row3 ready");
        }
        if e.input_fire {
            pending.pop_front();
        }
        if next.is_some() && e.advance && (row != 0 || e.free) {
            beats += 1;
            if row == 3 {
                assert!(e.publish_fire);
                let q = next.unwrap();
                r.unpassed[usize::from(q.id)] = q.groups.len();
                pending.extend(q.groups.iter().copied());
                published += 1;
                row = 0;
            } else {
                row += 1;
            }
        }
        if published == quads.len() && pending.is_empty() && r.model.idle() {
            break;
        }
    }
    assert_eq!(published, quads.len());
    assert_eq!(beats, 4 * quads.len());
    assert!(r.model.idle());
}
fn simple(id: u8, weights: [u32; 4], first: bool, last: bool) -> Input {
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
            first,
            last,
            quad_id: id,
            lane: 0,
        }
        .pack72()
        .unwrap() as i128,
        texels: [0xffff; 4],
    }
}
fn publish(r: &mut Replay, id: u8, mask: u8, groups: usize) {
    assert!(
        r.edge(Offer {
            publish: Some((id, mask)),
            ..Default::default()
        })
        .publish_fire
    );
    r.unpassed[usize::from(id)] = groups;
}
fn reset(r: &mut Replay) {
    r.edge(Offer {
        reset: true,
        ce: false,
        allow: false,
        ..Default::default()
    });
}
fn fault_edges(r: &mut Replay) {
    reset(r);
    let e = r.edge(Offer {
        allow: false,
        publish: Some((0, 0)),
        ..Default::default()
    });
    assert!(e.bad && !e.advance);
    r.edge(Offer::default());
    reset(r);
    let e = r.edge(Offer {
        input: Some(simple(15, [511, 0, 0, 0], true, true)),
        allow: false,
        ..Default::default()
    });
    assert!(e.bad);
    reset(r);
    publish(r, 0, 1, 1);
    let mut p = simple(0, [511, 0, 0, 0], true, true);
    p.payload = (p.payload & !(15 << 4)) | (11 << 4);
    assert!(
        r.edge(Offer {
            input: Some(p),
            allow: false,
            ..Default::default()
        })
        .bad
    );
    reset(r);
    publish(r, 0, 1, 1);
    assert!(
        r.edge(Offer {
            input: Some(simple(0, [511, 0, 0, 0], false, true)),
            ..Default::default()
        })
        .bad
    );
    reset(r);
    publish(r, 0, 1, 2);
    r.edge(Offer {
        input: Some(simple(0, [255, 0, 0, 0], true, false)),
        ..Default::default()
    });
    assert!(
        r.edge(Offer {
            input: Some(simple(0, [256, 0, 0, 0], true, true)),
            allow: false,
            ..Default::default()
        })
        .bad
    );
    reset(r);
    publish(r, 0, 1, 1);
    r.edge(Offer {
        input: Some(simple(0, [511; 4], true, true)),
        ready: false,
        ..Default::default()
    });
    r.edge(Offer {
        ready: false,
        ..Default::default()
    });
    assert!(
        r.edge(Offer {
            allow: false,
            ready: false,
            ..Default::default()
        })
        .bad,
        "sum-domain raw preflight under joined denial"
    );
    reset(r);
    publish(r, 3, 1, 1);
    r.edge(Offer {
        input: Some(simple(3, [511, 0, 0, 0], true, true)),
        ready: false,
        ..Default::default()
    });
    for _ in 0..2 {
        r.edge(Offer {
            ready: false,
            ..Default::default()
        });
    }
    let e = r.edge(Offer {
        check: 3,
        ce: false,
        ..Default::default()
    });
    assert!(e.last && !e.free && !e.pop);
    let e = r.edge(Offer {
        check: 3,
        allow: false,
        ..Default::default()
    });
    assert!(e.last && !e.free && !e.pop && e.eligible);
    let e = r.edge(Offer {
        check: 3,
        publish: Some((3, 1)),
        ..Default::default()
    });
    assert!(
        e.bad && !e.pop && !e.publish_fire && !e.free,
        "same-edge final pop cannot fund alias"
    );
    reset(r);
    // Live reset with decoded/product/result ownership simultaneously present.
    publish(r, 1, 15, 4);
    for lane in 0..4 {
        let mut p = simple(1, [511, 0, 0, 0], true, true);
        p.payload |= i128::from(lane) << 70;
        r.edge(Offer {
            input: Some(p),
            ready: false,
            ..Default::default()
        });
    }
    assert!(r.model.d.is_some() && r.model.p.is_some() && !r.model.queue.is_empty());
    reset(r);
}
fn replay() -> Replay {
    let q = fixture();
    let mut groups = [false; 9];
    for quad in &q {
        for lane in 0..4 {
            let n = quad.groups.iter().filter(|p| key(**p) % 4 == lane).count();
            groups[n] = true;
        }
    }
    assert!(
        groups[1] && groups[4] && groups[8],
        "actual nearest/bilinear/trilinear seam groups {groups:?}"
    );
    let mut r = Replay::new();
    ordinary(&mut r, &q, false);
    ordinary(&mut r, &q, true);
    fault_edges(&mut r);
    ordinary(&mut r, &q[..16], true);
    assert!(
        r.metrics.full > 0
            && r.metrics.ce0 > 0
            && r.metrics.denied > 0
            && r.metrics.stalls > 0
            && r.metrics.public_peak == 16
    );
    r
}
#[test]
fn closed_owner_numeric_and_remaining_mask_reference() {
    let r = replay();
    println!("PASS {} edges, {} publications, {} groups, {} results, {} finals, {} CE0, {} shared-denied, {} stalls, {} full, {} resets, {} faults",r.metrics.edges,r.metrics.publications,r.metrics.inputs,r.metrics.results,r.metrics.finals,r.metrics.ce0,r.metrics.denied,r.metrics.stalls,r.metrics.full,r.metrics.resets,r.metrics.faults);
    assert_eq!(runtime_color::PUBLIC_OWNER_BITS, 48);
    let source = runtime_color::source(true);
    assert!(
        source.contains("MULT9X9 #(") && !source.contains("ALU54D") && !source.contains("MULTALU")
    );
    assert!(source.contains("f_count") && source.contains("credits < 5'd16"));
}
fn bit(x: bool) -> u8 {
    u8::from(x)
}
fn bench(r: &Replay, native: bool) -> String {
    let mut s=String::from("module tb;\nreg clk=0,reset=0,proposal_ce=0,commit_allow=0,publish_valid=0,in_valid=0,out_ready=0;\nreg [3:0] check_quad=0,publish_quad=0,publish_mask=1;reg [71:0] in_payload=0;reg [63:0] in_texels=0;\nwire public_free,publish_ready,publish_fire,in_ready,input_fire,out_valid,quad_last,result_fire,eligible,advance,preflight_fault,fault;wire [5:0] out_key;wire [23:0] out_rgb;wire [15:0] public_live;\n");
    if native {
        s.push_str("GSR GSR(.GSRI(1'b1));\n");
    }
    writeln!(s, "{} dut(.*);", runtime_color::MODULE).unwrap();
    s.push_str("initial begin #2000000;$fatal(1,\"watchdog\");end\ninitial begin reset=1;#1;clk=1;#1;clk=0;#1;reset=0;\n");
    for (i, (t, e)) in r.edges.iter().enumerate() {
        let (id, mask) = t.publish.unwrap_or((0, 1));
        let p = t.input.unwrap_or(simple(0, [0; 4], true, true));
        let tex: u64 = p
            .texels
            .iter()
            .enumerate()
            .map(|(j, &v)| u64::from(v) << (16 * j))
            .sum();
        writeln!(s,"reset={};proposal_ce={};commit_allow={};check_quad={};publish_valid={};publish_quad={};publish_mask={};in_valid={};in_payload=72'h{:018x};in_texels=64'h{:016x};out_ready={};#1;",bit(t.reset),bit(t.ce),bit(t.allow),t.check,bit(t.publish.is_some()),id,mask,bit(t.input.is_some()),p.payload,tex,bit(t.ready)).unwrap();
        for (name, value) in [
            ("public_free", e.free),
            ("publish_ready", e.publish_ready),
            ("publish_fire", e.publish_fire),
            ("in_ready", e.input_ready),
            ("input_fire", e.input_fire),
            ("out_valid", e.output.is_some()),
            ("quad_last", e.last),
            ("result_fire", e.pop),
            ("eligible", e.eligible),
            ("advance", e.advance),
            ("preflight_fault", e.bad),
            ("fault", e.fault),
        ] {
            writeln!(
                s,
                "if({name}!==1'b{}) $fatal(1,\"{name} edge{i} got=%b\",{name});",
                bit(value)
            )
            .unwrap();
        }
        writeln!(
            s,
            "if(public_live!==16'h{:04x}) $fatal(1,\"live edge{i}\");",
            e.live
        )
        .unwrap();
        for id in 0..16 {
            if e.live & (1 << id) != 0 {
                writeln!(
                    s,
                    "if(dut.last_lane[{id}]!==2'd{}) $fatal(1,\"last lane bank edge{i} id{id}\");",
                    e.last_lane[id]
                )
                .unwrap();
            }
        }
        if let Some(o) = e.output {
            let rgb: u32 = o
                .rgb
                .iter()
                .enumerate()
                .map(|(j, &v)| u32::from(v) << (j * 8))
                .sum();
            writeln!(
                s,
                "if(out_key!==6'd{} || out_rgb!==24'h{:06x}) $fatal(1,\"RGB/key edge{i}\");",
                o.key, rgb
            )
            .unwrap();
        }
        s.push_str("clk=1;#1;clk=0;#1;\n");
    }
    writeln!(
        s,
        "$display(\"PASS edges={}\");$finish;end endmodule",
        r.edges.len()
    )
    .unwrap();
    s
}
fn bounded(mut cmd: Command, dir: &Path, name: &str) {
    let mut c = cmd
        .stdout(Stdio::from(
            File::create(dir.join(format!("{name}.out"))).unwrap(),
        ))
        .stderr(Stdio::from(
            File::create(dir.join(format!("{name}.err"))).unwrap(),
        ))
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        if let Some(status) = c.try_wait().unwrap() {
            assert!(
                status.success(),
                "{name}: {} {}",
                fs::read_to_string(dir.join(format!("{name}.out"))).unwrap(),
                fs::read_to_string(dir.join(format!("{name}.err"))).unwrap()
            );
            break;
        }
        if start.elapsed() > Duration::from_secs(60) {
            c.kill().unwrap();
            c.wait().unwrap();
            panic!("process watchdog {name}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
#[test]
#[ignore = "requires Icarus and Gowin primitive library"]
fn actual_closed_color_portable_native() {
    let r = replay();
    for native in [false, true] {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(DIR)
            .join(if native { "native" } else { "portable" });
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("color.v"), runtime_color::source(native)).unwrap();
        fs::write(dir.join("tb.v"), bench(&r, native)).unwrap();
        let mut cmd =
            Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()));
        cmd.current_dir(&dir)
            .args(["-g2012", "-s", "tb", "-o", "test.vvp", "color.v", "tb.v"]);
        if native {
            cmd.arg(
                std::path::PathBuf::from(std::env::var_os("GOWIN_HOME").expect("GOWIN_HOME"))
                    .join("IDE/simlib/gw2a/prim_sim.v"),
            );
        }
        bounded(cmd, &dir, "compile");
        let mut cmd = Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()));
        cmd.current_dir(&dir).arg("test.vvp");
        bounded(cmd, &dir, "run");
        assert!(fs::read_to_string(dir.join("run.out"))
            .unwrap()
            .contains("PASS edges="));
    }
}

fn reserved_parent_fixture() -> Replay {
    let quads = fixture();
    let mut r = Replay::new();
    ordinary_rows(&mut r, &quads, false);
    ordinary_rows(&mut r, &quads, true);
    // Reset while d/p/result/public state is live, then resume the sole-tail
    // protocol. No malformed publication enters the reserved contract.
    publish(&mut r, 1, 15, 4);
    for lane in 0..4 {
        let mut p = simple(1, [511, 0, 0, 0], true, true);
        p.payload |= i128::from(lane) << 70;
        r.edge(Offer {
            input: Some(p),
            ready: false,
            ..Default::default()
        });
    }
    assert!(r.model.d.is_some() && r.model.p.is_some() && !r.model.queue.is_empty());
    reset(&mut r);
    ordinary_rows(&mut r, &quads[..16], true);
    assert!(
        r.metrics.full > 0 && r.metrics.ce0 > 0 && r.metrics.denied > 0 && r.metrics.stalls > 0
    );
    assert_eq!(r.metrics.faults, 0);
    for (t, e) in &r.edges {
        if t.publish.is_some() {
            assert!(e.publish_ready);
        }
    }
    r
}

#[test]
fn reserved_publication_has_only_the_redundant_preflight_difference() {
    for native in [false, true] {
        let checked = runtime_color::source(native);
        assert_eq!(
            runtime_color::source_reserved(native),
            checked.replace(
                "wire publish_bad=publish_valid && !publish_ready;",
                "wire publish_bad=1'b0;"
            )
        );
    }
    let r = reserved_parent_fixture();
    println!("PASS reserved parent: {} edges, {} publications, {} groups, {} results, {} finals, {} CE0, {} denied, {} stalls, {} full", r.metrics.edges, r.metrics.publications, r.metrics.inputs, r.metrics.results, r.metrics.finals, r.metrics.ce0, r.metrics.denied, r.metrics.stalls, r.metrics.full);
}

fn dual_bench(r: &Replay, native: bool) -> String {
    let mut value = bench(r, native);
    let inputs = [
        "clk",
        "reset",
        "proposal_ce",
        "commit_allow",
        "check_quad",
        "publish_valid",
        "publish_quad",
        "publish_mask",
        "in_valid",
        "in_payload",
        "in_texels",
        "out_ready",
    ];
    let outputs = [
        ("public_free", ""),
        ("publish_ready", ""),
        ("publish_fire", ""),
        ("in_ready", ""),
        ("input_fire", ""),
        ("out_valid", ""),
        ("out_key", "[5:0]"),
        ("out_rgb", "[23:0]"),
        ("quad_last", ""),
        ("result_fire", ""),
        ("eligible", ""),
        ("advance", ""),
        ("preflight_fault", ""),
        ("fault", ""),
        ("public_live", "[15:0]"),
    ];
    let mut inst = String::new();
    for (name, width) in outputs {
        writeln!(inst, "wire {width} reserved_{name};").unwrap();
    }
    writeln!(inst, "{} dut(.*);", runtime_color::MODULE).unwrap();
    write!(inst, "{}_reserved other(", runtime_color::MODULE).unwrap();
    for name in inputs {
        write!(inst, ".{name}({name}),").unwrap();
    }
    for (i, (name, _)) in outputs.iter().enumerate() {
        write!(
            inst,
            ".{name}(reserved_{name}){}",
            if i + 1 == outputs.len() { ");\n" } else { "," }
        )
        .unwrap();
    }
    inst.push_str("integer qi;\n");
    let anchor = format!("{} dut(.*);\n", runtime_color::MODULE);
    assert_eq!(value.matches(&anchor).count(), 1);
    value = value.replace(&anchor, &inst);
    let mut checks = String::new();
    for (name, _) in outputs {
        if name != "out_key" && name != "out_rgb" {
            writeln!(
                checks,
                "if({name}!==reserved_{name}) $fatal(1,\"checked/reserved {name}\");"
            )
            .unwrap();
        }
    }
    checks.push_str("if(out_valid && ({out_key,out_rgb}!=={reserved_out_key,reserved_out_rgb})) $fatal(1,\"checked/reserved RGB/key\");\nfor(qi=0;qi<16;qi=qi+1) if(public_live[qi] && (dut.last_lane[qi]!==other.last_lane[qi])) $fatal(1,\"checked/reserved owner bank\");\n");
    value.replace("clk=1;#1;clk=0;#1;\n", &(checks + "clk=1;#1;clk=0;#1;\n"))
}

#[test]
#[ignore = "requires Icarus and Gowin primitive library"]
fn reserved_publication_actual_checked_and_reserved_match() {
    let r = reserved_parent_fixture();
    for native in [false, true] {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/runtime-color-reserved-20261010")
            .join(if native { "native" } else { "portable" });
        fs::create_dir_all(&dir).unwrap();
        let checked = runtime_color::source(native);
        let reserved = runtime_color::source_reserved(native).replace(
            runtime_color::MODULE,
            &format!("{}_reserved", runtime_color::MODULE),
        );
        fs::write(dir.join("checked.v"), &checked).unwrap();
        fs::write(dir.join("reserved.v"), &reserved).unwrap();
        fs::write(dir.join("tb.v"), dual_bench(&r, native)).unwrap();
        let mut cmd =
            Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()));
        cmd.current_dir(&dir).args([
            "-g2012",
            "-s",
            "tb",
            "-o",
            "test.vvp",
            "checked.v",
            "reserved.v",
            "tb.v",
        ]);
        if native {
            cmd.arg(
                std::path::PathBuf::from(std::env::var_os("GOWIN_HOME").expect("GOWIN_HOME"))
                    .join("IDE/simlib/gw2a/prim_sim.v"),
            );
        }
        bounded(cmd, &dir, "compile");
        let mut cmd = Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()));
        cmd.current_dir(&dir).arg("test.vvp");
        bounded(cmd, &dir, "run");
        assert!(fs::read_to_string(dir.join("run.out"))
            .unwrap()
            .contains("PASS edges="));
    }
}
