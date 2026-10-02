//! Conservative structural logic proofs. Input samples never establish a binding.
use super::{bad, LogicCone, Timing};
use crate::{Fault, Format, FrameReport, Operation, Resource};
use std::collections::{BTreeMap, BTreeSet};

fn mask(bits: u32) -> u128 {
    (1_u128 << bits) - 1
}

fn audit_frame(frame: &FrameReport) -> Result<(), Fault> {
    frame.audit()?;
    if !frame.valid {
        return Err(bad("logic lowering requires a valid numerical frame"));
    }
    Ok(())
}

fn same_value(frame: &FrameReport, a: usize, b: usize) -> bool {
    a == b
        || (frame.events[frame.values[a].producer].operation == Operation::Literal
            && frame.events[frame.values[b].producer].operation == Operation::Literal
            && frame.values[a].format == frame.values[b].format
            && frame.values[a].raw == frame.values[b].raw)
}

#[derive(Clone, Copy)]
struct Facts {
    ones: u128,
    min: i128,
    max: i128,
}
impl Facts {
    fn unknown(f: Format) -> Self {
        Self::from_mask(mask(f.bits), f)
    }
    fn from_mask(ones: u128, f: Format) -> Self {
        let sign = 1_u128 << (f.bits - 1);
        Self {
            ones,
            min: if f.signed && ones & sign != 0 {
                -(sign as i128)
            } else {
                0
            },
            max: (ones & if f.signed { sign - 1 } else { mask(f.bits) }) as i128,
        }
    }
    fn fits(self, f: Format) -> bool {
        f.fits(self.min) && f.fits(self.max)
    }
    fn extended(self, source: Format, target: Format) -> u128 {
        let mut ones = self.ones & mask(target.bits);
        if source.signed && target.bits > source.bits && self.min < 0 {
            ones |= mask(target.bits) ^ mask(source.bits);
        }
        ones
    }
}

fn sum_facts(frame: &FrameReport, event: usize, facts: &[Facts]) -> Option<(Facts, [u128; 2])> {
    let e = &frame.events[event];
    if e.operation != Operation::Add || e.inputs.len() != 2 {
        return None;
    }
    let out = frame.values[e.output?].format;
    let a = facts[e.inputs[0]];
    let b = facts[e.inputs[1]];
    let formats = [
        frame.values[e.inputs[0]].format,
        frame.values[e.inputs[1]].format,
    ];
    if formats.iter().any(|f| f.fraction != out.fraction) || !a.fits(out) || !b.fits(out) {
        return None;
    }
    let ones = [a.extended(formats[0], out), b.extended(formats[1], out)];
    let sum = Facts {
        ones: ones[0] | ones[1],
        min: a.min.checked_add(b.min)?,
        max: a.max.checked_add(b.max)?,
    };
    (ones[0] & ones[1] == 0 && sum.fits(out)).then_some((sum, ones))
}

/// Inspect only format/producer structure. The sole raw-value read is a Literal.
fn infer(frame: &FrameReport) -> Vec<Facts> {
    let mut facts: Vec<_> = frame
        .values
        .iter()
        .map(|v| Facts::unknown(v.format))
        .collect();
    for e in &frame.events {
        let Some(v) = e.output else { continue };
        let out = frame.values[v].format;
        let candidate = match e.operation {
            Operation::Literal => {
                let raw = frame.values[v].raw;
                Some(Facts {
                    ones: raw as u128 & mask(out.bits),
                    min: raw,
                    max: raw,
                })
            }
            Operation::Resize if e.inputs.len() == 1 => {
                let source = frame.values[e.inputs[0]].format;
                let a = facts[e.inputs[0]];
                (source.fraction == out.fraction && a.fits(out)).then(|| Facts {
                    ones: a.extended(source, out),
                    ..a
                })
            }
            Operation::Slice(low) if e.inputs.len() == 1 => Some(Facts::from_mask(
                (facts[e.inputs[0]].ones >> low) & mask(out.bits),
                out,
            )),
            Operation::ShiftLeft(n) if e.inputs.len() == 1 => {
                let a = facts[e.inputs[0]];
                a.min
                    .checked_mul(1_i128 << n)
                    .zip(a.max.checked_mul(1_i128 << n))
                    .map(|(min, max)| Facts {
                        ones: (a.ones << n) & mask(out.bits),
                        min,
                        max,
                    })
                    .filter(|f| f.fits(out))
            }
            Operation::BinaryScale if e.inputs.len() == 1 => Some(facts[e.inputs[0]]),
            Operation::Add => sum_facts(frame, e.id, &facts).map(|(f, _)| f),
            // Even a zero-valued input, product or dynamic shift retains its full type domain.
            _ => None,
        };
        if let Some(f) = candidate {
            facts[v] = f;
        }
    }
    facts
}

/// An existing numerical Add implemented by disjoint field wiring, without a carry chain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WiringAdd {
    pub result_event: usize,
    pub operands: [usize; 2],
    /// Possible-one masks after legal extension to the result type, not sampled masks.
    pub possible_ones: [u128; 2],
}
impl WiringAdd {
    pub fn prove(frame: &FrameReport, result_event: usize) -> Result<Self, Fault> {
        audit_frame(frame)?;
        if result_event >= frame.events.len() {
            return Err(bad("wiring add event"));
        }
        let (_, possible_ones) = sum_facts(frame, result_event, &infer(frame))
            .ok_or_else(|| bad("wiring add domains overlap or overflow"))?;
        let inputs = &frame.events[result_event].inputs;
        Ok(Self {
            result_event,
            operands: [inputs[0], inputs[1]],
            possible_ones,
        })
    }
    pub fn audit(&self, frame: &FrameReport) -> Result<(), Fault> {
        if Self::prove(frame, self.result_event)? != *self {
            return Err(bad("wiring add certificate"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EqualityKind {
    Bits,
    /// One same-format operand is the literal zero; reduce the other operand with NOR.
    ReduceNor,
}

/// Closed `(a < b) + (b < a) < literal(1)` implementation as one equality comparator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Equality {
    pub result_event: usize,
    pub absorbed_events: [usize; 3],
    pub operands: [usize; 2],
    pub width: u32,
    pub kind: EqualityKind,
}
impl Equality {
    pub fn prove(frame: &FrameReport, result_event: usize) -> Result<Self, Fault> {
        audit_frame(frame)?;
        let root = frame
            .events
            .get(result_event)
            .ok_or_else(|| bad("equality event"))?;
        let boolean = Format {
            bits: 1,
            fraction: 0,
            signed: false,
        };
        if root.operation != Operation::Less
            || root.inputs.len() != 2
            || frame.values[root.output.ok_or_else(|| bad("equality output"))?].format != boolean
        {
            return Err(bad("equality root"));
        }
        let threshold = &frame.values[root.inputs[1]];
        if frame.events[threshold.producer].operation != Operation::Literal
            || threshold.raw != 1
            || threshold.format.fraction != 0
        {
            return Err(bad("equality constant threshold"));
        }
        let sum_value = &frame.values[root.inputs[0]];
        let sum = &frame.events[sum_value.producer];
        // Opposite comparisons are mutually exclusive, so the complete sum domain is [0,1].
        if sum.operation != Operation::Add
            || sum.inputs.len() != 2
            || sum_value.format.fraction != 0
            || !sum_value.format.fits(0)
            || !sum_value.format.fits(1)
        {
            return Err(bad("equality sum format"));
        }
        let left = &frame.events[frame.values[sum.inputs[0]].producer];
        let right = &frame.events[frame.values[sum.inputs[1]].producer];
        if left.operation != Operation::Less
            || right.operation != Operation::Less
            || left.inputs.len() != 2
            || right.inputs.len() != 2
            || !same_value(frame, left.inputs[0], right.inputs[1])
            || !same_value(frame, left.inputs[1], right.inputs[0])
            || sum
                .inputs
                .iter()
                .any(|&v| frame.values[v].format != boolean)
        {
            return Err(bad("equality opposite comparisons"));
        }
        let a = frame.values[left.inputs[0]].format;
        let b = frame.values[left.inputs[1]].format;
        if a != b {
            return Err(bad("equality operand formats"));
        }
        let kind = if left.inputs.iter().any(|&v| {
            frame.events[frame.values[v].producer].operation == Operation::Literal
                && frame.values[v].raw == 0
        }) {
            EqualityKind::ReduceNor
        } else {
            EqualityKind::Bits
        };
        let proof = Self {
            result_event,
            absorbed_events: [left.id, right.id, sum.id],
            operands: [left.inputs[0], left.inputs[1]],
            width: a.bits,
            kind,
        };
        // Existing cone audit supplies independent closure, width and control checks.
        proof.cone(frame, 1).audit(frame)?;
        Ok(proof)
    }
    pub fn audit(&self, frame: &FrameReport) -> Result<(), Fault> {
        if Self::prove(frame, self.result_event)? != *self {
            return Err(bad("equality certificate"));
        }
        Ok(())
    }
    fn cone(&self, frame: &FrameReport, latency: u64) -> LogicCone {
        let members: BTreeSet<_> = std::iter::once(self.result_event)
            .chain(self.absorbed_events)
            .collect();
        let operands: BTreeSet<_> = members
            .iter()
            .flat_map(|&id| frame.events[id].inputs.iter().copied())
            .filter(|&v| !members.contains(&frame.values[v].producer))
            .collect();
        LogicCone {
            result_event: self.result_event,
            absorbed_events: self.absorbed_events.to_vec(),
            operands: operands.into_iter().collect(),
            max_width: members
                .iter()
                .flat_map(|&id| {
                    frame.events[id]
                        .inputs
                        .iter()
                        .copied()
                        .chain(frame.events[id].output)
                })
                .map(|v| frame.values[v].format.bits)
                .max()
                .unwrap_or(1),
            latency,
        }
    }
}

/// Physical work only. Numerical events/resources remain present and replayable.
#[derive(Clone, Debug, Default)]
pub struct Plan {
    pub wiring_adds: Vec<WiringAdd>,
    pub equalities: Vec<Equality>,
}
impl Plan {
    pub fn resources(&self, frame: &FrameReport) -> Result<Vec<Option<Resource>>, Fault> {
        audit_frame(frame)?;
        let mut used = BTreeSet::new();
        let mut resources: Vec<_> = frame.events.iter().map(|e| e.resource).collect();
        for proof in &self.wiring_adds {
            proof.audit(frame)?;
            if !used.insert(proof.result_event) {
                return Err(bad("overlapping logic lowering"));
            }
            resources[proof.result_event] = None;
        }
        for proof in &self.equalities {
            proof.audit(frame)?;
            for id in std::iter::once(proof.result_event).chain(proof.absorbed_events) {
                if !used.insert(id) {
                    return Err(bad("overlapping logic lowering"));
                }
                resources[id] = None;
            }
            // NOR uses a conservative comparator-width budget; kind records the actual circuit.
            resources[proof.result_event] = Some(Resource::Compare(proof.width));
        }
        Ok(resources)
    }
    pub fn counts(&self, frame: &FrameReport) -> Result<BTreeMap<Resource, u64>, Fault> {
        let mut counts = BTreeMap::new();
        for resource in self.resources(frame)?.into_iter().flatten() {
            *counts.entry(resource).or_default() += 1;
        }
        Ok(counts)
    }
    /// Reuse the existing composed dependency/memory/lifecycle APIs with these single-output cones.
    pub fn logic_cones(&self, frame: &FrameReport, latency: u64) -> Result<Vec<LogicCone>, Fault> {
        self.resources(frame)?;
        if latency == 0 {
            return Err(bad("equality latency"));
        }
        Ok(self
            .equalities
            .iter()
            .map(|p| p.cone(frame, latency))
            .collect())
    }
    /// Check lowerings and dependencies; a scheduler must also enforce the returned resource budget.
    pub fn audit_timing(
        &self,
        frame: &FrameReport,
        times: &[Timing],
        latency: u64,
        max_cycle: u64,
    ) -> Result<(), Fault> {
        let cones = self.logic_cones(frame, latency)?;
        super::audit_logic_dependencies(frame, times, &cones, max_cycle)?;
        if self
            .wiring_adds
            .iter()
            .any(|p| times[p.result_event].issue != times[p.result_event].ready)
        {
            return Err(bad("wiring add latency"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
