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

impl PeriodicSchedule {
    fn graph(plan: &Plan) -> Result<resource_scheduler::ModuloGraph, String> {
        use resource_scheduler::{Graph, ModuloGraph, Node, Resource as Unit};
        let mut graph = Graph::default();
        let mut resource_ids = BTreeMap::new();
        for (id, kind) in plan.binding.kinds.iter().enumerate() {
            let resource = kind.as_ref().map(|k| {
                *resource_ids.entry(k.clone()).or_insert_with(|| {
                    let (lanes, latency) = plan.hardware.unit(k);
                    let index = graph.resources.len();
                    graph.resources.push(Unit {
                        name: format!("{k:?}"),
                        lanes,
                        latency,
                        initiation_interval: 1,
                    });
                    index
                })
            });
            graph.nodes.push(Node {
                name: format!("event{id}"),
                predecessors: plan.binding.dependencies[id].clone(),
                earliest: 0,
                resource,
            });
        }
        let result_events: Vec<_> = plan
            .template
            .events
            .iter()
            .filter(|e| matches!(&e.operation,Operation::Publish(name) if name=="g"||name=="h"))
            .map(|e| e.id)
            .collect();
        if result_events.len() != 2 {
            return Err("missing g/h".into());
        }
        let output_resource = graph.resources.len();
        graph.resources.push(Unit {
            name: "result row".into(),
            lanes: 1,
            latency: 1,
            initiation_interval: 1,
        });
        graph.nodes.push(Node {
            name: "commit".into(),
            predecessors: result_events,
            earliest: 0,
            resource: Some(output_resource),
        });
        let modulo =
            ModuloGraph::from_graph(&graph).map_err(|e| format!("periodic graph: {e:?}"))?;
        Ok(modulo)
    }

    /// Use the generic modulo tool, then independently check the lighting binding.
    pub fn search(plan: &Plan, ii: u64, candidates: usize) -> Result<Self, String> {
        use resource_scheduler::{Limits, SearchConfig};
        plan.audit()?;
        if !matches!(plan.storage, Storage::Registers) {
            return Err("periodic scheduling currently requires register inputs".into());
        }
        if ii == 0 || ii > 64 || candidates == 0 || candidates > 64 {
            return Err("periodic II/candidate limit".into());
        }
        let modulo = Self::graph(plan)?;
        if modulo.resource_lower_bound() > ii {
            return Err(format!(
                "II={ii} capacity: lower bound {}",
                modulo.resource_lower_bound()
            ));
        }
        let calendar = resource_scheduler::modulo_schedule_bounded(
            &modulo,
            ii,
            &Limits::new(4096, plan.hardware.max_cycles, candidates),
            &SearchConfig::default(),
        )
        .map_err(|e| format!("periodic search: {e:?}"))?;
        if !resource_scheduler::check_modulo(&modulo, &calendar).is_ok() {
            return Err("generic modulo audit".into());
        }
        let slots = calendar.nodes[..plan.template.events.len()]
            .iter()
            .enumerate()
            .map(|(event, s)| Reservation {
                pixel: 0,
                event,
                kind: plan.binding.kinds[event].clone(),
                lane: s.lane,
                issue: s.issue,
                ready: s.issue
                    + plan.binding.kinds[event]
                        .as_ref()
                        .map_or(0, |k| plan.hardware.unit(k).1),
            })
            .collect();
        let write_issue = calendar.nodes.last().ok_or("missing commit")?.issue;
        let proposed = Self {
            initiation_interval: ii,
            slots,
            write_issue,
            latency: write_issue + 1,
            searched_candidates: candidates,
        };
        proposed.audit(plan)?;
        Ok(proposed)
    }
    /// Try ALAP placement with unchanged phases and latency. Compare live bits
    /// before selecting it: shorter individual lifetimes do not guarantee a lower peak.
    pub fn compact_lifetimes(mut self, plan: &Plan) -> Result<Self, String> {
        use resource_scheduler::{ModuloNode, ModuloSchedule};
        self.audit(plan)?;
        let graph = Self::graph(plan)?;
        let mut nodes: Vec<_> = self
            .slots
            .iter()
            .map(|r| ModuloNode {
                issue: r.issue,
                lane: r.lane,
            })
            .collect();
        nodes.push(ModuloNode {
            issue: self.write_issue,
            lane: Some(0),
        });
        let compact = resource_scheduler::compact_modulo(
            &graph,
            &ModuloSchedule {
                initiation_interval: self.initiation_interval,
                nodes,
                span: self.latency,
            },
        )
        .map_err(|e| format!("lifetime compact: {e:?}"))?;
        for (r, n) in self.slots.iter_mut().zip(&compact.nodes) {
            r.issue = n.issue;
            r.ready = n.issue + r.kind.as_ref().map_or(0, |k| plan.hardware.unit(k).1);
            r.lane = n.lane;
        }
        for cone in plan.logic_cones() {
            let ready = self.slots[cone.result_event].ready;
            for &id in &cone.absorbed_events {
                self.slots[id].issue = ready;
                self.slots[id].ready = ready;
            }
        }
        self.write_issue = compact.nodes.last().ok_or("missing commit")?.issue;
        self.audit(plan)?;
        Ok(self)
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
