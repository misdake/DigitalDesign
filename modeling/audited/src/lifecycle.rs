//! Width-weighted retained values; wiring aliases share their producer storage.
//! Pipeline-internal registers and control state are separate inventories.
use crate::{
    physical::{audit_composed_dependencies, FusedGroup, LogicCone, Timing},
    Fault, FrameReport, Operation,
};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveInterval {
    pub value: usize,
    pub bits: u32,
    pub start: u64,
    pub end: u64,
    pub invariant: bool,
}
#[derive(Clone, Debug)]
pub struct LifetimePolicy {
    pub commit_cycle: u64,
    pub period: Option<u64>,
    pub max_cycle: u64,
    pub invariant_inputs: Vec<usize>,
    /// None retains all observations; Some names distinguishes ports from goldens.
    pub retained_outputs: Option<Vec<String>>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveReport {
    pub intervals: Vec<LiveInterval>,
    pub peak_bits: u64,
    pub peak_bits_by_width: BTreeMap<u32, u64>,
    pub period: Option<u64>,
}
#[derive(Clone, Debug)]
pub struct RegisterBudget {
    pub total_bits: u64,
    pub by_width: BTreeMap<u32, u64>,
}
impl LiveReport {
    pub fn audit_budget(&self, budget: &RegisterBudget) -> Result<(), Fault> {
        if self.peak_bits > budget.total_bits
            || self
                .peak_bits_by_width
                .iter()
                .any(|(w, n)| budget.by_width.get(w).is_some_and(|limit| n > limit))
        {
            return Err(Fault::Audit("retained-value register capacity".into()));
        }
        Ok(())
    }
}
/// Publications retain their source until `commit_cycle`, not just observation.
/// Periodic mode repeats lifetimes at +II; it includes cross-iteration overlap.
pub fn analyze(
    frame: &FrameReport,
    times: &[Timing],
    commit_cycle: u64,
    period: Option<u64>,
    max_cycle: u64,
) -> Result<LiveReport, Fault> {
    analyze_bound(frame, times, &[], commit_cycle, period, max_cycle)
}

pub fn analyze_bound(
    frame: &FrameReport,
    times: &[Timing],
    groups: &[FusedGroup],
    commit_cycle: u64,
    period: Option<u64>,
    max_cycle: u64,
) -> Result<LiveReport, Fault> {
    analyze_bound_policy(
        frame,
        times,
        groups,
        &LifetimePolicy {
            commit_cycle,
            period,
            max_cycle,
            invariant_inputs: Vec::new(),
            retained_outputs: None,
        },
    )
}

pub fn analyze_bound_policy(
    frame: &FrameReport,
    times: &[Timing],
    groups: &[FusedGroup],
    policy: &LifetimePolicy,
) -> Result<LiveReport, Fault> {
    analyze_composed_policy(frame, times, groups, &[], policy)
}

pub fn analyze_composed_policy(
    frame: &FrameReport,
    times: &[Timing],
    groups: &[FusedGroup],
    cones: &[LogicCone],
    policy: &LifetimePolicy,
) -> Result<LiveReport, Fault> {
    let LifetimePolicy {
        commit_cycle,
        period,
        max_cycle,
        ..
    } = *policy;
    audit_composed_dependencies(frame, times, groups, cones, max_cycle)?;
    let roots: BTreeMap<_, _> = groups
        .iter()
        .map(|g| (g.result_event, &g.operands))
        .chain(cones.iter().map(|c| (c.result_event, &c.operands)))
        .collect();
    let absorbed: std::collections::BTreeSet<_> = groups
        .iter()
        .flat_map(|g| g.absorbed_events.iter().copied())
        .chain(cones.iter().flat_map(|c| c.absorbed_events.iter().copied()))
        .collect();
    let bad = || Fault::Audit("lifetime bounds or arithmetic".into());
    if period == Some(0) || commit_cycle > max_cycle {
        return Err(bad());
    }
    let mut origins = vec![None; frame.values.len()];
    let mut intervals = BTreeMap::<usize, LiveInterval>::new();
    let mut input_rows = BTreeMap::new();
    if let Some(names) = &policy.retained_outputs {
        let unique: std::collections::BTreeSet<_> = names.iter().collect();
        if unique.len() != names.len()
            || names
                .iter()
                .any(|name| !frame.outputs.iter().any(|o| o.name == *name))
        {
            return Err(bad());
        }
    }
    let invariant_ids: std::collections::BTreeSet<_> =
        policy.invariant_inputs.iter().copied().collect();
    if invariant_ids.len() != policy.invariant_inputs.len()
        || invariant_ids.iter().any(|&id| {
            !frame
                .memories
                .get(id)
                .is_some_and(|m| m.kind == crate::MemoryKind::Input)
        })
    {
        return Err(bad());
    }
    for e in &frame.events {
        if absorbed.contains(&e.id)
            || matches!(e.operation, Operation::ProductMapping { .. })
            || matches!(&e.operation,Operation::Publish(name) if policy.retained_outputs.as_ref().is_some_and(|names|!names.contains(name)))
        {
            continue;
        }
        if let Some(value) = e.output {
            let alias = !roots.contains_key(&e.id)
                && e.inputs.len() == 1
                && matches!(
                    e.operation,
                    Operation::Resize
                        | Operation::BinaryScale
                        | Operation::Slice(_)
                        | Operation::ShiftLeft(_)
                        | Operation::RescaleFloor(_)
                );
            origins[value] = if matches!(e.operation, Operation::Literal) {
                None
            } else if alias {
                origins[e.inputs[0]]
            } else {
                Some(value)
            };
            let mut invariant = false;
            if let Operation::Read { memory, row } = e.operation {
                if frame.memories[memory].kind == crate::MemoryKind::Input {
                    invariant = invariant_ids.contains(&memory);
                    origins[value] = Some(*input_rows.entry((memory, row)).or_insert(value));
                }
            }
            if origins[value] == Some(value) {
                // Conservative capture even for a same-cycle consumer.
                let start = if matches!(e.operation,Operation::Read{memory,..} if frame.memories[memory].kind==crate::MemoryKind::Input)
                {
                    0
                } else {
                    times[e.id].ready
                };
                let end = start.checked_add(1).ok_or_else(bad)?;
                intervals.insert(
                    value,
                    LiveInterval {
                        value,
                        bits: frame.values[value].format.bits,
                        start,
                        end,
                        invariant,
                    },
                );
            }
        }
        let inputs = roots.get(&e.id).map_or(&e.inputs, |inputs| *inputs);
        let mut controls: Vec<_> = e
            .control
            .map(|c| frame.events[c].inputs.as_slice())
            .unwrap_or(&[])
            .to_vec();
        for (_, members) in groups
            .iter()
            .map(|g| (g.result_event, &g.absorbed_events))
            .chain(cones.iter().map(|c| (c.result_event, &c.absorbed_events)))
            .filter(|(root, _)| *root == e.id)
        {
            for &id in members {
                if let Some(control) = frame.events[id].control {
                    controls.extend(&frame.events[control].inputs);
                }
            }
        }
        for &input in inputs.iter().chain(&controls) {
            if let Some(origin) = origins[input] {
                let hold = if matches!(e.operation, Operation::Publish(_)) {
                    if commit_cycle < times[e.id].issue {
                        return Err(bad());
                    }
                    commit_cycle
                } else {
                    times[e.id].issue
                };
                let end = hold.checked_add(1).ok_or_else(bad)?;
                intervals.get_mut(&origin).ok_or_else(bad)?.end = intervals[&origin].end.max(end);
            }
        }
    }
    let intervals: Vec<_> = intervals.into_values().collect();
    let mut events = BTreeMap::<u64, Vec<(u32, i64)>>::new();
    let mut current = BTreeMap::<u32, i64>::new();
    for i in &intervals {
        let bits = i64::from(i.bits);
        if let Some(ii) = period {
            if i.invariant {
                *current.entry(i.bits).or_default() += bits;
                continue;
            }
            let length = i.end - i.start;
            let full = (length / ii)
                .checked_mul(u64::from(i.bits))
                .ok_or_else(bad)?;
            let full: i64 = full.try_into().map_err(|_| bad())?;
            let entry = current.entry(i.bits).or_default();
            *entry = entry.checked_add(full).ok_or_else(bad)?;
            let remainder = length % ii;
            if remainder == 0 {
                continue;
            }
            let start = i.start % ii;
            // Avoid overflow in start+remainder, even for very large II.
            let end = if remainder >= ii - start {
                remainder - (ii - start)
            } else {
                start + remainder
            };
            if end < start {
                *current.entry(i.bits).or_default() += bits;
                events.entry(end).or_default().push((i.bits, -bits));
                events.entry(start).or_default().push((i.bits, bits));
            } else {
                events.entry(start).or_default().push((i.bits, bits));
                events.entry(end).or_default().push((i.bits, -bits));
            }
        } else {
            events.entry(i.start).or_default().push((i.bits, bits));
            events.entry(i.end).or_default().push((i.bits, -bits));
        }
    }
    let mut peak_bits = current
        .values()
        .try_fold(0_u64, |sum, &n| sum.checked_add(n as u64))
        .ok_or_else(bad)?;
    let mut peaks: BTreeMap<_, _> = current.iter().map(|(&w, &n)| (w, n as u64)).collect();
    for changes in events.values() {
        for &(width, delta) in changes {
            let n = current.entry(width).or_default();
            *n = n.checked_add(delta).ok_or_else(bad)?;
        }
        if current.values().any(|&n| n < 0) {
            return Err(bad());
        }
        let total = current
            .values()
            .try_fold(0_u64, |sum, &n| sum.checked_add(n as u64))
            .ok_or_else(bad)?;
        peak_bits = peak_bits.max(total);
        for (&width, &n) in &current {
            let peak = peaks.entry(width).or_default();
            *peak = (*peak).max(n as u64);
        }
    }
    Ok(LiveReport {
        intervals,
        peak_bits,
        peak_bits_by_width: peaks,
        period,
    })
}
