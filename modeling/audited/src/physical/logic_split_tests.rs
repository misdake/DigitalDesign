use super::*;
use crate::{lifecycle, Fixed, Model};

fn lod(slope: i128) -> FrameReport {
    let mut m = Model::numerical();
    let input = m.input::<20, 0, false>("safe slope", &[slope]).unwrap();
    let f = m.compute("shared LOD", 64).unwrap();
    let zeros = f.leading_zeros(f.read(input.at::<0>()).unwrap()).unwrap();
    let h = f
        .sub_same(Fixed::<18, 0, true>::constant::<19>(), zeros)
        .unwrap();
    let shift = f
        .sub_same(Fixed::<18, 0, true>::constant::<19>(), h)
        .unwrap();
    let exponent = f
        .sub_same(h, Fixed::<18, 0, true>::constant::<18>())
        .unwrap();
    f.publish("h", h).unwrap();
    f.publish("shift", shift).unwrap();
    f.publish("exponent", exponent).unwrap();
    f.finish()
}

fn output(frame: &FrameReport, name: &str) -> usize {
    frame.outputs.iter().find(|o| o.name == name).unwrap().value
}

fn singleton(frame: &FrameReport, event: usize) -> LogicCone {
    LogicCone::singleton(frame, event, 1).unwrap()
}

fn times(frame: &FrameReport, cones: &[LogicCone]) -> Vec<Timing> {
    times_with_resources(
        frame,
        cones,
        &frame.events.iter().map(|e| e.resource).collect::<Vec<_>>(),
    )
}

fn times_with_resources(
    frame: &FrameReport,
    cones: &[LogicCone],
    resources: &[Option<crate::Resource>],
) -> Vec<Timing> {
    let deps = logic_dependencies(frame, cones).unwrap();
    let mut times = vec![Timing { issue: 0, ready: 0 }; frame.events.len()];
    for e in &frame.events {
        let issue = deps[e.id]
            .iter()
            .map(|&id| times[id].ready)
            .max()
            .unwrap_or(0);
        times[e.id] = Timing {
            issue,
            ready: issue + u64::from(resources[e.id].is_some()),
        };
    }
    times
}

#[test]
fn shared_lod_is_legally_split_without_absorbing_either_output() {
    for slope in [1, 2, 255, 524288] {
        let frame = lod(slope);
        frame.audit().unwrap();
        let h = frame.values[output(&frame, "h")].producer;
        let shift = frame.values[output(&frame, "shift")].producer;
        let mut hidden = singleton(&frame, shift);
        hidden.absorbed_events = vec![h];
        hidden.operands = frame.events[h]
            .inputs
            .iter()
            .chain(frame.events[shift].inputs.iter())
            .copied()
            .filter(|&v| frame.values[v].producer != h)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        assert!(
            matches!(hidden.audit(&frame), Err(crate::Fault::Audit(s)) if s == "logic cone internal value escapes")
        );
        let cones = [singleton(&frame, h), singleton(&frame, shift)];
        let times = times(&frame, &cones);
        audit_logic_dependencies(&frame, &times, &cones, 64).unwrap();
        let clz = frame.values[frame.events[h].inputs[1]].producer;
        assert_eq!(times[h].ready, times[clz].ready + 1);
        assert_eq!(times[shift].ready, times[clz].ready + 2);
        assert_eq!(frame.counts.resources[&crate::Resource::Adder(18)], 3);
        assert_eq!(
            lowering::Plan::default().counts(&frame).unwrap(),
            frame.counts.resources
        );
        let live = lifecycle::analyze_composed_policy(
            &frame,
            &times,
            &[],
            &cones,
            &lifecycle::LifetimePolicy {
                commit_cycle: 8,
                period: None,
                max_cycle: 64,
                invariant_inputs: vec![],
                retained_outputs: None,
            },
        )
        .unwrap();
        for name in ["h", "shift"] {
            let value = output(&frame, name);
            let interval = live.intervals.iter().find(|i| i.value == value).unwrap();
            assert_eq!(interval.bits, 18);
            assert_eq!(interval.start, times[frame.values[value].producer].ready);
            assert_eq!(interval.end, 9);
        }
        let zeros = 20 - (128 - (slope as u128).leading_zeros());
        assert_eq!(frame.outputs[0].raw, 19 - i128::from(zeros));
        assert_eq!(frame.outputs[1].raw, i128::from(zeros));
    }
}

#[test]
fn split_rejects_missing_boundaries_overlap_and_shortened_paths() {
    let frame = lod(255);
    let h = frame.values[output(&frame, "h")].producer;
    let shift = frame.values[output(&frame, "shift")].producer;
    let cones = [singleton(&frame, h), singleton(&frame, shift)];
    let correct = times(&frame, &cones);
    let mut missing = cones[1].clone();
    missing.operands.clear();
    assert!(missing.audit(&frame).is_err());
    let mut narrow = cones[0].clone();
    narrow.max_width = 17;
    assert!(narrow.audit(&frame).is_err());
    assert!(logic_dependencies(&frame, &[cones[0].clone(), cones[0].clone()]).is_err());
    let mut early = correct.clone();
    early[shift].issue -= 1;
    early[shift].ready -= 1;
    assert!(audit_logic_dependencies(&frame, &early, &cones, 64).is_err());
    let mut free = correct;
    free[h].ready = free[h].issue;
    assert!(audit_logic_dependencies(&frame, &free, &cones, 64).is_err());
    let mut cyclic = frame;
    cyclic.events[h].inputs[1] = output(&cyclic, "shift");
    assert!(cones[0].audit(&cyclic).is_err());
}

#[test]
fn singleton_keeps_external_control_and_rejects_uncharged_wiring() {
    let mut m = Model::numerical();
    let input = m.input::<18, 0, true>("input", &[7]).unwrap();
    let f = m.compute("controlled logic", 64).unwrap();
    let a = f.read(input.at::<0>()).unwrap();
    let predicate = f.less(a, Fixed::<18, 0, true>::constant::<8>()).unwrap();
    f.branch(
        predicate,
        |f| {
            let h = f.sub_same(Fixed::<18, 0, true>::constant::<19>(), a)?;
            f.publish("h", h)?;
            Ok(())
        },
        |_| Ok(()),
    )
    .unwrap();
    let frame = f.finish();
    let root = frame.values[output(&frame, "h")].producer;
    let cone = singleton(&frame, root);
    let gate = frame.events[root].control.unwrap();
    let deps = logic_dependencies(&frame, std::slice::from_ref(&cone)).unwrap();
    assert!(deps[root].contains(&gate));
    let mut timing = times(&frame, std::slice::from_ref(&cone));
    audit_logic_dependencies(&frame, &timing, std::slice::from_ref(&cone), 64).unwrap();
    timing[gate].ready = timing[root].issue + 1;
    assert!(audit_logic_dependencies(&frame, &timing, &[cone], 64).is_err());
    assert!(LogicCone::singleton(&frame, root, 0).is_err());
    assert!(LogicCone::singleton(&frame, usize::MAX, 1).is_err());

    let mut m = Model::numerical();
    let input = m.input::<8, 0, false>("wire", &[7]).unwrap();
    let f = m.compute("wire", 64).unwrap();
    let a: Fixed<18, 0, false> = f.resize_exact(f.read(input.at::<0>()).unwrap()).unwrap();
    f.publish("a", a).unwrap();
    let frame = f.finish();
    let root = frame.values[output(&frame, "a")].producer;
    assert!(LogicCone::singleton(&frame, root, 1).is_err());
}

#[test]
fn singleton_cannot_hide_memory_dsp_or_invalid_numerical_work() {
    let mut m = Model::numerical();
    let input = m.input::<8, 0, true>("input", &[3]).unwrap();
    let rom = m
        .table("rom", &[Fixed::<8, 0, true>::constant::<4>()])
        .unwrap();
    let ram = m.scratch::<8, 0, true>("ram", 1).unwrap();
    let f = m.compute("forbidden singleton operations", 64).unwrap();
    let a = f.read(input.at::<0>()).unwrap();
    let b = f.read(rom.at::<0>()).unwrap();
    f.write(ram.at::<0>(), a).unwrap();
    let c = f.read(ram.at::<0>()).unwrap();
    let product: Fixed<16, 0, true> = f.mul(a, b, crate::ProductRoute::Native18).unwrap();
    f.publish("p", product).unwrap();
    f.publish("ram", c).unwrap();
    let frame = f.finish();
    frame.audit().unwrap();
    for e in &frame.events {
        if matches!(
            e.operation,
            Operation::Read { .. }
                | Operation::Write { .. }
                | Operation::Multiply
                | Operation::ProductMapping { .. }
        ) {
            assert!(LogicCone::singleton(&frame, e.id, 1).is_err());
        }
    }
    let mut invalid = lod(1);
    let h = invalid.values[output(&invalid, "h")].producer;
    let proof = singleton(&invalid, h);
    invalid.valid = false;
    invalid.faults.push(crate::Fault::Range);
    invalid.audit().unwrap();
    assert!(proof.audit(&invalid).is_err());
}

#[test]
fn split_composes_with_zero_time_wiring_without_removing_real_adders() {
    let mut m = Model::numerical();
    let input = m.input::<8, 0, false>("fields", &[3, 4]).unwrap();
    let f = m.compute("packing and shared LOD", 64).unwrap();
    let a = f.read(input.at::<0>()).unwrap();
    let b: Fixed<16, 0, false> = f.resize_exact(f.read(input.at::<1>()).unwrap()).unwrap();
    let b = f.shift_left_const::<8, 16, 0, false>(b).unwrap();
    let packed: Fixed<16, 0, false> = f.add(a, b).unwrap();
    let zeros = f.leading_zeros(packed).unwrap();
    let h = f
        .sub_same(Fixed::<18, 0, true>::constant::<19>(), zeros)
        .unwrap();
    let shift = f
        .sub_same(Fixed::<18, 0, true>::constant::<19>(), h)
        .unwrap();
    f.publish("h", h).unwrap();
    f.publish("shift", shift).unwrap();
    let frame = f.finish();
    let packing = frame
        .events
        .iter()
        .find(|e| e.operation == Operation::Add)
        .unwrap()
        .id;
    let plan = lowering::Plan {
        wiring_adds: vec![lowering::WiringAdd::prove(&frame, packing).unwrap()],
        ..Default::default()
    };
    let h = frame.values[output(&frame, "h")].producer;
    let shift = frame.values[output(&frame, "shift")].producer;
    let cones = [singleton(&frame, h), singleton(&frame, shift)];
    let timing = times_with_resources(&frame, &cones, &plan.resources(&frame).unwrap());
    plan.audit_timing(&frame, &timing, 1, 64).unwrap();
    audit_logic_dependencies(&frame, &timing, &cones, 64).unwrap();
    assert_eq!(timing[packing].issue, timing[packing].ready);
    assert_eq!(timing[shift].ready, timing[h].ready + 1);
    assert_eq!(plan.counts(&frame).unwrap()[&crate::Resource::Adder(18)], 2);
    assert!(!plan
        .counts(&frame)
        .unwrap()
        .contains_key(&crate::Resource::Adder(16)));
}
