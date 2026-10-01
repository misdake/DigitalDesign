//! Bounded input/result reservation and context-token replay across uniform modes.
//! Arithmetic outputs are separately evaluated by counted; this is not an emu.
use super::*;
use audited::flow::{ContextLease, FlowConfig, FlowMachine, FlowTrace, Tick};
use std::collections::BTreeSet;
#[derive(Clone, Copy, Debug)]
pub struct StreamContext {
    pub material: Material,
    pub light: Light,
    pub projection: Projection,
}
#[derive(Clone, Debug)]
pub struct StreamResult {
    pub id: u64,
    pub context: usize,
    pub accepted: u64,
    pub completed: u64,
    pub output: LightingOutput,
}
pub struct StreamRun {
    pub results: Vec<StreamResult>,
    pub trace: FlowTrace,
    pub calendars: Vec<PeriodicSchedule>,
}
/// At most two resident contexts, 64 pixels, and 20,000 wall ticks. CE pauses
/// freeze issue phase, arithmetic time, result reservations and context references.
/// Admission reserves every future resource slot AND strictly ordered result time;
/// short-mode transitions wait only as long as these two contracts require.
pub fn stream(
    inputs: &[(u64, PixelInput, usize)],
    contexts: &[StreamContext],
    hardware: Hardware,
    max_ticks: usize,
    pauses: &BTreeSet<usize>,
) -> Result<StreamRun, String> {
    if inputs.is_empty()
        || inputs.len() > 64
        || contexts.is_empty()
        || contexts.len() > 2
        || max_ticks == 0
        || max_ticks > 20000
    {
        return Err("stream bounds".into());
    }
    if hardware.kernel.shared_half || hardware.kernel.flat_normal {
        return Err("shared geometry stream requires an external cache owner".into());
    }
    let mut ids = BTreeSet::new();
    if inputs
        .iter()
        .any(|(id, _, c)| *id > u64::from(u32::MAX) || !ids.insert(*id) || *c >= contexts.len())
    {
        return Err("stream ID/context".into());
    }
    let mut plans = Vec::new();
    let mut calendars = Vec::new();
    for c in contexts {
        let p = plan(
            &[inputs[0].1],
            c.material,
            c.light,
            c.projection,
            hardware,
            Storage::Registers,
            Strategy::Interleaved,
        )?;
        let s = (1..=8)
            .find_map(|ii| PeriodicSchedule::search(&p, ii, 32).ok())
            .ok_or("no legal stream body II in 1..8")?;
        s.audit_physical(
            &p,
            audited::physical::GowinMemoryBudget {
                bsram_blocks: 46,
                ssram_cells: 2048,
            },
            &audited::lifecycle::RegisterBudget {
                total_bits: 1000000,
                by_width: Default::default(),
            },
        )?;
        plans.push(p);
        calendars.push(s);
    }
    let mut flow = FlowMachine::new(FlowConfig {
        context_banks: contexts.len(),
        fifo_capacity: 128,
        id_bits: 32,
        epoch_bits: 16,
        payload_bits: 18,
        phase_interval: 1,
        max_steps: max_ticks,
    })
    .map_err(|e| format!("{e:?}"))?;
    let mut leases = Vec::<ContextLease>::new();
    for bank in 0..contexts.len() {
        flow.tick(Tick {
            ce: true,
            load_context: Some(bank),
            ..Tick::default()
        })
        .map_err(|e| format!("{e:?}"))?;
        leases.push(ContextLease {
            bank,
            epoch: flow.snapshot().epochs[bank],
        });
    }
    let mut reservations = BTreeSet::new();
    let mut results = Vec::<StreamResult>::new();
    let mut next = 0;
    let mut committed = 0;
    let mut previous_finish = None;
    let mut next_issue = 0;
    let mut wall = contexts.len();
    while committed < inputs.len() {
        if wall >= max_ticks {
            return Err("stream watchdog".into());
        }
        let ce = !pauses.contains(&wall);
        let cycle = flow.snapshot().advancing_cycles;
        let complete = if ce && committed < results.len() && results[committed].completed == cycle {
            Some(results[committed].id)
        } else {
            None
        };
        let mut accept = None;
        if ce && next < inputs.len() && cycle >= next_issue {
            let (id, pixel, context) = inputs[next];
            let s = &calendars[context];
            let finish = cycle
                .checked_add(s.latency)
                .ok_or("stream timing overflow")?;
            let free = previous_finish.is_none_or(|p| finish > p)
                && s.slots.iter().all(|r| {
                    r.kind.as_ref().is_none_or(|k| {
                        !reservations.contains(&(k.clone(), r.lane.unwrap(), cycle + r.issue))
                    })
                });
            if free {
                let c = contexts[context];
                let output = counted::evaluate_with_config(
                    pixel,
                    c.material,
                    c.light,
                    c.projection,
                    2048,
                    hardware.kernel,
                )
                .map_err(|e| format!("{e:?}"))?
                .output;
                for r in &s.slots {
                    if let Some(k) = &r.kind {
                        if !reservations.insert((k.clone(), r.lane.unwrap(), cycle + r.issue)) {
                            return Err("cross-mode resource collision".into());
                        }
                    }
                }
                results.push(StreamResult {
                    id,
                    context,
                    accepted: cycle,
                    completed: finish,
                    output,
                });
                accept = Some((id, leases[context]));
                previous_finish = Some(finish);
                next_issue = cycle + s.initiation_interval;
                next += 1;
            }
        }
        flow.tick(Tick {
            ce,
            accept,
            complete,
            commit: complete.is_some(),
            ..Tick::default()
        })
        .map_err(|e| format!("stream token: {e:?}"))?;
        wall += 1;
        if complete.is_some() {
            committed += 1;
        }
    }
    if flow.snapshot().references.iter().any(|&r| r != 0)
        || flow.snapshot().committed != inputs.iter().map(|(id, _, _)| *id).collect::<Vec<_>>()
    {
        return Err("ordered context release".into());
    }
    let trace = flow.finish();
    trace.audit().map_err(|e| format!("trace: {e:?}"))?;
    // Independent cross-body check, reconstructed without the admission set.
    let mut replay = BTreeSet::new();
    for r in &results {
        let s = &calendars[r.context];
        s.audit(&plans[r.context])?;
        for op in &s.slots {
            if let Some(k) = &op.kind {
                if !replay.insert((k.clone(), op.lane.unwrap(), r.accepted + op.issue)) {
                    return Err("replayed cross-body collision".into());
                }
            }
        }
    }
    let run = StreamRun {
        results,
        trace,
        calendars,
    };
    run.audit(inputs, contexts, hardware)?;
    Ok(run)
}

impl StreamRun {
    /// Reconstruct every arithmetic reservation and token/ID relationship. Numerical
    /// results use the independent oracle, rather than the admission implementation.
    pub fn audit(
        &self,
        inputs: &[(u64, PixelInput, usize)],
        contexts: &[StreamContext],
        hardware: Hardware,
    ) -> Result<(), String> {
        self.trace.audit().map_err(|e| format!("trace: {e:?}"))?;
        if inputs.is_empty()
            || inputs.len() > 64
            || contexts.is_empty()
            || contexts.len() > 2
            || self.results.len() != inputs.len()
            || self.calendars.len() != contexts.len()
        {
            return Err("stream certificate shape".into());
        }
        let mut ids = BTreeSet::new();
        if inputs.iter().any(|(id, _, context)| {
            *id > u64::from(u32::MAX) || !ids.insert(*id) || *context >= contexts.len()
        }) || hardware.kernel.shared_half
            || hardware.kernel.flat_normal
        {
            return Err("stream certificate ID/context".into());
        }
        let mut resources = BTreeSet::new();
        let mut previous: Option<&StreamResult> = None;
        for (context, s) in contexts.iter().zip(&self.calendars) {
            let p = plan(
                &[inputs[0].1],
                context.material,
                context.light,
                context.projection,
                hardware,
                Storage::Registers,
                Strategy::Interleaved,
            )?;
            s.audit(&p)?;
        }
        for ((id, pixel, context), r) in inputs.iter().zip(&self.results) {
            if r.id != *id || r.context != *context || *context >= contexts.len() {
                return Err("stream identity".into());
            }
            let s = &self.calendars[*context];
            if r.accepted.checked_add(s.latency) != Some(r.completed)
                || previous.is_some_and(|p| {
                    r.completed <= p.completed
                        || p.accepted
                            .checked_add(self.calendars[p.context].initiation_interval)
                            .is_none_or(|next| r.accepted < next)
                })
            {
                return Err("stream ordered timing".into());
            }
            for op in &s.slots {
                if let Some(k) = &op.kind {
                    let cycle = r
                        .accepted
                        .checked_add(op.issue)
                        .ok_or("stream timing overflow")?;
                    if !resources.insert((k.clone(), op.lane.unwrap(), cycle)) {
                        return Err("stream replay resource collision".into());
                    }
                }
            }
            let c = contexts[*context];
            let o = oracle::evaluate(
                *pixel,
                c.material,
                c.light,
                c.projection,
                oracle::Config {
                    rounding: oracle::RoundingPolicy {
                        power: if hardware.kernel.power_floor {
                            oracle::Rounding::Floor
                        } else {
                            oracle::Rounding::NearestEven
                        },
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .map_err(|e| format!("oracle: {e:?}"))?;
            if (i128::from(r.output.g), i128::from(r.output.h)) != (o.g, o.h) {
                return Err("stream numerical output".into());
            }
            previous = Some(r);
        }
        let mut cycle = 0;
        let mut accepted = 0;
        let mut completed = 0;
        for tick in &self.trace.ticks {
            if !tick.request.ce {
                continue;
            }
            if let Some((id, lease)) = tick.request.accept {
                let r = self.results.get(accepted).ok_or("stream accept trace")?;
                if id != r.id || lease.bank != r.context || cycle != r.accepted {
                    return Err("stream accept trace".into());
                }
                accepted += 1;
            }
            if let Some(id) = tick.request.complete {
                let r = self
                    .results
                    .get(completed)
                    .ok_or("stream completion trace")?;
                if id != r.id || cycle != r.completed || tick.committed != Some(id) {
                    return Err("stream completion trace".into());
                }
                completed += 1;
            }
            cycle += 1;
        }
        if accepted != inputs.len() || completed != inputs.len() {
            return Err("stream trace completeness".into());
        }
        Ok(())
    }
}
