//! Display color choices and CPU gray/color swatches.
use crate::dsl_rt::*;
mod cpu_canvas;
use cpu_canvas::*;
mod gpu_api;
use crate::common::palette::*;
use crate::common::state::*;
use crate::common::ui::*;
use gpu_api::*;
mod boot_resources;
use boot_resources::*;

pub fn color_page(
    grid: &mut UiGrid,
    canvas: CpuCanvas,
    cursor: DisplayFormat,
    mode: DisplayFormat,
) {
    page_start(grid, PageId::Color, false, b"COLOR");
    canvas_rect(
        canvas,
        32,
        (3 + cursor as u16 * 2) as i16 * 16,
        144,
        16,
        SELECTION,
    );
    let mut option: u16 = 0;
    while option < 2 {
        let mut icon = ICON_RADIO_OFF;
        if option == mode as u16 {
            icon = ICON_RADIO_ON;
        }
        grid.write_glyph(5, 3 + option * 2, icon);
        option += 1;
    }
    text(grid, 8, 3, b"SRGB");
    text(grid, 8, 5, b"LINEAR");
    color_graphics(canvas);
}
fn color_graphics(canvas: CpuCanvas) {
    let mut step: u16 = 0;
    while step < 32 {
        let color = (step << 11) | ((step * 2 + (step >> 4)) << 5) | step;
        canvas_rect(canvas, (32 + step * 8) as i16, 112, 8, 32, color);
        step += 1;
    }
    canvas_rect(canvas, 32, 160, 56, 16, RED);
    canvas_rect(canvas, 96, 160, 56, 16, GREEN);
    canvas_rect(canvas, 160, 160, 56, 16, BLUE);
    canvas_rect(canvas, 224, 160, 56, 16, CYAN);
    canvas_rect(canvas, 288, 160, 56, 16, YELLOW);
}
