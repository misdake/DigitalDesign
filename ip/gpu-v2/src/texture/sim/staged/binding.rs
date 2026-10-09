//! Structural work/dependency evidence. This is not a fixed stage latency or
//! area certificate: lanes, memory ports and multi-output logic still need binding.
use audited::{
    physical::{
        self,
        lowering::{Equality, Plan, WiringAdd},
        LogicCone, Timing,
    },
    Fault, FrameReport, Operation, Resource,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug)]
pub struct Evidence {
    pub lowering: Plan,
    /// Unlimited lanes, one registered cycle per remaining logic/read primitive,
    /// three per multiply. Only a dependency comparison, not a hardware calendar.
    pub times: Vec<Timing>,
    pub cycles: u64,
    pub work: BTreeMap<Resource, u64>,
    pub output_bits: u64,
}
fn discover(frame: &FrameReport) -> Result<Plan, Fault> {
    frame.audit()?;
    let mut plan = Plan::default();
    let mut absorbed = BTreeSet::new();
    for e in &frame.events {
        if e.operation == Operation::Less {
            if let Ok(proof) = Equality::prove(frame, e.id) {
                absorbed.extend(proof.absorbed_events);
                plan.equalities.push(proof);
            }
        }
    }
    for e in &frame.events {
        if e.operation == Operation::Add && !absorbed.contains(&e.id) {
            if let Ok(proof) = WiringAdd::prove(frame, e.id) {
                plan.wiring_adds.push(proof);
            }
        }
    }
    plan.resources(frame)?;
    Ok(plan)
}
fn visit(
    id: usize,
    deps: &[Vec<usize>],
    resources: &[Option<Resource>],
    marks: &mut [u8],
    times: &mut [Timing],
) -> Result<(), Fault> {
    if marks[id] == 2 {
        return Ok(());
    }
    if marks[id] == 1 {
        return Err(Fault::Audit("staged dependency cycle".into()));
    }
    marks[id] = 1;
    let mut issue = 0;
    for &p in &deps[id] {
        visit(p, deps, resources, marks, times)?;
        issue = issue.max(times[p].ready);
    }
    let latency = match resources[id] {
        None => 0,
        Some(Resource::Dsp18 | Resource::Dsp36) => 3,
        Some(_) => 1,
    };
    times[id] = Timing {
        issue,
        ready: issue + latency,
    };
    marks[id] = 2;
    Ok(())
}
impl Evidence {
    pub fn build(frame: &FrameReport) -> Result<Self, Fault> {
        let lowering = discover(frame)?;
        let resources = lowering.resources(frame)?;
        let cones = lowering.logic_cones(frame, 1)?;
        let deps = physical::logic_dependencies(frame, &cones)?;
        let mut times = vec![Timing { issue: 0, ready: 0 }; frame.events.len()];
        let mut marks = vec![0; times.len()];
        for id in 0..times.len() {
            visit(id, &deps, &resources, &mut marks, &mut times)?;
        }
        let cycles = times.iter().map(|t| t.ready).max().unwrap_or(0);
        lowering.audit_timing(frame, &times, 1, cycles.max(1))?;
        let work = lowering.counts(frame)?;
        let unique: BTreeSet<_> = frame.outputs.iter().map(|o| o.value).collect();
        let output_bits = unique
            .iter()
            .map(|&v| u64::from(frame.values[v].format.bits))
            .sum();
        Ok(Self {
            lowering,
            times,
            cycles,
            work,
            output_bits,
        })
    }
    /// Recheck provenance and declared zero-cost wiring without trusting discovery.
    pub fn audit(&self, frame: &FrameReport) -> Result<(), Fault> {
        self.lowering
            .audit_timing(frame, &self.times, 1, self.cycles.max(1))?;
        if self.work != self.lowering.counts(frame)?
            || self.cycles != self.times.iter().map(|t| t.ready).max().unwrap_or(0)
            || self.output_bits
                != frame
                    .outputs
                    .iter()
                    .map(|o| o.value)
                    .collect::<BTreeSet<_>>()
                    .iter()
                    .map(|&v| u64::from(frame.values[v].format.bits))
                    .sum::<u64>()
        {
            return Err(Fault::Audit("staged binding evidence totals".into()));
        }
        let resources = self.lowering.resources(frame)?;
        for (r, t) in resources.iter().zip(&self.times) {
            let expected = match r {
                None => 0,
                Some(Resource::Dsp18 | Resource::Dsp36) => 3,
                Some(_) => 1,
            };
            if t.ready != t.issue + expected {
                return Err(Fault::Audit("staged primitive latency".into()));
            }
        }
        Ok(())
    }
}

/// Actual LOD sharing counterexample: h=19-clz(s), shift=19-h. A cone ending
/// at shift cannot absorb h because exponent=h-18 also consumes it. A proposed
/// two-output region must explicitly retain BOTH results and their lifetimes.
pub fn lod_shared_h_cone(frame: &FrameReport) -> Result<LogicCone, Fault> {
    let literal19 = |v: usize| {
        frame.events[frame.values[v].producer].operation == Operation::Literal
            && frame.values[v].raw == 19
    };
    for root in &frame.events {
        if root.operation != Operation::Sub || root.inputs.len() != 2 || !literal19(root.inputs[0])
        {
            continue;
        }
        let h = &frame.events[frame.values[root.inputs[1]].producer];
        if h.operation != Operation::Sub
            || h.inputs.len() != 2
            || !literal19(h.inputs[0])
            || frame.events[frame.values[h.inputs[1]].producer].operation != Operation::LeadingZeros
        {
            continue;
        }
        let members = [root.id, h.id];
        let operands = members
            .iter()
            .flat_map(|&id| frame.events[id].inputs.iter().copied())
            .filter(|&v| !members.contains(&frame.values[v].producer))
            .collect::<BTreeSet<_>>();
        let max_width = members
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
            .unwrap();
        return Ok(LogicCone {
            result_event: root.id,
            absorbed_events: vec![h.id],
            exported_events: Vec::new(),
            operands: operands.into_iter().collect(),
            max_width,
            latency: 1,
        });
    }
    Err(Fault::Audit("LOD shared h example missing".into()))
}
