//! Gallery entries and their selection highlight.
use crate::dsl_rt::*;
mod cpu_canvas;
use cpu_canvas::*;
mod gpu_api;
use gpu_api::*;
mod palette;
use palette::*;
mod state;
use state::*;
mod ui;
use ui::*;

pub fn home_page(grid: &mut UiGrid, canvas: CpuCanvas, state: DemoState) {
    page_start(grid, state, b"DEMONSTRATIONS");
    menu_entry(grid, 1, state.selection, b"TRIANGLES");
    menu_entry(grid, 2, state.selection, b"WAVES");
    menu_entry(grid, 3, state.selection, b"CONTROLLER");
    menu_entry(grid, 4, state.selection, b"COLOR");
    menu_entry(grid, 5, state.selection, b"GEOMETRY");
    canvas_rect(
        canvas,
        16,
        (2 + state.selection * 2) as i16 * 16,
        352,
        16,
        SELECTION,
    );
}
