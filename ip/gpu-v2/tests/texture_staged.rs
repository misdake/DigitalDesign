#[path = "support/texture.rs"]
#[allow(dead_code)]
mod support;
use gpu_v2::texture::{
    ports::*,
    sim::{counted, oracle, staged},
};
use support::*;
#[test]
fn checked_lowering_keeps_dependencies_and_exposes_shared_lod_output() {
    use staged::binding::*;
    let mut q = input(9, Filter::Trilinear, [0.3723, 0.7917]);
    q.uv[1][0] += 0.0025;
    let p = staged::prepare(&q, &[slot(9, true)]).unwrap();
    let lane = &p.lanes[0];
    let frames = [
        ("derivative", &p.derivative.frame),
        ("lod", &p.lod.frame),
        ("coordinate", &lane.coordinate.frame),
        ("rows", &lane.rows.frame),
        ("columns", &lane.columns.frame),
        ("plane", &lane.planes[0].frame),
    ];
    for (name, f) in frames {
        let mut e = Evidence::build(f).unwrap();
        e.audit(f).unwrap();
        println!("{name}: events={} wiring={} equality={} primitive_dependency_cycles={} outputs={} work={:?}",
            f.events.len(), e.lowering.wiring_adds.len(), e.lowering.equalities.len(), e.cycles, e.output_bits, e.work);
        if name == "plane" {
            assert!(e.lowering.wiring_adds.len() >= 8);
            assert!(e.lowering.equalities.len() >= 6);
            let root = e.lowering.wiring_adds[0].result_event;
            e.times[root].ready += 1;
            assert!(e.audit(f).is_err());
        }
    }
    let cone = lod_shared_h_cone(&p.lod.frame).unwrap();
    assert!(
        matches!(cone.audit(&p.lod.frame), Err(audited::Fault::Audit(s)) if s == "logic cone internal value escapes")
    );
    let h = cone.absorbed_events[0];
    let hv = p.lod.frame.events[h].output.unwrap();
    let users: Vec<_> = p
        .lod
        .frame
        .events
        .iter()
        .filter(|e| e.inputs.contains(&hv))
        .map(|e| e.id)
        .collect();
    println!(
        "shared LOD: h_event={h}, shift_event={}, h_consumers={users:?}",
        cone.result_event
    );
    assert!(users.len() >= 2);
}
fn compare(q: &QuadInput, s: Slot) {
    let p = staged::prepare(q, &[s]).unwrap();
    let old = counted::prepare(q, &[s]).unwrap();
    let gold = oracle::prepare(q, &[s], Config::counted()).unwrap();
    assert_eq!(p.groups, old.groups);
    assert_eq!(
        p.groups,
        gold.pixels
            .iter()
            .flat_map(|p| p.groups.iter().cloned())
            .collect::<Vec<_>>()
    );
    let get = |name: &str| {
        p.lod
            .frame
            .outputs
            .iter()
            .find(|o| o.name == name)
            .unwrap()
            .raw
    };
    assert_eq!(get("lod"), i128::from(gold.lod.raw));
    assert_eq!(get("lambda"), i128::from(old.lambda));
    for (group, word) in p.groups.iter().zip(&p.payloads) {
        assert_eq!(group.pack72().unwrap(), *word as u128);
    }
    for lane in &p.lanes {
        let g = gold.pixels.iter().find(|p| p.lane == lane.lane).unwrap();
        for layer in &g.layers {
            let which = usize::from(layer.n != g.layers[0].n);
            for axis in 0..2 {
                let coord = lane
                    .coordinate
                    .frame
                    .outputs
                    .iter()
                    .find(|o| o.name == format!("q{which}.{axis}"))
                    .unwrap();
                assert_eq!(
                    coord.raw,
                    i128::from(layer.fraction[axis]) + i128::from(layer.integer[axis]) * 256
                );
            }
            for t in 0..4 {
                let w = lane
                    .columns
                    .frame
                    .outputs
                    .iter()
                    .find(|o| o.name == format!("w{which}.{t}"))
                    .unwrap();
                assert_eq!(w.raw, i128::from(layer.coefficients[t]));
            }
        }
    }
}
#[test]
fn closed_stage_payloads_match_counted_and_oracle() {
    let mut random = 0x637a_0921_u32;
    for n in 0..=10 {
        for mip in [false, true] {
            for filter in [Filter::Nearest, Filter::Bilinear, Filter::Trilinear] {
                for case in 0..16 {
                    let mut q = input(n, filter, [0.0; 2]);
                    q.mask = case;
                    q.quad_id = case;
                    for uv in &mut q.uv {
                        for v in uv {
                            random = random.wrapping_mul(1664525).wrapping_add(1013904223);
                            *v = f64::from(random as i32) / 2147483648.0;
                        }
                    }
                    if case % 3 == 0 {
                        q.uv = [[0.0; 2]; 4];
                        q.uv[3][0] = 1.0 / 262144.0;
                    }
                    q.lod_bias = [-32.0, -2.0, 0.0, 0.5, 1.75, 8.5, 9.5, 32.0][case as usize % 8];
                    compare(&q, slot(n, mip));
                }
            }
        }
    }
    for uv in [
        0.0,
        -1.0 / 262144.0,
        1.0 - 1.0 / 262144.0,
        1048576.0,
        -1048576.0,
    ] {
        compare(&input(10, Filter::Bilinear, [uv; 2]), slot(10, true));
    }
}
#[test]
fn single_boundary_wrap_matches_general_modulo_at_texture_edges() {
    for n in 0..=10 {
        for mip in [false, true] {
            for filter in [Filter::Nearest, Filter::Bilinear, Filter::Trilinear] {
                for raw in [
                    -262145, -262144, -1, 0, 1, 127, 128, 129, 262015, 262016, 262017, 262143,
                    262144,
                ] {
                    let mut q = input(n, filter, [f64::from(raw) / 262144.0; 2]);
                    q.mask = 1;
                    q.lod_bias = 0.5;
                    compare(&q, slot(n, mip));
                }
            }
        }
    }
}
#[test]
fn short_normalization_is_exact_at_lod_grid_and_halfway_boundaries() {
    for h in 0..=19 {
        for k in 0..=64 {
            for delta in [-1.0 / 262144.0, 0.0, 1.0 / 262144.0] {
                let mut q = input(9, Filter::Trilinear, [0.13, 0.31]);
                let rho = 2_f64.powi(h - 18) * (1.0 + f64::from(k) / 64.0 + 1.0 / 128.0);
                q.uv[1][0] += rho + delta;
                q.lod_bias = -2.5;
                compare(&q, slot(9, true));
            }
        }
    }
}

#[test]
fn fixed_phase_control_preserves_contexts_and_packets_under_backpressure() {
    use staged::stream::*;
    let mut inputs = vec![];
    for i in 0..48 {
        let mut q = input(
            9,
            [Filter::Nearest, Filter::Bilinear, Filter::Trilinear][i % 3],
            [0.0; 2],
        );
        q.quad_id = (i % 16) as u8;
        q.mask = [15, 9, 6, 1, 0][i % 5];
        q.uv[3][0] = 1.0 / 262144.0;
        q.lod_bias = [0.0, 9.5][i % 2];
        inputs.push(q);
    }
    for release_after_capture in [false, true] {
        for contexts in [1, 2, 4] {
            for work_credits in [2, 8] {
                let report = run(
                    &inputs,
                    &[slot(9, true)],
                    Hardware {
                        contexts,
                        work_credits,
                        release_after_capture,
                        ..Default::default()
                    },
                    |c| (c % 11 >= 3, c > 400 && c % 17 < 5),
                )
                .unwrap();
                assert_eq!(report.stats.accepted, 48);
                assert_eq!(report.stats.released, 48);
                assert!(
                    report.stats.peak_contexts <= contexts
                        && report.stats.peak_work <= work_credits
                );
                assert!(report.stats.output_stalls > 0);
                for pair in report.steps.windows(2) {
                    if !pair[1].ce {
                        assert_eq!(pair[0].snapshot, pair[1].snapshot);
                        assert!(pair[1].events.is_empty());
                    }
                }
            }
        }
    }
}

#[test]
fn fixed_coefficient_slots_accept_mixed_paths_and_prove_all_products() {
    use staged::stream::*;
    let mut inputs = vec![];
    for i in 0..32 {
        let mut q = input(
            9,
            [Filter::Nearest, Filter::Bilinear, Filter::Trilinear][i % 3],
            [i as f64 / 65.0, 0.213],
        );
        q.quad_id = (i % 16) as u8;
        q.uv[1][0] += 1.0 / 512.0;
        q.lod_bias = 0.5;
        inputs.push(q);
    }
    let mut report = run(&inputs, &[slot(9, true)], Hardware::default(), |c| {
        (true, c % 7 != 0)
    })
    .unwrap();
    let at = report
        .steps
        .iter()
        .position(|s| s.events.iter().any(|e| matches!(e, Event::Packet { .. })))
        .unwrap();
    let saved = report.steps[at].clone();
    report.steps[at].ready = false;
    assert!(report.audit().is_err());
    report.steps[at] = saved;
    report.stats.peak_work += 1;
    assert!(report.audit().is_err());
}
#[test]
fn detached_records_allow_shared_slot_reuse_without_payload_aliasing() {
    use staged::stream::*;
    let inputs: Vec<_> = (0..64)
        .map(|i| {
            let mut q = input(9, Filter::Bilinear, [0.003 + f64::from(i) / 512.0, 0.003]);
            q.quad_id = (i % 16) as u8;
            q.uv[1][0] += 1.0 / 512.0;
            q
        })
        .collect();
    let late = run(&inputs, &[slot(9, true)], Hardware::default(), |_| {
        (true, true)
    })
    .unwrap();
    let mut early = run(
        &inputs,
        &[slot(9, true)],
        Hardware {
            release_after_capture: true,
            ..Default::default()
        },
        |c| (c % 13 != 0, c > 200 && c % 19 >= 4),
    )
    .unwrap();
    assert_eq!(early.payloads, late.payloads);
    assert!(early.stats.peak_live_quads > early.hardware.contexts);
    let first_shared = early
        .steps
        .iter()
        .position(|s| {
            s.events
                .iter()
                .any(|e| matches!(e, Event::SharedRelease { program: 0, .. }))
        })
        .unwrap();
    let final_packet = early
        .steps
        .iter()
        .position(|s| {
            s.events
                .iter()
                .any(|e| matches!(e, Event::Release { program: 0, .. }))
        })
        .unwrap();
    assert!(first_shared < final_packet);
    assert!(early.steps[first_shared..final_packet]
        .iter()
        .flat_map(|s| &s.events)
        .any(|e| matches!(e, Event::Accepted { program: 4, .. })));
    let at = early
        .steps
        .iter()
        .position(|s| {
            s.events
                .iter()
                .any(|e| matches!(e, Event::PlaneCapture { .. }))
        })
        .unwrap();
    let saved = early.steps[at].clone();
    for e in &mut early.steps[at].events {
        if let Event::PlaneCapture { data, .. } = e {
            data.weights[0] += 1;
            break;
        }
    }
    assert!(early.audit().is_err());
    early.steps[at] = saved;
    early.audit().unwrap();
}
