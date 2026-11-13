//! App-owned illustrations. All shapes below are rasterized by the CPU.
use crate::dsl_rt::*;
mod cpu_canvas;
use cpu_canvas::*;
mod gpu_api;
use gpu_api::*;
mod gamepad;
use gamepad::*;
mod state;
use state::*;
mod ui;
use ui::*;
mod boot_resources;
use boot_resources::*;

fn phase(state: DemoState) -> fix32 {
    fix32::from_int((state.elapsed % 6283) as i16) * fix32::from_words(66, 0)
}
fn edge(ax: i16, ay: i16, bx: i16, by: i16, x: i16, y: i16) -> i16 {
    (bx - ax) * (y - ay) - (by - ay) * (x - ax)
}
struct Triangle {
    ax: i16,
    ay: i16,
    bx: i16,
    by: i16,
    cx: i16,
    cy: i16,
}
fn triangle(canvas: CpuCanvas, t: Triangle) {
    let center: i16 = 100;
    // Edge setup runs once; advancing a pixel or row needs only additions.
    let abx = t.bx - t.ax;
    let aby = t.by - t.ay;
    let bcx = t.cx - t.bx;
    let bcy = t.cy - t.by;
    let cax = t.ax - t.cx;
    let cay = t.ay - t.cy;
    let mut row_ab = edge(t.ax, t.ay, t.bx, t.by, center - 64, 72);
    let mut row_bc = edge(t.bx, t.by, t.cx, t.cy, center - 64, 72);
    let mut row_ca = edge(t.cx, t.cy, t.ax, t.ay, center - 64, 72);
    let mut y: i16 = 72;
    while y < 200 {
        let mut x = center - 64;
        let mut left = center + 64;
        let mut right = center - 64;
        let mut eab = row_ab;
        let mut ebc = row_bc;
        let mut eca = row_ca;
        while x < center + 64 {
            if eab >= 0 && ebc >= 0 && eca >= 0 {
                if x < left {
                    left = x;
                }
                right = x + 1;
            }
            eab -= aby;
            ebc -= bcy;
            eca -= cay;
            x += 1;
        }
        row_ab += abx;
        row_bc += bcx;
        row_ca += cax;
        if left < right {
            let red = ((y - 72) as u16) / 4;
            canvas_span(
                canvas,
                left,
                right,
                y,
                (red << 11) | ((63 - red) << 5) | (31 - red),
            );
        }
        y += 1;
    }
}
pub fn software_page(grid: &mut UiGrid, canvas: CpuCanvas, state: DemoState) {
    page_start(grid, state, b"TRIANGLES");
    text(grid, 7, 4, b"FILLED");
    text(grid, 31, 4, b"WIREFRAME");
    let angle = phase(state) + fix32::from_int(state.parameter as i16) * fix32::from_words(8192, 0);
    let sc = fsincos(angle);
    let ax = (sc.y() * fix32::from_int(-40) - sc.x() * fix32::from_int(-30))
        .round()
        .to_int();
    let ay = (sc.x() * fix32::from_int(-40) + sc.y() * fix32::from_int(-30))
        .round()
        .to_int();
    let bx = (sc.y() * fix32::from_int(50)).round().to_int();
    let by = (sc.x() * fix32::from_int(50)).round().to_int();
    let cx = (sc.y() * fix32::from_int(-40) - sc.x() * fix32::from_int(30))
        .round()
        .to_int();
    let cy = (sc.x() * fix32::from_int(-40) + sc.y() * fix32::from_int(30))
        .round()
        .to_int();
    triangle(
        canvas,
        Triangle {
            ax: 100 + ax,
            ay: 136 + ay,
            bx: 100 + bx,
            by: 136 + by,
            cx: 100 + cx,
            cy: 136 + cy,
        },
    );
    canvas_circle(canvas, 292, 136, 56, 0x2108);
    canvas_line(canvas, 292 + ax, 136 + ay, 292 + bx, 136 + by, 0xf800);
    canvas_line(canvas, 292 + bx, 136 + by, 292 + cx, 136 + cy, 0x07e0);
    canvas_line(canvas, 292 + cx, 136 + cy, 292 + ax, 136 + ay, 0x07ff);
}
pub fn wave_page(grid: &mut UiGrid, canvas: CpuCanvas, state: DemoState) {
    page_start(grid, state, b"WAVES");
    text(grid, 4, 4, b"SIN");
    text(grid, 4, 8, b"COS");
    let angle = phase(state);
    let amplitude = fix32::from_int((state.parameter % 24 + 8) as i16);
    canvas_line(canvas, 32, 100, 272, 100, 0x2108);
    canvas_line(canvas, 32, 164, 272, 164, 0x2108);
    let mut x: i16 = 32;
    while x < 273 {
        let sc = fsincos(angle + fix32::from_int(x - 32) * fix32::from_words(3217, 0));
        let sine = (sc.x() * amplitude).round().to_int();
        let cosine = (sc.y() * amplitude).round().to_int();
        canvas_rect(canvas, x, 100 - sine, 2, 2, 0x07e0);
        canvas_rect(canvas, x, 164 - cosine, 2, 2, 0x07ff);
        x += 1;
    }
    canvas_circle(canvas, 332, 136, 38, 0x39e7);
    canvas_line(canvas, 292, 136, 372, 136, 0x2108);
    canvas_line(canvas, 332, 96, 332, 176, 0x2108);
    let sc = fsincos(angle);
    let px = 332 + (sc.y() * fix32::from_int(38)).round().to_int();
    let py = 136 - (sc.x() * fix32::from_int(38)).round().to_int();
    canvas_line(canvas, 332, 136, px, py, 0xffe0);
    canvas_rect(canvas, px - 2, py - 2, 5, 5, 0xf800);
}
fn stick(canvas: CpuCanvas, cx: i16, x: fix32, y: fix32) {
    canvas_circle(canvas, cx, 128, 30, 0x39e7);
    canvas_circle(canvas, cx, 128, 6, 0x2108);
    canvas_line(canvas, cx - 30, 128, cx + 30, 128, 0x2108);
    canvas_line(canvas, cx, 98, cx, 158, 0x2108);
    let px = cx + (x * fix32::from_int(28)).round().to_int();
    // Shared stick output is +Y down, matching native screen coordinates.
    let py = 128 + (y * fix32::from_int(28)).round().to_int();
    canvas_line(canvas, cx, 128, px, py, 0x07ff);
    canvas_rect(canvas, px - 2, py - 2, 5, 5, 0x07e0);
}
pub fn controller_page(
    grid: &mut UiGrid,
    canvas: CpuCanvas,
    state: DemoState,
    input: GamepadFrame,
) {
    page_start(grid, state, b"CONTROLLER");
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
        let mut color: u16 = 0x18c3;
        if input.buttons & (1u16 << button) != 0 {
            color = 0x048c;
        }
        canvas_rect(canvas, x as i16, y as i16, 30, 24, color);
        button += 1;
    }
    text(grid, 33, 4, b"SEL L3  R3  STA");
    text(grid, 33, 6, b"UP  RIG DN  LEF");
    text(grid, 33, 8, b"L2  R2  L1  R1");
    text(grid, 33, 10, b"TRI CIR X   SQR");
}
pub fn color_graphics(canvas: CpuCanvas) {
    let mut step: u16 = 0;
    while step < 32 {
        let color = (step << 11) | ((step * 2 + (step >> 4)) << 5) | step;
        canvas_rect(canvas, (32 + step * 8) as i16, 112, 8, 32, color);
        step += 1;
    }
    canvas_rect(canvas, 32, 160, 56, 16, 0xf800);
    canvas_rect(canvas, 96, 160, 56, 16, 0x07e0);
    canvas_rect(canvas, 160, 160, 56, 16, 0x001f);
    canvas_rect(canvas, 224, 160, 56, 16, 0x07ff);
    canvas_rect(canvas, 288, 160, 56, 16, 0xffe0);
}
