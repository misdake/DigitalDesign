//! A short gallery and explicit color-interpretation choices.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
mod palette;
use palette::*;
mod cpu_canvas;
use cpu_canvas::*;
mod boot_resources;
use boot_resources::*;
mod ui;
use ui::*;
mod state;
use state::*;

pub fn home_page(grid: &mut UiGrid, state: DemoState) {
    page_start(grid, state, b"DEMONSTRATIONS");
    menu_entry(grid, 1, state.selection, b"TRIANGLES");
    menu_entry(grid, 2, state.selection, b"WAVES");
    menu_entry(grid, 3, state.selection, b"CONTROLLER");
    menu_entry(grid, 4, state.selection, b"COLOR");
}
pub fn color_page(grid: &mut UiGrid, canvas: CpuCanvas, state: DemoState) {
    page_start(grid, state, b"COLOR");
    canvas_rect(
        canvas,
        32,
        (3 + state.color_cursor * 2) as i16 * 16,
        144,
        16,
        SELECTION,
    );
    let mut option: u16 = 0;
    while option < 2 {
        let mut icon = ICON_RADIO_OFF;
        if option == state.color_mode {
            icon = ICON_RADIO_ON;
        }
        grid.write_glyph(5, 3 + option * 2, icon);
        option += 1;
    }
    text(grid, 8, 3, b"SRGB");
    text(grid, 8, 5, b"LINEAR");
}
