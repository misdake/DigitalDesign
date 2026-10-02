//! Public framework acceptance, scoped to the GPU v2 development workspace.
use audited::{lifecycle, physical::*, Fixed, FrameReport, Model, Operation};

const MAX_EVENTS: usize = 64;
const MAX_CYCLE: u64 = 32;

fn clamp(a: i128, escape: bool) -> (FrameReport, LogicCone) {
    let mut m = Model::numerical();
    let input = m.input::<8, 0, true>("input", &[a, 3]).unwrap();
    let f = m.compute("bounded clamp cone", MAX_EVENTS).unwrap();
    let a = f.read(input.at::<0>()).unwrap();
    let b = f.read(input.at::<1>()).unwrap();
    let sum = f.add::<9, 0, true>(a, b).unwrap();
    let zero = Fixed::<9, 0, true>::constant::<0>();
    let negative = f.less(sum, zero).unwrap();
    let selected = f.select(negative, zero, sum).unwrap();
    let out = f.resize_exact::<8, 0, true>(selected).unwrap();
    f.publish("out", out).unwrap();
    if escape {
        f.publish("internal", sum).unwrap();
    }
    let report = f.finish();
    let root = report
        .events
        .iter()
        .find(|e| matches!(e.operation, Operation::Resize))
        .unwrap()
        .id;
    let absorbed: Vec<_> = report
        .events
        .iter()
        .filter(|e| {
            matches!(
                e.operation,
                Operation::Add | Operation::Less | Operation::Select
            )
        })
        .map(|e| e.id)
        .collect();
    let mut operands: Vec<_> = report
        .events
        .iter()
        .filter(|e| matches!(e.operation, Operation::Read { .. } | Operation::Literal))
        .filter_map(|e| e.output)
        .collect();
    operands.sort_unstable();
    operands.dedup();
    (
        report,
        LogicCone {
            result_event: root,
            absorbed_events: absorbed,
            operands,
            max_width: 9,
            latency: 2,
        },
    )
}

fn times(frame: &FrameReport, cone: &LogicCone) -> Vec<Timing> {
    frame
        .events
        .iter()
        .map(|e| {
            if e.id == cone.result_event {
                Timing { issue: 1, ready: 3 }
            } else if cone.absorbed_events.contains(&e.id) {
                Timing { issue: 3, ready: 3 }
            } else if matches!(e.operation, Operation::Publish(_)) {
                Timing { issue: 4, ready: 4 }
            } else {
                Timing { issue: 0, ready: 0 }
            }
        })
        .collect()
}

#[test]
fn cone_keeps_semantics_dependencies_and_wiring_root_storage() {
    for (a, expected) in [(-10, 0), (-3, 0), (0, 3), (80, 83)] {
        let (frame, cone) = clamp(a, false);
        frame.audit().unwrap();
        assert_eq!(frame.outputs[0].raw, expected);
        let t = times(&frame, &cone);
        audit_logic_dependencies(&frame, &t, std::slice::from_ref(&cone), MAX_CYCLE).unwrap();
        let deps = logic_dependencies(&frame, std::slice::from_ref(&cone)).unwrap();
        assert_eq!(deps[cone.result_event].len(), 4); // Two reads and two literal occurrences.
        for &id in &cone.absorbed_events {
            assert_eq!(deps[id], [cone.result_event]);
        }
        let live = lifecycle::analyze_composed_policy(
            &frame,
            &t,
            &[],
            std::slice::from_ref(&cone),
            &lifecycle::LifetimePolicy {
                commit_cycle: 5,
                period: None,
                max_cycle: MAX_CYCLE,
                invariant_inputs: vec![],
                retained_outputs: None,
            },
        )
        .unwrap();
        assert_eq!(live.intervals.len(), 3); // Two input rows and the actual 8-bit result.
        let result = frame.events[cone.result_event].output.unwrap();
        let interval = live.intervals.iter().find(|i| i.value == result).unwrap();
        assert_eq!((interval.bits, interval.start, interval.end), (8, 3, 6));
        MemoryLayout {
            banks: vec![],
            placements: vec![],
        }
        .audit_periodic_composed_accesses(&frame, &t, &[], &[cone], &[], 1, MAX_CYCLE)
        .unwrap();
    }
}

#[test]
fn cone_rejects_escape_missing_operands_overlap_width_and_latency_tampering() {
    let (escaped, cone) = clamp(5, true);
    assert!(cone.audit(&escaped).is_err());
    let (frame, cone) = clamp(5, false);
    let mut bad = cone.clone();
    bad.operands.pop();
    assert!(bad.audit(&frame).is_err());
    bad = cone.clone();
    bad.max_width = 8;
    assert!(bad.audit(&frame).is_err());
    bad = cone.clone();
    bad.absorbed_events.push(bad.result_event);
    assert!(bad.audit(&frame).is_err());
    bad = cone.clone();
    bad.absorbed_events.push(0);
    assert!(bad.audit(&frame).is_err()); // Memory cannot be swallowed.
    assert!(composed_dependencies(&frame, &[], &[cone.clone(), cone.clone()]).is_err());
    let mut t = times(&frame, &cone);
    t[cone.result_event].ready = 2;
    assert!(audit_logic_dependencies(&frame, &t, std::slice::from_ref(&cone), MAX_CYCLE).is_err());
    t = times(&frame, &cone);
    t[cone.absorbed_events[0]].ready = 4;
    assert!(audit_logic_dependencies(&frame, &t, std::slice::from_ref(&cone), MAX_CYCLE).is_err());
    t = times(&frame, &cone);
    t[0].ready = 2;
    assert!(audit_logic_dependencies(&frame, &t, &[cone], MAX_CYCLE).is_err());
}

#[test]
fn cone_rejects_disconnected_pure_work_and_multiplication() {
    let mut m = Model::numerical();
    let input = m.input::<8, 0, true>("input", &[3]).unwrap();
    let f = m.compute("closure", MAX_EVENTS).unwrap();
    let a = f.read(input.at::<0>()).unwrap();
    let b = f.add_same(a, a).unwrap();
    let _unused = f.add_same(a, a).unwrap();
    let c = f.add_same(b, a).unwrap();
    let product = f.product::<16, 0, true>(a, a).unwrap();
    f.publish("out", c).unwrap();
    f.publish("product", product).unwrap();
    let frame = f.finish();
    let adds: Vec<_> = frame
        .events
        .iter()
        .filter(|e| matches!(e.operation, Operation::Add))
        .map(|e| e.id)
        .collect();
    let operand = frame.events[0].output.unwrap();
    let mut cone = LogicCone {
        result_event: adds[2],
        absorbed_events: vec![adds[0], adds[1]],
        operands: vec![operand],
        max_width: 8,
        latency: 1,
    };
    assert!(cone.audit(&frame).is_err());
    cone.absorbed_events.pop();
    cone.audit(&frame).unwrap();
    cone.result_event = frame
        .events
        .iter()
        .find(|e| matches!(e.operation, Operation::Multiply))
        .unwrap()
        .id;
    assert!(cone.audit(&frame).is_err());
}

#[test]
fn cone_preserves_external_control_and_its_retained_predicate() {
    let mut m = Model::numerical();
    let input = m.input::<8, 0, true>("input", &[3]).unwrap();
    let f = m.compute("controlled cone", MAX_EVENTS).unwrap();
    let a = f.read(input.at::<0>()).unwrap();
    let p = f.less(a, Fixed::<8, 0, true>::constant::<7>()).unwrap();
    let result = f
        .branch_value(
            p,
            |f| {
                let b = f.add_same(a, a)?;
                f.add_same(b, a)
            },
            |f| f.sub_same(a, a),
        )
        .unwrap();
    f.publish("out", result).unwrap();
    let frame = f.finish();
    let root = frame
        .events
        .iter()
        .find(|e| matches!(e.operation, Operation::Resize))
        .unwrap()
        .id;
    let members: Vec<_> = frame
        .events
        .iter()
        .filter(|e| matches!(e.operation, Operation::Add))
        .map(|e| e.id)
        .collect();
    let cone = LogicCone {
        result_event: root,
        absorbed_events: members,
        operands: vec![frame.events[0].output.unwrap()],
        max_width: 8,
        latency: 2,
    };
    let gate = frame.events[root].control.unwrap();
    let mut t = times(&frame, &cone);
    t[gate] = Timing { issue: 1, ready: 1 };
    t[frame.values[frame.events[gate].inputs[0]].producer] = Timing { issue: 0, ready: 1 };
    audit_logic_dependencies(&frame, &t, std::slice::from_ref(&cone), MAX_CYCLE).unwrap();
    assert!(logic_dependencies(&frame, std::slice::from_ref(&cone)).unwrap()[root].contains(&gate));
    let live = lifecycle::analyze_composed_policy(
        &frame,
        &t,
        &[],
        std::slice::from_ref(&cone),
        &lifecycle::LifetimePolicy {
            commit_cycle: 4,
            period: None,
            max_cycle: MAX_CYCLE,
            invariant_inputs: vec![],
            retained_outputs: None,
        },
    )
    .unwrap();
    let predicate = frame.events[gate].inputs[0];
    assert!(live
        .intervals
        .iter()
        .any(|i| i.value == predicate && i.end >= 2));
    t[gate].ready = 2;
    assert!(audit_logic_dependencies(&frame, &t, &[cone], MAX_CYCLE).is_err());
}
