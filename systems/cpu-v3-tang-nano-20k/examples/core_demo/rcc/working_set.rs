//! Application-owned storage; no dependency on the board's legacy presets.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
mod surface;
use surface::*;

#[static_buffer]
pub const COLOR_A: Rgb565Surface = Rgb565Surface::allocate();
#[static_buffer]
pub const COLOR_B: Rgb565Surface = Rgb565Surface::allocate();
#[static_buffer]
pub const DEPTH: DepthSurface = DepthSurface::allocate();
#[static_buffer]
const COMMANDS: CommandMemory = CommandMemory::allocate(65536, 65536);
#[static_buffer]
pub const UI_STORAGE: UiGridStorage = UiGridStorage::allocate();

#[derive(Copy, Clone)]
pub struct DemoStorage {
    pub color_a: Rgb565Surface,
    pub color_b: Rgb565Surface,
    pub depth: DepthSurface,
    pub ui: UiGridStorage,
    pub gpu: GpuConfig,
}
pub const STORAGE: DemoStorage = DemoStorage {
    color_a: COLOR_A,
    color_b: COLOR_B,
    depth: DEPTH,
    ui: UI_STORAGE,
    gpu: GpuConfig::single(COMMANDS),
};
