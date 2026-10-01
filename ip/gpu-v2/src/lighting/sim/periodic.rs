//! Static modulo reservations with an independent infinite-repeat checker.
//! There is no loop-carried state: one uniform context and register inputs.
use super::*;
use std::collections::BTreeSet;

#[derive(Clone, Debug)]
pub struct PeriodicSchedule {
    pub initiation_interval: u64,
    /// One pixel's reservations. Each repeats with the same lane at +pixel*II.
    pub slots: Vec<Reservation>,
    pub write_issue: u64,
    pub latency: u64,
    pub searched_candidates: usize,
}

fn priority(id: usize, candidate: usize, tail: &[u64]) -> u64 {
    match candidate {
        0 => 0,
        1 => u64::MAX - tail[id],
        _ => {
            let mut x = (id as u64) ^ (candidate as u64 * 0x9e37_79b9);
            x ^= x >> 30;
            x = x.wrapping_mul(0xbf58_476d_1ce4_e5b9);
            x ^= x >> 27;
            x.wrapping_mul(0x94d0_49bb_1331_11eb) ^ (x >> 31)
        }
    }
}

impl PeriodicSchedule {
    /// Bounded phase-aware list search. Reusing a lane/phase would collide
    /// with another pixel in steady state, irrespective of local stage time.
    pub fn search(plan: &Plan, ii: u64, candidates: usize) -> Result<Self, String> {
        plan.audit()?;
        if !matches!(plan.storage, Storage::Registers) {
            return Err("periodic scheduling currently requires register inputs".into());
        }
        if ii == 0 || ii > 64 || candidates == 0 || candidates > 64 {
            return Err("periodic II/candidate limit".into());
        }
        let n = plan.template.events.len();
        let h = plan.hardware;
        let mut children = vec![Vec::new(); n];
        let mut degree = vec![0; n];
        let mut counts = BTreeMap::<LaneKind, usize>::new();
        for (id, deps) in plan.binding.dependencies.iter().enumerate() {
            degree[id] = deps.len();
            for &d in deps {
                children[d].push(id);
            }
            if let Some(k) = &plan.binding.kinds[id] {
                *counts.entry(k.clone()).or_default() += 1;
            }
        }
        for (k, count) in &counts {
            let (lanes, latency) = h.unit(k);
            if lanes == 0 || lanes > 64 || latency == 0 {
                return Err("invalid periodic hardware".into());
            }
            if *count as u64 > lanes as u64 * ii {
                return Err(format!(
                    "II={ii} capacity: {k:?} needs {count}, has {}",
                    lanes as u64 * ii
                ));
            }
        }
        let mut topo_degree = degree.clone();
        let mut queue: Vec<_> = (0..n).filter(|&id| topo_degree[id] == 0).collect();
        let mut index = 0;
        while index < queue.len() {
            let id = queue[index];
            index += 1;
            for &child in &children[id] {
                topo_degree[child] -= 1;
                if topo_degree[child] == 0 {
                    queue.push(child);
                }
            }
        }
        if queue.len() != n {
            return Err("periodic dependency cycle".into());
        }
        let mut tail = vec![0; n];
        for &id in queue.iter().rev() {
            let own = plan.binding.kinds[id].as_ref().map_or(0, |k| h.unit(k).1);
            tail[id] = own + children[id].iter().map(|&c| tail[c]).max().unwrap_or(0);
        }
        let results: Vec<_> = plan
            .template
            .events
            .iter()
            .filter(
                |e| matches!(&e.operation, Operation::Publish(name) if name == "g" || name == "h"),
            )
            .map(|e| e.id)
            .collect();
        let mut best: Option<Self> = None;
        for candidate in 0..candidates {
            let mut pending = degree.clone();
            let mut earliest = vec![0; n];
            let mut used: BTreeMap<_, _> = counts
                .keys()
                .map(|k| (k.clone(), vec![vec![false; ii as usize]; h.unit(k).0]))
                .collect();
            let mut heap = BinaryHeap::new();
            for (id, &remaining) in pending.iter().enumerate() {
                if remaining == 0 {
                    heap.push(Reverse((0, priority(id, candidate, &tail), id)));
                }
            }
            let mut slots = vec![None; n];
            while let Some(Reverse((start, _, id))) = heap.pop() {
                let kind = plan.binding.kinds[id].clone();
                let (issue, ready, lane) = if let Some(k) = &kind {
                    let calendar = used.get_mut(k).unwrap();
                    let mut selected = None;
                    for delay in 0..ii {
                        let issue = start + delay;
                        let phase = (issue % ii) as usize;
                        if let Some(lane) = calendar.iter().position(|phases| !phases[phase]) {
                            calendar[lane][phase] = true;
                            selected = Some((issue, issue + h.unit(k).1, Some(lane)));
                            break;
                        }
                    }
                    selected.ok_or("periodic resource exhausted")?
                } else {
                    (start, start, None)
                };
                if ready > h.max_cycles {
                    return Err("periodic cycle limit".into());
                }
                slots[id] = Some(Reservation {
                    pixel: 0,
                    event: id,
                    kind,
                    lane,
                    issue,
                    ready,
                });
                for &child in &children[id] {
                    earliest[child] = earliest[child].max(ready);
                    pending[child] -= 1;
                    if pending[child] == 0 {
                        heap.push(Reverse((
                            earliest[child],
                            priority(child, candidate, &tail),
                            child,
                        )));
                    }
                }
            }
            let slots: Vec<_> = slots
                .into_iter()
                .map(|s| s.ok_or("missing periodic slot"))
                .collect::<Result<_, _>>()?;
            let write_issue = results
                .iter()
                .map(|&id| slots[id].ready)
                .max()
                .ok_or("missing g/h")?;
            let proposed = Self {
                initiation_interval: ii,
                slots,
                write_issue,
                latency: write_issue + 1,
                searched_candidates: candidates,
            };
            proposed.audit(plan)?;
            if best.as_ref().is_none_or(|b| proposed.latency < b.latency) {
                best = Some(proposed);
            }
        }
        best.ok_or("no periodic candidate".into())
    }

    /// Lane/phase uniqueness proves noncollision for arbitrarily many pixels.
    /// Also check same-pixel data/control dependencies and fixed output delay.
    pub fn audit(&self, plan: &Plan) -> Result<(), String> {
        plan.audit()?;
        let ii = self.initiation_interval;
        if !matches!(plan.storage, Storage::Registers)
            || ii == 0
            || ii > 64
            || self.slots.len() != plan.template.events.len()
            || self.searched_candidates == 0
            || self.searched_candidates > 64
        {
            return Err("periodic shape".into());
        }
        let mut phases = BTreeSet::new();
        for (id, slot) in self.slots.iter().enumerate() {
            if slot.pixel != 0
                || slot.event != id
                || slot.kind != plan.binding.kinds[id]
                || slot.issue > slot.ready
                || slot.ready > plan.hardware.max_cycles
            {
                return Err("periodic slot identity".into());
            }
            if let Some(k) = &slot.kind {
                let (lanes, latency) = plan.hardware.unit(k);
                let lane = slot.lane.ok_or("periodic missing lane")?;
                if lane >= lanes || slot.issue.checked_add(latency) != Some(slot.ready) {
                    return Err("periodic lane/latency".into());
                }
                if !phases.insert((k.clone(), lane, slot.issue % ii)) {
                    return Err("periodic lane collision".into());
                }
            } else if slot.lane.is_some() || slot.issue != slot.ready {
                return Err("periodic wiring timing".into());
            }
        }
        for (id, slot) in self.slots.iter().enumerate() {
            for &d in &plan.binding.dependencies[id] {
                if self.slots[d].ready > slot.issue {
                    return Err("periodic operand/control not ready".into());
                }
            }
        }
        let result_ready = plan
            .template
            .events
            .iter()
            .filter(
                |e| matches!(&e.operation, Operation::Publish(name) if name == "g" || name == "h"),
            )
            .map(|e| self.slots[e.id].ready)
            .max()
            .ok_or("missing g/h")?;
        if self.write_issue < result_ready
            || self.write_issue.checked_add(1) != Some(self.latency)
            || self.latency > plan.hardware.max_cycles
        {
            return Err("periodic result latency".into());
        }
        Ok(())
    }

    /// Expand into the existing finite-plan checker, including input release
    /// gates and ordered writes. Numerical results remain evaluated by counted.
    pub fn expand(&self, mut plan: Plan) -> Result<Plan, String> {
        self.audit(&plan)?;
        let ii = self.initiation_interval;
        for (id, r) in plan.events.iter_mut().enumerate() {
            let pixel = r.pixel;
            let event = r.event;
            let start = pixel as u64 * ii;
            *r = self.slots[event].clone();
            r.pixel = pixel;
            r.issue += start;
            r.ready += start;
            plan.gates[id] = start;
        }
        for (pixel, w) in plan.writes.iter_mut().enumerate() {
            let start = pixel as u64 * ii;
            w.arithmetic_ready = plan.template.events.iter().filter(|e| {
                matches!(&e.operation, Operation::Publish(name) if name == "g" || name == "h")
            }).map(|e| self.slots[e.id].ready + start).max().ok_or("missing g/h")?;
            w.issue = self.write_issue + start;
            w.ready = self.latency + start;
        }
        plan.cycles = plan.writes.last().ok_or("missing writes")?.ready;
        plan.audit()?;
        Ok(plan)
    }
}
