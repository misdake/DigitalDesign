use super::*;
use audited::{lifecycle, Model};

fn fixture() -> (FrameReport, LogicCone, Vec<Timing>, Vec<Field>) {
    let mut m = Model::numerical();
    let input = m.input::<8, 0, false>("xy", &[7, 11]).unwrap();
    let f = m.compute("multioutput-register-test", 64).unwrap();
    let x = f.read(input.at::<0>()).unwrap();
    let y = f.read(input.at::<1>()).unwrap();
    let sum = f.add::<9, 0, false>(x, y).unwrap();
    let low = f.slice::<8, 0, false, 0>(sum).unwrap();
    let twice = f.add::<10, 0, false>(sum, sum).unwrap();
    let later = f.add::<9, 0, false>(low, y).unwrap();
    f.publish("low", low).unwrap();
    f.publish("twice", twice).unwrap();
    f.publish("later", later).unwrap();
    let frame = f.finish();
    let event = |op| frame.events.iter().find(|e| e.operation == op).unwrap();
    let sum = event(Operation::Add).id;
    let low = event(Operation::Slice(0)).id;
    let adds: Vec<_> = frame
        .events
        .iter()
        .filter(|e| e.operation == Operation::Add)
        .collect();
    let twice = adds[1].id;
    let later = adds[2].id;
    let cone = LogicCone {
        result_event: twice,
        absorbed_events: vec![sum, low],
        exported_events: vec![sum, low],
        operands: frame.events[sum].inputs.clone(),
        max_width: 10,
        latency: 1,
    };
    cone.audit(&frame).unwrap();
    let mut times = vec![Timing { issue: 0, ready: 0 }; frame.events.len()];
    times[twice] = Timing { issue: 0, ready: 1 };
    for id in [sum, low] {
        times[id] = Timing { issue: 1, ready: 1 };
    }
    times[later] = Timing { issue: 1, ready: 2 };
    for e in &frame.events {
        if matches!(e.operation, Operation::Publish(_)) {
            let ready = times[frame.values[e.inputs[0]].producer].ready;
            times[e.id] = Timing {
                issue: ready,
                ready,
            };
        }
    }
    let value = |id: usize| frame.events[id].output.unwrap();
    let field = |value, width, birth, last_read, low| Field {
        value,
        source_low: 0,
        width,
        birth,
        last_read,
        lows: vec![low],
    };
    let fields = vec![
        field(value(sum), 8, 1, 2, 0),
        field(value(twice), 10, 1, 2, 8),
        field(frame.events[sum].inputs[1], 8, 0, 1, 18),
        field(value(later), 9, 2, 2, 26),
        field(frame.events[sum].inputs[0], 8, 0, 0, 35),
    ];
    (frame, cone, times, fields)
}

fn calendar(
    frame: &FrameReport,
    cone: &LogicCone,
    times: &[Timing],
    fields: Vec<Field>,
) -> Result<Calendar, String> {
    Calendar::from_structure(
        frame,
        times,
        std::slice::from_ref(cone),
        &[],
        fields,
        43,
        2,
        2,
        2,
        &["low", "twice", "later"],
    )
}

#[test]
fn fusion_exported_alias_retains_source_and_uses_old_external_operand_through_ce() {
    let (frame, cone, times, fields) = fixture();
    let life = lifecycle::analyze_composed_policy(
        &frame,
        &times,
        &[],
        std::slice::from_ref(&cone),
        &lifecycle::LifetimePolicy {
            commit_cycle: 2,
            period: None,
            max_cycle: 32,
            invariant_inputs: vec![],
            retained_outputs: None,
        },
    )
    .unwrap();
    let sum_value = frame.events[cone.absorbed_events[0]].output.unwrap();
    let alias_value = frame.events[cone.absorbed_events[1]].output.unwrap();
    let interval = life
        .intervals
        .iter()
        .find(|v| v.value == sum_value)
        .unwrap();
    assert_eq!((interval.start, interval.end), (1, 3));
    assert!(!life.intervals.iter().any(|v| v.value == alias_value));
    let cal = calendar(&frame, &cone, &times, fields).unwrap();
    let mut r = Registers::new(cal);
    let cases = [(250_i128, 10_i128), (1, 3), (199, 77), (0, 255)];
    let mut sent = 0;
    let mut received = 0;
    let mut expected = std::collections::VecDeque::new();
    for wall in 0..64 {
        let ce = wall % 5 != 2 && wall % 5 != 3;
        let before = (r.bank.clone(), r.phase, r.valid, r.output().unwrap());
        if ce {
            if let Some(out) = before.3.as_ref() {
                let (x, y) = expected.pop_front().unwrap();
                assert_eq!(scalar(out, "low"), (x + y) & 255);
                assert_eq!(scalar(out, "twice"), 2 * (x + y));
                assert_eq!(scalar(out, "later"), ((x + y) & 255) + y);
                received += 1;
            }
        }
        let input = if ce && r.phase().is_multiple_of(2) && sent < cases.len() {
            let (x, y) = cases[sent];
            sent += 1;
            expected.push_back((x, y));
            Some(vec![("xy", 0, x), ("xy", 1, y)])
        } else {
            None
        };
        r.tick(ce, input.as_deref()).unwrap();
        if !ce {
            assert_eq!(
                (r.bank.clone(), r.phase, r.valid, r.output().unwrap()),
                before
            );
        }
        if received == cases.len() {
            break;
        }
    }
    assert_eq!((sent, received), (cases.len(), cases.len()));
}

#[test]
fn fusion_constructor_rejects_operand_export_shape_and_timing_corruption() {
    let (frame, cone, times, fields) = fixture();
    let mut bad = cone.clone();
    bad.exported_events.clear();
    assert!(calendar(&frame, &bad, &times, fields.clone()).is_err());
    let mut bad = cone.clone();
    bad.operands.pop();
    assert!(calendar(&frame, &bad, &times, fields.clone()).is_err());
    let mut bad = cone.clone();
    bad.exported_events.push(usize::MAX);
    assert!(calendar(&frame, &bad, &times, fields.clone()).is_err());
    let mut bad = cone.clone();
    bad.absorbed_events.push(bad.result_event);
    assert!(calendar(&frame, &bad, &times, fields.clone()).is_err());
    let mut bad_times = times.clone();
    bad_times[cone.absorbed_events[0]].ready += 1;
    assert!(calendar(&frame, &cone, &bad_times, fields.clone()).is_err());
    let mut bad_frame = frame.clone();
    bad_frame.events[cone.result_event].operation = Operation::Read { memory: 0, row: 0 };
    assert!(calendar(&bad_frame, &cone, &times, fields.clone()).is_err());
    let mut missing = fields;
    missing.retain(|f| f.value != frame.events[cone.absorbed_events[0]].output.unwrap());
    assert!(calendar(&frame, &cone, &times, missing).is_err());
}

#[test]
fn fusion_constructor_rejects_early_consumer_in_another_cone() {
    let (frame, cone, mut times, mut fields) = fixture();
    let later = frame
        .outputs
        .iter()
        .find(|o| o.name == "later")
        .unwrap()
        .value;
    let consumer = frame.values[later].producer;
    let next = LogicCone::singleton(&frame, consumer, 1).unwrap();
    times[consumer] = Timing { issue: 0, ready: 1 };
    fields.iter_mut().find(|f| f.value == later).unwrap().birth = 1;
    let cones = [cone, next];
    assert!(audited::physical::audit_logic_dependencies(&frame, &times, &cones, 32).is_err());
    assert!(Calendar::from_structure(
        &frame,
        &times,
        &cones,
        &[],
        fields,
        43,
        2,
        2,
        2,
        &["low", "twice", "later"]
    )
    .is_err());
}

#[test]
fn fusion_exported_alias_cannot_hide_its_absorbed_storage_source() {
    let (frame, mut cone, times, fields) = fixture();
    // Keep only the exported low-byte view; its full sum producer is hidden.
    // A certificate may not silently turn that live byte into no storage.
    cone.exported_events.remove(0);
    assert!(lifecycle::analyze_composed_policy(
        &frame,
        &times,
        &[],
        std::slice::from_ref(&cone),
        &lifecycle::LifetimePolicy {
            commit_cycle: 2,
            period: None,
            max_cycle: 32,
            invariant_inputs: vec![],
            retained_outputs: None
        }
    )
    .is_err());
    assert!(calendar(&frame, &cone, &times, fields).is_err());
}
