use crate::{scratchpad::ports::*, vertex::ports::Context};
#[derive(Clone, Debug)]
pub enum Command {
    Dma(DmaDescriptor),
    Wait(u8),
    Draw {
        region: usize,
        byte_offset: usize,
        vertices: usize,
        context: Context,
    },
    Release {
        slot: usize,
        epoch: u32,
    },
    Fence,
    Unsupported(u8),
}
pub const HANDLERS: [&str; 8] = [
    "H_DMA0",
    "H_DMA1",
    "H_DMA2",
    "H_DMA3",
    "H_COMMAND",
    "H_TRIANGLE_CREDIT",
    "H_CACHE_DONE",
    "H_FAULT",
];
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EventState {
    pub pending: u8,
    pub cursor: u8,
}
impl EventState {
    pub fn update(&mut self, set: u8, ack: u8) {
        self.pending = (self.pending & !ack) | set;
    }
    pub fn take(&mut self, idle_or_wait: bool) -> Option<u8> {
        if !idle_or_wait {
            return None;
        }
        // Geometry/cache events have no producer in this milestone and stay masked.
        for offset in 0..8 {
            let id = (self.cursor + offset) % 8;
            if self.pending & (1 << id) != 0 && id != 5 && id != 6 {
                self.cursor = (id + 1) % 8;
                return Some(id);
            }
        }
        None
    }
}
