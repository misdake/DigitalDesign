//! Actual register-only branch and Final composition. The independent sampler
//! owns its read-only transport view; an external callback owns the physical
//! wall clock after that view is polled. No counted Program is admitted here.
use super::{
    composition::Tick,
    dispatch::{self, CommonContext, Config, ContextId, Dispatcher, LightJob},
    final_stage::{self, emu::FinalEmu},
    LightWrite, PixelKey, SampleWrite,
};
use crate::{
    lighting::{emu::LightingEmu, ports::*, LightingProfile, LightingQuantization},
    memory::ports::MemoryPort,
    texture::{
        emu::{
            derivative,
            sampler::{self, SamplerEmu},
        },
        ports::Slot,
        sim::staged::bound::serial,
    },
};

#[derive(Clone, Copy)]
struct ActiveLight {
    job: LightJob,
    lane: u8,
}
pub struct Step {
    pub dispatch: dispatch::Step,
    pub lighting: LightingSignals,
    pub sampling: sampler::Step,
    pub final_stage: final_stage::Step,
}
pub struct RegisteredBranches<M: MemoryPort> {
    dispatch: Dispatcher,
    lighting: LightingEmu,
    sampler: SamplerEmu<M>,
    final_stage: FinalEmu,
    active_light: Option<ActiveLight>,
    loaded_light: Option<ContextId>,
    final_owner: Option<PixelKey>, // host generation witness; hardware already carries key6
    sample_lane: u8,               // two-bit cursor; sampler retains its own quad result bank
    fault: bool,
}
impl<M: MemoryPort> RegisteredBranches<M> {
    pub fn new(
        config: Config,
        slots: Vec<Slot>,
        memory: M,
        preparation: serial::Config,
    ) -> Result<Self, String> {
        Ok(Self {
            dispatch: Dispatcher::new(config)?,
            lighting: LightingEmu::lit_queue_resource_profile(
                LightingProfile::Fast,
                LightingQuantization::CompensatedFloor,
                config.max_wall,
            )?,
            sampler: SamplerEmu::with_config(slots, memory, config.max_wall, preparation)?,
            final_stage: FinalEmu::new(config.max_wall)?,
            active_light: None,
            loaded_light: None,
            final_owner: None,
            sample_lane: 0,
            fault: false,
        })
    }
    pub fn dispatch(&self) -> &Dispatcher {
        &self.dispatch
    }
    pub fn sampler(&self) -> &SamplerEmu<M> {
        &self.sampler
    }
    pub fn sampler_mut(&mut self) -> &mut SamplerEmu<M> {
        &mut self.sampler
    }
    pub fn set_context(&mut self, slot: u8, context: CommonContext) -> Result<ContextId, String> {
        if self.fault {
            return Err("registered branches terminal".into());
        }
        if let Some(s) = context.sample {
            if self
                .sampler
                .cache()
                .slots()
                .get(s.slot as usize)
                .filter(|t| t.valid)
                .map(|t| t.max_size_log2)
                != Some(s.size_log2)
            {
                return Err("registered sample context/slot size".into());
            }
        }
        self.dispatch.set_context(slot, context)
    }
    pub fn complete(&self) -> bool {
        !self.fault
            && self.dispatch.complete()
            && self.active_light.is_none()
            && self.final_owner.is_none()
            && self.sampler.idle()
            && self.final_stage.idle()
    }
    pub fn step_with_rop(
        &mut self,
        t: Tick,
        rop: impl FnOnce(Option<dispatch::RopRow>) -> Result<bool, String>,
    ) -> Result<Step, String> {
        if self.fault {
            return Err("registered branches terminal; drain accepted memory".into());
        }
        let result = self.advance(t, rop);
        if result.is_err() {
            self.fault = true;
            self.sampler.abort();
        }
        result
    }
    fn advance(
        &mut self,
        t: Tick,
        rop: impl FnOnce(Option<dispatch::RopRow>) -> Result<bool, String>,
    ) -> Result<Step, String> {
        let before = self.dispatch.signals();
        let final_job = before.final_input.filter(|_| t.final_issue_ready);
        let final_stage = self.final_stage.tick(final_stage::Tick {
            reset: false,
            ce: t.ce,
            input: final_job.map(|j| final_stage::Input {
                key: j.key.ticket.quad * 4 + j.key.lane,
                tint: j.tint,
                texture: j.texture,
                g: j.light.g,
                h: j.light.h,
                specular: j.specular,
            }),
            output_ready: before.final_output_ready && t.final_result_ready,
        })?;
        let final_result = if final_stage.consumed {
            let owner = self
                .final_owner
                .take()
                .ok_or("registered final missing owner")?;
            let out = final_stage
                .output
                .ok_or("registered final missing result")?;
            if out.key != owner.ticket.quad * 4 + owner.lane {
                return Err("registered final wrong owner".into());
            }
            Some((owner, out.rgb))
        } else {
            None
        };
        if final_stage.accepted {
            if self.final_owner.is_some() {
                return Err("registered final exceeded reservation".into());
            }
            self.final_owner = Some(final_job.ok_or("registered final missing job")?.key);
        }
        let capture_light = self.active_light.is_none();
        let mut lt = LightingTick {
            reset: false,
            ce: t.ce,
            context: None,
            input: None,
            output_ready: t.lighting_result_ready,
        };
        let mut light_key = None;
        if let Some(a) = self.active_light {
            if self.loaded_light != Some(a.job.context) {
                lt.context = Some(self.dispatch.context(a.job.context)?.lighting);
            } else {
                let lane = (a.lane..4)
                    .find(|&l| a.job.mask & (1 << l) != 0)
                    .ok_or("registered empty light")?;
                let key = PixelKey {
                    ticket: a.job.ticket,
                    lane,
                };
                light_key = Some(key);
                lt.input = Some(LightingRequest {
                    id: u32::from(key.ticket.quad) * 4 + u32::from(lane),
                    pixel: a.job.pixels[lane as usize]
                        .expanded()
                        .map_err(|e| format!("registered normal {e:?}"))?,
                });
            }
        }
        let lighting = self.lighting.tick(lt)?;
        if t.ce {
            if lt.context.is_some() && lighting.context_ready {
                self.loaded_light = self.active_light.map(|a| a.job.context);
            }
            if lighting.input_ready && lt.input.is_some() {
                let k = light_key.unwrap();
                let mut a = self.active_light.unwrap();
                a.lane = k.lane + 1;
                self.active_light = (a.job.mask >> a.lane != 0).then_some(a);
            }
        }
        let light = lighting
            .output
            .filter(|_| t.ce && t.lighting_result_ready)
            .map(|o| {
                if o.id >= 64 {
                    return Err("registered light key width".to_string());
                }
                let ticket = self
                    .dispatch
                    .ticket((o.id / 4) as u8)
                    .ok_or("registered light stale owner")?;
                let context = self.dispatch.context(self.dispatch.context_for(ticket)?)?;
                if o.epoch != context.lighting.epoch {
                    return Err("registered light epoch".into());
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

        // Reuse the sampler's held RGB96 bank; a 2-bit cursor serializes the
        // four completion writes through the existing status write port.
        let output = self.sampler.output();
        let lane = output.and_then(|o| (self.sample_lane..4).find(|&l| o.mask & (1 << l) != 0));
        let last = output
            .zip(lane)
            .is_some_and(|(o, l)| o.mask >> (l + 1) == 0);
        let sample = if t.ce && t.sampling_result_ready {
            output
                .zip(lane)
                .map(|(o, l)| {
                    let ticket = self
                        .dispatch
                        .ticket(o.quad)
                        .ok_or("registered sample stale owner")?;
                    Ok::<_, String>(SampleWrite {
                        key: PixelKey { ticket, lane: l },
                        rgb: o.colors[l as usize],
                    })
                })
                .transpose()?
        } else {
            None
        };
        let offered = before
            .sampling
            .filter(|_| t.sampling_issue_ready)
            .map(|j| {
                let c = self
                    .dispatch
                    .context(j.context)?
                    .sample
                    .ok_or("untextured entered sampler")?;
                let slot = self
                    .sampler
                    .cache()
                    .slots()
                    .get(c.slot as usize)
                    .ok_or("registered sample slot")?;
                Ok::<_, String>(derivative::Input {
                    force_coarsest: j.force_coarsest,
                    uv: std::array::from_fn(|i| j.uv_q16[i / 2][i % 2]),
                    bias: c.bias_q8,
                    header: derivative::Header {
                        quad: j.ticket.quad,
                        mask: j.mask,
                        slot: c.slot,
                        max_n: c.size_log2,
                        has_mip: slot.has_full_mip,
                        filter: c.filter as u8,
                    },
                })
            })
            .transpose()?;
        let sampling = self.sampler.tick(sampler::Tick {
            ce: t.ce,
            input: offered,
            output_ready: last && t.sampling_result_ready,
        })?;
        if sample.is_some() {
            self.sample_lane = if last { 0 } else { lane.unwrap() + 1 };
        }
        // RO view has polled; only this callback may clock physical MC.
        let rop_ready = rop(before.rop)?;
        let dispatch = self.dispatch.tick(dispatch::Tick {
            ce: t.ce,
            input: t.input,
            lighting_ready: capture_light,
            sampling_ready: sampling.accepted,
            light,
            sample,
            final_ready: t.final_issue_ready && final_stage.input_ready,
            final_result,
            rop_ready,
            finish: t.finish,
        })?;
        if t.ce && capture_light {
            if let Some(job) = before.lighting {
                self.active_light = Some(ActiveLight { job, lane: 0 });
            }
        }
        Ok(Step {
            dispatch,
            lighting,
            sampling,
            final_stage,
        })
    }
}
