//! Live branch engines connected to explicit independent quad queues/status.
//!
//! Lighting uses its current lit-only physical calendar. Sampling uses the
//! persistent Runtime; its remaining replay boundaries are described by Texture.
//! Final and ROP ports remain external here so all leaf engines stay independent.

use super::dispatch::{self, CommonContext, Config, ContextId, Dispatcher, Input, LightJob};
use super::{LightWrite, PixelKey, SampleWrite};
use crate::lighting::{emu::LightingEmu, ports::*, LightingProfile, LightingQuantization};
use crate::texture::{
    ports::{QuadInput, RefillPort, Slot},
    sim::{
        staged::bound::{control, runtime::Runtime},
        timed,
    },
};

#[derive(Clone, Copy, Debug, Default)]
pub struct Tick {
    pub ce: bool,
    pub input: Option<Input>,
    pub lighting_result_ready: bool,
    pub sampling_issue_ready: bool,
    pub sampling_result_ready: bool,
    pub final_ready: bool,
    pub final_result: Option<(PixelKey, [u8; 3])>,
    pub rop_ready: bool,
    pub finish: bool,
}

pub struct Step {
    pub dispatch: dispatch::Step,
    pub lighting: LightingSignals,
    pub sampling: crate::texture::sim::staged::bound::runtime::Step,
    pub light_issued: Option<PixelKey>,
    pub sample_issued: bool,
}

#[derive(Clone, Copy)]
struct ActiveLight {
    job: LightJob,
    lane: u8,
}

pub struct BranchEngines {
    dispatch: Dispatcher,
    lighting: LightingEmu,
    sampling: Runtime,
    // Checked frozen slot-shape view (valid1 + size4 per slot), not a second
    // texture payload/cache or an implicit extra BRAM port.
    slot_sizes: [Option<u8>; 16],
    active_light: Option<ActiveLight>,
    loaded_light: Option<ContextId>,
    faulted: bool,
}

impl BranchEngines {
    pub fn new(config: Config, slots: &[Slot]) -> Result<Self, String> {
        Ok(Self {
            dispatch: Dispatcher::new(config)?,
            lighting: LightingEmu::lit_queue_resource_profile(
                LightingProfile::Fast,
                LightingQuantization::CompensatedFloor,
                config.max_wall,
            )?,
            sampling: Runtime::new(
                slots,
                control::Hardware {
                    storage: control::Storage::Packed,
                    max_cycles: config.max_wall,
                    ..Default::default()
                },
                timed::Hardware {
                    prefetch: false,
                    max_cycles: config.max_wall,
                    ..Default::default()
                },
                config.max_wall,
            )?,
            active_light: None,
            slot_sizes: std::array::from_fn(|i| {
                slots.get(i).filter(|s| s.valid).map(|s| s.max_size_log2)
            }),
            loaded_light: None,
            faulted: false,
        })
    }

    pub fn set_context(&mut self, slot: u8, context: CommonContext) -> Result<ContextId, String> {
        if context.sample.is_some_and(|s| {
            self.slot_sizes.get(usize::from(s.slot)).copied().flatten() != Some(s.size_log2)
        }) {
            return Err("sampling context disagrees with frozen texture slot".into());
        }
        self.dispatch.set_context(slot, context)
    }

    pub fn dispatch(&self) -> &Dispatcher {
        &self.dispatch
    }
    pub fn sampling(&self) -> &Runtime {
        &self.sampling
    }
    pub fn complete(&self) -> bool {
        !self.faulted
            && self.dispatch.complete()
            && self.active_light.is_none()
            && self.sampling.idle()
    }

    pub fn step(&mut self, memory: &mut impl RefillPort, tick: Tick) -> Result<Step, String> {
        self.step_with_rop(memory, tick, |_| Ok(tick.rop_ready))
    }

    /// Advance Sampling's read view before the external ROP/memory clock owner.
    /// The callback sees the pre-edge held row and returns its actual acceptance.
    /// It must clock the framebuffer/physical memory exactly once even with no
    /// row or CE=0. This preserves one wall clock shared by two independent ports.
    /// On an earlier branch error no callback is invoked; the caller must enter
    /// its explicit accepted-memory fault-drain loop instead of retrying this edge.
    pub fn step_with_rop(
        &mut self,
        memory: &mut impl RefillPort,
        tick: Tick,
        rop: impl FnOnce(Option<dispatch::RopRow>) -> Result<bool, String>,
    ) -> Result<Step, String> {
        if self.faulted {
            return Err(
                "branch engines terminal fault; external owner must drain accepted MC".into(),
            );
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
        tick: Tick,
        rop: impl FnOnce(Option<dispatch::RopRow>) -> Result<bool, String>,
    ) -> Result<Step, String> {
        let before = self.dispatch.signals();
        let capture_light = self.active_light.is_none();
        let mut light_tick = LightingTick {
            reset: false,
            ce: tick.ce,
            context: None,
            input: None,
            output_ready: tick.lighting_result_ready,
        };
        let mut key = None;
        if let Some(active) = self.active_light {
            if self.loaded_light != Some(active.job.context) {
                light_tick.context = Some(self.dispatch.context(active.job.context)?.lighting);
            } else {
                let lane = (active.lane..4)
                    .find(|&lane| active.job.mask & (1 << lane) != 0)
                    .ok_or("empty lighting work")?;
                let k = PixelKey {
                    ticket: active.job.ticket,
                    lane,
                };
                key = Some(k);
                light_tick.input = Some(LightingRequest {
                    id: u32::from(k.ticket.quad) * 4 + u32::from(lane),
                    pixel: active.job.pixels[usize::from(lane)]
                        .expanded()
                        .map_err(|e| format!("normal expansion: {e:?}"))?,
                });
            }
        }
        let lighting = self.lighting.tick(light_tick)?;
        let mut light_issued = None;
        if tick.ce {
            if light_tick.context.is_some() && lighting.context_ready {
                self.loaded_light = self.active_light.map(|a| a.job.context);
            }
            if lighting.input_ready && light_tick.input.is_some() {
                let k = key.unwrap();
                light_issued = Some(k);
                let mut active = self.active_light.unwrap();
                active.lane = k.lane + 1;
                self.active_light = (active.job.mask >> active.lane != 0).then_some(active);
            }
        }
        let light = lighting
            .output
            .filter(|_| tick.ce && tick.lighting_result_ready)
            .map(|o| {
                let quad =
                    u8::try_from(o.id / 4).map_err(|_| "lighting returned key outside range")?;
                let ticket = self
                    .dispatch
                    .ticket(quad)
                    .ok_or("lighting returned unowned quad")?;
                let context = self.dispatch.context(self.dispatch.context_for(ticket)?)?;
                if o.id >= 64 || o.epoch != context.lighting.epoch {
                    return Err("lighting epoch/key mismatch".to_string());
                }
                Ok(LightWrite {
                    key: PixelKey {
                        ticket,
                        lane: (o.id % 4) as u8,
                    },
                    value: o.output,
                })
            })
            .transpose()?;

        let offered = before
            .sampling
            .filter(|_| tick.sampling_issue_ready)
            .map(|job| {
                let context = self
                    .dispatch
                    .context(job.context)?
                    .sample
                    .ok_or("untextured entered sample queue")?;
                Ok::<_, String>(QuadInput {
                    quad_id: job.ticket.quad,
                    mask: job.mask,
                    uv: job.uv_q18.map(|v| v.map(|x| x as f64 / 262144.0)),
                    slot: context.slot,
                    material_size_log2: context.size_log2,
                    filter: context.filter,
                    lod_bias: f64::from(context.bias_q8) / 256.0,
                })
            })
            .transpose()?;
        let sampling = self.sampling.step(
            memory,
            offered.as_ref(),
            timed::Control {
                ce: tick.ce,
                result_ready: tick.sampling_result_ready,
            },
        )?;
        if sampling.results.len() > 1 {
            return Err("sampling exceeds status write port".into());
        }
        let sample = sampling
            .results
            .first()
            .map(|o| {
                let ticket = self
                    .dispatch
                    .ticket(o.key / 4)
                    .ok_or("sample returned unowned quad")?;
                Ok::<_, String>(SampleWrite {
                    key: PixelKey {
                        ticket,
                        lane: o.key % 4,
                    },
                    rgb: o.rgb,
                })
            })
            .transpose()?;
        let sample_issued = sampling.accepted;
        // Sampling has polled the previous physical edge. ROP may now advance
        // the shared controller and return row acceptance, never guessed ready.
        let rop_ready = rop(before.rop)?;
        let dispatch = self.dispatch.tick(dispatch::Tick {
            ce: tick.ce,
            input: tick.input,
            lighting_ready: capture_light,
            sampling_ready: sampling.accepted,
            light,
            sample,
            final_ready: tick.final_ready,
            final_result: tick.final_result,
            rop_ready,
            finish: tick.finish,
        })?;
        if tick.ce && capture_light {
            if let Some(job) = before.lighting {
                self.active_light = Some(ActiveLight { job, lane: 0 });
            }
        }
        Ok(Step {
            dispatch,
            lighting,
            sampling,
            light_issued,
            sample_issued,
        })
    }
}
