//! Shared Back-plane passes; submission and retirement stay with main.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
mod state;
use state::*;

pub fn clear_cpu_page(
    buffer: &mut CommandBuffer,
    color_a: &ColorBuffer,
    color_b: &ColorBuffer,
    depth: &DepthBuffer,
    state: DemoState,
) {
    if state.frames & 1 == 0 {
        let pass = buffer.pass(
            color_b.ptr(),
            depth.ptr(),
            ColorLoad::clear(Rgb565::rgb(8, 8, 24)),
            DepthLoad::load(),
        );
    } else {
        let pass = buffer.pass(
            color_a.ptr(),
            depth.ptr(),
            ColorLoad::clear(Rgb565::rgb(8, 8, 24)),
            DepthLoad::load(),
        );
    }
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
    if state.frames & 1 == 0 {
        return page_color_pass(buffer, color_b, depth, state.page == 5);
    }
    page_color_pass(buffer, color_a, depth, state.page == 5)
}
