//! Calibrated stick plots, button panels and connection state.
use crate::dsl_rt::*;
mod cpu_canvas;
use cpu_canvas::*;
mod gpu_api;
use crate::common::palette::*;
use crate::common::state::*;
use crate::common::ui::*;
use gpu_api::*;
mod gamepad;
use gamepad::*;
mod boot_resources;
use boot_resources::*;

fn stick(canvas: CpuCanvas, cx: i16, x: fix32, y: fix32) {
    canvas_circle(canvas, cx, 128, 30, RING);
    canvas_circle(canvas, cx, 128, 6, GUIDE);
    canvas_line(canvas, cx - 30, 128, cx + 30, 128, GUIDE);
    canvas_line(canvas, cx, 98, cx, 158, GUIDE);
    let px = cx + (x * fix32::from_int(28)).round().to_int();
    // Shared stick output is +Y down, matching native screen coordinates.
    let py = 128 + (y * fix32::from_int(28)).round().to_int();
    canvas_line(canvas, cx, 128, px, py, CYAN);
    canvas_rect(canvas, px - 2, py - 2, 5, 5, GREEN);
}
pub fn controller_page(grid: &mut UiGrid, canvas: CpuCanvas, input: GamepadFrame) {
    page_start(grid, PageId::Controller, false, b"CONTROLLER");
    text(grid, 5, 4, b"LEFT");
    text(grid, 19, 4, b"RIGHT");
    let mut link = ICON_RADIO_OFF;
    if input.connected != 0 {
        link = ICON_RADIO_ON;
    }
    grid.write_glyph(2, 12, link);
    stick(canvas, 72, input.left_x, input.left_y);
    stick(canvas, 184, input.right_x, input.right_y);
    let mut button: u16 = 0;
    while button < 16 {
        let x = 264 + (button % 4) * 32;
        let y = 64 + (button / 4) * 32;
        let mut color: u16 = BUTTON_IDLE;
        if input.buttons & (1u16 << button) != 0 {
            color = BUTTON_ACTIVE;
        }
        canvas_rect(canvas, x as i16, y as i16, 30, 24, color);
        button += 1;
    }
    text(grid, 33, 4, b"SEL L3  R3  STA");
    text(grid, 33, 6, b"UP  RIG DN  LEF");
    text(grid, 33, 8, b"L2  R2  L1  R1");
    text(grid, 33, 10, b"TRI CIR X   SQR");
}
