//! Persistent per-cycle connection unit for the sampling backend.
//!
//! The existing prepared/pooled/cache path already exposes a persistent
//! per-cycle interface (`control::Machine::step_packet_port` and
//! `timed::Machine::step_pooled`), so this module is only an adapter: it owns
//! both machines across calls, transfers an offered quad's ownership only on a
//! real `accepted` edge, and consumes committed results only on a real
//! `result_ready` edge.
//!
//! The independently verified `emu::color::ColorEmu` is driven live by the
//! actual cache `Captured` texels and the actual packet payloads. Golden
//! `timed::Program` data is used only by the cache's provenance check; it never
//! feeds ColorEmu and never releases payload/credit. The cache's own closed
//! payload color is kept as the existing lifecycle owner and is reported
//! separately; it is not the public result stream.
use super::control::{
    Event as PreparationEvent, Hardware as PreparationHardware, Machine as Preparation,
    Step as PreparationStep,
};
use super::system::packet_issues;
use super::Program;
use crate::texture::emu::color::{
    ColorEmu, Event as ColorEvent, Input as ColorInput, Output as ColorOutput, Step as ColorStep,
    Tick as ColorTick,
};
use crate::texture::ports::{QuadInput, RefillPort, Slot};
use crate::texture::sim::timed;
use std::sync::Arc;

const FAULT: &str = "sampling session terminal fault; recreate before reuse";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub wall: u64,
    pub enabled: u64,
    pub accepted: u64,
    pub packets: u64,
    pub captures: u64,
    pub color_groups: u64,
    pub results: u64,
    /// Edges the bounded color backpressure held the cache (not a caller CE).
    pub color_gated_edges: u64,
    pub color_stalls: u64,
    pub peak_captured_input: usize,
    pub peak_color_credits: usize,
    pub cache_result_credit_stalls: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub wall: u64,
    pub color_input: bool,
    pub result_lanes: [u8; 16],
    pub color: crate::texture::emu::color::Snapshot,
    pub cache: timed::Snapshot,
    pub next: usize,
    pub programs: usize,
}

#[derive(Clone, Debug)]
pub struct Step {
    pub cycle: u64,
    pub control: timed::Control,
    /// Caller control plus the bounded color link; `false` does not stop MC/refill.
    pub effective_ce: bool,
    pub offered: Option<usize>,
    pub accepted: bool,
    /// Actual ColorEmu commits: the public result stream of this unit.
    pub results: Vec<ColorOutput>,
    /// Existing cache/closed-color commits, kept for independent cross-check.
    pub cache_commits: Vec<timed::PixelResult>,
    pub color: ColorStep,
    pub cache: timed::Step,
    pub preparation: PreparationStep,
    pub snapshot: Snapshot,
}

pub struct Session {
    preparation: Preparation,
    cache: timed::Machine,
    color: ColorEmu,
    programs: Vec<Arc<Program>>,
    cache_programs: Vec<Arc<timed::Program>>,
    next: usize,
    color_input: Option<ColorInput>,
    /// Required public-result lanes. Cache's closed-color commit is not an
    /// ownership release for the independent ColorEmu result stream.
    remaining: [u8; 16],
    wall: u64,
    max_wall: u64,
    faulted: bool,
    pub stats: Stats,
}

impl Session {
    pub fn new(
        inputs: &[QuadInput],
        slots: &[Slot],
        preparation_hardware: PreparationHardware,
        mut cache_hardware: timed::Hardware,
        max_wall: u64,
    ) -> Result<Self, String> {
        if max_wall == 0 || max_wall > 2_000_000 {
            return Err("sampling session wall bound".into());
        }
        cache_hardware.preparation = timed::PreparationMode::BoundStages;
        cache_hardware.packet_storage = timed::PacketStorage::Pool64;
        if cache_hardware.prefetch {
            return Err("sampling session requires no prefetch".into());
        }
        if cache_hardware.group_capacity != 32 {
            return Err("sampling session fixes Group32".into());
        }
        if preparation_hardware.packet_credits != 16 {
            return Err("sampling session fixes producer16".into());
        }
        if cache_hardware.read_latency != 1 {
            return Err("sampling session fixes one-edge cache reads".into());
        }
        if cache_hardware.result_capacity != crate::texture::emu::color::RESULT_CAPACITY {
            return Err("sampling session fixes result16".into());
        }
        if inputs.len() > cache_hardware.max_quads {
            return Err("sampling session quad bound".into());
        }
        // The connection keeps the single texture RefillPort as the only MC owner.
        let binding = super::Binding::build()?;
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
        let preparation = Preparation::new(binding, preparation_hardware)?;
        let cache = timed::Machine::new(slots.to_vec(), cache_hardware)
            .map_err(|e| format!("cache machine: {e:?}"))?;
        let color = ColorEmu::new(max_wall)?;
        Ok(Self {
            preparation,
            cache,
            color,
            programs,
            cache_programs,
            next: 0,
            color_input: None,
            remaining: [0; 16],
            wall: 0,
            max_wall,
            faulted: false,
            stats: Stats::default(),
        })
    }

    pub fn faulted(&self) -> bool {
        self.faulted
    }

    pub fn idle(&self) -> bool {
        self.next == self.programs.len()
            && self.preparation.idle()
            && self.cache.idle()
            && self.color_input.is_none()
            && self.color.idle()
            && self.remaining == [0; 16]
    }

    pub fn cache_stats(&self) -> &timed::Stats {
        &self.cache.stats
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            wall: self.wall,
            color_input: self.color_input.is_some(),
            result_lanes: self.remaining,
            color: self.color.snapshot(),
            cache: self.cache.snapshot(),
            next: self.next,
            programs: self.programs.len(),
        }
    }

    pub fn step<M: RefillPort + ?Sized>(
        &mut self,
        memory: &mut M,
        control: timed::Control,
    ) -> Result<Step, String> {
        if self.faulted {
            return Err(FAULT.into());
        }
        if self.wall >= self.max_wall {
            self.faulted = true;
            return Err("sampling session wall watchdog".into());
        }
        let result = self.step_inner(memory, control);
        if result.is_err() {
            self.faulted = true;
        }
        result
    }

    fn step_inner<M: RefillPort + ?Sized>(
        &mut self,
        memory: &mut M,
        control: timed::Control,
    ) -> Result<Step, String> {
        self.wall += 1;
        let cycle = self.wall;
        let mut results = vec![];

        // 1. Drive the actual color block from the pending actual capture. It is
        //    held until a real accepted edge, so no result credit is returned
        //    before the output is actually consumed.
        let color = self.color.tick(ColorTick {
            ce: control.ce,
            input: self.color_input,
            output_ready: control.result_ready,
        })?;
        for e in &color.events {
            match e {
                ColorEvent::Commit(o) => {
                    let mask = &mut self.remaining[usize::from(o.key / 4)];
                    let bit = 1 << (o.key % 4);
                    if *mask & bit == 0 {
                        return Err("sampling result for unowned lane".into());
                    }
                    *mask &= !bit;
                    results.push(*o);
                }
                ColorEvent::Accepted { .. } => self.stats.color_groups += 1,
                _ => {}
            }
        }
        if color.accepted {
            self.color_input = None;
        } else if self.color_input.is_some() {
            self.stats.color_stalls += 1;
        }
        // One 72+64-bit captured-input register. A real ColorEmu acceptance
        // releases it; only then may the cache advance its existing CE-gated
        // return stage. The reserved cache read is still bounded in its owner.
        // No additional packet/texel FIFO is layered on top of that pipeline.
        let effective_ce = control.ce && self.color_input.is_none();
        if control.ce && !effective_ce {
            self.stats.color_gated_edges += 1;
        }

        // 3. Existing persistent preparation step; ownership moves only here.
        let offered = self
            .programs
            .get(self.next)
            .filter(|p| {
                self.remaining[usize::from(p.input().quad_id)] == 0
                    && self.cache.external_ready(p.input().quad_id)
            })
            .map(|p| (self.next, p.clone()));
        let preparation = self.preparation.step_packet_port(
            offered,
            effective_ce,
            self.cache.packet_ready(),
            self.cache.packet_issue_ready(),
        )?;
        let packet = preparation.events.iter().find_map(|e| match e {
            PreparationEvent::Packet { payload, .. } => Some(*payload),
            _ => None,
        });
        let done = preparation
            .events
            .iter()
            .filter_map(|e| match e {
                PreparationEvent::Release { program } => {
                    Some(self.programs[*program].input().quad_id)
                }
                _ => None,
            })
            .collect();
        let issues = packet_issues(&preparation, &self.programs, true)?;

        // 4. Existing persistent cache/pool step, driven by the actual packet.
        let cache_offer = if preparation.accepted {
            Some((self.next, self.cache_programs[self.next].clone()))
        } else {
            None
        };
        // The cache's own closed payload color is an existing lifecycle owner:
        // it is kept drained so it does not become the result/credit authority.
        // The independently verified ColorEmu above owns the public result
        // credit and the real backpressure.
        let cache = self
            .cache
            .step_pooled(
                memory,
                cache_offer,
                timed::Control {
                    ce: effective_ce,
                    result_ready: true,
                },
                packet,
                done,
                issues,
            )
            .map_err(|e| format!("cache step: {e:?}"))?;
        if preparation.accepted != cache.accepted {
            return Err("sampling session handshake divergence".into());
        }

        // 5. Actual capture into the reserved input register. Golden data never
        // reaches ColorEmu. At most one capture occurs on an advancing edge.
        for e in &cache.events {
            match e {
                timed::Event::Captured { group, words, .. } => {
                    let payload = group.pack72()? as i128;
                    if self.color_input.is_some() {
                        return Err("sampling session captured-input overwrite".into());
                    }
                    self.color_input = Some(ColorInput {
                        payload,
                        texels: *words,
                    });
                    self.stats.captures += 1;
                }
                timed::Event::Produced { .. } => self.stats.packets += 1,
                _ => {}
            }
        }
        let mut cache_commits = vec![];
        for e in &cache.events {
            if let timed::Event::Commit { pixel } = e {
                cache_commits.push(pixel.clone());
            }
        }
        self.stats.results += results.len() as u64;
        self.stats.cache_result_credit_stalls = self.cache.stats.result_credit_stalls;
        self.stats.peak_color_credits = self
            .stats
            .peak_color_credits
            .max(color.snapshot.result_credits);
        self.stats.peak_captured_input = self
            .stats
            .peak_captured_input
            .max(usize::from(self.color_input.is_some()));
        if effective_ce {
            self.stats.enabled += 1;
        }
        if preparation.accepted {
            let q = self.programs[self.next].input();
            if self.remaining[usize::from(q.quad_id)] != 0 {
                return Err("sampling admission reused a live result identity".into());
            }
            self.remaining[usize::from(q.quad_id)] = q.mask;
            self.next += 1;
            self.stats.accepted += 1;
        }
        self.stats.wall = self.wall;
        Ok(Step {
            cycle,
            control,
            effective_ce,
            offered: preparation.offered,
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
