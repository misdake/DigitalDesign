//! Actual branch/Final/ROP/cache composition. Transport adapters remain outside
//! GPU production code. Sampling polls first; framebuffer advances the sole
//! physical memory owner exactly once per wall edge, including compute CE=0.
use super::{
    composition,
    dispatch::{CommonContext, Config, ContextId},
    registered::{self, RegisteredBranches},
    Ticket,
};
use crate::{
    framebuffer::{
        emu::{Cycle, FramebufferEmu, OutputRow},
        ports::{Context, Header, MaterializedSurface},
    },
    memory::ports::MemoryPort,
    texture::{ports::Slot, sim::staged::bound::serial},
};

#[derive(Clone, Copy, Debug)]
struct RowOwner {
    ticket: Ticket,
    header: Header,
    context: Context,
    next: u8,
}

pub struct Step {
    pub pixels: registered::Step,
    pub framebuffer: Cycle,
}

pub struct Backend<M: MemoryPort> {
    pixels: RegisteredBranches<M>,
    framebuffer: FramebufferEmu,
    context: Context,
    row_owner: Option<RowOwner>,
    flushing: bool,
    faulted: bool,
}

impl<M: MemoryPort> Backend<M> {
    pub fn new(
        config: Config,
        slots: Vec<Slot>,
        sampling: M,
        preparation: serial::Config,
        surface: MaterializedSurface,
    ) -> Result<Self, String> {
        let context = Context {
            depth: crate::framebuffer::ports::DepthFunc::Always,
            depth_write: false,
            blend: crate::framebuffer::ports::Blend::Replace,
        };
        Ok(Self {
            pixels: RegisteredBranches::new(config, slots, sampling, preparation)?,
            framebuffer: FramebufferEmu::new(surface, context, config.max_wall)?,
            context,
            row_owner: None,
            flushing: false,
            faulted: false,
        })
    }
    pub fn set_context(&mut self, slot: u8, context: CommonContext) -> Result<ContextId, String> {
        if self.faulted {
            return Err("backend terminal fault".into());
        }
        self.pixels.set_context(slot, context)
    }
    pub fn pixels(&self) -> &RegisteredBranches<M> {
        &self.pixels
    }
    pub fn framebuffer(&self) -> &FramebufferEmu {
        &self.framebuffer
    }
    pub fn complete(&self) -> bool {
        !self.faulted
            && self.pixels.complete()
            && self.row_owner.is_none()
            && self.framebuffer.idle()
            && self.framebuffer.flush_complete
    }
    pub fn faulted(&self) -> bool {
        self.faulted || self.framebuffer.fault
    }

    /// A failed edge is never retried. If failure precedes the callback, the
    /// external owner must cancel unpresented Sampling and drain accepted work.
    pub fn step(
        &mut self,
        memory: &mut impl MemoryPort,
        tick: composition::Tick,
    ) -> Result<Step, String> {
        if self.faulted() {
            return Err("backend fault; drain accepted transport externally".into());
        }
        let result = self.advance(memory, tick);
        if result.is_err() {
            self.faulted = true;
            self.framebuffer.abort();
        }
        result
    }
    fn advance(
        &mut self,
        memory: &mut impl MemoryPort,
        tick: composition::Tick,
    ) -> Result<Step, String> {
        let mut framebuffer_cycle = None;
        let framebuffer = &mut self.framebuffer;
        let context = &mut self.context;
        let owner = &mut self.row_owner;
        let pixels = self.pixels.step_with_rop(tick, |offered| {
            let mut row = offered.filter(|_| tick.ce && tick.rop_ready);
            if let Some(r) = row {
                if let Some(o) = *owner {
                    if o.ticket != r.ticket
                        || o.header != r.header
                        || o.context != r.context
                        || o.next != r.row
                    {
                        return Err("backend ROP row ownership/sequence".into());
                    }
                } else {
                    if r.row != 0 {
                        return Err("backend ROP starts without row zero".into());
                    }
                    if *context != r.context {
                        if framebuffer.idle() {
                            framebuffer.set_context(r.context)?;
                            *context = r.context;
                        } else {
                            row = None;
                        }
                    }
                }
            }
            let fb_row = row.map(|r| OutputRow {
                header: r.header,
                row: r.row,
                data: r.data,
            });
            let cycle = framebuffer.step(tick.ce, fb_row, memory)?;
            let accepted = cycle.input_accepted;
            if accepted {
                let r = row.ok_or("cache accepted without output row")?;
                if r.row == 7 {
                    *owner = None;
                } else {
                    *owner = Some(RowOwner {
                        ticket: r.ticket,
                        header: r.header,
                        context: r.context,
                        next: r.row + 1,
                    });
                }
            }
            framebuffer_cycle = Some(cycle);
            Ok(accepted)
        })?;
        if self.pixels.complete() && !self.flushing {
            if self.row_owner.is_some() {
                return Err("backend closes partial ROP quad".into());
            }
            self.framebuffer.request_flush()?;
            self.flushing = true;
        }
        Ok(Step {
            pixels,
            framebuffer: framebuffer_cycle.ok_or("backend physical clock callback omitted")?,
        })
    }
}
