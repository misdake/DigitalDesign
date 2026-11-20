//! App palette, shared by CPU illustrations and menu selection.
mod gpu_api;
use gpu_api::*;

pub const GUIDE: u16 = Rgb565::rgb(32, 32, 64).raw;
pub const RED: u16 = Rgb565::rgb(255, 0, 0).raw;
pub const GREEN: u16 = Rgb565::rgb(0, 255, 0).raw;
pub const CYAN: u16 = Rgb565::rgb(0, 255, 255).raw;
pub const RING: u16 = Rgb565::rgb(56, 60, 56).raw;
pub const YELLOW: u16 = Rgb565::rgb(255, 255, 0).raw;
pub const BUTTON_IDLE: u16 = Rgb565::rgb(24, 24, 24).raw;
pub const BUTTON_ACTIVE: u16 = Rgb565::rgb(0, 144, 96).raw;
pub const BLUE: u16 = Rgb565::rgb(0, 0, 255).raw;
pub const SELECTION: u16 = Rgb565::rgb(0, 100, 64).raw;
