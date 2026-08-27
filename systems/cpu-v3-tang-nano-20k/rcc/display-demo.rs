//! Draws into alternating 320x240 RGB565 framebuffers and publishes each
//! completed back buffer at vertical blanking.

use crate::dsl_rt::*;

const WIDTH: u16 = 320;
const HEIGHT: u16 = 240;
const FB_A_SEGMENT: u16 = 0x20;
const FB_A_OFFSET: u16 = 0x0100;
const FB_B_SEGMENT: u16 = 0x21;
const FB_B_OFFSET: u16 = 0x2d00;
const NEXT_SWAP: u16 = 1;

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

fn fill_buffer(base_segment: u16, base_offset: u16) {
    let mut segment = base_segment;
    let mut offset = base_offset;
    let mut y: u16 = 0;
    while y < HEIGHT {
        let mut x: u16 = 0;
        while x < WIDTH {
            store_at(segment, offset, background(x, y));
            offset += 1;
            if offset == 0 {
                segment += 1;
            }
            x += 1;
        }
        y += 1;
    }
}

fn paint_square(base_segment: u16, base_offset: u16, left: u16, color: u16, restore: u16) {
    let mut y: u16 = 64;
    let mut row_segment = base_segment;
    let mut row_offset = base_offset + 0x5000;
    if row_offset < base_offset {
        row_segment += 1;
    }
    while y < 96 {
        let mut pixel_segment = row_segment;
        let mut pixel_offset = row_offset + left;
        if pixel_offset < row_offset {
            pixel_segment += 1;
        }
        let mut x: u16 = left;
        while x < left + 32 {
            if restore != 0 {
                store_at(pixel_segment, pixel_offset, background(x, y));
            } else {
                store_at(pixel_segment, pixel_offset, color);
            }
            pixel_offset += 1;
            if pixel_offset == 0 {
                pixel_segment += 1;
            }
            x += 1;
        }
        row_offset += WIDTH;
        if row_offset < WIDTH {
            row_segment += 1;
        }
        y += 1;
    }
}

fn select_next_framebuffer(segment: u16, offset: u16) {
    dev_send(3, 1, offset);
    dev_send(3, 2, segment);
    dev_send(3, 3, NEXT_SWAP);
}

fn wait_next_frame() {
    let frame = dev_recv(3, 0);
    let mut current = frame;
    while current == frame {
        current = dev_recv(3, 0);
    }
}

#[allow(clippy::eq_op)]
fn main() {
    fill_buffer(FB_A_SEGMENT, FB_A_OFFSET);
    fill_buffer(FB_B_SEGMENT, FB_B_OFFSET);

    let mut left: u16 = 0;
    let mut direction: u16 = 1;
    let mut frame: u16 = 0;
    let mut back: u16 = 1;
    let mut a_valid: u16 = 0;
    let mut b_valid: u16 = 0;
    let mut a_left: u16 = 0;
    let mut b_left: u16 = 0;
    while 1 == 1 {
        let color = 0xf800 | ((frame & 63) << 5) | (frame & 31);
        if back == 0 {
            if a_valid != 0 {
                paint_square(FB_A_SEGMENT, FB_A_OFFSET, a_left, 0, 1);
            }
            paint_square(FB_A_SEGMENT, FB_A_OFFSET, left, color, 0);
            a_left = left;
            a_valid = 1;
            select_next_framebuffer(FB_A_SEGMENT, FB_A_OFFSET);
            back = 1;
        } else {
            if b_valid != 0 {
                paint_square(FB_B_SEGMENT, FB_B_OFFSET, b_left, 0, 1);
            }
            paint_square(FB_B_SEGMENT, FB_B_OFFSET, left, color, 0);
            b_left = left;
            b_valid = 1;
            select_next_framebuffer(FB_B_SEGMENT, FB_B_OFFSET);
            back = 0;
        }
        wait_next_frame();
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
