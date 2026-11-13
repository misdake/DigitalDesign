//! The core demonstration's ordinary public-API entry. No profiler device hooks.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
mod working_set;
use working_set::*;
mod boot_resources;
use boot_resources::*;
mod gamepad;
use gamepad::*;
mod time;
use time::*;
mod state;
use state::*;
mod geometry_page;
use geometry_page::*;
mod cpu_pages;
use cpu_pages::*;
mod frame;
use frame::*;

fn main() {
    let mut gpu = Gpu::init(STORAGE.gpu);
    let mut buffer = gpu.command_buffer();
    let color_a = ColorBuffer::from_surface(STORAGE.color_a);
    let color_b = ColorBuffer::from_surface(STORAGE.color_b);
    let depth = DepthBuffer::from_surface(STORAGE.depth);
    let mut display = Display::open();
    let atlas = UiAtlas::from_desc(CORE_ATLAS);
    let mut grid = UiGrid::from_storage(STORAGE.ui, &atlas);
    let mut geometry = GeometryPage::new();
    let mut state = initial_state(CORE_DEMO_PAGE);
    gamepad_init();
    loop {
        gamepad_capture();
        let input = gamepad_take_frame();
        state = update(state, input.pressed, time_milliseconds());
        buffer.reset();
        if state.frames == 0 {
            buffer.invalidate_textures();
        }
        if state.page == 5 {
            geometry.prepare(&mut grid, state);
        } else {
            // The CPU draws only after the GPU clear of Back has retired.
            clear_cpu_page(&mut buffer, &color_a, &color_b, &depth, state);
            gpu.submit(buffer);
            buffer = gpu.wait();
            cpu_pages_draw(&mut grid, state, input);
            buffer.reset();
        }
        {
            let mut pass = page_pass(&mut buffer, &color_a, &color_b, &depth, state);
            if state.page == 5 {
                geometry.draw(&mut pass);
            }
            // submit publishes CPU pixels before this common text overlay.
            pass.draw_ui(grid.ptr(), CellRect::full_grid());
        }
        gpu.submit(buffer);
        buffer = gpu.wait();
        let format = page_display_format(state);
        if state.frames & 1 == 0 {
            display.set_next_swap_with_format(color_b.ptr(), format);
        } else {
            display.set_next_swap_with_format(color_a.ptr(), format);
        }
        display.wait_vblank();
        state.frames += 1;
        if CORE_DEMO_FRAMES != 0 && state.frames == CORE_DEMO_FRAMES {
            break;
        }
    }
    halt(0x600d);
}
