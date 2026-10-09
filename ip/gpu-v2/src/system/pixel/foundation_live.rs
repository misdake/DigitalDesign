//! Replacement composition entry: published row pipeline plus actual branch cores.
//! Lighting selects the frozen free/Floor Q13 calendar, without editing its DAG.
//! Context changes still drain that leaf locally. Sampling consumes raw integer
//! attributes; this adapter does not turn the combined controller into RTL.

use super::{
    dispatch::{CommonContext, ContextId, RopRow},
    foundation::{self, Beat, Pipeline},
    LightWrite, PixelKey, SampleWrite,
};
use crate::{
    lighting::{
        calendars::UnifiedCalendar, emu::LightingEmu, ports::*, LightingProfile,
        LightingQuantization,
    },
    texture::{
        ports::{RawQuadInput, RefillPort, Slot},
        sim::{
            staged::bound::{control, runtime::Runtime},
            timed,
        },
    },
};

#[derive(Clone, Copy, Debug, Default)]
pub struct Tick {
    pub ce: bool,
    pub input: Option<Beat>,
    pub lighting_issue_ready: bool,
    pub lighting_result_ready: bool,
    pub sampling_issue_ready: bool,
    pub sampling_result_ready: bool,
    pub final_issue_ready: bool,
    pub final_result_ready: bool,
    pub rop_ready: bool,
}

pub struct Step {
    pub pixels: foundation::Step,
    pub lighting: LightingSignals,
    pub sampling: crate::texture::sim::staged::bound::runtime::Step,
    pub lighting_context_wait: bool,
    pub sampling_alias_wait: bool,
    pub sample_written: Option<SampleWrite>,
}

#[derive(Clone, Copy)]
struct SampleDestination {
    // Serial/full ticket are host witnesses. Hardware keeps only destination
    // high1, valid1 and remaining-mask4 for each fixed low-four-bit public ID.
    ticket: super::Ticket,
    remaining: u8,
}

pub struct Live {
    pipeline: Pipeline,
    lighting: LightingEmu,
    sampling: Runtime,
    loaded_light: Option<ContextId>,
    sample_destinations: [Option<SampleDestination>; 16],
    slot_sizes: [Option<u8>; 16],
    faulted: bool,
}

impl Live {
    pub fn new(slots: &[Slot], max_wall: u64) -> Result<Self, String> {
        let quantization = LightingQuantization::CompensatedFloor;
        let calendar = UnifiedCalendar::selected(quantization);
        Ok(Self {
            pipeline: Pipeline::new(max_wall)?,
            lighting: LightingEmu::with_schedule_plans(
                LightingProfile::Fast,
                calendar.options(quantization),
                &calendar.plans(quantization)?,
                max_wall,
            )?,
            sampling: Runtime::new(
                slots,
                control::Hardware {
                    storage: control::Storage::Packed,
                    max_cycles: max_wall,
                    ..Default::default()
                },
                timed::Hardware {
                    prefetch: false,
                    max_cycles: max_wall,
                    ..Default::default()
                },
                max_wall,
            )?,
            loaded_light: None,
            sample_destinations: [None; 16],
            slot_sizes: std::array::from_fn(|i| {
                slots.get(i).filter(|s| s.valid).map(|s| s.max_size_log2)
            }),
            faulted: false,
        })
    }

    pub fn pipeline(&self) -> &Pipeline {
        &self.pipeline
    }
    pub fn sampling(&self) -> &Runtime {
        &self.sampling
    }
    /// Diagnostic full destination for the fixed low-four-bit public ID.
    pub fn sampling_destination(&self, public_quad: u8) -> Option<super::Ticket> {
        self.sample_destinations
            .get(usize::from(public_quad))
            .copied()
            .flatten()
            .map(|d| d.ticket)
    }
    pub fn idle(&self) -> bool {
        !self.faulted
            && self.pipeline.idle()
            && self.lighting.in_flight() == 0
            && self.sampling.idle()
            && self.sample_destinations.iter().all(Option::is_none)
    }
    pub fn faulted(&self) -> bool {
        self.faulted || self.pipeline.faulted() || self.sampling.faulted()
    }
    pub fn open_draw(&mut self, value: CommonContext) -> Result<Option<ContextId>, String> {
        if self.faulted() {
            return Err("live pixel foundation terminal fault".into());
        }
        if value.sample.is_some_and(|s| {
            self.slot_sizes.get(usize::from(s.slot)).copied().flatten() != Some(s.size_log2)
        }) {
            return Err("Sampling draw disagrees with immutable texture slots".into());
        }
        self.pipeline.open_draw(value)
    }
    pub fn close_draw(&mut self, id: ContextId) -> Result<(), String> {
        self.pipeline.close_draw(id)
    }

    pub fn step(&mut self, memory: &mut impl RefillPort, tick: Tick) -> Result<Step, String> {
        self.step_with_rop(memory, tick, |_| Ok(tick.rop_ready))
    }

    /// Sampling polls first. The callback owns the single physical wall clock,
    /// even for CE=0/empty rows, and returns actual ROP input acceptance.
    pub fn step_with_rop(
        &mut self,
        memory: &mut impl RefillPort,
        tick: Tick,
        rop: impl FnOnce(Option<RopRow>) -> Result<bool, String>,
    ) -> Result<Step, String> {
        if self.faulted() {
            return Err("live pixel fault; drain accepted MC externally".into());
        }
        let result = self.advance(memory, tick, rop);
        if result.is_err() {
            self.faulted = true;
        }
        result
    }

    fn advance(
        &mut self,
        memory: &mut impl RefillPort,
        t: Tick,
        rop: impl FnOnce(Option<RopRow>) -> Result<bool, String>,
    ) -> Result<Step, String> {
        let before = self.pipeline.signals(t.input.and_then(|b| b.begin), t.ce)?;
        let job = before.lighting.filter(|_| t.lighting_issue_ready);
        let switching =
            job.is_some_and(|j| self.loaded_light.map(|id| id.slot) != Some(j.context.slot));
        let light_tick = LightingTick {
            reset: false,
            ce: t.ce,
            output_ready: t.lighting_result_ready,
            context: job
                .filter(|_| switching)
                .map(|j| self.pipeline.context(j.context).map(|c| c.lighting))
                .transpose()?,
            input: job
                .filter(|_| !switching)
                .map(|j| {
                    Ok::<_, String>(LightingRequest {
                        id: u32::from(j.key.ticket.quad) * 4 + u32::from(j.key.lane),
                        pixel: j
                            .pixel
                            .expanded()
                            .map_err(|e| format!("Lighting expansion: {e:?}"))?,
                    })
                })
                .transpose()?,
        };
        let lighting = self.lighting.tick(light_tick)?;
        if t.ce && switching && lighting.context_ready {
            self.loaded_light = job.map(|j| j.context);
        }
        let light = lighting
            .output
            .filter(|_| t.ce && t.lighting_result_ready)
            .map(|o| {
                if o.id >= foundation::GLOBAL_SLOTS as u32 * 4 {
                    return Err("Lighting key outside global slots".into());
                }
                let ticket = self
                    .pipeline
                    .ticket((o.id / 4) as u8)
                    .ok_or("Lighting lost destination")?;
                let id = self.pipeline.context_for(ticket)?;
                if self.pipeline.context(id)?.lighting.epoch != o.epoch {
                    return Err("Lighting draw epoch mismatch".into());
                }
                Ok::<_, String>(LightWrite {
                    key: PixelKey {
                        ticket,
                        lane: (o.id % 4) as u8,
                    },
                    value: o.output,
                })
            })
            .transpose()?;
        // A fixed public ID cannot be reassigned until the old destination's
        // last actual result write. Eligibility observes pre-edge state, so
        // this edge's result release never funds an aliased new admission.
        let sampling_alias_wait = before
            .sampling
            .is_some_and(|j| self.sample_destinations[usize::from(j.ticket.quad & 15)].is_some());
        let sample_job = before
            .sampling
            .filter(|_| t.sampling_issue_ready && !sampling_alias_wait);
        let offered = sample_job
            .map(|j| {
                let c = self
                    .pipeline
                    .context(j.context)?
                    .sample
                    .ok_or("bypass entered Sampling")?;
                Ok::<_, String>(RawQuadInput {
                    quad_id: j.ticket.quad & 15,
                    mask: j.mask,
                    uv_q16: j.uv_q16,
                    slot: c.slot,
                    material_size_log2: c.size_log2,
                    filter: c.filter,
                    bias_q8: c.bias_q8,
                    force_coarsest: j.force_coarsest,
                })
            })
            .transpose()?;
        let sampling = self.sampling.step_raw(
            memory,
            offered.as_ref(),
            timed::Control {
                ce: t.ce,
                result_ready: t.sampling_result_ready,
            },
        )?;
        if sampling.results.len() > 1 {
            return Err("Sampling exceeds one result write per edge".into());
        }
        let sample = sampling
            .results
            .first()
            .map(|o| {
                let destination = self
                    .sample_destinations
                    .get(usize::from(o.key / 4))
                    .copied()
                    .flatten()
                    .ok_or("Sampling result without public destination")?;
                if destination.remaining & (1 << (o.key % 4)) == 0 {
                    return Err("Sampling public destination duplicate/uncovered lane".into());
                }
                Ok::<_, String>(SampleWrite {
                    key: PixelKey {
                        ticket: destination.ticket,
                        lane: o.key % 4,
                    },
                    rgb: o.rgb,
                })
            })
            .transpose()?;
        let rop_ready = rop(before.rop)?;
        let pixels = self.pipeline.tick(foundation::Tick {
            ce: t.ce,
            input: t.input,
            lighting_ready: t.lighting_issue_ready && lighting.input_ready && !switching,
            sampling_ready: sampling.accepted,
            light,
            sample,
            final_issue_ready: t.final_issue_ready,
            final_result_ready: t.final_result_ready,
            rop_ready,
        })?;
        if pixels.lighting_accepted != (t.ce && lighting.input_ready && light_tick.input.is_some())
            || pixels.sampling_accepted != sampling.accepted
        {
            return Err("actual branch/store handshake disagreement".into());
        }
        if let Some(result) = sampling.results.first() {
            let slot = usize::from(result.key / 4);
            let owner = self.sample_destinations[slot]
                .as_mut()
                .ok_or("Sampling result destination disappeared")?;
            owner.remaining &= !(1 << (result.key % 4));
            if owner.remaining == 0 {
                self.sample_destinations[slot] = None;
            }
        }
        if sampling.accepted {
            let job = sample_job.ok_or("Sampling admitted absent raw input")?;
            let slot = usize::from(job.ticket.quad & 15);
            if self.sample_destinations[slot].is_some() || job.mask == 0 {
                return Err("Sampling public destination reassigned while live".into());
            }
            self.sample_destinations[slot] = Some(SampleDestination {
                ticket: job.ticket,
                remaining: job.mask,
            });
        }
        // Bank1 + loaded-valid suffice in hardware: invalidate when that draw
        // retires so reusing the same bank still reloads its new context.
        if self
            .loaded_light
            .is_some_and(|id| pixels.released_draws.contains(&id))
        {
            self.loaded_light = None;
        }
        Ok(Step {
            pixels,
            lighting,
            sampling,
            lighting_context_wait: switching && !lighting.context_ready,
            sampling_alias_wait,
            sample_written: sample,
        })
    }
}
