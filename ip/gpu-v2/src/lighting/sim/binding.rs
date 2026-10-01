//! Checked physical-planning lowering, separate from the counted numerical ledger.
use super::timed::{self, Binding, Hardware, LaneKind};
use audited::{FrameReport, Operation};
use std::collections::{BTreeMap, BTreeSet};

/// One MULTADDALU18X18 candidate: A0*B0 + A1*B1 + C, without rounding.
pub use audited::physical::FusedGroup;
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct BoundDag {
    pub kinds: Vec<Option<LaneKind>>,
    pub dependencies: Vec<Vec<usize>>,
    pub groups: Vec<FusedGroup>,
    pub cones: Vec<audited::physical::LogicCone>,
}
fn literal(f: &FrameReport, v: usize, raw: i128) -> bool {
    matches!(f.events[f.values[v].producer].operation, Operation::Literal) && f.values[v].raw == raw
}
fn widened_unsigned(f: &FrameReport, v: usize, source_bits: u32) -> bool {
    let e = &f.events[f.values[v].producer];
    matches!(e.operation, Operation::Resize)
        && e.inputs.len() == 1
        && f.values[e.inputs[0]].format.bits == source_bits
        && !f.values[e.inputs[0]].format.signed
        && !f.values[v].format.signed
        && f.values[v].format.fraction == 0
        && f.values[e.inputs[0]].format.fraction == 0
}
fn wiring_add(f: &FrameReport, id: usize) -> bool {
    let e = &f.events[id];
    if !matches!(e.operation, Operation::Add) || e.inputs.len() != 2 {
        return false;
    }
    let a = &f.events[f.values[e.inputs[0]].producer];
    let out = f.values[e.output.unwrap()].format;
    // (7-bit a << 1) + literal 1 == {a,1}; no carry crosses bit 0.
    if out.bits == 8
        && !out.signed
        && out.fraction == 0
        && matches!(a.operation, Operation::ShiftLeft(1))
        && widened_unsigned(f, a.inputs[0], 7)
        && literal(f, e.inputs[1], 1)
    {
        return true;
    }
    // For signed 8-bit a, (a<<1)+1 is signed 9-bit {a,1}, including a=-128.
    // The shift guarantees bit 0 is zero, so the literal cannot carry.
    if out.bits == 9
        && out.signed
        && out.fraction == 0
        && matches!(a.operation, Operation::ShiftLeft(1))
    {
        let resize = &f.events[f.values[a.inputs[0]].producer];
        if matches!(resize.operation, Operation::Resize) && resize.inputs.len() == 1 {
            let source = f.values[resize.inputs[0]].format;
            let widened = f.values[a.inputs[0]].format;
            if source.bits == 8
                && source.signed
                && source.fraction == 0
                && widened.bits == 9
                && widened.signed
                && widened.fraction == 0
                && literal(f, e.inputs[1], 1)
            {
                return true;
            }
        }
    }
    // A 1-bit parity at bit 6 plus a zero-extended 6-bit segment.
    out.bits == 7
        && !out.signed
        && out.fraction == 0
        && matches!(a.operation, Operation::ShiftLeft(6))
        && widened_unsigned(f, a.inputs[0], 1)
        && widened_unsigned(f, e.inputs[1], 6)
}
fn dot_group(f: &FrameReport, name: &str) -> Result<Option<FusedGroup>, String> {
    let Some(publish) = f
        .events
        .iter()
        .find(|e| matches!(&e.operation,Operation::Publish(n) if n==name))
    else {
        return Ok(None);
    };
    let result = f.values[publish.inputs[0]].producer;
    let root = &f.events[result];
    if matches!(root.operation,Operation::Read{memory,..} if f.memories[memory].name=="context.flat-nl")
    {
        return Ok(None);
    }
    if !matches!(root.operation, Operation::Add) || root.inputs.len() != 2 {
        return Err("dot tail pattern".into());
    }
    let xy = f.values[root.inputs[0]].producer;
    let sum = &f.events[xy];
    if !matches!(sum.operation, Operation::Add) || sum.inputs.len() != 2 {
        return Err("dot pair pattern".into());
    }
    let products = [
        f.values[sum.inputs[0]].producer,
        f.values[sum.inputs[1]].producer,
    ];
    let mut operands = Vec::new();
    for p in products {
        let mul = &f.events[p];
        if !matches!(mul.operation, Operation::Multiply)
            || mul.inputs.len() != 2
            || mul
                .inputs
                .iter()
                .any(|&v| f.values[v].format.bits > 18 || !f.values[v].format.signed)
            || f.values[mul.output.ok_or("product output")?]
                .format
                .fraction
                != 28
        {
            return Err("dot multiplier format".into());
        }
        operands.extend(&mul.inputs);
    }
    operands.push(root.inputs[1]);
    let absorbed = vec![products[0], products[1], xy];
    for &event in &absorbed {
        let value = f.events[event].output.ok_or("absorbed output")?;
        // No product/partial-sum output may escape the fused macro.
        if f.outputs.iter().any(|o| o.value == value)
            || f.events.iter().any(|e| {
                e.inputs.contains(&value)
                    && e.id != result
                    && !absorbed.contains(&e.id)
                    && !matches!(e.operation, Operation::ProductMapping { .. })
            })
        {
            return Err("fused intermediate escapes".into());
        }
    }
    let out = f.values[root.output.ok_or("dot output")?].format;
    let c = f.values[operands[4]].format;
    if !out.signed
        || out.fraction != 28
        || out.bits > 54
        || !c.signed
        || c.fraction != 28
        || c.bits > 54
    {
        return Err("dot ALU format".into());
    }
    let raw = |i: usize| f.values[operands[i]].raw;
    let expected = raw(0)
        .checked_mul(raw(1))
        .and_then(|a| raw(2).checked_mul(raw(3)).and_then(|b| a.checked_add(b)))
        .and_then(|ab| ab.checked_add(raw(4)))
        .ok_or("fused overflow")?;
    if expected != f.values[root.output.unwrap()].raw {
        return Err("fused arithmetic mismatch".into());
    }
    let group = FusedGroup {
        result_event: result,
        absorbed_events: absorbed,
        operands,
    };
    group.audit(f).map_err(|e| format!("fusion: {e:?}"))?;
    Ok(Some(group))
}
impl BoundDag {
    /// Rebuild primitive physical kinds and independently check every supplied
    /// cone certificate; scheduling audit does not trust the member search.
    pub(super) fn audit_logic_depth(&self, f: &FrameReport, h: Hardware) -> Result<(), String> {
        if self.cones.is_empty() {
            return Ok(());
        }
        let primitive = Self::new(f, Hardware { cone_depth: 0, ..h })?;
        for cone in &self.cones {
            cone.audit(f)
                .map_err(|e| format!("logic certificate: {e:?}"))?;
            let members = std::iter::once(cone.result_event)
                .chain(cone.absorbed_events.iter().copied())
                .collect();
            if longest_logic_path(f, &members, &primitive.kinds)? > h.cone_depth {
                return Err("logic certificate exceeds physical depth".into());
            }
        }
        Ok(())
    }
    pub fn new(f: &FrameReport, h: Hardware) -> Result<Self, String> {
        let mut kinds = (0..f.events.len())
            .map(|e| timed::kind(f, e))
            .collect::<Result<Vec<_>, _>>()?;
        let mut dependencies = (0..f.events.len())
            .map(|e| timed::dependencies(f, e))
            .collect::<Vec<_>>();
        let mut groups = Vec::new();
        if h.binding == Binding::LightingDsp {
            for e in &f.events {
                if wiring_add(f, e.id) {
                    kinds[e.id] = None;
                } else if h.kernel.dataflow
                    && matches!(e.operation, Operation::Sub)
                    && literal(f, e.inputs[0], 0)
                {
                    // Two's-complement negation is invert plus one rather than a
                    // two-variable adder; reserve a dedicated carry-chain lane.
                    kinds[e.id] = Some(LaneKind::Negate(18));
                } else if matches!(e.operation, Operation::Add)
                    && e.inputs.iter().any(|&v| {
                        matches!(
                            f.events[f.values[v].producer].operation,
                            Operation::RoundIncrement(_)
                        )
                    })
                {
                    let width = f.values[e.output.ok_or("round output")?].format.bits;
                    if width > 18 {
                        return Err("unexpected wide increment".into());
                    }
                    kinds[e.id] = Some(LaneKind::Increment(18));
                }
            }
            if h.paired_macros > 0 {
                for name in ["nl", "nh"] {
                    if let Some(g) = dot_group(f, name)? {
                        let mut deps: Vec<_> =
                            g.operands.iter().map(|&v| f.values[v].producer).collect();
                        for event in std::iter::once(&g.result_event).chain(&g.absorbed_events) {
                            deps.extend(f.events[*event].control);
                        }
                        deps.sort_unstable();
                        deps.dedup();
                        if deps
                            .iter()
                            .any(|d| g.absorbed_events.contains(d) || *d == g.result_event)
                        {
                            return Err("fused control cycle".into());
                        }
                        dependencies[g.result_event] = deps;
                        kinds[g.result_event] = Some(LaneKind::PairMultiplyAdd);
                        for &id in &g.absorbed_events {
                            kinds[id] = None;
                            dependencies[id] = vec![g.result_event];
                        }
                        groups.push(g);
                    }
                }
            }
        }
        let cones = if h.cone_depth > 0 {
            contract_logic(f, h, &mut kinds, &groups)?
        } else {
            Vec::new()
        };
        if !cones.is_empty() {
            dependencies = audited::physical::composed_dependencies(f, &groups, &cones)
                .map_err(|e| format!("logic dependencies: {e:?}"))?;
        }
        Ok(Self {
            cones,
            kinds,
            dependencies,
            groups,
        })
    }
}

fn pure(op: &Operation) -> bool {
    matches!(
        op,
        Operation::Add
            | Operation::Sub
            | Operation::Resize
            | Operation::ShiftLeft(_)
            | Operation::Shift
            | Operation::LeadingZeros
            | Operation::Slice(_)
            | Operation::RescaleFloor(_)
            | Operation::BinaryScale
            | Operation::Less
            | Operation::Select
            | Operation::RoundIncrement(_)
    )
}
/// Longest serial physical path in the selected numerical DAG. Ledger event IDs
/// are topological; reconvergent inputs take a maximum rather than first visit.
/// A proved wiring operation contributes zero, regardless of its ledger opcode.
fn longest_logic_path(
    f: &FrameReport,
    members: &BTreeSet<usize>,
    primitive_kinds: &[Option<LaneKind>],
) -> Result<usize, String> {
    let mut depths = BTreeMap::new();
    for &id in members {
        let mut input_depth = 0;
        for &value in &f.events[id].inputs {
            let producer = f.values[value].producer;
            if members.contains(&producer) {
                let depth = *depths.get(&producer).ok_or("non-topological logic cone")?;
                input_depth = input_depth.max(depth);
            }
        }
        depths.insert(id, input_depth + usize::from(primitive_kinds[id].is_some()));
    }
    Ok(depths.values().copied().max().unwrap_or(0))
}
fn contract_logic(
    f: &FrameReport,
    h: Hardware,
    kinds: &mut [Option<LaneKind>],
    groups: &[FusedGroup],
) -> Result<Vec<audited::physical::LogicCone>, String> {
    if h.cone_depth > 4
        || h.cone_latency == 0
        || h.cone_latency > 4
        || h.cone_lanes_per_shape == 0
        || h.cone_lanes_per_shape > 16
    {
        return Err("logic cone profile bounds".into());
    }
    let primitive_kinds = kinds.to_vec();
    let mut used = BTreeSet::new();
    for g in groups {
        used.insert(g.result_event);
        used.extend(g.absorbed_events.iter().copied());
    }
    let mut roots = Vec::new();
    // Retain all stage goldens; a golden may escape only as a cone root.
    for e in f.events.iter().rev() {
        if matches!(e.operation, Operation::Publish(_)) {
            roots.push(f.values[e.inputs[0]].producer);
        }
    }
    roots.extend((0..f.events.len()).rev());
    let mut cones = Vec::new();
    for root in roots {
        if used.contains(&root) || !pure(&f.events[root].operation) {
            continue;
        }
        let mut members = BTreeSet::new();
        // Reverse topological discovery visits every possible selected user
        // before its producer. A reconvergent producer accumulates the maximum
        // distance from the root before deciding whether it fits the bound.
        let mut pending = BTreeMap::from([(root, 0usize)]);
        while let Some((id, depth)) = pending.pop_last() {
            let e = &f.events[id];
            if depth > h.cone_depth
                || used.contains(&id)
                || !pure(&e.operation)
                || members.len() >= 64
            {
                continue;
            }
            let level = usize::from(e.resource.is_some() && kinds[id].is_some());
            if depth + level > h.cone_depth || !members.insert(id) {
                continue;
            }
            if depth + level <= h.cone_depth {
                for &value in &e.inputs {
                    let producer = f.values[value].producer;
                    pending
                        .entry(producer)
                        .and_modify(|distance| *distance = (*distance).max(depth + level))
                        .or_insert(depth + level);
                }
            }
        }
        loop {
            let escape: Vec<_> = members
                .iter()
                .copied()
                .filter(|&id| id != root)
                .filter(|&id| {
                    let value = f.events[id].output.unwrap();
                    f.outputs.iter().any(|o| o.value == value)
                        || f.events.iter().any(|e| {
                            !members.contains(&e.id)
                                && (e.inputs.contains(&value) || e.control == Some(id))
                        })
                })
                .collect();
            if escape.is_empty() {
                break;
            }
            for id in escape {
                members.remove(&id);
            }
        }
        if members.len() < 2 || members.iter().filter(|&&id| kinds[id].is_some()).count() < 2 {
            continue;
        }
        // Independently prove the completed subgraph after escape pruning.
        if longest_logic_path(f, &members, &primitive_kinds)? > h.cone_depth {
            continue;
        }
        let mut operands = BTreeSet::new();
        let mut max_width = 1;
        let external: BTreeSet<_> = members
            .iter()
            .flat_map(|&id| f.events[id].inputs.iter().copied())
            .filter(|&v| !members.contains(&f.values[v].producer))
            .collect();
        let external_order: BTreeMap<_, _> =
            external.iter().enumerate().map(|(i, &v)| (v, i)).collect();
        let mut shape = String::new();
        let order: BTreeMap<_, _> = members.iter().enumerate().map(|(i, &id)| (id, i)).collect();
        for &id in &members {
            let e = &f.events[id];
            let format = f.values[e.output.unwrap()].format;
            let mut signature = format!("{:?}:{format:?}:", e.operation);
            max_width = max_width.max(format.bits);
            for &v in &e.inputs {
                let producer = f.values[v].producer;
                let vf = f.values[v].format;
                max_width = max_width.max(vf.bits);
                if !members.contains(&producer) {
                    operands.insert(v);
                }
                if let Some(i) = order.get(&producer) {
                    signature.push_str(&format!("member{i}:{vf:?};"));
                } else if matches!(f.events[producer].operation, Operation::Literal) {
                    signature.push_str(&format!("constant{}:{vf:?};", f.values[v].raw));
                } else {
                    signature.push_str(&format!("external{}:{vf:?};", external_order[&v]));
                }
            }
            shape.push_str(&signature);
            shape.push('|');
        }
        let cone = audited::physical::LogicCone {
            result_event: root,
            absorbed_events: members.iter().copied().filter(|&id| id != root).collect(),
            operands: operands.into_iter().collect(),
            max_width,
            latency: h.cone_latency,
        };
        cone.audit(f)
            .map_err(|e| format!("contracted logic: {e:?}"))?;
        for &id in &cone.absorbed_events {
            kinds[id] = None;
        }
        kinds[root] = Some(LaneKind::LogicCone {
            shape,
            width: max_width,
        });
        used.extend(members);
        cones.push(cone);
    }
    Ok(cones)
}

#[cfg(test)]
mod tests {
    use super::*;
    use audited::{Fixed, Model};
    #[test]
    fn reconvergent_cone_depth_uses_the_longest_path_and_zero_cost_wiring() {
        for short_first in [true, false] {
            let mut model = Model::numerical();
            let input = model.input::<16, 0, false>("input", &[1]).unwrap();
            let f = model.compute("reconvergent depth", 128).unwrap();
            let x = f.read(input.at::<0>()).unwrap();
            let shared = f.add_same(x, x).unwrap();
            let arm1 = f.add_same(shared, x).unwrap();
            let arm2 = f.add_same(arm1, x).unwrap();
            let arm3 = f.add_same(arm2, x).unwrap();
            let wire: Fixed<17, 0, false> = f.resize_exact(arm3).unwrap();
            // LIFO DFS visits the last operand first. The short-first case used
            // to absorb all five adders into a declared four-level cone.
            let root: Fixed<18, 0, false> = if short_first {
                f.add(wire, shared).unwrap()
            } else {
                f.add(shared, wire).unwrap()
            };
            f.publish("result", root).unwrap();
            let frame = f.finish();
            frame.audit().unwrap();
            let primitive = BoundDag::new(&frame, Hardware::default()).unwrap();
            let all_logic = frame
                .events
                .iter()
                .filter(|e| pure(&e.operation))
                .map(|e| e.id)
                .collect::<BTreeSet<_>>();
            assert_eq!(all_logic.len(), 6);
            assert_eq!(
                longest_logic_path(&frame, &all_logic, &primitive.kinds).unwrap(),
                5
            );
            let hardware = Hardware {
                cone_depth: 4,
                ..Hardware::default()
            };
            let bound = BoundDag::new(&frame, hardware).unwrap();
            // Independently recurse over every emitted certificate. There is
            // no visited-set shortcut, so reconvergence counts each full path.
            fn depth(
                f: &FrameReport,
                id: usize,
                members: &BTreeSet<usize>,
                kinds: &[Option<LaneKind>],
            ) -> usize {
                f.events[id]
                    .inputs
                    .iter()
                    .map(|&v| f.values[v].producer)
                    .filter(|p| members.contains(p))
                    .map(|p| depth(f, p, members, kinds))
                    .max()
                    .unwrap_or(0)
                    + usize::from(kinds[id].is_some())
            }
            assert!(!bound.cones.is_empty());
            for cone in &bound.cones {
                let members = std::iter::once(cone.result_event)
                    .chain(cone.absorbed_events.iter().copied())
                    .collect();
                assert!(depth(&frame, cone.result_event, &members, &primitive.kinds) <= 4);
            }
            bound.audit_logic_depth(&frame, hardware).unwrap();
            let root = frame.values[frame.outputs[0].value].producer;
            let operands = all_logic
                .iter()
                .flat_map(|&id| frame.events[id].inputs.iter().copied())
                .filter(|&v| !all_logic.contains(&frame.values[v].producer))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            let too_deep = audited::physical::LogicCone {
                result_event: root,
                absorbed_events: all_logic.iter().copied().filter(|&id| id != root).collect(),
                operands,
                max_width: 18,
                latency: 1,
            };
            too_deep.audit(&frame).unwrap(); // Numerical subgraph is still valid.
            let mut forged = bound;
            forged.cones = vec![too_deep];
            assert!(forged
                .audit_logic_depth(&frame, hardware)
                .unwrap_err()
                .contains("depth"));
        }
    }
    #[test]
    fn valid_numerical_frame_with_observed_product_cannot_be_fused() {
        let mut model = Model::numerical();
        let input = model
            .input::<16, 14, true>("dot.inputs", &[16384, -16384, 8192, 16384, -8192, 4096])
            .unwrap();
        let f = model.compute("dot with visible product", 128).unwrap();
        let p: [Fixed<32, 28, true>; 3] = [
            f.product(
                f.read(input.at::<0>()).unwrap(),
                f.read(input.at::<1>()).unwrap(),
            )
            .unwrap(),
            f.product(
                f.read(input.at::<2>()).unwrap(),
                f.read(input.at::<3>()).unwrap(),
            )
            .unwrap(),
            f.product(
                f.read(input.at::<4>()).unwrap(),
                f.read(input.at::<5>()).unwrap(),
            )
            .unwrap(),
        ];
        let xy: Fixed<34, 28, true> = f.add(p[0], p[1]).unwrap();
        let dot: Fixed<34, 28, true> = f.add(xy, p[2]).unwrap();
        f.publish("nl", dot).unwrap();
        f.publish("visible-product", p[0]).unwrap();
        let frame = f.finish();
        frame.audit().unwrap();
        assert_eq!(
            BoundDag::new(&frame, Hardware::lighting_dsp()).unwrap_err(),
            "fused intermediate escapes"
        );
    }
}
