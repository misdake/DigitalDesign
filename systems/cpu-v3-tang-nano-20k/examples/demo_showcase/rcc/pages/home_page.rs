//! Gallery entries and their selection highlight.
use crate::dsl_rt::*;
mod cpu_canvas;
use cpu_canvas::*;
mod gpu_api;
use crate::common::palette::*;
use crate::common::state::*;
use crate::common::ui::*;
use gpu_api::*;

pub fn home_page(grid: &mut UiGrid, canvas: CpuCanvas, selection: u16) {
    page_start(grid, PageId::Home, false, b"DEMONSTRATIONS");
    menu_entry(grid, 1, selection, b"TRIANGLES");
    menu_entry(grid, 2, selection, b"WAVES");
    menu_entry(grid, 3, selection, b"CONTROLLER");
    menu_entry(grid, 4, selection, b"COLOR");
    menu_entry(grid, 5, selection, b"GEOMETRY");
    menu_entry(grid, 6, selection, b"GPU LAB");
    canvas_rect(canvas, 16, (3 + selection) as i16 * 16, 352, 16, SELECTION);
}
