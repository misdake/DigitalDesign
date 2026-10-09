//! J1: controlled branch-result writes, owned stores, ordered final and existing ROP.
//! `Model` still takes controlled branch results. `live` connects the real
//! LightingEmu to that light store; `branches` adds actual baseline Sampling
//! Runtime/cache/ColorEmu results using distinct controlled memory ports.

mod branches;
mod color;
mod live;
mod model;

use crate::{framebuffer::ports::*, lighting::ports::LightingOutput};
pub use branches::{BranchCycle, BranchQuad, BranchTick, PixelBranches, SAMPLING_OFFER_BITS};
pub use color::final_rgb;
pub use live::{LightingLive, LiveCycle, LiveQuad, LiveTick};
pub use model::{Access, Cycle, Event, Model, Phase, Snapshot, Stats, Store};

/// Host witness of one allocation. The serial detects stale injection at wrap;
/// it is not a proposed wide hardware tag or a change to the six-bit pixel key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ticket {
    pub quad: u8,
    pub serial: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelKey {
    pub ticket: Ticket,
    pub lane: u8,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Basic {
    pub tint: [u8; 3],
    pub depth: u16,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuadInput {
    pub header: Header,
    pub basic: [Basic; 4],
    pub default_light: bool,
    pub default_sample: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LightWrite {
    pub key: PixelKey,
    pub value: LightingOutput,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampleWrite {
    pub key: PixelKey,
    pub rgb: [u8; 3],
}
#[derive(Clone, Copy, Debug)]
pub struct Context {
    pub surface: MaterializedSurface,
    pub rop: crate::framebuffer::ports::Context,
    pub specular: [u8; 3],
    pub alpha: u8,
}
/// Compute/injection transfers require CE. Already issued store returns and
/// accepted MC work advance on wall clocks even with CE or final_ready low.
#[derive(Clone, Copy, Debug)]
pub struct Tick {
    pub ce: bool,
    pub final_ready: bool,
    pub quad: Option<QuadInput>,
    pub light: Option<LightWrite>,
    pub sample: Option<SampleWrite>,
    /// Stop new quads, receive existing results, then flush after final drain.
    pub finish: bool,
}
impl Default for Tick {
    fn default() -> Self {
        Self {
            ce: true,
            final_ready: true,
            quad: None,
            light: None,
            sample: None,
            finish: false,
        }
    }
}
