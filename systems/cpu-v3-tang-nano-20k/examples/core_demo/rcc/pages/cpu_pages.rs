//! CPU page dispatch after the shared clear has retired.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
mod cpu_canvas;
use cpu_canvas::*;
mod working_set;
use working_set::*;
mod state;
use state::*;
mod gamepad;
use gamepad::*;
mod home_page;
use home_page::*;
mod triangles_page;
use triangles_page::*;
mod waves_page;
use waves_page::*;
mod controller_page;
use controller_page::*;
mod color_page;
use color_page::*;

pub fn cpu_pages_draw(grid: &mut UiGrid, state: DemoState, input: GamepadFrame) {
    let mut surface = COLOR_A;
    if state.frames & 1 == 0 {
        surface = COLOR_B;
    }
    let canvas = unsafe { canvas_from_surface(surface) };
    if state.page == 0 {
        home_page(grid, canvas, state);
    } else if state.page == 1 {
        triangles_page(grid, canvas, state);
    } else if state.page == 2 {
        waves_page(grid, canvas, state);
    } else if state.page == 3 {
        controller_page(grid, canvas, state, input);
    } else if state.page == 4 {
        color_page(grid, canvas, state);
    }
}
pub fn page_display_format(state: DemoState) -> DisplayFormat {
    if state.page == 4 {
        color_display_format(state.color_mode)
    } else {
        DisplayFormat::Rgb565
    }
}
