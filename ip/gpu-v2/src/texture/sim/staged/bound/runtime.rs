//! Live quad ingress over the existing bounded preparation/cache/color path.
//!
//! Raw input drives bounded scalar preparation, Work and packet pipelines.
//! No input-dependent Program is constructed or retained. Real cache captures drive ColorEmu;
//! only actual public result consumption returns public lane ownership.
use super::control::{
    Event as PreparationEvent, Hardware as PreparationHardware, Step as PreparationStep,
};
use super::{runtime_preparation::Machine as Preparation, session};
use crate::texture::emu::color::{
    ColorEmu, Event as ColorEvent, Input as ColorInput, Output as ColorOutput, Step as ColorStep,
    Tick as ColorTick,
};
use crate::texture::emu::derivative;
use crate::texture::ports::{QuadInput, RawQuadInput, RefillPort, Slot};
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
    /// Accepted live inputs; independent of covered-lane or packet counts.
    pub admissions: u64,
    /// Compatibility diagnostic: always zero on this live path.
    pub compilations: u64,
    /// Compatibility diagnostic: no live preparation Programs are retained.
    pub peak_preparation_programs: usize,
    /// Distinct live preparation IDs, bounded by the existing 16 IDs.
    pub peak_live_quads: usize,
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
}

#[derive(Clone, Copy)]
enum Offer<'a> {
    Quantized(&'a RawQuadInput),
    Legacy(&'a QuadInput),
}
impl Offer<'_> {
    fn quad(self) -> u8 {
        match self {
            Self::Quantized(q) => q.quad_id,
            Self::Legacy(q) => q.quad_id,
        }
    }
    fn capture(self) -> Result<RawQuadInput, String> {
        match self {
            Self::Quantized(q) => Ok(*q),
            Self::Legacy(q) => RawQuadInput::capture(q),
        }
    }
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
        self.advance(memory, offered.map(Offer::Legacy), control)
    }

    /// Integer rasterizer input and stable draw metadata. Captures only on an
    /// eligible CE edge; helper UVs and bias never make a floating-point round trip.
    pub fn step_raw<M: RefillPort + ?Sized>(
        &mut self,
        memory: &mut M,
        offered: Option<&RawQuadInput>,
        control: timed::Control,
    ) -> Result<Step, String> {
        self.advance(memory, offered.map(Offer::Quantized), control)
    }

    fn advance<M: RefillPort + ?Sized>(
        &mut self,
        memory: &mut M,
        offered: Option<Offer<'_>>,
        control: timed::Control,
    ) -> Result<Step, String> {
        if self.faulted {
            return Err(FAULT.into());
        }
        if self.wall >= self.max_wall {
            self.faulted = true;
            return Err("sampling runtime wall watchdog".into());
        }
        #[cfg(test)]
        let _live = super::counted_call_guard::Scope::enter();
        let result = self.step_inner(memory, offered, control);
        if result.is_err() {
            self.faulted = true;
        }
        result
    }

    fn step_inner<M: RefillPort + ?Sized>(
        &mut self,
        memory: &mut M,
        offered: Option<Offer<'_>>,
        control: timed::Control,
    ) -> Result<Step, String> {
        if control.ce && offered.is_some_and(|q| q.quad() >= 16) {
            return Err("sampling runtime quad ID exceeds key6".into());
        }
        let input_ready = offered.is_some_and(|q| self.input_ready(q.quad()));
        // Capture only boundary fields, never input-dependent arithmetic or
        // expected packet/result counts. Each stage computes from old registers.
        let captured = if input_ready && control.ce {
            let q = offered.unwrap().capture()?;
            let slots = self.cache.external_context().0;
            let slot = *slots
                .get(usize::from(q.slot))
                .ok_or("sampling runtime slot")?;
            Some(derivative::Input::from_raw(&q, slot)?)
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
            captured,
            effective_ce,
            self.cache.packet_ready(),
            self.cache.packet_issue_ready(),
            &self.masks,
        )?;
        if preparation.accepted != captured.is_some() {
            return Err("sampling runtime pre-edge ingress divergence".into());
        }
        if let Some(q) = captured {
            self.masks[usize::from(q.header.quad)] = q.header.mask;
            self.remaining[usize::from(q.header.quad)] = q.header.mask;
            self.stats.link.accepted += 1;
            self.stats.admissions += 1;
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
        // Cache remains the single MC owner; step_live drains memory on every
        // successful wall edge even when the caller or color link freezes CE.
        let cache = self
            .cache
            .step_live(
                memory,
                captured.map(|q| timed::LiveAdmission {
                    quad: q.header.quad,
                    mask: q.header.mask,
                    slot: q.header.slot,
                }),
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
        self.stats.peak_live_quads = self
            .stats
            .peak_live_quads
            .max(self.preparation.live_mask().count_ones() as usize);
        Ok(Step {
            cycle: self.wall,
            control,
            effective_ce,
            offered: offered.map(Offer::quad),
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
