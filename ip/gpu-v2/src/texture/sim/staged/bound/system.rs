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
        let mut previous_producer = 0;
        let pooled = self.cache.hardware.packet_storage == timed::PacketStorage::Pool64;
        if pooled && self.preparation_hardware.packet_credits != 16 {
            return Err("composition fixed producer16 contract".into());
        }
        let mut previous_live = 0_u16;
        for (p, c) in self.preparation.iter().zip(&self.cache.steps) {
            if p.ready != (pooled || previous_groups < self.cache.hardware.group_capacity)
                || p.packet_issue_ready != (!pooled || previous_producer < 16)
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
            let got = m.step_packet_port(offer, c.control.ce, p.ready, p.packet_issue_ready)?;
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
            if packet_issues(p, &self.programs, pooled)? != c.packet_issues {
                return Err("composition packet reservation provenance".into());
            }
            previous_groups = c.snapshot.groups;
            previous_producer = c.snapshot.packet_pool.as_ref().map_or(0, |s| s.producer);
            if pooled && c.snapshot.packet_pool.as_ref().unwrap().unwritten != m.pending_packets() {
                return Err("composition producer credit duplicated/lost".into());
            }
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
        if self.preparation_hardware.storage == Storage::Packed {
            storage::audit_trace(self)?;
        }
        Ok(())
    }
}
pub(super) fn packet_issues(
    step: &Step,
    programs: &[Arc<Program>],
    pooled: bool,
) -> Result<Vec<timed::packet::Owner>, String> {
    if !pooled {
        return Ok(vec![]);
    }
    step.events
        .iter()
        .filter_map(|e| match e {
            Event::Issue {
                stage: "packet",
                program,
                lane,
                ..
            } => Some(
                programs
                    .get(*program)
                    .ok_or_else(|| "packet issue program".to_owned())
                    .and_then(|p| {
                        let physical_lane = (0..4_u8)
                            .filter(|i| p.input.mask >> i & 1 != 0)
                            .nth(*lane)
                            .ok_or_else(|| "packet issue covered lane".to_owned())?;
                        Ok(timed::packet::Owner {
                            quad: p.input.quad_id,
                            lane: physical_lane,
                        })
                    }),
            ),
            _ => None,
        })
        .collect()
}
/// Minimal replacement of Group payload storage; all other preparation/cache
/// and color boundaries use the existing checked implementation.
pub fn run_pooled<M: RefillPort + ?Sized>(
    inputs: &[QuadInput],
    slots: &[Slot],
    memory: &mut M,
    preparation_hardware: Hardware,
    mut cache_hardware: timed::Hardware,
    control: impl FnMut(u64) -> timed::Control,
) -> Result<Report, String> {
    cache_hardware.packet_storage = timed::PacketStorage::Pool64;
    run(
        inputs,
        slots,
        memory,
        preparation_hardware,
        cache_hardware,
        control,
    )
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
    let pooled = cache_hardware.packet_storage == timed::PacketStorage::Pool64;
    if pooled && (preparation_hardware.packet_credits != 16 || cache_hardware.group_capacity != 32)
    {
        return Err("pooled producer16/Group32 are fixed".into());
    }
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
        let ps = p.step_packet_port(offer, ctl.ce, c.packet_ready(), c.packet_issue_ready())?;
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
        let offered_cache = if ps.accepted {
            Some((next, cache_programs[next].clone()))
        } else {
            None
        };
        let issues = packet_issues(&ps, &programs, pooled)?;
        let cs = if pooled {
            c.step_pooled(memory, offered_cache, ctl, packet, done, issues)
        } else {
            c.step_external(memory, offered_cache, ctl, packet, done)
        }
        .map_err(|e| format!("cache step: {e:?}"))?;
        if ps.accepted != cs.accepted {
            return Err("composition handshake divergence".into());
        }
        if pooled && cs.snapshot.packet_pool.as_ref().unwrap().unwritten != p.pending_packets() {
            return Err("pooled producer credit not shared with preparation".into());
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
