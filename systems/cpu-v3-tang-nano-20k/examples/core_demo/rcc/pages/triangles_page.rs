//! CPU-filled and wireframe triangles.
use crate::dsl_rt::*;
mod cpu_canvas;
use cpu_canvas::*;
mod gpu_api;
use crate::common::palette::*;
use crate::common::state::*;
use crate::common::ui::*;
use gpu_api::*;

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
pub fn triangles_page(grid: &mut UiGrid, canvas: CpuCanvas, state: AnimationState) {
    page_start(grid, PageId::Triangles, state.paused, b"TRIANGLES");
    text(grid, 7, 4, b"FILLED");
    text(grid, 31, 4, b"WIREFRAME");
    let angle = animation_phase(state) + fix32::from_int(state.parameter as i16) * fix!(0.125);
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
    canvas_circle(canvas, 292, 136, 56, GUIDE);
    canvas_line(canvas, 292 + ax, 136 + ay, 292 + bx, 136 + by, RED);
    canvas_line(canvas, 292 + bx, 136 + by, 292 + cx, 136 + cy, GREEN);
    canvas_line(canvas, 292 + cx, 136 + cy, 292 + ax, 136 + ay, CYAN);
}
