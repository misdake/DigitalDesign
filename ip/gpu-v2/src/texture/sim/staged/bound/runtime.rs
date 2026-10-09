//! Live quad ingress over the existing bounded preparation/cache/color path.
//!
//! Coefficients and Work RAM execute actual bounded scalar handshakes. Other
//! preparation arithmetic is counted replay at eligible ingress. No input
//! list or program history is retained. Real cache captures drive ColorEmu;
//! only actual public result consumption returns public lane ownership.
use super::control::{
    Event as PreparationEvent, Hardware as PreparationHardware, Step as PreparationStep,
};
use super::{runtime_preparation::Machine as Preparation, session, Program};
use crate::texture::emu::color::{
    ColorEmu, Event as ColorEvent, Input as ColorInput, Output as ColorOutput, Step as ColorStep,
    Tick as ColorTick,
};
use crate::texture::ports::{QuadInput, RefillPort, Slot};
use crate::texture::sim::timed;

const FAULT: &str = "sampling runtime terminal fault; recreate before reuse";
/// Capture137 + public lanes64 + accepted coverage64 + terminal fault1.
/// Existing preparation/cache/ColorEmu storage is charged by its owner.
pub const LINK_STATE_BITS: usize = 137 + 64 + 64 + 1;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub link: session::Stats,
    pub offers: u64,
    pub rejected: u64,
    /// Successful quad compilations (one preparation/provenance pair), not
    /// hardware operations or pipeline latency.
    pub compilations: u64,
    /// Distinct live preparation sources, bounded by the existing 16 IDs.
    pub peak_preparation_programs: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub wall: u64,
    pub color_input: bool,
    pub result_lanes: [u8; 16],
    pub preparation_live: u16,
    pub preparation_masks: [u8; 16],
    pub color: crate::texture::emu::color::Snapshot,
    pub cache: timed::Snapshot,
}

#[derive(Clone, Debug)]
pub struct Step {
    pub cycle: u64,
    pub control: timed::Control,
    pub effective_ce: bool,
    pub offered: Option<u8>,
    /// Pre-edge ready for the offered ID; CE is a separate acceptance condition.
    pub input_ready: bool,
    pub accepted: bool,
    pub results: Vec<ColorOutput>,
    /// Closed-color lifecycle shadow, never public result ownership.
    pub cache_commits: Vec<timed::PixelResult>,
    pub color: ColorStep,
    pub cache: timed::Step,
    pub preparation: PreparationStep,
    pub snapshot: Snapshot,
}

pub struct Runtime {
    preparation: Preparation,
    cache: timed::Machine,
    color: ColorEmu,
    color_input: Option<ColorInput>,
    remaining: [u8; 16],
    /// Actual accepted coverage for mapping covered-lane ordinals to key6.
    /// Not a golden program table; expires after the last preparation packet.
    masks: [u8; 16],
    wall: u64,
    max_wall: u64,
    faulted: bool,
    pub stats: Stats,
    #[cfg(test)]
    poison: bool,
    #[cfg(test)]
    poison_hits: usize,
}

impl Runtime {
    /// One immutable texture context. To change slots, drain then recreate.
    /// max_wall bounds the lifetime of this instance, not a preloaded workload.
    pub fn new(
        slots: &[Slot],
        preparation_hardware: PreparationHardware,
        mut cache_hardware: timed::Hardware,
        max_wall: u64,
    ) -> Result<Self, String> {
        if max_wall == 0 || max_wall > 2_000_000 {
            return Err("sampling runtime wall bound".into());
        }
        cache_hardware.preparation = timed::PreparationMode::BoundStages;
        cache_hardware.packet_storage = timed::PacketStorage::Pool64;
        if cache_hardware.prefetch
            || cache_hardware.group_capacity != 32
            || preparation_hardware.packet_credits != 16
            || cache_hardware.read_latency != 1
            || cache_hardware.result_capacity != crate::texture::emu::color::RESULT_CAPACITY
        {
            return Err("sampling runtime fixes no hints/P16/G32/read1/result16".into());
        }
        let binding = super::Binding::build()?;
        // Separate allocation receipt; legacy Session inventory/audit stays intact.
        super::runtime_inventory::describe(&binding, preparation_hardware, &cache_hardware)?;
        Ok(Self {
            preparation: Preparation::new(binding, preparation_hardware)?,
            cache: timed::Machine::new(slots.to_vec(), cache_hardware)
                .map_err(|e| format!("cache machine: {e:?}"))?,
            color: ColorEmu::new(max_wall)?,
            color_input: None,
            remaining: [0; 16],
            masks: [0; 16],
            wall: 0,
            max_wall,
            faulted: false,
            stats: Stats::default(),
            #[cfg(test)]
            poison: false,
            #[cfg(test)]
            poison_hits: 0,
        })
    }

    pub fn faulted(&self) -> bool {
        self.faulted
    }

    /// Combinational ready for a caller-owned ID, independent of caller CE.
    /// Conservative pre-edge credit: no same-edge release or color acceptance
    /// is predicted. A downstream credit shortage eventually gates this port.
    pub fn input_ready(&self, quad: u8) -> bool {
        !self.faulted
            && self.wall < self.max_wall
            && quad < 16
            && self.color_input.is_none()
            && self.remaining[usize::from(quad)] == 0
            && self.preparation.input_ready(quad)
            && self.cache.external_ready(quad)
    }

    /// No end-of-stream marker is needed: caller owns future offers.
    pub fn idle(&self) -> bool {
        !self.faulted
            && self.preparation.idle()
            && self.cache.idle()
            && self.color_input.is_none()
            && self.color.idle()
            && self.remaining == [0; 16]
    }

    pub fn cache_stats(&self) -> &timed::Stats {
        &self.cache.stats
    }

    pub fn preparation_stats(&self) -> &super::control::Stats {
        &self.preparation.stats
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            wall: self.wall,
            color_input: self.color_input.is_some(),
            result_lanes: self.remaining,
            preparation_live: self.preparation.live_mask(),
            preparation_masks: self.masks,
            color: self.color.snapshot(),
            cache: self.cache.snapshot(),
        }
    }

    /// Valid is `offered.is_some()`. The borrow transfers no caller storage;
    /// only `accepted` captures its contents. Rejected input may be replaced.
    /// Default-sample is an upstream constant bypass: pass None, create no work.
    /// Terminal errors require recreation; the external owner separately drains
    /// any MC work already accepted. This unit reports no successful cancellation.
    pub fn step<M: RefillPort + ?Sized>(
        &mut self,
        memory: &mut M,
        offered: Option<&QuadInput>,
        control: timed::Control,
    ) -> Result<Step, String> {
        if self.faulted {
            return Err(FAULT.into());
        }
        if self.wall >= self.max_wall {
            self.faulted = true;
            return Err("sampling runtime wall watchdog".into());
        }
        let result = self.step_inner(memory, offered, control);
        if result.is_err() {
            self.faulted = true;
        }
        result
    }

    fn step_inner<M: RefillPort + ?Sized>(
        &mut self,
        memory: &mut M,
        offered: Option<&QuadInput>,
        control: timed::Control,
    ) -> Result<Step, String> {
        if control.ce && offered.is_some_and(|q| q.quad_id >= 16) {
            return Err("sampling runtime quad ID exceeds key6".into());
        }
        let input_ready = offered.is_some_and(|q| self.input_ready(q.quad_id));
        // Host arithmetic replay is prepared only for a real eligible edge.
        // No compile/clone/retention occurs for a CE pause or rejected offer.
        let compiled = if input_ready && control.ce {
            let q = offered.unwrap();
            let (slots, hardware) = self.cache.external_context();
            let preparation = Program::compile(q, slots, self.preparation.binding.clone())?;
            #[cfg(test)]
            let preparation = {
                let mut preparation = preparation;
                if self.poison {
                    let p = std::sync::Arc::get_mut(&mut preparation)
                        .expect("unique just-compiled source");
                    for stage in [&mut p.preparation.derivative, &mut p.preparation.lod] {
                        for output in &mut stage.frame.outputs {
                            output.raw ^= 511;
                            self.poison_hits += 1;
                        }
                    }
                    for lane in &mut p.preparation.lanes {
                        for output in &mut lane.coordinate.frame.outputs {
                            output.raw ^= 1023;
                            self.poison_hits += 1;
                        }
                        for output in &mut lane.coefficient.frame.outputs {
                            output.raw ^= 511;
                            self.poison_hits += 1;
                        }
                        for member in &mut lane.memberships {
                            for output in &mut member.frame.outputs {
                                // All closed Member fields are poisoned, not
                                // merely weights; live pipeline uses none.
                                output.raw ^= 1;
                                self.poison_hits += 1;
                            }
                        }
                        for plane in &mut lane.packets {
                            for packet in plane {
                                for output in &mut packet.frame.outputs {
                                    output.raw ^= 1 << 28;
                                    self.poison_hits += 1;
                                }
                            }
                        }
                    }
                }
                preparation
            };
            let cache = timed::Program::compile(q, slots, hardware)
                .map_err(|e| format!("cache provenance: {e:?}"))?;
            self.stats.compilations += 1;
            Some((q, preparation, cache))
        } else {
            None
        };
        self.wall += 1;
        let color = self.color.tick(ColorTick {
            ce: control.ce,
            input: self.color_input,
            output_ready: control.result_ready,
        })?;
        let mut results = vec![];
        for e in &color.events {
            match e {
                ColorEvent::Commit(o) => {
                    let mask = &mut self.remaining[usize::from(o.key / 4)];
                    let bit = 1 << (o.key % 4);
                    if *mask & bit == 0 {
                        return Err("sampling runtime result for unowned lane".into());
                    }
                    *mask &= !bit;
                    results.push(*o);
                }
                ColorEvent::Accepted { .. } => self.stats.link.color_groups += 1,
                _ => {}
            }
        }
        if color.accepted {
            self.color_input = None;
        } else if self.color_input.is_some() {
            self.stats.link.color_stalls += 1;
        }
        let effective_ce = control.ce && self.color_input.is_none();
        if control.ce && !effective_ce {
            self.stats.link.color_gated_edges += 1;
        }
        let preparation = self.preparation.step_packet_port(
            compiled
                .as_ref()
                .map(|(q, p, _)| (usize::from(q.quad_id), p.clone())),
            effective_ce,
            self.cache.packet_ready(),
            self.cache.packet_issue_ready(),
            &self.masks,
            self.cache.external_context().0,
        )?;
        if preparation.accepted != compiled.is_some() {
            return Err("sampling runtime pre-edge ingress divergence".into());
        }
        if let Some((q, _, _)) = &compiled {
            self.masks[usize::from(q.quad_id)] = q.mask;
            self.remaining[usize::from(q.quad_id)] = q.mask;
            self.stats.link.accepted += 1;
        }
        let mut packet = None;
        let mut done = vec![];
        let mut issues = vec![];
        for e in &preparation.events {
            match e {
                PreparationEvent::Packet { payload, .. } => packet = Some(*payload),
                PreparationEvent::Release { program } => done.push(*program as u8),
                PreparationEvent::Issue {
                    stage: "packet",
                    program,
                    lane,
                    ..
                } => {
                    let mask = self.masks[*program];
                    let physical_lane = (0..4_u8)
                        .filter(|i| mask >> i & 1 != 0)
                        .nth(*lane)
                        .ok_or("sampling runtime packet covered lane")?;
                    issues.push(timed::packet::Owner {
                        quad: *program as u8,
                        lane: physical_lane,
                    });
                }
                _ => {}
            }
        }
        // Cache remains the single MC owner; step_pooled drains memory on every
        // successful wall edge even when the caller or color link freezes CE.
        let cache = self
            .cache
            .step_pooled(
                memory,
                compiled.map(|(q, _, p)| (usize::from(q.quad_id), p)),
                timed::Control {
                    ce: effective_ce,
                    result_ready: true,
                },
                packet,
                done.clone(),
                issues,
            )
            .map_err(|e| format!("cache step: {e:?}"))?;
        if preparation.accepted != cache.accepted {
            return Err("sampling runtime cache ingress divergence".into());
        }
        for q in done {
            self.masks[usize::from(q)] = 0;
        }
        let mut cache_commits = vec![];
        for e in &cache.events {
            match e {
                timed::Event::Captured { group, words, .. } => {
                    if self.color_input.is_some() {
                        return Err("sampling runtime captured-input overwrite".into());
                    }
                    self.color_input = Some(ColorInput {
                        payload: group.pack72()? as i128,
                        texels: *words,
                    });
                    self.stats.link.captures += 1;
                }
                timed::Event::Produced { .. } => self.stats.link.packets += 1,
                timed::Event::Commit { pixel } => cache_commits.push(pixel.clone()),
                _ => {}
            }
        }
        self.stats.offers += u64::from(offered.is_some());
        self.stats.rejected += u64::from(offered.is_some() && !preparation.accepted);
        self.stats.link.results += results.len() as u64;
        self.stats.link.enabled += u64::from(effective_ce);
        self.stats.link.wall = self.wall;
        self.stats.link.cache_result_credit_stalls = self.cache.stats.result_credit_stalls;
        self.stats.link.peak_color_credits = self
            .stats
            .link
            .peak_color_credits
            .max(color.snapshot.result_credits);
        self.stats.link.peak_captured_input = self
            .stats
            .link
            .peak_captured_input
            .max(usize::from(self.color_input.is_some()));
        self.stats.peak_preparation_programs = self
            .stats
            .peak_preparation_programs
            .max(self.preparation.live_mask().count_ones() as usize);
        Ok(Step {
            cycle: self.wall,
            control,
            effective_ce,
            offered: offered.map(|q| q.quad_id),
            input_ready,
            accepted: preparation.accepted,
            results,
            cache_commits,
            color,
            cache,
            preparation,
            snapshot: self.snapshot(),
        })
    }
}

#[cfg(test)]
#[path = "runtime_numerical_qualification.rs"]
mod numerical_qualification;
#[cfg(test)]
#[path = "runtime_qualification.rs"]
mod qualification;
