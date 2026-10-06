//! Actual FinalEmu connected to independent Lighting/Sampling branch engines.
//! One final owner is reserved by the dispatcher; depth stays in its working
//! state. This module owns no ROP/cache or physical memory clock.
use super::{
    dispatch::{self, CommonContext, Config, ContextId, Input},
    engines::{self, BranchEngines},
    final_stage::{self, emu::FinalEmu},
    PixelKey,
};
use crate::texture::ports::{RefillPort, Slot};

#[derive(Clone, Copy, Debug, Default)]
pub struct Tick {
    pub ce: bool,
    pub input: Option<Input>,
    pub lighting_result_ready: bool,
    pub sampling_issue_ready: bool,
    pub sampling_result_ready: bool,
    pub final_issue_ready: bool,
    pub final_result_ready: bool,
    pub rop_ready: bool,
    pub finish: bool,
}

pub struct Step {
    pub branches: engines::Step,
    pub final_stage: final_stage::Step,
}

pub struct FinalBranches {
    branches: BranchEngines,
    final_stage: FinalEmu,
    // Diagnostic host witness only; hardware carries the existing 6-bit key.
    final_owner: Option<PixelKey>,
    faulted: bool,
}

impl FinalBranches {
    pub fn new(config: Config, slots: &[Slot]) -> Result<Self, String> {
        Ok(Self {
            branches: BranchEngines::new(config, slots)?,
            final_stage: FinalEmu::new(config.max_wall)?,
            final_owner: None,
            faulted: false,
        })
    }

    pub fn set_context(&mut self, slot: u8, context: CommonContext) -> Result<ContextId, String> {
        if self.faulted {
            return Err("final composition terminal fault; context update rejected".into());
        }
        self.branches.set_context(slot, context)
    }
    pub fn faulted(&self) -> bool {
        self.faulted
    }
    pub fn branches(&self) -> &BranchEngines {
        &self.branches
    }
    pub fn final_stage(&self) -> &FinalEmu {
        &self.final_stage
    }
    pub fn complete(&self) -> bool {
        !self.faulted
            && self.final_owner.is_none()
            && self.final_stage.idle()
            && self.branches.complete()
    }
    pub fn step(&mut self, memory: &mut impl RefillPort, tick: Tick) -> Result<Step, String> {
        self.step_with_rop(memory, tick, |_| Ok(tick.rop_ready))
    }

    /// Same single physical-clock ownership contract as BranchEngines. An
    /// earlier error invokes no callback: accepted MC must be drained externally.
    pub fn step_with_rop(
        &mut self,
        memory: &mut impl RefillPort,
        tick: Tick,
        rop: impl FnOnce(Option<dispatch::RopRow>) -> Result<bool, String>,
    ) -> Result<Step, String> {
        if self.faulted {
            return Err("final composition terminal fault; external MC drain required".into());
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
        let before = self.branches.dispatch().signals();
        let job = before.final_input.filter(|_| tick.final_issue_ready);
        let input = job.map(|j| final_stage::Input {
            key: j.key.ticket.quad * 4 + j.key.lane,
            tint: j.tint,
            texture: j.texture,
            g: j.light.g,
            h: j.light.h,
            specular: j.specular,
        });
        let final_stage = self.final_stage.tick(final_stage::Tick {
            ce: tick.ce,
            input,
            output_ready: before.final_output_ready && tick.final_result_ready,
        })?;
        let returned = if final_stage.consumed {
            let owner = self
                .final_owner
                .take()
                .ok_or("final leaf returned without owner")?;
            let output = final_stage
                .output
                .ok_or("final leaf lost transferred output")?;
            if output.key != owner.ticket.quad * 4 + owner.lane {
                return Err("final leaf returned mismatched owner".into());
            }
            Some((owner, output.rgb))
        } else {
            None
        };
        if final_stage.accepted {
            if self.final_owner.is_some() {
                return Err("dispatcher exceeded single final reservation".into());
            }
            self.final_owner = Some(job.ok_or("final acceptance without job")?.key);
        }
        let branches = self.branches.step_with_rop(
            memory,
            engines::Tick {
                ce: tick.ce,
                input: tick.input,
                lighting_result_ready: tick.lighting_result_ready,
                sampling_issue_ready: tick.sampling_issue_ready,
                sampling_result_ready: tick.sampling_result_ready,
                final_ready: tick.final_issue_ready && final_stage.input_ready,
                final_result: returned,
                rop_ready: false,
                finish: tick.finish,
            },
            rop,
        )?;
        if job.is_some()
            && final_stage.accepted
                != (tick.ce
                    && branches.dispatch.signals.final_input.is_some()
                    && tick.final_issue_ready
                    && final_stage.input_ready)
        {
            return Err("final/dispatcher transfer disagreement".into());
        }
        Ok(Step {
            branches,
            final_stage,
        })
    }
}
