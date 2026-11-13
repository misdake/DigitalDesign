//! The core demonstration's ordinary public-API entry. No profiler device hooks.
use crate::dsl_rt::*;
mod common;
mod gpu_api;
mod pages;
use crate::common::working_set::*;
use gpu_api::*;
mod boot_resources;
use boot_resources::*;
mod gamepad;
use gamepad::*;
mod time;
use crate::common::frame::*;
use crate::common::state::*;
use crate::pages::cpu_pages::*;
use crate::pages::geometry_page::*;
use crate::pages::gpu_lab::*;
use time::*;

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
    let mut lab = GpuLab::new(time_milliseconds());
    let mut state = initial_state(CORE_DEMO_PAGE, time_milliseconds());
    gamepad_init();
    loop {
        gamepad_capture();
        let input = gamepad_take_frame();
        let now = time_milliseconds();
        state = update(state, input.pressed, now);
        buffer.reset();
        if state.frames == 0 {
            buffer.invalidate_textures();
        }
        if state.page == PageId::Geometry {
            geometry.prepare(&mut grid, state.geometry);
        } else if state.page == PageId::GpuLab {
            lab.prepare(&mut grid, input, now);
        } else {
            // The CPU draws only after the GPU clear of Back has retired.
            clear_cpu_page(&mut buffer, &color_a, &color_b, &depth, state.frames);
            gpu.submit(buffer);
            buffer = gpu.wait();
            cpu_pages_draw(&mut grid, state, input);
            buffer.reset();
        }
        {
            let mut pass = page_pass(&mut buffer, &color_a, &color_b, &depth, state);
            if state.page == PageId::Geometry {
                geometry.draw(&mut pass);
            } else if state.page == PageId::GpuLab {
                lab.draw(&mut pass);
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
