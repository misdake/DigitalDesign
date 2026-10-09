//! Conservative offline arithmetic reservations, separate from cache control.
use super::{counted, Hardware};
use audited::{
    physical::{DspIssue, DspWork},
    FrameReport, Operation, Resource,
};
use resource_scheduler::{Graph, Limits, Node, Resource as Site, Schedule, SearchConfig};
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct ArithmeticPlan {
    pub graph: Graph,
    pub schedule: Schedule,
    pub group_ready: Vec<u64>,
    pub multiplier_issues: Vec<DspIssue>,
    /// Peak width-weighted lifetime occupancy of the conservative value pool.
    pub live_bits: usize,
}
impl ArithmeticPlan {
    pub fn audit(&self, frame: &FrameReport, hardware: &Hardware) -> Result<(), String> {
        frame
            .audit()
            .map_err(|e| format!("numerical preparation: {e:?}"))?;
        let graph = graph(frame, hardware)?;
        if self.graph != graph {
            return Err("arithmetic graph/provenance mismatch".into());
        }
        let checked = resource_scheduler::check(&self.graph, &limits(hardware), &self.schedule);
        if !checked.is_ok() {
            return Err(format!("arithmetic calendar: {checked:?}"));
        }
        let (ready, issues) = extract(frame, &self.schedule)?;
        let live_bits = live_bits(frame, &self.schedule)?;
        if self.live_bits != live_bits || live_bits > hardware.preparation_register_bits {
            return Err("preparation register lifetime capacity".into());
        }
        if self.group_ready != ready
            || self.multiplier_issues.len() != issues.len()
            || self.multiplier_issues.iter().zip(&issues).any(|(a, b)| {
                a.instance != b.instance
                    || a.issue != b.issue
                    || a.ready != b.ready
                    || !matches!(
                        (a.work, b.work),
                        (
                            DspWork::Multiply {
                                a_bits: 9,
                                b_bits: 8
                            },
                            DspWork::Multiply {
                                a_bits: 9,
                                b_bits: 8
                            }
                        )
                    )
            })
        {
            return Err("arithmetic payload/multiplier timing mismatch".into());
        }
        hardware
            .inventory()?
            .audit_issues(&issues, None, hardware.max_arithmetic_cycles)
            .map_err(|e| format!("preparation DSP packing: {e:?}"))
    }
}
fn live_bits(frame: &FrameReport, schedule: &Schedule) -> Result<usize, String> {
    let mut ends = vec![None; frame.values.len()];
    for (e, t) in frame.events.iter().zip(&schedule.nodes) {
        for &v in &e.inputs {
            ends[v] = Some(ends[v].unwrap_or(0).max(t.issue));
        }
    }
    let mut edges = BTreeMap::<u64, i64>::new();
    for (v, end) in frame.values.iter().zip(ends) {
        let Some(end) = end else {
            continue;
        };
        let ready = schedule.nodes[v.producer].ready;
        if end < ready {
            return Err("negative preparation lifetime".into());
        }
        let bits = i64::from(v.format.bits);
        *edges.entry(ready).or_default() += bits;
        *edges.entry(end + 1).or_default() -= bits;
    }
    let mut live = 0_i64;
    let mut peak = 0_i64;
    for delta in edges.values() {
        live += delta;
        peak = peak.max(live);
    }
    Ok(peak as usize)
}
fn limits(h: &Hardware) -> Limits {
    Limits::new(12000, h.max_arithmetic_cycles, 2)
}
fn graph(frame: &FrameReport, h: &Hardware) -> Result<Graph, String> {
    let mut resources = Vec::new();
    let mut mapping = BTreeMap::new();
    for event in &frame.events {
        if let Some(r) = event.resource {
            if mapping.contains_key(&r) {
                continue;
            }
            let (lanes, latency) = match r {
                Resource::Dsp18 => (h.coefficient_lanes, h.multiply_latency),
                Resource::Dsp36 => return Err("texture has no wide products".into()),
                Resource::Read(m) => (
                    if frame.memories[m].name == "helper_uv" {
                        8
                    } else {
                        1
                    },
                    1,
                ),
                Resource::Write(_) => (1, 1),
                _ => (h.logic_lanes_per_width, 1),
            };
            mapping.insert(r, resources.len());
            resources.push(Site {
                name: format!("{r:?}"),
                lanes,
                latency,
                initiation_interval: 1,
            });
        }
    }
    let mut last_write = None;
    let nodes = frame
        .events
        .iter()
        .map(|e| {
            let mut predecessors: Vec<_> =
                e.inputs.iter().map(|&v| frame.values[v].producer).collect();
            if let Some(c) = e.control {
                predecessors.push(c);
            }
            if matches!(e.operation, Operation::Write { .. }) {
                if let Some(p) = last_write {
                    predecessors.push(p);
                }
                last_write = Some(e.id);
            }
            predecessors.sort_unstable();
            predecessors.dedup();
            Node {
                name: format!("{:?}", e.operation),
                predecessors,
                earliest: 0,
                resource: e.resource.map(|r| mapping[&r]),
            }
        })
        .collect();
    Ok(Graph { nodes, resources })
}
fn extract(frame: &FrameReport, schedule: &Schedule) -> Result<(Vec<u64>, Vec<DspIssue>), String> {
    let mut ready = Vec::new();
    let mut issues = Vec::new();
    for (e, t) in frame.events.iter().zip(&schedule.nodes) {
        if matches!(e.operation, Operation::Write { .. }) {
            ready.push(t.ready);
        }
        if matches!(e.operation, Operation::Multiply) {
            if e.inputs.len() != 2
                || frame.values[e.inputs[0]].format.bits != 9
                || frame.values[e.inputs[1]].format.bits != 8
            {
                return Err("texture product shape is not 9x8".into());
            }
            issues.push(DspIssue {
                instance: t.lane.ok_or("multiply has no lane")?,
                issue: t.issue,
                ready: t.ready,
                work: DspWork::Multiply {
                    a_bits: 9,
                    b_bits: 8,
                },
            });
        }
    }
    if ready.windows(2).any(|w| w[1] <= w[0]) {
        return Err("Group4 write ordering".into());
    }
    Ok((ready, issues))
}
pub fn plan(preparation: &counted::Preparation, h: &Hardware) -> Result<ArithmeticPlan, String> {
    let frame = &preparation.frame;
    frame.audit().map_err(|e| format!("counted audit: {e:?}"))?;
    let graph = graph(frame, h)?;
    let outcome = resource_scheduler::plan(&graph, &limits(h), &SearchConfig::default())
        .map_err(|e| format!("prepare reservation: {e:?}"))?;
    let mut valid = Vec::new();
    for candidate in &outcome.candidates {
        if !candidate.within_deadline {
            continue;
        }
        let schedule = candidate.schedule.clone();
        let (group_ready, multiplier_issues) = extract(frame, &schedule)?;
        let live_bits = live_bits(frame, &schedule)?;
        if live_bits > h.preparation_register_bits {
            continue;
        }
        let result = ArithmeticPlan {
            live_bits,
            graph: graph.clone(),
            schedule,
            group_ready,
            multiplier_issues,
        };
        result.audit(frame, h)?;
        valid.push(result);
    }
    valid
        .into_iter()
        .min_by_key(|p| (p.schedule.cycles, p.live_bits))
        .ok_or_else(|| "no bounded preparation candidate fits value storage".into())
}
