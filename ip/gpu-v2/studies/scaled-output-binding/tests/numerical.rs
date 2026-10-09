use audited::{Fault, Operation};
use gpu_v2_scaled_output_binding::{binding, prepare, reference, Spec, SPECS};
use resource_scheduler::{Graph, Node, Resource};
use std::collections::BTreeSet;

fn compare(spec: Spec, value: i128, exponent: i128) {
    let expected = match reference::run(spec, value, exponent) {
        Ok((result, frame)) => {
            if result.is_ok() {
                frame.audit().unwrap();
            }
            result
        }
        Err(fault) => Err(fault),
    };
    let actual = prepare(spec, value, exponent).and_then(|w| w.finish());
    assert_eq!(actual, expected, "{spec:?}, v={value}, e={exponent}");
    if let Ok(raw) = actual {
        // Independent rational scaling and Euclidean ties-even oracle. No jam,
        // recover, low-bit select or intermediate graph is reused here.
        let scale = i128::from(spec.fraction - spec.out_fraction) + exponent;
        let exact = if scale <= 0 {
            value * (1_i128 << (-scale) as u32)
        } else {
            let denominator = 1_i128 << scale as u32;
            let q = value.div_euclid(denominator);
            let r = value.rem_euclid(denominator);
            q + i128::from(r > denominator / 2 || r == denominator / 2 && q % 2 != 0)
        };
        assert_eq!(raw, exact);
    }
}

#[test]
fn every_actual_format_signed_shifts_ties_sticky_and_range() {
    let mut exponents: BTreeSet<i128> = (-40..=32).collect();
    exponents.extend([
        -131072, -127, -126, -125, -74, -73, -72, -71, -70, -64, 33, 126, 127, 131071,
    ]);
    let mut seed = 0x1234_5678_9abc_def0_u64;
    let mut checked = 0;
    for spec in SPECS {
        for &e in &exponents {
            let mut values = vec![
                -(1_i128 << 71),
                (1_i128 << 71) - 1,
                (1_i128 << 71) - 2,
                -3,
                -2,
                -1,
                0,
                1,
                2,
                3,
            ];
            let drop = i128::from(spec.fraction - spec.out_fraction) + e;
            if (1..=69).contains(&drop) {
                let half = 1_i128 << (drop - 1) as u32;
                for floor in [-3_i128, -2, -1, 0, 1, 2] {
                    for tail in [0, half - 1, half, half + 1, (half << 1) - 1] {
                        let v = (floor << drop as u32) + tail;
                        if (-(1_i128 << 71)..(1_i128 << 71)).contains(&v) {
                            values.push(v);
                        }
                    }
                }
            }
            for _ in 0..24 {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                values.push((i128::from(seed) << 7) - (1_i128 << 70));
            }
            for v in values {
                compare(spec, v, e);
                checked += 1;
            }
        }
    }
    assert!(
        checked > 25_000 && checked < 60_000,
        "finite cases {checked}"
    );
    println!("independent/reference/window checked cases: {checked}");
}

#[test]
fn small_signed_domain_exhaustion() {
    let mut checked = 0;
    for spec in SPECS {
        for e in [-8, -1, 0, 1, 8, 31] {
            for v in -256..=256 {
                compare(spec, v, e);
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 24_624);
}

#[test]
fn eager_unselected_overflow_and_earlier_narrowing_are_preserved() {
    let spec = SPECS[3];
    // e=0 has no sticky/jam, but the eagerly computed signed72 +1 rejects
    // max72 before any final narrowing can run. Fault stage also matters.
    assert_eq!(prepare(spec, (1_i128 << 71) - 1, 0), Err(Fault::Range));
    compare(spec, (1_i128 << 71) - 1, 0);
    let (_, failed) = reference::run(spec, (1_i128 << 71) - 1, 0).unwrap();
    assert!(!failed
        .events
        .iter()
        .any(|e| matches!(e.operation, Operation::RescaleFloor(_))));
    for (v, e) in [
        (1, -126),
        (0, -131072),
        (0, -127),
        (0, 32),
        (-(1_i128 << 71), -1),
    ] {
        compare(spec, v, e);
    }
    compare(SPECS[6], -(513_i128 << 19), 0);
    assert!(prepare(SPECS[6], -(513_i128 << 19), 0).is_ok()); // final output failure occurs in finish.
    assert_eq!(
        prepare(SPECS[6], -(513_i128 << 19), 0).unwrap().finish(),
        Err(Fault::Range)
    );
    compare(SPECS[6], -(1024_i128 << 19) - 1, 0); // intermediate floor itself is outside signed11.
    assert_eq!(
        prepare(SPECS[6], -(1024_i128 << 19) - 1, 0),
        Err(Fault::Range)
    );
    compare(spec, 1_i128 << 71, 0); // input-store width failure
    let invalid = Spec {
        fraction: 28,
        bits: 18,
        out_fraction: 28,
    };
    assert_eq!(prepare(invalid, 0, 0), Err(Fault::Format));
    println!("eager and staged range negative examples passed");
}

fn graph(frame: &audited::FrameReport) -> Graph {
    use audited::Resource as R;
    let declarations = [
        ("shift72", 2),
        ("round72", 1),
        ("small-control", 1),
        ("add36", 1),
        ("add54", 1),
        ("add73", 2),
        ("compare72", 1),
        ("select72", 1),
        ("select36", 1),
        ("pool-port-a", 2),
    ];
    let resources = declarations
        .into_iter()
        .map(|(name, latency)| Resource {
            name: name.into(),
            latency,
            lanes: 1,
            initiation_interval: 1,
        })
        .collect::<Vec<_>>();
    let nodes = frame
        .events
        .iter()
        .map(|event| {
            let name = match event.resource {
                Some(R::Shift(_)) => Some("shift72"),
                Some(R::RoundControl(_)) => Some("round72"),
                Some(R::Adder(w)) => Some(if w <= 18 {
                    "small-control"
                } else if w <= 36 {
                    "add36"
                } else if w <= 54 {
                    "add54"
                } else {
                    "add73"
                }),
                Some(R::Compare(w)) => Some(if w <= 18 {
                    "small-control"
                } else {
                    "compare72"
                }),
                Some(R::Select(w)) => Some(if w <= 18 {
                    "small-control"
                } else if w <= 36 {
                    "select36"
                } else {
                    "select72"
                }),
                Some(R::Read(_)) => Some("pool-port-a"),
                None => None,
                other => panic!("unexpected {other:?}"),
            };
            let mut deps = event
                .inputs
                .iter()
                .map(|&v| frame.values[v].producer)
                .collect::<BTreeSet<_>>();
            deps.extend(event.control);
            Node {
                name: event.id.to_string(),
                predecessors: deps.into_iter().collect(),
                earliest: 0,
                resource: name.map(|n| resources.iter().position(|r| r.name == n).unwrap()),
            }
        })
        .collect();
    Graph { resources, nodes }
}

#[test]
fn closed_binding_keeps_final_observation_and_rejects_internal_escapes() {
    for spec in SPECS {
        let (_, frame) = reference::run(spec, 123_456, 1).unwrap();
        let found = binding::groups(&frame).unwrap();
        assert_eq!(found.len(), 1);
        let baseline = graph(&frame);
        let result = binding::compare(&frame, &baseline).unwrap();
        assert_eq!(baseline.resources, result.graph.resources);
        assert!(result.candidate.resource_issues < result.baseline.resource_issues);
        assert!(result.candidate.cycles < result.baseline.cycles);
        let internal = frame
            .events
            .iter()
            .find(|e| e.operation == Operation::Shift)
            .unwrap()
            .output
            .unwrap();
        let mut observed = frame.clone();
        observed.events.last_mut().unwrap().inputs = vec![internal];
        observed.outputs[0] = audited::Observation {
            name: "result".into(),
            value: internal,
            format: frame.values[internal].format,
            raw: frame.values[internal].raw,
        };
        observed.audit().unwrap();
        assert!(binding::groups(&observed).unwrap_err().contains("consumer"));
        // Another valid result observes an existing internal node through Publish.
        let mut consumed = frame.clone();
        let mut publish = consumed.events.last().unwrap().clone();
        publish.id = consumed.events.len();
        publish.operation = Operation::Publish("extra-consumer".into());
        publish.inputs = vec![internal];
        consumed.events.push(publish);
        *consumed.counts.operations.get_mut("publish").unwrap() += 1;
        consumed.outputs.push(audited::Observation {
            name: "extra-consumer".into(),
            value: internal,
            format: frame.values[internal].format,
            raw: frame.values[internal].raw,
        });
        consumed.audit().unwrap();
        assert!(binding::groups(&consumed).unwrap_err().contains("consumer"));

        // An ordinary arithmetic consumer also escapes the closed region,
        // even when no observation publishes the extra value.
        let mut used = frame.clone();
        let mut consumer = used.events.last().unwrap().clone();
        consumer.id = used.events.len();
        consumer.operation = Operation::Resize;
        consumer.inputs = vec![internal];
        consumer.output = Some(used.values.len());
        let mut value = used.values[internal].clone();
        value.producer = consumer.id;
        value.ready_cycle = consumer.ready_cycle;
        used.values.push(value);
        used.events.push(consumer);
        *used.counts.operations.get_mut("resize").unwrap() += 1;
        used.audit().unwrap();
        assert!(binding::groups(&used).unwrap_err().contains("consumer"));
    }
}

#[test]
fn auditable_fault_frames_cannot_be_bound_or_scheduled() {
    for (value, exponent) in [((1_i128 << 71) - 1, 0), (0, 32), (0, -127)] {
        let (result, frame) = reference::run(SPECS[3], value, exponent).unwrap();
        assert!(result.is_err());
        assert!(!frame.valid && !frame.faults.is_empty());
        frame.audit().unwrap();
        assert_eq!(
            binding::groups(&frame).unwrap_err(),
            "failed numerical frame"
        );
        match binding::compare(&frame, &graph(&frame)) {
            Err(error) => assert_eq!(error, "failed numerical frame"),
            Ok(_) => panic!("a failed expression received a schedule"),
        }
    }
}
