//! CPU sine/cosine traces and phase-circle animation.
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

// Q16.16 nearest representation of 2*pi/128 for the raster illustration.
const WAVE_SAMPLE_STEP: fix32 = fix32::from_words(3217, 0);

pub fn waves_page(grid: &mut UiGrid, canvas: CpuCanvas, state: DemoState) {
    page_start(grid, state, b"WAVES");
    text(grid, 4, 4, b"SIN");
    text(grid, 4, 8, b"COS");
    let angle = animation_phase(state);
    let amplitude = fix32::from_int((state.parameter % 24 + 8) as i16);
    canvas_line(canvas, 32, 100, 272, 100, GUIDE);
    canvas_line(canvas, 32, 164, 272, 164, GUIDE);
    let mut x: i16 = 32;
    while x < 273 {
        let sc = fsincos(angle + fix32::from_int(x - 32) * WAVE_SAMPLE_STEP);
        let sine = (sc.x() * amplitude).round().to_int();
        let cosine = (sc.y() * amplitude).round().to_int();
        canvas_rect(canvas, x, 100 - sine, 2, 2, GREEN);
        canvas_rect(canvas, x, 164 - cosine, 2, 2, CYAN);
        x += 1;
    }
    canvas_circle(canvas, 332, 136, 38, RING);
    canvas_line(canvas, 292, 136, 372, 136, GUIDE);
    canvas_line(canvas, 332, 96, 332, 176, GUIDE);
    let sc = fsincos(angle);
    let px = 332 + (sc.y() * fix32::from_int(38)).round().to_int();
    let py = 136 - (sc.x() * fix32::from_int(38)).round().to_int();
    canvas_line(canvas, 332, 136, px, py, YELLOW);
    canvas_rect(canvas, px - 2, py - 2, 5, 5, RED);
}
