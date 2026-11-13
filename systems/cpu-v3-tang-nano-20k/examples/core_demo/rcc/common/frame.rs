//! Shared Back-plane passes; submission and retirement stay with main.
use crate::dsl_rt::*;
mod gpu_api;
use crate::common::state::*;
use gpu_api::*;

pub fn clear_cpu_page(
    buffer: &mut CommandBuffer,
    color_a: &ColorBuffer,
    color_b: &ColorBuffer,
    depth: &DepthBuffer,
    frames: u16,
) {
    let color = if frames & 1 == 0 { color_b } else { color_a };
    let pass = buffer.pass(
        color.ptr(),
        depth.ptr(),
        ColorLoad::clear(Rgb565::rgb(8, 8, 24)),
        DepthLoad::load(),
    );
}
fn page_color_pass(
    buffer: &mut CommandBuffer,
    color: &ColorBuffer,
    depth: &DepthBuffer,
    geometry: bool,
) -> Pass {
    let mut color_load = ColorLoad::load();
    let mut depth_load = DepthLoad::load();
    if geometry {
        color_load = ColorLoad::clear(Rgb565::rgb(8, 8, 24));
        depth_load = DepthLoad::clear_far();
    }
    buffer.pass(color.ptr(), depth.ptr(), color_load, depth_load)
}
pub fn page_pass(
    buffer: &mut CommandBuffer,
    color_a: &ColorBuffer,
    color_b: &ColorBuffer,
    depth: &DepthBuffer,
    state: DemoState,
) -> Pass {
    let color = if state.frames & 1 == 0 {
        color_b
    } else {
        color_a
    };
    page_color_pass(buffer, color, depth, state.page.uses_gpu_scene())
}
