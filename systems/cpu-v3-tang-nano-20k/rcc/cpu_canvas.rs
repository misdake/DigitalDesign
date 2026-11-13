//! CPU-owned RGB565 column16 canvas. Drawing never submits GPU or Display work.
use crate::dsl_rt::*;

#[derive(Copy, Clone)]
pub struct CpuCanvas {
    sector: u16,
}
/// Caller owns the complete plane for CPU writes, with no GPU/Display reader.
pub unsafe fn canvas_from_sectors(first_sector: u16, sector_count: u16) -> CpuCanvas {
    if sector_count < 1600 || sector_count - 1 > 65535 - first_sector {
        halt(0xca01);
    }
    CpuCanvas {
        sector: first_sector,
    }
}
/// Clipped, half-open horizontal span. No logical window pointer escapes.
pub fn canvas_span(canvas: CpuCanvas, mut left: i16, mut right: i16, y: i16, color: u16) {
    if y < 0 || y >= 240 || left >= 400 || right <= 0 {
        return;
    }
    if left < 0 {
        left = 0;
    }
    if right > 400 {
        right = 400;
    }
    let saved = read_dseg_page(3);
    let mut x = left as u16;
    while x < right as u16 {
        let sector = canvas.sector + (x >> 4) * 64 + ((y as u16) >> 2);
        let offset = ((sector & 255) << 6) | (((y as u16) & 3) << 4) | (x & 15);
        let mut count = 16 - (x & 15);
        if count > right as u16 - x {
            count = right as u16 - x;
        }
        dcache_wait();
        unsafe {
            write_dseg_page(3, sector >> 8);
        }
        let destination = Ptr::from_addr(0xc000 | offset);
        let mut i: u16 = 0;
        while i < count {
            unsafe {
                destination.write(i as i16, color);
            }
            i += 1;
        }
        x += count;
    }
    dcache_wait();
    unsafe {
        write_dseg_page(3, saved);
    }
}
pub fn canvas_pixel(canvas: CpuCanvas, x: i16, y: i16, color: u16) {
    if x < 0 || x >= 400 || y < 0 || y >= 240 {
        return;
    }
    canvas_span(canvas, x, x + 1, y, color);
}
pub fn canvas_rect(canvas: CpuCanvas, x: i16, y: i16, width: i16, height: i16, color: u16) {
    if width < 0
        || width > 400
        || height < 0
        || height > 240
        || x < -400
        || x >= 400
        || y < -240
        || y >= 240
    {
        return;
    }
    let mut row: i16 = 0;
    while row < height {
        canvas_span(canvas, x, x + width, y + row, color);
        row += 1;
    }
}
pub fn canvas_line(canvas: CpuCanvas, mut x: i16, mut y: i16, end_x: i16, end_y: i16, color: u16) {
    // In-screen endpoints keep all signed differences representable.
    if x < 0
        || x >= 400
        || end_x < 0
        || end_x >= 400
        || y < 0
        || y >= 240
        || end_y < 0
        || end_y >= 240
    {
        return;
    }
    let mut dx = end_x - x;
    let mut dy = end_y - y;
    let mut sx: i16 = 1;
    let mut sy: i16 = 1;
    if dx < 0 {
        dx = -dx;
        sx = -1;
    }
    if dy < 0 {
        dy = -dy;
        sy = -1;
    }
    let mut error = dx - dy;
    loop {
        canvas_pixel(canvas, x, y, color);
        if x == end_x && y == end_y {
            break;
        }
        let twice = error * 2;
        if twice > -dy {
            error -= dy;
            x += sx;
        }
        if twice < dx {
            error += dx;
            y += sy;
        }
    }
}
pub fn canvas_circle(canvas: CpuCanvas, cx: i16, cy: i16, radius: i16, color: u16) {
    if radius < 0 || radius > 200 || cx < 0 || cx >= 400 || cy < 0 || cy >= 240 {
        return;
    }
    let mut x = radius;
    let mut y: i16 = 0;
    let mut error = 1 - radius;
    while x >= y {
        canvas_pixel(canvas, cx + x, cy + y, color);
        canvas_pixel(canvas, cx + y, cy + x, color);
        canvas_pixel(canvas, cx - y, cy + x, color);
        canvas_pixel(canvas, cx - x, cy + y, color);
        canvas_pixel(canvas, cx - x, cy - y, color);
        canvas_pixel(canvas, cx - y, cy - x, color);
        canvas_pixel(canvas, cx + y, cy - x, color);
        canvas_pixel(canvas, cx + x, cy - y, color);
        y += 1;
        if error < 0 {
            error += 2 * y + 1;
        } else {
            x -= 1;
            error += 2 * (y - x) + 1;
        }
    }
}
