//! Audited classic orbit data, read through the existing bounded window.
use crate::dsl_rt::*;
mod gpu_window;
use gpu_window::*;
mod boot_resources;
use boot_resources::*;
pub fn demo_camera(out: Array<u16>, phase: u16) -> bool {
    gpu_window_copy(out, DEMO_CAMERA_FIRST + (phase >> 8), 0, 45, true)
}
