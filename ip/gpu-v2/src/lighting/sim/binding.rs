//! Checked physical-planning lowering, separate from the counted numerical ledger.
use super::timed::{self, Binding, Hardware, LaneKind};
use audited::{FrameReport, Operation};

/// One MULTADDALU18X18 candidate: A0*B0 + A1*B1 + C, without rounding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FusedGroup {
    pub result_event: usize,
    /// Internal products/sum have no independently accessible physical result.
    pub absorbed_events: Vec<usize>,
    /// A0, B0, A1, B1, C value ids in the numerical ledger.
    pub operands: Vec<usize>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct BoundDag {
    pub kinds: Vec<Option<LaneKind>>,
    pub dependencies: Vec<Vec<usize>>,
    pub groups: Vec<FusedGroup>,
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
    Ok(Some(FusedGroup {
        result_event: result,
        absorbed_events: absorbed,
        operands,
    }))
}
impl BoundDag {
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
        Ok(Self {
            kinds,
            dependencies,
            groups,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use audited::{Fixed, Model};
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
