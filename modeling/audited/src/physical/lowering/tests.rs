use super::*;
use crate::{Fixed, Model};

fn packing<const SHIFT: u32>(a: i128, b: i128) -> FrameReport {
    let mut model = Model::numerical();
    let input = model.input::<8, 0, false>("fields", &[a, b]).unwrap();
    let f = model.compute("packing", 64).unwrap();
    let a = f.read(input.at::<0>()).unwrap();
    let b = f.read(input.at::<1>()).unwrap();
    let b: Fixed<16, 0, false> = f.resize_exact(b).unwrap();
    let b = f.shift_left_const::<SHIFT, 16, 0, false>(b).unwrap();
    let out: Fixed<16, 0, false> = f.add(a, b).unwrap();
    f.publish("out", out).unwrap();
    f.finish()
}
fn add_event(frame: &FrameReport) -> usize {
    frame
        .events
        .iter()
        .find(|e| e.operation == Operation::Add)
        .unwrap()
        .id
}

#[test]
fn wiring_uses_type_masks_and_rejects_lucky_no_carry_sample() {
    for (a, b) in [(1, 1), (128, 1), (0, 0)] {
        let frame = packing::<7>(a, b);
        frame.audit().unwrap();
        assert!(WiringAdd::prove(&frame, add_event(&frame)).is_err());
    }
    for (a, b) in [(0, 0), (1, 1), (255, 255)] {
        let frame = packing::<8>(a, b);
        let proof = WiringAdd::prove(&frame, add_event(&frame)).unwrap();
        assert_eq!(proof.possible_ones, [0xff, 0xff00]);
        let plan = Plan {
            wiring_adds: vec![proof.clone()],
            ..Default::default()
        };
        assert_eq!(plan.counts(&frame).unwrap().get(&Resource::Adder(16)), None);
        assert_eq!(frame.counts.resources[&Resource::Adder(16)], 1);
        let mut forged = proof;
        forged.possible_ones[0] = 0;
        assert!(forged.audit(&frame).is_err());
        let mut overlap = plan.clone();
        overlap.wiring_adds.extend(plan.wiring_adds.clone());
        assert!(overlap.resources(&frame).is_err());
    }
}

#[test]
fn wiring_handles_slices_literals_signed_extension_and_result_overflow() {
    let mut model = Model::numerical();
    let input = model.input::<16, 0, false>("bits", &[0xabcd]).unwrap();
    let f = model.compute("slices", 64).unwrap();
    let word = f.read(input.at::<0>()).unwrap();
    let lo: Fixed<8, 0, false> = f.slice::<8, 0, false, 0>(word).unwrap();
    let hi: Fixed<8, 0, false> = f.slice::<8, 0, false, 8>(word).unwrap();
    let hi: Fixed<16, 0, false> = f.resize_exact(hi).unwrap();
    let hi = f.shift_left_const::<8, 16, 0, false>(hi).unwrap();
    let out: Fixed<16, 0, false> = f.add(lo, hi).unwrap();
    f.publish("out", out).unwrap();
    let frame = f.finish();
    WiringAdd::prove(&frame, add_event(&frame)).unwrap();

    for b in [-128, -1, 0, 127] {
        let mut model = Model::numerical();
        let lo = model.input::<8, 0, false>("low", &[255]).unwrap();
        let hi = model.input::<8, 0, true>("signed high", &[b]).unwrap();
        let f = model.compute("signed packing", 64).unwrap();
        let lo = f.read(lo.at::<0>()).unwrap();
        let hi = f.read(hi.at::<0>()).unwrap();
        let hi: Fixed<16, 0, true> = f.resize_exact(hi).unwrap();
        let hi = f.shift_left_const::<8, 16, 0, true>(hi).unwrap();
        let out: Fixed<16, 0, true> = f.add(lo, hi).unwrap();
        f.publish("out", out).unwrap();
        let frame = f.finish();
        assert_eq!(
            WiringAdd::prove(&frame, add_event(&frame))
                .unwrap()
                .possible_ones,
            [0xff, 0xff00]
        );
    }
    // Disjoint fields do not prove a signed 8-bit result can hold unsigned 255.
    let mut model = Model::numerical();
    let input = model.input::<8, 0, false>("unsigned", &[1]).unwrap();
    let f = model.compute("unsafe result narrowing", 32).unwrap();
    let a = f.read(input.at::<0>()).unwrap();
    let out: Fixed<8, 0, true> = f.add(a, Fixed::<8, 0, false>::constant::<0>()).unwrap();
    f.publish("out", out).unwrap();
    let frame = f.finish();
    assert!(WiringAdd::prove(&frame, add_event(&frame)).is_err());

    // Literal zeros are genuine structural zeros; changing scale cannot bypass frame audit.
    let mut model = Model::numerical();
    let input = model.input::<8, 3, false>("fractional", &[17]).unwrap();
    let f = model.compute("literal zero", 32).unwrap();
    let a = f.read(input.at::<0>()).unwrap();
    let out: Fixed<8, 3, false> = f.add(a, Fixed::<8, 3, false>::constant::<0>()).unwrap();
    f.publish("out", out).unwrap();
    let mut frame = f.finish();
    let root = add_event(&frame);
    WiringAdd::prove(&frame, root).unwrap();
    frame.values[frame.events[root].inputs[0]].format.fraction = 2;
    assert!(WiringAdd::prove(&frame, root).is_err());
}

fn equality(a: i128, b: i128, keep_internal: bool, literal_zero: bool) -> FrameReport {
    let mut model = Model::numerical();
    let input = model.input::<8, 2, true>("compare", &[a, b]).unwrap();
    let f = model.compute("equality", 64).unwrap();
    let a = f.read(input.at::<0>()).unwrap();
    let b = if literal_zero {
        Fixed::<8, 2, true>::constant::<0>()
    } else {
        f.read(input.at::<1>()).unwrap()
    };
    let lt = f.less(a, b).unwrap();
    let gt = f.less(b, a).unwrap();
    if keep_internal {
        f.publish("lt", lt).unwrap();
    }
    let sum: Fixed<18, 0, true> = f.add(lt, gt).unwrap();
    let eq = f.less(sum, Fixed::<18, 0, true>::constant::<1>()).unwrap();
    f.publish("eq", eq).unwrap();
    f.finish()
}
fn root(frame: &FrameReport) -> usize {
    frame.values[frame.outputs.last().unwrap().value].producer
}

#[test]
fn equality_replays_three_relations_and_charges_one_comparator() {
    for (a, b) in [(-128, 127), (127, -128), (-1, -1), (0, 0)] {
        let frame = equality(a, b, false, false);
        assert_eq!(frame.outputs[0].raw, i128::from(a == b));
        let proof = Equality::prove(&frame, root(&frame)).unwrap();
        assert_eq!(proof.kind, EqualityKind::Bits);
        assert_eq!(proof.width, 8);
        let plan = Plan {
            equalities: vec![proof.clone()],
            ..Default::default()
        };
        assert_eq!(
            plan.counts(&frame).unwrap().get(&Resource::Compare(8)),
            Some(&1)
        );
        assert!(!plan
            .counts(&frame)
            .unwrap()
            .keys()
            .any(|r| matches!(r, Resource::Adder(_))));
        assert_eq!(frame.counts.resources[&Resource::Compare(8)], 2);
        let cones = plan.logic_cones(&frame, 1).unwrap();
        assert_eq!(cones.len(), 1);
        cones[0].audit(&frame).unwrap();
        let mut times = vec![Timing { issue: 0, ready: 0 }; frame.events.len()];
        times[proof.result_event] = Timing { issue: 0, ready: 1 };
        for id in proof.absorbed_events {
            times[id] = Timing { issue: 1, ready: 1 };
        }
        for e in &frame.events {
            if matches!(e.operation, Operation::Publish(_)) {
                times[e.id] = Timing { issue: 1, ready: 1 };
            }
        }
        plan.audit_timing(&frame, &times, 1, 32).unwrap();
        let mut forged = proof;
        forged.width = 1;
        assert!(forged.audit(&frame).is_err());
    }
    for a in [-128, -1, 0, 127] {
        let frame = equality(a, 0, false, true);
        assert_eq!(frame.outputs[0].raw, i128::from(a == 0));
        assert_eq!(
            Equality::prove(&frame, root(&frame)).unwrap().kind,
            EqualityKind::ReduceNor
        );
    }
    let frame = equality(0, 0, true, false);
    assert!(Equality::prove(&frame, root(&frame)).is_err());
}

#[test]
fn equality_rejects_mixed_types_and_non_opposite_comparisons() {
    let mut model = Model::numerical();
    let input = model.input::<8, 0, false>("mixed", &[1, 1]).unwrap();
    let f = model.compute("mixed signedness", 64).unwrap();
    let a = f.read(input.at::<0>()).unwrap();
    let b = f.read(input.at::<1>()).unwrap();
    let b: Fixed<8, 0, true> = f.resize_exact(b).unwrap();
    let lt = f.less(a, b).unwrap();
    let gt = f.less(b, a).unwrap();
    let sum: Fixed<2, 0, false> = f.add(lt, gt).unwrap();
    let eq = f.less(sum, Fixed::<2, 0, false>::constant::<1>()).unwrap();
    f.publish("eq", eq).unwrap();
    let frame = f.finish();
    assert!(Equality::prove(&frame, root(&frame)).is_err());
    let mut model = Model::numerical();
    let input = model
        .input::<8, 0, false>("independent", &[1, 1, 1])
        .unwrap();
    let f = model.compute("not opposite", 64).unwrap();
    let a = f.read(input.at::<0>()).unwrap();
    let b = f.read(input.at::<1>()).unwrap();
    let c = f.read(input.at::<2>()).unwrap();
    let lt = f.less(a, b).unwrap();
    let gt = f.less(c, a).unwrap();
    let sum: Fixed<2, 0, false> = f.add(lt, gt).unwrap();
    let eq = f.less(sum, Fixed::<2, 0, false>::constant::<1>()).unwrap();
    f.publish("eq", eq).unwrap();
    let frame = f.finish();
    assert!(Equality::prove(&frame, root(&frame)).is_err());
}

#[test]
fn lowering_rejects_dynamic_shift_samples_invalid_frames_and_wrong_threshold() {
    let mut model = Model::numerical();
    let low = model.input::<8, 0, false>("low", &[1]).unwrap();
    let high = model.input::<16, 0, false>("high", &[1]).unwrap();
    let f = model
        .compute("dynamic shift is not static wiring", 64)
        .unwrap();
    let a = f.read(low.at::<0>()).unwrap();
    let b = f.read(high.at::<0>()).unwrap();
    let b = f.shift(b, Fixed::<18, 0, true>::constant::<8>()).unwrap();
    let out: Fixed<16, 0, false> = f.add(a, b).unwrap();
    f.publish("out", out).unwrap();
    let frame = f.finish();
    assert!(WiringAdd::prove(&frame, add_event(&frame)).is_err());

    let mut frame = packing::<8>(1, 1);
    frame.valid = false;
    frame.faults.push(Fault::Range);
    frame.audit().unwrap();
    assert!(WiringAdd::prove(&frame, add_event(&frame)).is_err());

    let mut model = Model::numerical();
    let input = model.input::<8, 0, false>("compare", &[1, 1]).unwrap();
    let f = model.compute("wrong constant threshold", 64).unwrap();
    let a = f.read(input.at::<0>()).unwrap();
    let b = f.read(input.at::<1>()).unwrap();
    let lt = f.less(a, b).unwrap();
    let gt = f.less(b, a).unwrap();
    let sum: Fixed<2, 0, false> = f.add(lt, gt).unwrap();
    let out = f.less(sum, Fixed::<2, 0, false>::constant::<2>()).unwrap();
    f.publish("out", out).unwrap();
    let frame = f.finish();
    assert!(Equality::prove(&frame, root(&frame)).is_err());
}
