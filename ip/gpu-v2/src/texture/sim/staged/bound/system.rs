//! Checked preparation/cache/color composition. Source MC is chosen externally.
use super::{control::*, *};
use crate::texture::sim::timed;
pub struct Report {
    pub cache: timed::Report,
    pub preparation_hardware: Hardware,
    pub binding: Arc<Binding>,
    pub programs: Vec<Arc<Program>>,
    pub preparation: Vec<Step>,
    pub stats: Stats,
}
impl Report {
    pub fn audit(&self) -> Result<(), String> {
        self.cache
            .audit()
            .map_err(|e| format!("cache audit: {e:?}"))?;
        if self.preparation.len() != self.cache.steps.len()
            || self.programs.len() != self.cache.programs.len()
        {
            return Err("composition trace length".into());
        }
        let mut m = Machine::new(self.binding.clone(), self.preparation_hardware)?;
        let mut next = 0;
        let mut previous_groups = 0;
        let mut previous_live = 0_u16;
        for (p, c) in self.preparation.iter().zip(&self.cache.steps) {
            if p.ready != (previous_groups < self.cache.hardware.group_capacity)
                || p.ce != c.control.ce
            {
                return Err("composition ready/CE source".into());
            }
            let offer = match p.offered {
                Some(i) if i == next => {
                    let program = self.programs.get(i).ok_or("prep offer index")?;
                    if previous_live >> program.input.quad_id & 1 != 0 {
                        return Err("outer quad still owns results".into());
                    }
                    Some((i, program.clone()))
                }
                None => None,
                _ => return Err("prep offer order".into()),
            };
            let got = m.step(offer, c.control.ce, p.ready)?;
            if &got != p || c.accepted != p.accepted {
                return Err("composition admission/replay".into());
            }
            if p.accepted {
                next += 1;
            }
            let packet = p.events.iter().find_map(|e| {
                if let Event::Packet { payload, .. } = e {
                    Some(*payload)
                } else {
                    None
                }
            });
            let done: Vec<_> = p
                .events
                .iter()
                .filter_map(|e| {
                    if let Event::Release { program } = e {
                        Some(self.programs[*program].input.quad_id)
                    } else {
                        None
                    }
                })
                .collect();
            if packet != c.external_packet || done != c.prepared {
                return Err("composition port provenance".into());
            }
            previous_groups = c.snapshot.groups;
            previous_live = c.snapshot.live_quads;
        }
        if !m.idle() || next != self.programs.len() || m.stats != self.stats {
            return Err("composition preparation drain".into());
        }
        self.cache
            .hardware
            .inventory()?
            .audit_issues(&m.dsp_issues, None, 2_000_000)
            .map_err(|e| format!("composition coefficient packing: {e:?}"))?;
        Ok(())
    }
}
pub fn run<M: RefillPort + ?Sized>(
    inputs: &[QuadInput],
    slots: &[Slot],
    memory: &mut M,
    preparation_hardware: Hardware,
    mut cache_hardware: timed::Hardware,
    mut control: impl FnMut(u64) -> timed::Control,
) -> Result<Report, String> {
    cache_hardware.preparation = timed::PreparationMode::BoundStages;
    if cache_hardware.prefetch {
        return Err("bound acceptance profile requires no prefetch".into());
    }
    if inputs.len() > cache_hardware.max_quads {
        return Err("composition quad bound".into());
    }
    let binding = Binding::build()?;
    let programs = inputs
        .iter()
        .map(|q| Program::compile(q, slots, binding.clone()))
        .collect::<Result<Vec<_>, _>>()?;
    let cache_programs = inputs
        .iter()
        .map(|q| {
            timed::Program::compile(q, slots, &cache_hardware)
                .map_err(|e| format!("cache golden: {e:?}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut p = Machine::new(binding.clone(), preparation_hardware)?;
    let mut c = timed::Machine::new(slots.to_vec(), cache_hardware.clone())
        .map_err(|e| format!("cache machine: {e:?}"))?;
    let mut next = 0;
    let mut prep_steps = vec![];
    let mut steps = vec![];
    let mut pixels = vec![];
    while next < inputs.len() || !p.idle() || !c.idle() {
        let ctl = control(c.stats.wall_cycles + 1);
        let offer = programs
            .get(next)
            .filter(|pr| c.external_ready(pr.input.quad_id))
            .map(|pr| (next, pr.clone()));
        let ps = p.step(offer, ctl.ce, c.packet_ready())?;
        let packet = ps.events.iter().find_map(|e| {
            if let Event::Packet { payload, .. } = e {
                Some(*payload)
            } else {
                None
            }
        });
        let done = ps
            .events
            .iter()
            .filter_map(|e| {
                if let Event::Release { program } = e {
                    Some(programs[*program].input.quad_id)
                } else {
                    None
                }
            })
            .collect();
        let cs = c
            .step_external(
                memory,
                if ps.accepted {
                    Some((next, cache_programs[next].clone()))
                } else {
                    None
                },
                ctl,
                packet,
                done,
            )
            .map_err(|e| format!("cache step: {e:?}"))?;
        if ps.accepted != cs.accepted {
            return Err("composition handshake divergence".into());
        }
        if ps.accepted {
            next += 1;
        }
        for e in &cs.events {
            if let timed::Event::Commit { pixel } = e {
                pixels.push(pixel.clone());
            }
        }
        prep_steps.push(ps);
        steps.push(cs);
    }
    let report = Report {
        cache: timed::Report {
            hardware: cache_hardware,
            slots: slots.to_vec(),
            programs: cache_programs,
            steps,
            pixels,
            stats: c.stats,
        },
        preparation_hardware,
        binding,
        programs,
        preparation: prep_steps,
        stats: p.stats,
    };
    report.audit()?;
    Ok(report)
}
