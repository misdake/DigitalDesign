//! CPU page dispatch after the shared clear has retired.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
mod cpu_canvas;
use crate::common::state::*;
use crate::common::working_set::*;
use cpu_canvas::*;
mod gamepad;
use crate::pages::color_page::*;
use crate::pages::controller_page::*;
use crate::pages::home_page::*;
use crate::pages::triangles_page::*;
use crate::pages::waves_page::*;
use gamepad::*;

pub fn cpu_pages_draw(grid: &mut UiGrid, state: DemoState, input: GamepadFrame) {
    let mut surface = COLOR_A;
    if state.frames & 1 == 0 {
        surface = COLOR_B;
    }
    let canvas = unsafe { canvas_from_surface(surface) };
    if state.page == PageId::Home {
        home_page(grid, canvas, state.selection);
    } else if state.page == PageId::Triangles {
        triangles_page(grid, canvas, state.triangles);
    } else if state.page == PageId::Waves {
        waves_page(grid, canvas, state.waves);
    } else if state.page == PageId::Controller {
        controller_page(grid, canvas, input);
    } else if state.page == PageId::Color {
        color_page(grid, canvas, state.color_cursor, state.color_mode);
    }
}
pub fn page_display_format(state: DemoState) -> DisplayFormat {
    if state.page == PageId::Color {
        color_display_format(state.color_mode)
    } else {
        DisplayFormat::Rgb565
    }
}
