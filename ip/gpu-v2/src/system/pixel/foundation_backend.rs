//! Actual FramebufferEmu attachment to the published pipeline replacement.
//! This preserves the existing serial ROP/cache implementation and its real
//! backpressure. The II2 target is qualified at its eight-row input boundary.

use super::{
    dispatch::{CommonContext, ContextId, RopRow},
    foundation_live::{self, Live},
    Ticket,
};
use crate::{
    framebuffer::{
        emu::{Cycle, FramebufferEmu, OutputRow},
        ports::{Context, Header, MaterializedSurface},
    },
    memory::ports::MemoryPort,
    texture::ports::{RefillPort, Slot},
};

#[derive(Clone, Copy)]
struct RowOwner {
    ticket: Ticket,
    header: Header,
    context: Context,
    next: u8,
}

pub struct Step {
    pub pixels: foundation_live::Step,
    pub framebuffer: Cycle,
    pub context_stall: bool,
}

pub struct Backend {
    pixels: Live,
    framebuffer: FramebufferEmu,
    context: Context,
    row_owner: Option<RowOwner>,
    finishing: bool,
    flushing: bool,
    faulted: bool,
}

impl Backend {
    pub fn new(
        slots: &[Slot],
        surface: MaterializedSurface,
        max_wall: u64,
    ) -> Result<Self, String> {
        let context = Context {
            depth: crate::framebuffer::ports::DepthFunc::Always,
            depth_write: false,
            blend: crate::framebuffer::ports::Blend::Replace,
        };
        Ok(Self {
            pixels: Live::new(slots, max_wall)?,
            framebuffer: FramebufferEmu::new(surface, context, max_wall)?,
            context,
            row_owner: None,
            finishing: false,
            flushing: false,
            faulted: false,
        })
    }
    pub fn pixels(&self) -> &Live {
        &self.pixels
    }
    pub fn framebuffer(&self) -> &FramebufferEmu {
        &self.framebuffer
    }
    pub fn open_draw(&mut self, context: CommonContext) -> Result<Option<ContextId>, String> {
        if self.faulted() || self.finishing {
            return Err("closed/faulted framebuffer composition".into());
        }
        self.pixels.open_draw(context)
    }
    pub fn close_draw(&mut self, id: ContextId) -> Result<(), String> {
        self.pixels.close_draw(id)
    }
    /// End this frame explicitly. Draw boundaries and temporary idleness alone
    /// do not request an external-memory flush or prohibit another draw.
    pub fn request_finish(&mut self) -> Result<(), String> {
        if self.faulted() || self.finishing {
            return Err("duplicate/faulted frame finish".into());
        }
        self.finishing = true;
        Ok(())
    }
    pub fn complete(&self) -> bool {
        !self.faulted
            && self.flushing
            && self.pixels.idle()
            && self.row_owner.is_none()
            && self.framebuffer.idle()
            && self.framebuffer.flush_complete
    }
    pub fn faulted(&self) -> bool {
        self.faulted || self.pixels.faulted() || self.framebuffer.fault
    }

    pub fn step(
        &mut self,
        sampling: &mut impl RefillPort,
        memory: &mut impl MemoryPort,
        t: foundation_live::Tick,
    ) -> Result<Step, String> {
        if self.faulted() {
            return Err("framebuffer composition fault; drain accepted MC externally".into());
        }
        let result = self.advance(sampling, memory, t);
        if result.is_err() {
            self.faulted = true;
            self.framebuffer.abort();
        }
        result
    }
    fn advance(
        &mut self,
        sampling: &mut impl RefillPort,
        memory: &mut impl MemoryPort,
        t: foundation_live::Tick,
    ) -> Result<Step, String> {
        let framebuffer = &mut self.framebuffer;
        let context = &mut self.context;
        let owner = &mut self.row_owner;
        let mut cycle = None;
        let mut context_stall = false;
        let pixels = self
            .pixels
            .step_with_rop(sampling, t, |offered: Option<RopRow>| {
                let mut row = offered.filter(|_| t.ce && t.rop_ready);
                if let Some(r) = row {
                    if let Some(o) = *owner {
                        if o.ticket != r.ticket
                            || o.header != r.header
                            || o.context != r.context
                            || o.next != r.row
                        {
                            return Err("ROP input ownership/row sequence".into());
                        }
                    } else {
                        if r.row != 0 {
                            return Err("ROP input starts after row zero".into());
                        }
                        if *context != r.context {
                            if framebuffer.idle() {
                                framebuffer.set_context(r.context)?;
                                *context = r.context;
                            } else {
                                row = None;
                                context_stall = true;
                            }
                        }
                    }
                }
                let result = framebuffer.step(
                    t.ce,
                    row.map(|r| OutputRow {
                        header: r.header,
                        row: r.row,
                        data: r.data,
                    }),
                    memory,
                )?;
                if result.input_accepted {
                    let r = row.ok_or("ROP accepted absent row")?;
                    *owner = if r.row == 7 {
                        None
                    } else {
                        Some(RowOwner {
                            ticket: r.ticket,
                            header: r.header,
                            context: r.context,
                            next: r.row + 1,
                        })
                    };
                }
                let accepted = result.input_accepted;
                cycle = Some(result);
                Ok(accepted)
            })?;
        if self.finishing && self.pixels.idle() && !self.flushing {
            if self.row_owner.is_some() {
                return Err("flush during partial ROP quad".into());
            }
            self.framebuffer.request_flush()?;
            self.flushing = true;
        }
        Ok(Step {
            pixels,
            framebuffer: cycle.ok_or("physical clock callback omitted")?,
            context_stall,
        })
    }
}
