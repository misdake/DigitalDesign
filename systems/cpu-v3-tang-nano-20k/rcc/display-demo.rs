//! Continuously writes the fixed 320x240 RGB565 framebuffer. The display is
//! deliberately unsynchronized: tearing is part of this first bring-up.

use crate::dsl_rt::*;

const WIDTH: u16 = 320;
const HEIGHT: u16 = 240;
// Rows 0..203 exactly fill segment 0x20 offsets 0x0100..0xFFFF.
const FIRST_ROWS: u16 = 204;

fn background(x: u16, y: u16) -> u16 {
    if x & 31 == 0 || y & 31 == 0 {
        0x2104
    } else {
        ((x & 31) << 11) | ((y & 63) << 5) | ((x + y) & 31)
    }
}

/// Compute every address/value before changing DSEG. Only the store itself is
/// performed while the framebuffer segment is selected, so the compiler's
/// stack and static data remain in segment zero.
fn store_at(segment: u16, offset: u16, value: u16) {
    let mut pixel = Ptr::from_addr(offset).as_u16_array();
    mtsr_dseg(segment);
    pixel[0u16] = value;
    mtsr_dseg(0);
}

fn paint_square(left: u16, color: u16, restore: u16) {
    let mut y: u16 = 64;
    let mut row_offset: u16 = 0x5100;
    while y < 96 {
        let mut x: u16 = left;
        while x < left + 32 {
            if restore != 0 {
                store_at(0x20, row_offset + x, background(x, y));
            } else {
                store_at(0x20, row_offset + x, color);
            }
            x += 1;
        }
        row_offset += WIDTH;
        y += 1;
    }
}

#[allow(clippy::eq_op)]
fn main() {
    let mut y: u16 = 0;
    let mut segment: u16 = 0x20;
    let mut row_offset: u16 = 0x0100;
    while y < HEIGHT {
        let mut x: u16 = 0;
        while x < WIDTH {
            store_at(segment, row_offset + x, background(x, y));
            x += 1;
        }
        row_offset += WIDTH;
        y += 1;
        if y == FIRST_ROWS {
            segment = 0x21;
            row_offset = 0;
        }
    }

    let mut left: u16 = 0;
    let mut direction: u16 = 1;
    let mut frame: u16 = 0;
    while 1 == 1 {
        let color = 0xf800 | ((frame & 63) << 5) | (frame & 31);
        paint_square(left, color, 0);

        // The write-through SDRAM stores provide most of the visible pacing.
        let mut delay: u16 = 0;
        while delay < 2000 {
            delay += 1;
        }

        paint_square(left, 0, 1);
        if left == 288 {
            direction = 0;
        } else if left == 0 {
            direction = 1;
        }
        if direction == 1 {
            left += 1;
        } else {
            left -= 1;
        }
        frame += 1;
    }
}
