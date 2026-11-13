//! The core demonstration's ordinary public-API entry. No profiler device hooks.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
mod gpu_board;
use gpu_board::*;
mod boot_resources;
use boot_resources::*;
mod gamepad;
use gamepad::*;
mod time;
use time::*;
mod state;
use state::*;
mod ui;
use ui::*;
mod cpu_canvas;
use cpu_canvas::*;
mod gpu_layout;
use gpu_layout::*;
mod graphics_pages;
use graphics_pages::*;
mod system_pages;
use system_pages::*;

fn record(
    buffer: &mut CommandBuffer,
    grid: &UiGrid,
    color: &ColorBuffer,
    depth: &DepthBuffer,
    overlay: bool,
) {
    let mut load = ColorLoad::clear(Rgb565::from_raw(0x0843));
    if overlay {
        load = ColorLoad::load();
    }
    let mut pass = buffer.pass(color, depth, load, DepthLoad::load());
    if overlay {
        pass.draw_ui(
            grid,
            CellRect {
                x: 0,
                y: 0,
                width: 50,
                height: 15,
            },
        );
    }
}
fn main() {
    let mut gpu = Gpu::init(board_gpu_config());
    let mut buffer = gpu.command_buffer();
    let color_a = board_color_a();
    let color_b = board_color_b();
    let depth = board_depth();
    let mut display = Display::open();
    let atlas = UiAtlas::from_native(
        unsafe { GpuMemory::from_sectors(CORE_ATLAS_SECTOR, CORE_ATLAS_SECTORS) },
        CORE_ATLAS_GLYPHS,
    );
    let mut grid = UiGrid::from_memory(board_ui_memory(false), &atlas);
    let mut state = initial_state(CORE_DEMO_PAGE);
    gamepad_init();
    loop {
        gamepad_capture();
        let input = gamepad_take_frame();
        state = update(state, input.pressed, time_milliseconds());
        // Clear and retire first: the CPU owns only the completed, non-Front back plane.
        buffer.reset();
        if state.frames == 0 {
            buffer.invalidate_textures();
        }
        if state.frames & 1 == 0 {
            record(&mut buffer, &grid, &color_b, &depth, false);
        } else {
            record(&mut buffer, &grid, &color_a, &depth, false);
        }
        gpu.submit(buffer);
        buffer = gpu.wait();
        let mut base = GPU_COLOR_A_SECTOR;
        if state.frames & 1 == 0 {
            base = GPU_COLOR_B_SECTOR;
        }
        let canvas = unsafe { canvas_from_sectors(base, GPU_PLANE_SECTORS) };
        if state.page == 0 {
            home_page(&mut grid, state);
            canvas_rect(
                canvas,
                16,
                (3 + state.selection * 2) as i16 * 16,
                352,
                16,
                0x0328,
            );
        } else if state.page == 1 {
            software_page(&mut grid, canvas, state);
        } else if state.page == 2 {
            wave_page(&mut grid, canvas, state);
        } else if state.page == 3 {
            controller_page(&mut grid, canvas, state, input);
        } else if state.page == 4 {
            color_page(&mut grid, canvas, state);
            color_graphics(canvas);
        }
        // submit publishes cached CPU pixels before the GPU adds its text overlay.
        buffer.reset();
        if state.frames & 1 == 0 {
            record(&mut buffer, &grid, &color_b, &depth, true);
        } else {
            record(&mut buffer, &grid, &color_a, &depth, true);
        }
        gpu.submit(buffer);
        buffer = gpu.wait();
        let mut format = DisplayFormat::Rgb565;
        if state.page == 4 && state.color_mode == 1 {
            format = DisplayFormat::LinearRgb565;
        }
        if state.frames & 1 == 0 {
            display.set_next_swap_with_format(&color_b, format);
        } else {
            display.set_next_swap_with_format(&color_a, format);
        }
        display.wait_vblank();
        state.frames += 1;
        if CORE_DEMO_FRAMES != 0 && state.frames == CORE_DEMO_FRAMES {
            break;
        }
    }
    halt(0x600d);
}
