//! Comprehensive physical-board demo for the CPU, write-back D-cache, FPU,
//! SDRAM framebuffer, and display handoff. The CPU redraws an RGB565 back
//! buffer, the FPU evaluates animated sine/cosine curves and a parametric
//! circle, and rounded FPU results return through integer registers before
//! normal cached stores write the pixels. Each completed buffer is cleaned
//! before it is published to the display at vertical blanking.

use crate::dsl_rt::*;
mod device_abi;

const WIDTH: u16 = 320;
const HEIGHT: u16 = 240;
const FB_A_SEGMENT: u16 = 0x20;
const FB_A_OFFSET: u16 = 0x0100;
const FB_B_SEGMENT: u16 = 0x21;
const FB_B_OFFSET: u16 = 0x2d00;

fn background(x: u16, y: u16) -> u16 {
    if x & 31 == 0 || y & 31 == 0 {
        0x18e3
    } else {
        0x0841 | ((x >> 4) & 1) | (((y >> 4) & 1) << 5)
    }
}

/// The immutable layer underneath the animated curves and circle. Keeping the
/// axes here lets an erase pass restore crossings without punching holes in
/// the static drawing.
fn static_pixel(x: u16, y: u16) -> u16 {
    if y == 58
        || y == 118
        || (x == 240 && y >= 132 && y < 232)
        || (y == 182 && x >= 188 && x < 293)
    {
        0x39e7
    } else {
        background(x, y)
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
            store_at(segment, offset, static_pixel(x, y));
            offset += 1;
            if offset == 0 {
                segment += 1;
            }
            x += 1;
        }
        y += 1;
    }
}

/// Resolve a screen coordinate in constant time without 32-bit arithmetic.
/// `239 * 320` crosses the 16-bit boundary once; the two following additions
/// can each carry into the segment as well.
fn plot(base_segment: u16, base_offset: u16, x: u16, y: u16, color: u16) {
    let mut segment = base_segment;
    let row_offset = y * WIDTH;
    if y >= 205 {
        segment += 1;
    }
    let offset = base_offset + row_offset;
    if offset < base_offset {
        segment += 1;
    }
    let pixel_offset = offset + x;
    if pixel_offset < offset {
        segment += 1;
    }
    store_at(segment, pixel_offset, color);
}

/// Draw two independently visible ROM-sincos results. Multiplication scales
/// Q8.8 values to pixels; FROUND followed by `to_int()` deliberately exercises
/// the FPU-to-integer path before every framebuffer store. When `restore` is
/// nonzero, recompute the old geometry but replace it with the static layer.
fn draw_waveforms(base_segment: u16, base_offset: u16, phase: u16, restore: u16) {
    let amplitude = fix16::from_int(24);
    let mut x: u16 = 0;
    while x < WIDTH {
        // 10 / 256 radians per pixel gives almost two periods across 320 px.
        let angle = fix16::from_bits(phase + x * 10);
        let sc = fsincos(angle);
        let sine_offset = (sc.x() * amplitude).round().to_int();
        let cosine_offset = (sc.y() * amplitude).round().to_int();
        let sine_y = (58i16 + sine_offset) as u16;
        let cosine_y = (118i16 + cosine_offset) as u16;
        let sine_color = if restore == 0 {
            0x07e0
        } else {
            static_pixel(x, sine_y)
        };
        let sine_color_2 = if restore == 0 {
            0x07e0
        } else {
            static_pixel(x, sine_y + 1)
        };
        let cosine_color = if restore == 0 {
            0x07ff
        } else {
            static_pixel(x, cosine_y)
        };
        let cosine_color_2 = if restore == 0 {
            0x07ff
        } else {
            static_pixel(x, cosine_y + 1)
        };
        plot(base_segment, base_offset, x, sine_y, sine_color);
        plot(base_segment, base_offset, x, sine_y + 1, sine_color_2);
        plot(base_segment, base_offset, x, cosine_y, cosine_color);
        plot(base_segment, base_offset, x, cosine_y + 1, cosine_color_2);
        x += 1;
    }
}

fn draw_circle(base_segment: u16, base_offset: u16, phase: u16, restore: u16) {
    let radius = fix16::from_int(46);
    let mut angle_bits = phase;
    let mut sample: u16 = 0;
    while sample < 256 {
        let sc = fsincos(fix16::from_bits(angle_bits));
        let x_offset = (sc.y() * radius).round().to_int();
        let y_offset = (sc.x() * radius).round().to_int();
        let x = (240i16 + x_offset) as u16;
        let y = (182i16 + y_offset) as u16;
        let color = if restore != 0 {
            static_pixel(x, y)
        } else {
            if sample < 128 {
                0xffe0
            } else {
                0xf81f
            }
        };
        plot(base_segment, base_offset, x, y, color);

        // 6.25 raw Q8.8 steps approximate 2*pi over 256 samples without
        // integer division: three 6s followed by a 7.
        angle_bits += 6;
        if sample & 3 == 3 {
            angle_bits += 1;
        }
        sample += 1;
    }

    // A red phase marker makes it obvious that new FPU results, CPU stores,
    // cache cleaning, and display swaps continue to complete frame by frame.
    let marker = fsincos(fix16::from_bits(phase));
    let marker_x = (240i16 + (marker.y() * radius).round().to_int()) as u16;
    let marker_y = (182i16 + (marker.x() * radius).round().to_int()) as u16;
    let marker_color = if restore == 0 {
        0xf800
    } else {
        static_pixel(marker_x, marker_y)
    };
    let marker_x_color = if restore == 0 {
        0xf800
    } else {
        static_pixel(marker_x + 1, marker_y)
    };
    let marker_y_color = if restore == 0 {
        0xf800
    } else {
        static_pixel(marker_x, marker_y + 1)
    };
    plot(base_segment, base_offset, marker_x, marker_y, marker_color);
    plot(
        base_segment,
        base_offset,
        marker_x + 1,
        marker_y,
        marker_x_color,
    );
    plot(
        base_segment,
        base_offset,
        marker_x,
        marker_y + 1,
        marker_y_color,
    );
}

fn render_dynamic(base_segment: u16, base_offset: u16, phase: u16, restore: u16) {
    draw_waveforms(base_segment, base_offset, phase, restore);
    draw_circle(base_segment, base_offset, phase, restore);
}

fn select_next_framebuffer(segment: u16, offset: u16) {
    // The display reads SDRAM directly and does not snoop the CPU's write-back
    // D-cache. Complete the ownership handoff before publishing this buffer.
    let clean_status = dcache_clean_all();
    if clean_status != CACHE_MAINTENANCE_STATUS_SUCCESS {
        halt(clean_status);
    }
    dev_send(DISPLAY_DEVICE, DISPLAY_STAGE_FRAMEBUFFER_LOW, offset);
    dev_send(DISPLAY_DEVICE, DISPLAY_STAGE_FRAMEBUFFER_HIGH, segment);
    dev_send(DISPLAY_DEVICE, DISPLAY_SWAP_COMMAND, DISPLAY_NEXT_SWAP);
}

fn wait_next_frame() {
    let frame = dev_recv(DISPLAY_DEVICE, DISPLAY_FRAME_INDEX);
    let mut current = frame;
    while current == frame {
        current = dev_recv(DISPLAY_DEVICE, DISPLAY_FRAME_INDEX);
    }
}

#[allow(clippy::eq_op)]
fn main() {
    let mut phase: u16 = 0;
    let mut phase_a: u16 = 0;
    let mut phase_b: u16 = 0;
    let mut back: u16 = 0;
    fill_buffer(FB_A_SEGMENT, FB_A_OFFSET);
    fill_buffer(FB_B_SEGMENT, FB_B_OFFSET);
    while 1 == 1 {
        if back == 0 {
            render_dynamic(FB_A_SEGMENT, FB_A_OFFSET, phase_a, 1);
            render_dynamic(FB_A_SEGMENT, FB_A_OFFSET, phase, 0);
            select_next_framebuffer(FB_A_SEGMENT, FB_A_OFFSET);
            phase_a = phase;
            back = 1;
        } else {
            render_dynamic(FB_B_SEGMENT, FB_B_OFFSET, phase_b, 1);
            render_dynamic(FB_B_SEGMENT, FB_B_OFFSET, phase, 0);
            select_next_framebuffer(FB_B_SEGMENT, FB_B_OFFSET);
            phase_b = phase;
            back = 0;
        }
        wait_next_frame();
        phase += 24;
    }
}
