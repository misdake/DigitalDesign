//! Persistent Sampling Runtime and LightingLive connection to J1/ROP.
//! A single captured offer bridges actual allocation to runtime acceptance.
//! Counted ingress/preparation and closed-cache arithmetic remain replay; only
//! real ColorEmu results feed the sample store. Two memory ports are distinct.

use super::{Context, LightingLive, LiveCycle, LiveQuad, LiveTick, Phase, SampleWrite};
use super::{PixelKey, Ticket};
use crate::lighting::ports::LightingContext;
use crate::memory::ports::MemoryPort;
use crate::texture::{
    ports::{QuadInput as TextureQuad, RefillPort, Slot},
    sim::{
        staged::bound::{control, runtime::Runtime},
        timed,
    },
};

#[derive(Clone, Debug)]
pub struct BranchQuad {
    pub live: LiveQuad,
    /// Helper UVs include all four lanes, even when coverage is partial. The
    /// caller quad_id is replaced only after actual J1 allocation assigns it.
    /// Default-sample/zero-coverage inputs never compile or issue sampling.
    pub sample: Option<TextureQuad>,
}

#[derive(Clone, Debug)]
pub struct BranchTick {
    pub ce: bool,
    pub final_ready: bool,
    pub light_ready: bool,
    /// Delays the held offer, without freezing accepted runtime work.
    pub sample_issue_ready: bool,
    /// Backpressure on the real sample-store write, independent of final.
    pub sample_ready: bool,
    pub quad: Option<BranchQuad>,
    pub finish: bool,
}
impl Default for BranchTick {
    fn default() -> Self {
        Self {
            ce: true,
            final_ready: true,
            light_ready: true,
            sample_issue_ready: true,
            sample_ready: true,
            quad: None,
            finish: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct BranchCycle {
    pub live: LiveCycle,
    pub sampling: Option<crate::texture::sim::staged::bound::runtime::Step>,
    pub sample_admitted: Option<Ticket>,
    pub sample_returned: Option<SampleWrite>,
    pub input_held: bool,
}

/// One offer: helper UV8x18, bias16, slot4, size4, filter2, mask4, quad4,
/// force-coarsest1 and valid1 = 180 logical bits. Terminal fault adds one bit. Integer containers
/// are host representations of these checked codes, not fitted storage.
pub const SAMPLING_OFFER_BITS: usize = 8 * 18 + 16 + 4 + 4 + 2 + 4 + 4 + 1 + 1;
#[derive(Clone, Copy)]
struct Offer {
    uv: [[i64; 2]; 4],
    force_coarsest: bool,
    bias: i16,
    slot: u8,
    size: u8,
    filter: crate::texture::ports::Filter,
    mask: u8,
    quad: u8,
}
impl Offer {
    /// Same external RNE Q16/Q8 capture as the frozen counted format. Decode
    /// produces exact binary rationals, so Runtime's capture is idempotent.
    /// This is host ingress capture, not independent preparation arithmetic.
    fn capture(input: &TextureQuad, quad: u8) -> Result<Self, String> {
        if input.mask > 15
            || input.slot > 15
            || input.material_size_log2 > 10
            || !input.lod_bias.is_finite()
            || input
                .uv
                .iter()
                .flatten()
                .any(|v| !v.is_finite() || v.abs() > 1_048_576.0)
        {
            return Err("sampling offer capture bounds".into());
        }
        let (uv, force_coarsest) = crate::texture::ports::capture_uv(input)?;
        Ok(Self {
            uv,
            force_coarsest,
            bias: (input.lod_bias.clamp(-32.0, 32.0) * 256.0).round_ties_even() as i16,
            slot: input.slot,
            size: input.material_size_log2,
            filter: input.filter,
            mask: input.mask,
            quad,
        })
    }
    fn input(self) -> TextureQuad {
        TextureQuad {
            force_coarsest: self.force_coarsest,
            quad_id: self.quad,
            mask: self.mask,
            uv: std::array::from_fn(|lane| {
                std::array::from_fn(|axis| self.uv[lane][axis] as f64 / 65536.0)
            }),
            slot: self.slot,
            material_size_log2: self.size,
            filter: self.filter,
            lod_bias: f64::from(self.bias) / 256.0,
        }
    }
}

/// Persistent runtime owns all existing preparation/cache/color capacities.
/// No result register/FIFO or new owner table is added. The offer exists from
/// actual J1 allocation to Runtime acceptance, including finish and CE stalls.
/// LightingLive's existing witnesses remain until actual global retirement.
pub struct PixelBranches {
    live: LightingLive,
    sample: Runtime,
    offer: Option<Offer>,
    faulted: bool,
}
impl PixelBranches {
    pub fn new(
        pixel: Context,
        light: LightingContext,
        slots: Vec<Slot>,
        max_cycles: u64,
    ) -> Result<Self, String> {
        if slots.len() > 16 || max_cycles > 2_000_000 {
            return Err("pixel branches bounds".into());
        }
        for slot in &slots {
            slot.validate()?;
        }
        Ok(Self {
            live: LightingLive::new(pixel, light, max_cycles)?,
            sample: Runtime::new(
                &slots,
                control::Hardware {
                    storage: control::Storage::Packed,
                    ..Default::default()
                },
                timed::Hardware {
                    prefetch: false,
                    max_cycles,
                    ..Default::default()
                },
                max_cycles,
            )?,
            offer: None,
            faulted: false,
        })
    }
    pub fn snapshot(&self) -> super::Snapshot {
        self.live.snapshot()
    }
    pub fn sampling_snapshot(&self) -> crate::texture::sim::staged::bound::runtime::Snapshot {
        self.sample.snapshot()
    }
    pub fn sampling_stats(&self) -> &crate::texture::sim::staged::bound::runtime::Stats {
        &self.sample.stats
    }
    pub fn complete(&self) -> bool {
        !self.faulted && self.live.complete() && self.sample.idle() && self.offer.is_none()
    }
    /// Terminal sampling state is preserved. Caller separately drains accepted
    /// texture transport; fault steps below drain only the framebuffer port.
    pub fn abort(&mut self) {
        self.faulted = true;
        self.offer = None;
        self.live.abort();
    }
    pub fn faulted(&self) -> bool {
        self.faulted
    }
    pub fn framebuffer_drained(&self) -> bool {
        self.live.drained()
    }
    pub fn sampling_idle(&self) -> bool {
        self.sample.idle() && self.offer.is_none()
    }
    pub fn step(
        &mut self,
        tick: BranchTick,
        texture: &mut impl RefillPort,
        framebuffer: &mut impl MemoryPort,
    ) -> Result<BranchCycle, String> {
        let result = self.step_inner(tick, texture, framebuffer);
        if result.is_err() {
            self.abort();
        }
        result
    }
    fn step_inner(
        &mut self,
        tick: BranchTick,
        texture: &mut impl RefillPort,
        framebuffer: &mut impl MemoryPort,
    ) -> Result<BranchCycle, String> {
        // Actual pre-edge offer credit only. An acceptance on this edge cannot
        // fund a same-edge global allocation into this register.
        let offer_free = self.offer.is_none();
        let running =
            matches!(self.live.snapshot().phase, Phase::Running | Phase::Closing) && !self.faulted;
        let offered_input = self
            .offer
            .filter(|_| tick.sample_issue_ready)
            .map(Offer::input);
        let mut sampling = None;
        let mut sample_admitted = None;
        let mut sample_returned = None;
        if !self.faulted {
            let step = self.sample.step(
                texture,
                offered_input.as_ref(),
                timed::Control {
                    ce: tick.ce,
                    result_ready: tick.sample_ready,
                },
            )?;
            if step.accepted {
                let offer = self
                    .offer
                    .take()
                    .ok_or("sampling accepted without allocated offer")?;
                sample_admitted = Some(
                    self.live
                        .allocated_ticket(offer.quad)
                        .ok_or("sampling offer lost global owner")?,
                );
            }
            if step.results.len() > 1 {
                return Err("sample store single write port".into());
            }
            if let Some(value) = step.results.first() {
                let ticket = self
                    .live
                    .allocated_ticket(value.key / 4)
                    .ok_or("sampling result without global owner")?;
                sample_returned = Some(SampleWrite {
                    key: PixelKey {
                        ticket,
                        lane: value.key % 4,
                    },
                    rgb: value.rgb,
                });
            }
            sampling = Some(step);
        }
        let offered = tick
            .quad
            .as_ref()
            .filter(|q| q.live.quad.default_sample || q.live.quad.header.mask == 0 || offer_free);
        if let Some(q) = offered.filter(|q| {
            running
                && tick.ce
                && !tick.finish
                && !q.live.quad.default_sample
                && q.live.quad.header.mask != 0
        }) {
            let sample = q
                .sample
                .as_ref()
                .ok_or("nondefault quad missing helper UVs")?;
            if sample.mask != q.live.quad.header.mask {
                return Err("sampling coverage mismatch".into());
            }
        }
        let live = self.live.step(
            LiveTick {
                ce: tick.ce,
                final_ready: tick.final_ready,
                light_ready: tick.light_ready,
                quad: offered.map(|q| q.live),
                sample: sample_returned,
                finish: tick.finish,
            },
            framebuffer,
        )?;
        if matches!(
            live.model.snapshot.phase,
            Phase::FaultDraining | Phase::Faulted
        ) {
            self.faulted = true;
            self.offer = None;
        }
        if !self.faulted && live.model.sample_accepted != sample_returned.is_some() {
            return Err("actual sampling result lost by sample store".into());
        }
        if self.faulted {
            sample_returned = None;
        }
        if live.model.quad_accepted {
            if let Some(ticket) = live.model.ticket {
                let q = offered.ok_or("allocated quad absent")?;
                if !q.live.quad.default_sample {
                    self.offer = Some(Offer::capture(
                        q.sample.as_ref().ok_or("allocated sampling input absent")?,
                        ticket.quad,
                    )?);
                }
            }
        }
        let input_held = tick.quad.is_some() && !live.model.quad_accepted;
        Ok(BranchCycle {
            live,
            sampling,
            sample_admitted,
            sample_returned,
            input_held,
        })
    }
}
