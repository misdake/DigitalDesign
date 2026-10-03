//! Bounded private qualification of the public Runtime path; no public mode.
use super::super::{control, runtime_inventory};
use super::*;
use crate::texture::{
    emu::{coefficient, color},
    ports::{Filter, Group4},
};
use std::io::Write;
#[path = "runtime_physical.rs"]
mod physical;
const BOUND: u64 = 20_000;

#[test]
fn connected_runtime_partial_return_fault_is_terminal() {
    let (slots, bytes) = fixture();
    let p = control::Hardware {
        max_cycles: BOUND,
        ..Default::default()
    };
    let mut r = runtime(&slots, p);
    let mut mc = physical::Physical::new(u64::from(BASE), bytes, true, false);
    let q = input(0, 1, [0.25, 0.25], Filter::Trilinear);
    assert!(
        r.step(&mut mc, Some(&q), timed::Control::default())
            .unwrap()
            .accepted
    );
    let mut injected = false;
    for _ in 0..BOUND {
        if r.preparation.corrupt_next_work_read() {
            injected = true;
            break;
        }
        r.step(&mut mc, None, timed::Control::default()).unwrap();
    }
    assert!(injected);
    let before = r.preparation.work_state();
    let mc_wall = mc.cycles;
    assert!(!before.pending && !before.valid && before.materialized > 0);
    let error = r
        .step(&mut mc, None, timed::Control::default())
        .unwrap_err();
    assert!(error.contains("immutable initial cursor"));
    assert!(r.faulted());
    assert!(!r.input_ready(1));
    let after = r.preparation.work_state();
    assert!(
        after.pending,
        "R reservation occurred before the protocol error"
    );
    assert_eq!(
        mc.cycles, mc_wall,
        "failure occurred before the sole MC owner"
    );
    let stopped = r.snapshot();
    assert!(r.step(&mut mc, None, timed::Control::default()).is_err());
    assert_eq!(r.snapshot(), stopped);
    assert_eq!(r.preparation.work_state(), after);
    assert_eq!(mc.cycles, mc_wall);
}
// Independent dyadic-fixture goldens; never consult sampler/Frame/Runtime.
const BASE: u32 = 4096;
fn pattern(n: u8, x: usize, y: usize) -> u16 {
    [0xf800, 0x07e0, 0x001f, 0xffff, 0xffe0, 0xf81f, 0x07ff, 0][(x / 4 + y + usize::from(n)) % 8]
}
fn fixture() -> (Vec<Slot>, Vec<u8>) {
    let mut bytes = vec![];
    for n in 0..=5 {
        let logical = 1_usize << n;
        let side = logical.div_ceil(8);
        for ty in 0..side {
            for tx in 0..side {
                for y in 0..8 {
                    for x in 0..8 {
                        bytes.extend(
                            pattern(n, (tx * 8 + x) % logical, (ty * 8 + y) % logical)
                                .to_le_bytes(),
                        );
                    }
                }
            }
        }
    }
    (
        vec![Slot {
            base_address: BASE,
            has_full_mip: true,
            max_size_log2: 5,
            valid: true,
        }],
        bytes,
    )
}
fn input(id: u8, mask: u8, uv: [f64; 2], filter: Filter) -> QuadInput {
    let mut q = QuadInput {
        quad_id: id,
        mask,
        uv: [uv; 4],
        slot: 0,
        material_size_log2: 5,
        filter,
        lod_bias: 0.5,
    };
    q.uv[1][0] += 1.0 / 32.0;
    q.uv[2][1] += 1.0 / 32.0;
    q
}
fn expand(word: u16) -> [u32; 3] {
    let r = u32::from(word >> 11);
    let g = u32::from(word >> 5 & 63);
    let b = u32::from(word & 31);
    [
        (r << 3) | (r >> 2),
        (g << 2) | (g >> 4),
        (b << 3) | (b >> 2),
    ]
}
/// Independent scalar integer reference on this dyadic/rho1 fixture. No sampler,
/// counted Frame, packet/color helper or cache provenance is read here.
fn golden(q: &QuadInput) -> Vec<color::Output> {
    let mut out = vec![];
    for lane in 0..4 {
        if q.mask >> lane & 1 == 0 {
            continue;
        }
        let mut sums = [0_u32; 3];
        for (which, n, parent) in [
            (
                0,
                5,
                if q.filter == Filter::Trilinear {
                    255
                } else {
                    511
                },
            ),
            (1, 4, 256),
        ] {
            if which == 1 && q.filter != Filter::Trilinear {
                continue;
            }
            let nearest = q.filter == Filter::Nearest;
            let mut axes = [(0_i32, 0_u32); 2];
            for (a, axis) in axes.iter_mut().enumerate() {
                let uv = ((q.uv[lane][a] * 262144.0).round_ties_even() as i64).rem_euclid(262144);
                let coord = (uv * (1 << n) * 256 / 262144) as i32 - if nearest { 0 } else { 128 };
                *axis = (
                    coord.div_euclid(256),
                    if nearest {
                        0
                    } else {
                        coord.rem_euclid(256) as u32
                    },
                );
            }
            let bottom = parent * axes[1].1 / 256;
            let top = parent - bottom;
            let br = bottom * axes[0].1 / 256;
            let tr = top * axes[0].1 / 256;
            for (tap, weight) in [top - tr, tr, bottom - br, br].into_iter().enumerate() {
                let x = (axes[0].0 + (tap & 1) as i32).rem_euclid(1 << n) as usize;
                let y = (axes[1].0 + (tap >> 1) as i32).rem_euclid(1 << n) as usize;
                for (sum, value) in sums.iter_mut().zip(expand(pattern(n, x, y))) {
                    *sum += value * weight;
                }
            }
        }
        out.push(color::Output {
            key: q.quad_id * 4 + lane as u8,
            rgb: sums.map(|s| ((s + 255) / 511) as u8),
        });
    }
    out
}
fn literal_packets() -> Vec<i128> {
    let mut packets = vec![];
    for lane in [0, 3] {
        for tap in 0..4 {
            // Independent hand layout: TL/TR/BL/BR land in four distinct
            // fine tiles; coarse coordinates3/4 all share tile0.
            packets.push(
                5 << 4
                    | (tap & 1) << 8
                    | (tap >> 1) << 15
                    | 7 << 22
                    | 7 << 25
                    | (if tap == 3 { 63 } else { 64 }) << (28 + tap * 9)
                    | i128::from(tap == 0) << 64
                    | 3 << 66
                    | lane << 70,
            );
        }
        packets.push(
            4 << 4
                | 3 << 22
                | 3 << 25
                | 64 << 28
                | 64 << 37
                | 64 << 46
                | 64 << 55
                | 1 << 65
                | 3 << 66
                | lane << 70,
        );
    }
    packets
}

#[derive(Default)]
struct Counts {
    products: usize,
    returns: usize,
    planes: usize,
    w: usize,
    r: usize,
    c: usize,
    ack: usize,
    captures: usize,
    packets: usize,
    wraps: usize,
}
fn evidence_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("GPU_RUNTIME_COEFFICIENT_OUTPUT").map(std::path::PathBuf::from)
}
fn receipt(r: &Runtime, p: control::Hardware, name: &str) {
    let c = r.cache.external_context().1;
    let actual = runtime_inventory::describe(&r.preparation.binding, p, c).unwrap();
    let baseline = super::super::inventory::describe(&r.preparation.binding, p, c).unwrap();
    let color_bits =
        (color::ALLOCATION.datapath_ff_bits + color::ALLOCATION.result_and_control_ff_bits) as u64;
    let a = u64::from(usize::BITS - (p.work_credits - 1).leading_zeros());
    let q = u64::from(usize::BITS - p.work_credits.leading_zeros());
    let correction = if p.storage == control::Storage::Dedicated {
        19_i64 - 104
    } else {
        0
    };
    assert_eq!(
        actual.ff_bits as i64,
        (baseline.ff_bits + color_bits + LINK_STATE_BITS as u64 + 108 + 2 * a + q) as i64
            + correction
    );
    assert_eq!(actual.sdp4_cells, baseline.sdp4_cells);
    assert_eq!(actual.ram16x1_cells, baseline.ram16x1_cells);
    assert_eq!(actual.bsram, 6);
    if p.work_credits == 16
        && p.coordinate_credits == 6
        && p.contexts == 8
        && p.storage == control::Storage::Packed
    {
        assert_eq!(
            (
                actual.ff_bits,
                actual.sdp4_cells,
                actual.ram16x1_cells,
                actual.hard_pipeline_bits
            ),
            (16939, 56, 211, 1377)
        );
    }
    println!(
        "ALLOCATION {name}: FF={} SDP4={} RAM16x1={} BSRAM={} hard={} DSP18eq={} fitted=false",
        actual.ff_bits,
        actual.sdp4_cells,
        actual.ram16x1_cells,
        actual.bsram,
        actual.hard_pipeline_bits,
        actual.dsp18
    );
    if let Some(dir) = evidence_dir() {
        let mut file = std::fs::File::create(dir.join(format!("inventory-{name}.json"))).unwrap();
        writeln!(file,"{{\"W\":{},\"coordinate\":{},\"contexts\":{},\"early\":{},\"storage\":\"{:?}\",\"FF\":{},\"SDP4\":{},\"RAM16x1\":{},\"BSRAM\":{},\"hard\":{},\"DSP18eq\":{},\"physical_claim\":false,\"rows\":[",p.work_credits,p.coordinate_credits,p.contexts,p.release_after_capture,p.storage,actual.ff_bits,actual.sdp4_cells,actual.ram16x1_cells,actual.bsram,actual.hard_pipeline_bits,actual.dsp18).unwrap();
        for (i, row) in actual.rows.iter().enumerate() {
            writeln!(file,"{}{{\"owner\":\"{}\",\"FF\":{},\"SDP4\":{},\"RAM16x1\":{},\"BSRAM\":{},\"hard\":{},\"ports\":\"{}\"}}",if i==0 {""} else {","},row.name,row.ff_bits,row.sdp4_cells,row.ram16x1_cells,row.bsram,row.hard_pipeline_bits,row.ports).unwrap();
        }
        writeln!(file, "]}}").unwrap();
    }
}
fn runtime(slots: &[Slot], p: control::Hardware) -> Runtime {
    let mut r = Runtime::new(
        slots,
        p,
        timed::Hardware {
            prefetch: false,
            max_quads: 1,
            max_cycles: BOUND,
            ..Default::default()
        },
        BOUND,
    )
    .unwrap();
    r.poison = true;
    r
}
fn observe(r: &Runtime, step: &Step, totals: &mut Counts) {
    let trace = &r.preparation.trace;
    let coefficient = trace.coefficient.as_ref().unwrap();
    let (work, coords, members) = r.preparation.counts();
    assert!(
        work <= r.preparation.stats.peak_work && coords <= r.preparation.stats.peak_coordinates
    );
    assert!(r.preparation.work_state().materialized + members <= work);
    let pool = step.cache.snapshot.packet_pool.as_ref().unwrap();
    assert!(
        pool.producer <= 16
            && pool.groups <= 32
            && pool.rows <= 64
            && pool.heads + usize::from(pool.pending) <= 2
    );
    assert_eq!(
        step.accepted,
        step.input_ready && step.control.ce && step.offered.is_some()
    );
    assert_eq!(step.accepted, step.cache.accepted);
    assert_eq!(step.accepted, step.preparation.accepted);
    totals.w += usize::from(trace.work.write.is_some());
    totals.r += usize::from(trace.work.read.is_some());
    totals.c += usize::from(trace.work.returned);
    totals.ack += usize::from(trace.work.ack);
    totals.wraps += usize::from(trace.work.write == Some(0));
    totals.planes += usize::from(trace.plane.is_some());
    totals.captures += usize::from(trace.work.capture.is_some());
    totals.packets += step
        .preparation
        .events
        .iter()
        .filter(|e| matches!(e, PreparationEvent::Packet { .. }))
        .count();
    for event in &coefficient.events {
        match event {
            coefficient::Event::Issued {
                site: coefficient::Site::Multiply(_),
                ..
            } => totals.products += 1,
            coefficient::Event::Returned { .. } => totals.returns += 1,
            _ => {}
        }
    }
}
fn finish(r: &Runtime, mc: &physical::Physical, c: &Counts, name: &str) {
    assert!(mc.init_cycles > 0 && mc.init_cycles < BOUND);
    assert!(r.idle(), "{name} did not drain");
    assert!(c.products > 0 && c.returns > 0 && c.planes > 0 && c.captures > 0);
    assert!(c.w > 0 && c.r == c.w && c.c == c.w && c.ack == c.w);
    assert_eq!(c.packets, c.captures);
    assert!(
        r.cache_stats().beats > 0
            && r.cache_stats().refills > 0
            && r.stats.link.captures > 0
            && r.stats.link.results > 0
    );
    assert!(r.poison_hits > 0);
    assert_eq!(mc.cycles, r.stats.link.wall, "one MC wall owner");
    println!("RUNTIME {name}: wall={} base={} coefficient={} quads={} products={} returns={} planes={} WorkW={} R={} C={} ACK={} packet_capture={} packetW={} beats={} READY={} Color_capture={} public={} wrap_visits={} poison={}",r.wall,r.stats.link.enabled,r.preparation.coefficient_snapshot().enabled,r.stats.link.accepted,c.products,c.returns,c.planes,c.w,c.r,c.c,c.ack,c.captures,c.packets,r.cache_stats().beats,r.cache_stats().refills,r.stats.link.captures,r.stats.link.results,c.wraps,r.poison_hits);
}
fn calendar(name: &str) -> Option<std::fs::File> {
    evidence_dir().map(|dir| {let mut file=std::fs::File::create(dir.join(format!("calendar-{name}.csv"))).unwrap();
        writeln!(file,"wall,base,coefficient,phase,pre_work,offer_cost,accepted,R,W,C,ACK,packet_capture,packetW,public").unwrap();file})
}
fn record(file: &mut Option<std::fs::File>, r: &Runtime, step: &Step) {
    if let Some(file) = file {
        let t = &r.preparation.trace;
        let c = t.coefficient.as_ref().unwrap();
        writeln!(
            file,
            "{},{},{},{},{},{},{},{:?},{:?},{},{},{},{},{}",
            r.wall,
            r.stats.link.enabled,
            c.snapshot.enabled,
            c.snapshot.phase,
            t.pre_work,
            t.offer_cost,
            c.accepted,
            t.work.read,
            t.work.write,
            t.work.returned,
            t.work.ack,
            t.work.capture.is_some(),
            step.preparation
                .events
                .iter()
                .any(|e| matches!(e, PreparationEvent::Packet { .. })),
            step.results.len()
        )
        .unwrap();
    }
}

#[test]
fn connected_runtime_literal_and_supported_configurations() {
    let (slots, bytes) = fixture();
    let p = control::Hardware {
        storage: control::Storage::Packed,
        max_cycles: BOUND,
        ..Default::default()
    };
    let mut r = runtime(&slots, p);
    receipt(&r, p, "literal");
    let mut mc = physical::Physical::new(u64::from(BASE), bytes.clone(), true, false);
    let qs = [
        input(3, 9, [0.25, 0.25], Filter::Trilinear),
        input(3, 9, [-0.03125, 0.46875], Filter::Bilinear),
        input(7, 1, [0.625, 0.15625], Filter::Nearest),
    ];
    let mut accepted = 0;
    let mut got = vec![];
    let mut packets = vec![];
    let mut totals = Counts::default();
    let mut denied = 0;
    let mut literal = 0;
    let mut csv = calendar("literal");
    for _ in 0..BOUND {
        let before_public = r.snapshot().result_lanes;
        let step = r
            .step(&mut mc, qs.get(accepted), timed::Control::default())
            .unwrap();
        observe(&r, &step, &mut totals);
        record(&mut csv, &r, &step);
        if accepted == 1 && !step.accepted {
            denied += 1;
        }
        if step.accepted {
            assert_eq!(before_public[usize::from(qs[accepted].quad_id)], 0);
            accepted += 1;
        }
        if let Some((o, _)) = r.preparation.trace.plane {
            if accepted == 1 {
                assert_eq!(o.weights, [[64, 64, 64, 63], [64, 64, 64, 64]]);
                assert_eq!(o.metadata.coordinates, [[7, 8, 7, 8], [3, 4, 3, 4]]);
                literal += 1;
            }
        }
        if let Some(packet) = r.preparation.trace.packet {
            assert!(Group4::unpack72(packet).is_ok());
            packets.push(packet);
        }
        got.extend(step.results);
        if accepted == qs.len() && r.idle() {
            break;
        }
    }
    assert!(denied > 0 && literal == 4);
    assert_eq!(&packets[..10], literal_packets());
    assert_eq!(got, qs.iter().flat_map(golden).collect::<Vec<_>>());
    assert_eq!(
        got.iter().map(|o| o.rgb).collect::<Vec<_>>(),
        vec![
            [96, 96, 96],
            [96, 96, 96],
            [127, 127, 255],
            [127, 127, 255],
            [0, 0, 0]
        ]
    );
    finish(&r, &mc, &totals, "literal");
    for (w, c, contexts, early, storage) in [
        (2, 1, 1, false, control::Storage::Dedicated),
        (3, 2, 2, true, control::Storage::Packed),
        (16, 6, 8, true, control::Storage::Packed),
        (17, 16, 8, false, control::Storage::Dedicated),
        (32, 16, 8, true, control::Storage::Packed),
        (32, 1, 1, false, control::Storage::Packed),
    ] {
        let p = control::Hardware {
            work_credits: w,
            coordinate_credits: c,
            contexts,
            release_after_capture: early,
            storage,
            max_cycles: BOUND,
            ..Default::default()
        };
        let name = format!("W{w}-C{c}-{storage:?}-{early}");
        let mut r = runtime(&slots, p);
        receipt(&r, p, &name);
        let mut mc = physical::Physical::new(u64::from(BASE), bytes.clone(), true, true);
        let qs: Vec<_> = (0..24)
            .map(|i| {
                input(
                    (i % 16) as u8,
                    [15, 9, 1, 0][i % 4],
                    if i % 3 == 2 {
                        [0.125, 0.25] // emit0/emit2: sparse tap differs from ordinal
                    } else if i % 2 == 0 {
                        [0.25, 0.25]
                    } else {
                        [-0.03125, 0.46875]
                    },
                    Filter::Trilinear,
                )
            })
            .collect();
        let mut accepted = 0;
        let mut got = vec![];
        let mut totals = Counts::default();
        let mut csv = calendar(&name);
        let mut pause = 0;
        let mut paused_states = [0; 3];
        let mut pending_packets = std::collections::VecDeque::new();
        let mut shared_release = 0;
        let mut prep_release = 0;
        let mut actual_masks = [0_u8; 16];
        let mut coordinate_issues = [0_usize; 16];
        let mut next_packet_ordinal = 0;
        let mut sparse_tap = 0;
        for tick in 0..BOUND {
            let old = r.preparation.work_state();
            let ce = pause == 0;
            let step = r
                .step(
                    &mut mc,
                    qs.get(accepted),
                    timed::Control {
                        ce,
                        result_ready: tick > 500 && tick % 19 > 3,
                    },
                )
                .unwrap();
            observe(&r, &step, &mut totals);
            record(&mut csv, &r, &step);
            if step.accepted {
                let q = &qs[accepted];
                actual_masks[usize::from(q.quad_id)] = q.mask;
                coordinate_issues[usize::from(q.quad_id)] = 0;
                accepted += 1;
            }
            let edge = r.preparation.trace.work;
            if let Some((_, tap)) = edge.capture {
                sparse_tap += usize::from(usize::from(tap) != next_packet_ordinal);
            }
            if !ce {
                assert_eq!(r.preparation.work_state(), old);
                assert!(step.results.is_empty());
                pause -= 1;
            } else if paused_states.contains(&0) {
                let state = if edge.read.is_some() {
                    Some(0)
                } else if edge.returned {
                    Some(1)
                } else if edge.ack {
                    Some(2)
                } else {
                    None
                };
                if let Some(state) = state {
                    if paused_states[state] == 0 {
                        paused_states[state] += 1;
                        pause = 2;
                    }
                }
            }
            if let Some(word) = r.preparation.trace.packet {
                pending_packets.push_back(word);
            }
            for e in &step.preparation.events {
                match e {
                    PreparationEvent::Packet { payload, .. } => {
                        assert_eq!(Some(*payload), pending_packets.pop_front())
                    }
                    PreparationEvent::Issue {
                        stage: "packet",
                        packet,
                        ..
                    } => {
                        assert_eq!(*packet, next_packet_ordinal);
                        next_packet_ordinal = if edge.ack { 0 } else { next_packet_ordinal + 1 };
                    }
                    PreparationEvent::Issue {
                        stage: "coordinate",
                        program,
                        ..
                    } => {
                        coordinate_issues[*program] += 1;
                        assert!(
                            coordinate_issues[*program]
                                <= actual_masks[*program].count_ones() as usize
                        );
                    }
                    PreparationEvent::SharedRelease { program, .. } => {
                        shared_release += 1;
                        assert_eq!(
                            coordinate_issues[*program],
                            actual_masks[*program].count_ones() as usize
                        );
                        if early && actual_masks[*program] != 0 {
                            assert!(step.preparation.events.iter().any(|e|matches!(e,
                                PreparationEvent::Issue {stage:"coordinate",program:p,..} if p==program)));
                        } else {
                            assert!(step.preparation.events.iter().any(|e| matches!(e,
                                PreparationEvent::Release {program:p} if p==program)));
                        }
                    }
                    PreparationEvent::Release { program } => {
                        prep_release += 1;
                        if actual_masks[*program] != 0 {
                            assert!(step.preparation.events.iter().any(|e|matches!(e,
                                PreparationEvent::Packet {program:p,payload} if p==program && payload>>65&1==1)));
                        }
                    }
                    _ => {}
                }
            }
            got.extend(step.results);
            if accepted == qs.len() && r.idle() {
                break;
            }
        }
        assert_eq!(accepted, qs.len());
        assert_eq!(shared_release, qs.len());
        assert_eq!(prep_release, qs.len());
        assert!(paused_states.iter().all(|&n| n > 0));
        assert!(totals.wraps >= 2, "true logical wrap");
        assert!(pending_packets.is_empty());
        assert!(sparse_tap > 0);
        assert_eq!(got, qs.iter().flat_map(golden).collect::<Vec<_>>());
        assert!(
            r.preparation_stats().peak_contexts <= contexts
                && r.preparation_stats().peak_coordinates <= c
        );
        finish(&r, &mc, &totals, &name);
        let mut invalid = qs[0].clone();
        invalid.quad_id = 16;
        let cycles = mc.cycles;
        assert!(r
            .step(&mut mc, Some(&invalid), timed::Control::default())
            .is_err());
        assert!(r.faulted());
        assert!(!r.input_ready(0));
        assert!(r.step(&mut mc, None, timed::Control::default()).is_err());
        assert_eq!(mc.cycles, cycles);
    }
}

#[test]
fn connected_runtime_full_work_local_hold_and_mc_ce0() {
    let (slots, bytes) = fixture();
    let p = control::Hardware {
        storage: control::Storage::Packed,
        max_cycles: BOUND,
        ..Default::default()
    };
    let qs: Vec<_> = (0..6)
        .map(|i| input(i, 15, [0.25, 0.25], Filter::Trilinear))
        .collect();
    for local_hold in [true, false] {
        let name = if local_hold {
            "local-hold"
        } else {
            "Work16-pressure"
        };
        let mut r = runtime(&slots, p);
        let mut mc = physical::Physical::new(u64::from(BASE), bytes.clone(), true, true);
        let mut accepted = 0;
        let mut got = vec![];
        let mut totals = Counts::default();
        let mut csv = calendar(name);
        let mut phase = 0;
        let mut held = 0;
        let mut drained = 0;
        let mut transferred = 0;
        let mut full = 0;
        let mut one = 0;
        let mut old_ack = 0;
        let mut pause = 0;
        let mut paused_once = false;
        let mut off_beats = 0;
        let mut off_ready = 0;
        let mut result_peak = 0;
        for wall in 0..BOUND {
            let before = r.preparation.coefficient_snapshot();
            if local_hold {
                if phase == 0 && totals.planes >= 4 {
                    phase = 1;
                }
                if phase == 1 && before.queued == 2 && r.preparation.work_state().materialized > 0 {
                    phase = 2;
                }
                if phase == 2 && held == 40 {
                    phase = 3;
                }
                r.preparation.membership_ready = phase == 0 || phase == 3;
                r.preparation.packet_ready = phase >= 2;
            } else {
                if r.preparation.counts().0 == 16 {
                    phase = 1;
                }
                r.preparation.packet_ready = phase == 1;
            }
            let ce = pause == 0;
            let step = r
                .step(
                    &mut mc,
                    qs.get(accepted),
                    timed::Control {
                        ce,
                        result_ready: local_hold || wall > 3000,
                    },
                )
                .unwrap();
            observe(&r, &step, &mut totals);
            record(&mut csv, &r, &step);
            if step.accepted {
                accepted += 1;
            }
            let trace = &r.preparation.trace;
            let coefficient = trace.coefficient.as_ref().unwrap();
            if local_hold && phase == 2 {
                assert_eq!(before.queued, 2);
                assert_eq!(coefficient.snapshot.enabled, before.enabled);
                assert_eq!(coefficient.snapshot.numeric_words, before.numeric_words);
                assert_eq!(
                    coefficient.snapshot.product_registers,
                    before.product_registers
                );
                assert_eq!(coefficient.snapshot.phase, before.phase);
                assert!(step.effective_ce);
                held += 1;
                drained += usize::from(
                    trace.packet.is_some() || trace.work.returned || trace.work.read.is_some(),
                );
                transferred += step
                    .cache
                    .packet_events
                    .iter()
                    .filter(|e| matches!(e, timed::packet::Event::Transfer { .. }))
                    .count();
            }
            if trace.pre_work == 16 && trace.offer_cost > 0 {
                full += 1;
                assert!(!coefficient.accepted);
            }
            if trace.pre_work == 15 && trace.offer_cost == 2 {
                one += 1;
                assert!(!coefficient.accepted);
            }
            if trace.work.ack && trace.offer_cost > 16 - trace.pre_work {
                old_ack += 1;
                assert!(!coefficient.accepted);
            }
            if !local_hold
                && !paused_once
                && step
                    .cache
                    .events
                    .iter()
                    .any(|e| matches!(e, timed::Event::Submitted { .. }))
            {
                pause = 600;
                paused_once = true;
            } else if pause > 0 {
                pause -= 1;
            }
            if !ce {
                assert_eq!(coefficient.snapshot.enabled, before.enabled);
                off_beats += step
                    .cache
                    .events
                    .iter()
                    .filter(|e| matches!(e, timed::Event::Beat { .. }))
                    .count();
                off_ready += step
                    .cache
                    .events
                    .iter()
                    .filter(|e| matches!(e, timed::Event::Ready { .. }))
                    .count();
            }
            result_peak = result_peak.max(r.color.snapshot().result_credits);
            got.extend(step.results);
            if accepted == qs.len() && r.idle() {
                break;
            }
        }
        assert_eq!(got, qs.iter().flat_map(golden).collect::<Vec<_>>());
        if local_hold {
            assert!(held == 40 && drained > 0 && transferred > 0);
        } else {
            assert!(full > 0 && one > 0 && old_ack > 0 && off_beats > 0 && off_ready > 0);
            assert_eq!(result_peak, 16);
        }
        finish(&r, &mc, &totals, name);
        println!("PRESSURE {name}: held={held} drained={drained} transfers={transferred} full={full} onefree={one} blocked_ACK={old_ack} CE0_beats={off_beats} READY={off_ready} result_peak={result_peak}");
    }
}
