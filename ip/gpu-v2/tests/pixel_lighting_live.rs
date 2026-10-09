//! Lighting-live: the real LightingEmu drives the existing J1 light store.
//! Sampling stays controlled (default white); the independent oracle only
//! supplies the post-hoc golden and never feeds the device.
use gpu_v2::{
    framebuffer::{ports as fb, sim::fixture::Fixture},
    lighting::{
        ports as light,
        sim::{counted, oracle},
    },
    system::pixel::{Basic, Context, Event, LightingLive, LiveQuad, LiveTick, QuadInput},
};
#[allow(dead_code)]
#[path = "support/pixel.rs"]
mod support;

const QUADS: usize = 40;

fn normals() -> [[i16; 3]; 8] {
    [
        [0, 0, 0],
        [0, 0, 8192],
        [0, 0, 16384],
        [32767, -32768, 16384],
        [-731, 2173, -9911],
        [0, 0, -16384],
        [1200, 4700, 7133],
        [1023, 0, 8],
    ]
}
fn ndc(i: usize) -> [i32; 2] {
    const N: [[i32; 2]; 6] = [
        [0, 0],
        [4096, 0],
        [-4096, 0],
        [8192, -8192],
        [-4096, 12288],
        [256, -1024],
    ];
    N[i % N.len()]
}
/// Controlled quads: varied coverage, defaults, tints, depth order, normals.
fn quads() -> Vec<LiveQuad> {
    (0..QUADS)
        .map(|i| {
            let mask = [15, 1, 2, 4, 8, 3, 6, 12, 5, 10, 0][i % 11];
            let header = fb::Header {
                x: ((i % 8) * 16) as u16,
                y: ((i / 8 % 2) * 16) as u8,
                mask,
            };
            let basic = std::array::from_fn(|lane| Basic {
                tint: [
                    (i * 17 + lane * 7) as u8,
                    (i * 31 + lane * 19) as u8,
                    (i * 5 + lane * 47) as u8,
                ],
                depth: (52000 - i * 700 - lane * 37) as u16,
            });
            let pixels = std::array::from_fn(|lane| light::PixelInput {
                normal: normals()[(i + lane * 3) % normals().len()],
                ndc: ndc(i + lane),
            });
            LiveQuad {
                quad: QuadInput {
                    header,
                    basic,
                    default_light: i % 5 == 0,
                    default_sample: true,
                },
                light: pixels,
            }
        })
        .collect()
}
fn oracle_out(pixel: light::PixelInput, ctx: light::LightingContext) -> light::LightingOutput {
    let golden = oracle::evaluate(
        pixel,
        ctx.material,
        ctx.light,
        ctx.projection,
        oracle::Config {
            rounding: oracle::RoundingPolicy {
                power: oracle::Rounding::Floor,
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .unwrap();
    light::LightingOutput {
        g: golden.g as u16,
        h: golden.h as u16,
    }
}
fn stimulus(quads: &[LiveQuad], ctx: light::LightingContext) -> Vec<support::Stimulus> {
    quads
        .iter()
        .map(|q| support::Stimulus {
            quad: q.quad,
            light: std::array::from_fn(|lane| oracle_out(q.light[lane], ctx)),
            sample: [[255; 3]; 4],
        })
        .collect()
}
fn ctx(
    shininess: u8,
    specular: [u8; 3],
    light_dir: [i16; 3],
    ambient: u16,
    directional: u16,
) -> light::LightingContext {
    light::LightingContext {
        material: light::Material {
            unlit: false,
            specular_color: specular,
            shininess_code: shininess,
        },
        light: light::Light {
            direction: light_dir,
            ambient,
            directional,
        },
        projection: light::Projection::default(),
        epoch: 0x2a5a,
    }
}
#[derive(Default, Debug)]
struct Evidence {
    issued: u64,
    returned: u64,
    light_done: u64,
    read: u64,
    captured: u64,
    consumed: u64,
    retired: u64,
    ce_frozen: u64,
    max_in_flight: usize,
    distinct: u64,
    non_default: u64,
    reused: u64,
}
fn run(
    pixel_ctx: Context,
    lighting: light::LightingContext,
    quads: &[LiveQuad],
    pauses: bool,
) -> (Vec<u8>, Evidence) {
    let initial = support::image();
    let expected = support::golden(initial.clone(), &stimulus(quads, lighting), pixel_ctx);
    let mut memory = Fixture::new(initial);
    memory.request_period = 5;
    memory.beat_period = 3;
    memory.ack_delay = 17;
    let max_steps = 200_000;
    let mut live = LightingLive::new(pixel_ctx, lighting, max_steps).unwrap();
    let mut proof = Evidence::default();
    let mut next = 0usize;
    let mut held_source: Option<usize> = None;
    let mut source_for_serial: Vec<usize> = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut slot_serial: [Option<u64>; 16] = [None; 16];
    for wall in 0..max_steps {
        let ce = !pauses || wall % 11 > 2;
        let final_ready = !pauses || wall > 240 && wall % 37 > 10;
        let offered = held_source.or_else(|| (next < quads.len()).then_some(next));
        let quad = offered.map(|i| quads[i]);
        let cycle = live
            .step(
                LiveTick {
                    ce,
                    final_ready,
                    light_ready: true,
                    quad,
                    sample: None,
                    finish: next == quads.len(),
                },
                &mut memory,
            )
            .unwrap();
        if let Some(i) = offered {
            held_source = Some(i);
        }
        if cycle.model.quad_accepted {
            if let (Some(ticket), Some(i)) = (cycle.model.ticket, held_source) {
                debug_assert_eq!(ticket.serial, source_for_serial.len() as u64);
                if let Some(previous) = slot_serial[usize::from(ticket.quad)] {
                    assert!(previous < ticket.serial, "slot serial must advance");
                    proof.reused += 1;
                }
                slot_serial[usize::from(ticket.quad)] = Some(ticket.serial);
                source_for_serial.push(i);
            }
            held_source = None;
            next += 1;
        } else if !cycle.input_held {
            held_source = None;
        }
        for event in &cycle.model.events {
            match event {
                Event::LightDone(key) => {
                    proof.light_done += 1;
                    let source = source_for_serial[key.ticket.serial as usize];
                    assert!(
                        quads[source].quad.header.mask >> key.lane & 1 != 0,
                        "done for uncovered lane"
                    );
                    assert!(!quads[source].quad.default_light, "done for default branch");
                }
                Event::ReadIssued { .. } => proof.read += 1,
                Event::Captured { .. } => proof.captured += 1,
                Event::Consumed { .. } => proof.consumed += 1,
                Event::Retired(ticket) => {
                    assert_eq!(ticket.serial, proof.retired);
                    proof.retired += 1;
                }
                _ => {}
            }
        }
        if let Some((key, value, epoch)) = cycle.light_returned {
            assert_eq!(epoch, lighting.epoch, "result epoch");
            assert!(
                cycle
                    .model
                    .events
                    .iter()
                    .any(|e| matches!(e, Event::LightDone(k) if *k == key)),
                "return without store done"
            );
            let source = source_for_serial[key.ticket.serial as usize];
            let q = &quads[source];
            assert_eq!(q.quad.header.mask >> key.lane & 1, 1);
            assert!(!q.quad.default_light);
            let want = oracle_out(q.light[usize::from(key.lane)], lighting);
            assert_eq!(value, want, "real executor must match oracle");
            let counted = counted::evaluate_with_config(
                q.light[usize::from(key.lane)],
                lighting.material,
                lighting.light,
                lighting.projection,
                4096,
                counted::Config::architecture(),
            )
            .unwrap();
            assert_eq!(value, counted.output, "real executor must match counted");
            if value != (light::LightingOutput { g: 256, h: 0 }) {
                proof.non_default += 1;
                seen.insert((value.g, value.h));
            }
            proof.returned += 1;
        }
        if let Some((key, id)) = cycle.light_issued {
            let source = source_for_serial[key.ticket.serial as usize];
            assert_eq!(id, u32::from(key.ticket.quad) << 2 | u32::from(key.lane));
            assert_eq!(key.lane, (id & 3) as u8);
            assert_eq!(key.ticket.quad, (id >> 2) as u8);
            assert!(quads[source].quad.header.mask >> key.lane & 1 != 0);
            proof.issued += 1;
        }
        if !ce && live.in_flight() > 0 {
            assert!(cycle.light_returned.is_none(), "CE must freeze returns");
            proof.ce_frozen += 1;
        }
        proof.max_in_flight = proof.max_in_flight.max(live.in_flight());
        if live.complete() {
            assert_eq!(next, quads.len());
            compare(&memory.bytes, &expected);
            assert!(memory.idle());
            assert_eq!(proof.issued, proof.returned);
            assert_eq!(proof.light_done, proof.returned);
            assert_eq!(proof.retired, nonempty(quads));
            assert_eq!(proof.light_done, light_lanes(quads));
            assert!(proof.reused > 0, "expected an actual slot reuse");
            proof.distinct = seen.len() as u64;
            return (memory.bytes, proof);
        }
    }
    panic!("lighting-live replay did not finish");
}
fn compare(dut: &[u8], golden: &[u8]) {
    if dut != golden {
        let i = dut.iter().zip(golden).position(|(a, b)| a != b).unwrap();
        panic!(
            "framebuffer/guard mismatch at {i}: dut={} golden={}",
            dut[i], golden[i]
        );
    }
}
fn nonempty(quads: &[LiveQuad]) -> u64 {
    quads.iter().filter(|q| q.quad.header.mask != 0).count() as u64
}
fn light_lanes(quads: &[LiveQuad]) -> u64 {
    quads
        .iter()
        .filter(|q| !q.quad.default_light)
        .map(|q| u64::from(q.quad.header.mask.count_ones()))
        .sum()
}
#[test]
fn real_executor_drives_full_and_partial_coverage_through_final_to_framebuffer() {
    let pixel_opts = [
        (97u8, [17u8, 93, 203]),
        (255, [5, 250, 1]),
        (0, [200, 200, 0]),
    ];
    let lighting_opts = [
        ctx(8, [255; 3], [0, 0, 16384], 32, 224),
        ctx(4, [255; 3], [0, 0, 16384], 32, 224),
        ctx(16, [255; 3], [0, 0, 16384], 0, 256),
        ctx(12, [0; 3], [0, 0, 16384], 64, 192),
        ctx(8, [255; 3], [11585, 11585, 0], 256, 0),
        ctx(6, [255; 3], [0, -16384, 0], 32, 224),
    ];
    let quads = quads();
    let mut total_non_default = 0u64;
    for lighting in lighting_opts {
        for (alpha, specular) in pixel_opts {
            for pauses in [false, true] {
                let pixel_ctx = Context {
                    alpha,
                    specular,
                    ..support::context()
                };
                let (bytes, proof) = run(pixel_ctx, lighting, &quads, pauses);
                assert_eq!(bytes.len(), 24576);
                assert_eq!(proof.issued, light_lanes(&quads));
                assert_eq!(proof.issued, proof.returned);
                assert_eq!(proof.light_done, proof.returned);
                assert!(proof.max_in_flight <= 64);
                assert!(proof.read > 0 && proof.captured > 0 && proof.consumed > 0);
                if pauses {
                    assert!(proof.ce_frozen > 0, "CE pause must freeze a live return");
                }
                total_non_default += proof.non_default;
            }
        }
    }
    assert!(total_non_default > 0);
}
#[test]
fn unlit_and_ambient_still_run_the_executor_and_retire() {
    for lighting in [
        light::LightingContext {
            material: light::Material {
                unlit: true,
                ..Default::default()
            },
            ..ctx(8, [255; 3], [0, 0, 16384], 32, 224)
        },
        ctx(8, [255; 3], [0, 0, 16384], 200, 0),
    ] {
        let (_, proof) = run(support::context(), lighting, &quads(), false);
        assert_eq!(proof.issued, proof.returned);
        assert_eq!(proof.light_done, proof.returned);
    }
}
#[test]
fn real_executor_matches_reference_lighting_in_framebuffer_fixture() {
    // Same independent integer function as J1, but values now come from the
    // cycle executor instead of an injected host array.
    let lighting = ctx(8, [17, 93, 203], [0, 0, 16384], 32, 224);
    let quads = quads();
    let initial = support::image();
    let expected = support::golden(initial, &stimulus(&quads, lighting), support::context());
    let (bytes, proof) = run(support::context(), lighting, &quads, true);
    compare(&bytes, &expected);
    assert_eq!(proof.issued, proof.returned);
}
#[test]
fn per_cycle_evidence_for_request_store_done_final_and_release() {
    use std::fmt::Write;
    let lighting = ctx(8, [255; 3], [0, 0, 16384], 32, 224);
    let quads = quads();
    let mut memory = Fixture::new(support::image());
    let mut live = LightingLive::new(support::context(), lighting, 20_000).unwrap();
    let mut next = 0usize;
    let mut held_source: Option<usize> = None;
    let mut lines = String::new();
    for wall in 0..20_000u64 {
        let offered = held_source.or_else(|| (next < quads.len()).then_some(next));
        let cycle = live
            .step(
                LiveTick {
                    quad: offered.map(|i| quads[i]),
                    finish: next == quads.len(),
                    ..Default::default()
                },
                &mut memory,
            )
            .unwrap();
        if offered.is_some() {
            held_source = offered;
        }
        if cycle.model.quad_accepted {
            held_source = None;
            next += 1;
        }
        let mut event = String::new();
        if let Some(ticket) = cycle.model.ticket {
            let _ = write!(event, " admit=q{}s{}", ticket.quad, ticket.serial);
        }
        if let Some((key, id)) = cycle.light_issued {
            let _ = write!(event, " issue=id{}->q{}l{}", id, key.ticket.quad, key.lane);
        }
        if let Some((key, value, epoch)) = cycle.light_returned {
            let _ = write!(
                event,
                " return=q{}l{} g{}h{} e{}",
                key.ticket.quad, key.lane, value.g, value.h, epoch
            );
        }
        for e in &cycle.model.events {
            match e {
                Event::LightDone(key) => {
                    let _ = write!(event, " done=q{}l{}", key.ticket.quad, key.lane);
                }
                Event::ReadIssued { key, depth } => {
                    let _ = write!(
                        event,
                        " read=q{}l{} d{}",
                        key.ticket.quad,
                        key.lane,
                        u8::from(*depth)
                    );
                }
                Event::Captured { key, .. } => {
                    let _ = write!(event, " cap=q{}l{}", key.ticket.quad, key.lane);
                }
                Event::Consumed { key, .. } => {
                    let _ = write!(event, " cons=q{}l{}", key.ticket.quad, key.lane);
                }
                Event::Retired(ticket) => {
                    let _ = write!(event, " retire=q{}s{}", ticket.quad, ticket.serial);
                }
                _ => {}
            }
        }
        if !event.is_empty() {
            let _ = writeln!(
                lines,
                "w{wall} ce{} live{}{event}",
                u8::from(cycle.ce),
                cycle.model.snapshot.live
            );
        }
        if live.complete() {
            break;
        }
    }
    assert!(live.complete());
    println!("PER-CYCLE EVIDENCE\n{lines}");
    for kind in ["issue=", "return=", "done=", "read=", "retire="] {
        assert!(lines.contains(kind), "missing {kind} evidence");
    }
}

#[test]
fn non_default_results_are_not_a_constant_or_precomputed_default() {
    let lighting = ctx(8, [255; 3], [0, 0, 16384], 32, 224);
    let (_, proof) = run(support::context(), lighting, &quads(), false);
    assert!(
        proof.non_default > 0,
        "executor returned only the default g/h"
    );
    assert!(
        proof.distinct > 4,
        "expected several distinct lighting outputs, got {}",
        proof.distinct
    );
}
#[test]
fn held_return_survives_prolonged_ce_pause_without_loss_or_duplication() {
    let lighting = ctx(8, [255; 3], [0, 0, 16384], 32, 224);
    let quads = quads();
    let initial = support::image();
    let expected = support::golden(initial, &stimulus(&quads, lighting), support::context());
    let mut memory = Fixture::new(support::image());
    let mut live = LightingLive::new(support::context(), lighting, 100_000).unwrap();
    let mut next = 0usize;
    let mut held_source: Option<usize> = None;
    let mut returned = 0u64;
    let mut issued = 0u64;
    let mut offline_edges = 0u64;
    for wall in 0..100_000u64 {
        // Alternate a long CE-off window with an on window, after issuing.
        let ce = wall % 31 < 17;
        let offered = held_source.or_else(|| (next < quads.len()).then_some(next));
        let cycle = live
            .step(
                LiveTick {
                    ce,
                    final_ready: true,
                    light_ready: true,
                    quad: offered.map(|i| quads[i]),
                    sample: None,
                    finish: next == quads.len(),
                },
                &mut memory,
            )
            .unwrap();
        if let Some(i) = offered {
            held_source = Some(i);
        }
        if cycle.model.quad_accepted {
            held_source = None;
            next += 1;
        } else if !cycle.input_held {
            held_source = None;
        }
        if !ce {
            offline_edges += 1;
            assert!(cycle.light_returned.is_none());
            assert!(cycle.light_issued.is_none());
        }
        issued += u64::from(cycle.light_issued.is_some());
        returned += u64::from(cycle.light_returned.is_some());
        if live.complete() {
            assert_eq!(next, quads.len());
            assert_eq!(issued, returned);
            assert!(offline_edges > 0);
            compare(&memory.bytes, &expected);
            return;
        }
    }
    panic!("CE-paused lighting-live replay did not finish");
}

#[test]
fn unaccepted_input_is_not_cached_and_real_output_backpressure_recovers() {
    let lighting = ctx(8, [255; 3], [0, 0, 16384], 32, 224);
    let mut a = quads()[1];
    a.light[0].normal = [0, 0, 16384];
    let mut b = a;
    b.light[0].normal = [0, 0, 0];
    let mut live = LightingLive::new(support::context(), lighting, 20_000).unwrap();
    let mut memory = Fixture::new(support::image());
    let rejected = live
        .step(
            LiveTick {
                ce: false,
                quad: Some(a),
                ..Default::default()
            },
            &mut memory,
        )
        .unwrap();
    assert!(!rejected.model.quad_accepted);
    let admitted = live
        .step(
            LiveTick {
                quad: Some(b),
                ..Default::default()
            },
            &mut memory,
        )
        .unwrap();
    assert!(admitted.model.quad_accepted);
    let mut issued = 0;
    // Hold the actual result-store port closed well past execution latency,
    // independently of CE and final readiness.
    for _ in 0..live.latency() + 30 {
        let cycle = live
            .step(
                LiveTick {
                    light_ready: false,
                    finish: true,
                    ..Default::default()
                },
                &mut memory,
            )
            .unwrap();
        issued += usize::from(cycle.light_issued.is_some());
        assert!(cycle.light_returned.is_none());
        assert!(cycle
            .model
            .events
            .iter()
            .all(|e| !matches!(e, Event::LightDone(_))));
    }
    assert_eq!(issued, 1);
    assert_eq!(
        live.in_flight(),
        1,
        "real result remains owned while blocked"
    );
    let expected = support::golden(
        support::image(),
        &stimulus(&[b], lighting),
        support::context(),
    );
    let mut returned = 0;
    for _ in 0..10_000 {
        let cycle = live
            .step(
                LiveTick {
                    finish: true,
                    ..Default::default()
                },
                &mut memory,
            )
            .unwrap();
        if let Some((_, value, _)) = cycle.light_returned {
            assert_eq!(value, oracle_out(b.light[0], lighting));
            returned += 1;
        }
        if live.complete() {
            break;
        }
    }
    assert!(live.complete());
    assert_eq!(returned, 1);
    compare(&memory.bytes, &expected);
}

#[test]
fn abort_discards_local_lighting_and_reports_only_fault_drain() {
    let lighting = ctx(8, [255; 3], [0, 0, 16384], 32, 224);
    let mut live = LightingLive::new(support::context(), lighting, 10_000).unwrap();
    let mut memory = Fixture::new(support::image());
    let accepted = live
        .step(
            LiveTick {
                quad: Some(quads()[1]),
                ..Default::default()
            },
            &mut memory,
        )
        .unwrap();
    assert!(accepted.model.quad_accepted);
    for _ in 0..16 {
        live.step(LiveTick::default(), &mut memory).unwrap();
        if live.in_flight() > 0 {
            break;
        }
    }
    assert!(live.in_flight() > 0);
    live.abort();
    for _ in 0..128 {
        let cycle = live
            .step(
                LiveTick {
                    ce: false,
                    ..Default::default()
                },
                &mut memory,
            )
            .unwrap();
        assert!(cycle.light_issued.is_none() && cycle.light_returned.is_none());
        if live.drained() {
            break;
        }
    }
    assert!(live.drained());
    assert_eq!(live.in_flight(), 0);
    assert!(!live.complete(), "abort cannot report successful rendering");
    assert!(memory.idle());
}
