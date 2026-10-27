//! CPU-only cold-boot, SDRAM and continuous Display diagnostic.
//! Scratch words 0x4000..0x7fff are private to this application; no heap is used.
use crate::dsl_rt::*;
mod device_abi;
use device_abi::*;

fn report(status: u16) {
    let mut index: u16 = 0;
    while index < 8 {
        let mut byte: u16 = 0x44;
        if index == 2 {
            byte = 0x48;
        }
        if index == 3 {
            byte = 0x54;
        }
        if index == 4 {
            byte = 1;
        }
        if index == 5 {
            byte = 0x0b;
        }
        if index == 6 {
            byte = status;
        }
        if index == 7 {
            byte = 0x48 ^ 0x54 ^ 1 ^ 0x0b ^ status;
        }
        while dev_recv(SYSTEM_CONTROL_DEVICE, SYSCTL_UART_STATUS) & 1 != 0 {}
        dev_send(SYSTEM_CONTROL_DEVICE, SYSCTL_UART_TX_DATA, byte);
        index += 1;
    }
}

fn fail(status: u16) {
    dev_send(SYSTEM_CONTROL_DEVICE, SYSCTL_LED, 0x20 | status);
    report(status);
    report(status);
    halt(0xbd00 | status);
}

fn color(column: u16, target: u16) -> u16 {
    if column == 0 && target == 1 {
        return 0x001f;
    }
    if column < 5 {
        return 0xf800;
    }
    if column < 9 {
        return 0x07e0;
    }
    if column < 13 {
        return 0x001f;
    }
    if column < 17 {
        return 0xffe0;
    }
    if column < 21 {
        return 0x07ff;
    }
    return 0xffff;
}

fn fill_tile(page: u16, offset: u16, value: u16) {
    let mut row: u16 = 0;
    while row < 16 {
        let source = Ptr::from_addr(0x4000 + offset + (row << 4));
        let mut x: u16 = 0;
        while x < 16 {
            unsafe {
                source.write(x as i16, value);
            }
            x += 1;
        }
        unsafe {
            dcache_line_copy(source, page);
        }
        dcache_wait();
        row += 1;
    }
}

fn fill_surface(first_page: u16, target: u16) {
    let mut page = first_page;
    let mut offset: u16 = 0;
    let mut tile: u16 = 0;
    let mut column: u16 = 0;
    while tile < 375 {
        fill_tile(page, offset, color(column, target));
        offset += 256;
        if offset == 0x4000 {
            page += 1;
            offset = 0;
        }
        column += 1;
        if column == 25 {
            column = 0;
        }
        tile += 1;
    }
}

// Diagnostic leaf only: entry and exit use legacy DSEG0. The generated
// instruction interval between these writes must contain only this load.
fn physical_read(segment: u16, offset: u16) -> u16 {
    mtsr_dseg(segment);
    let value = unsafe { Ptr::from_addr(offset).read(0) };
    mtsr_dseg(0);
    value
}

fn invalidate() {
    // DCLEANL drains older stores before admission; DWAIT completes it.
    // The sysctl maintenance pulse requires an idle D-cache on this baseline.
    unsafe {
        dcache_clean_line(Ptr::from_addr(0x4000));
    }
    dcache_wait();
    if dcache_invalidate_all() != 0 {
        fail(1);
    }
}

fn frame_wait(previous: u16) {
    let mut spins: u16 = 0;
    let mut wraps: u16 = 0;
    while dev_recv(DISPLAY_DEVICE, DISPLAY_FRAME_INDEX) == previous {
        spins += 1;
        if spins == 0 {
            wraps += 1;
        }
        if wraps == 16 {
            fail(4);
        }
    }
}

fn present(target: u16) {
    let low: u16 = if target == 0 { 0 } else { 0x8000 };
    let high: u16 = if target == 0 { 0x20 } else { 0x21 };
    dev_send(DISPLAY_DEVICE, DISPLAY_STAGE_FRAMEBUFFER_LOW, low);
    dev_send(DISPLAY_DEVICE, DISPLAY_STAGE_FRAMEBUFFER_HIGH, high);
    dev_send(DISPLAY_DEVICE, DISPLAY_COLOR_FORMAT, DISPLAY_RGB565);
    dev_send(DISPLAY_DEVICE, DISPLAY_SWAP_COMMAND, DISPLAY_NEXT_SWAP);
    let mut frames: u16 = 0;
    while 1 == 1 {
        let status = dev_recv(DISPLAY_DEVICE, DISPLAY_SWAP_COMMAND);
        // Underflow is sticky, as are malformed/failed memory reads.
        if status & 0x1c != 0 {
            fail(3);
        }
        if status & 3 == 0
            && dev_recv(DISPLAY_DEVICE, DISPLAY_STAGE_FRAMEBUFFER_LOW) == low
            && dev_recv(DISPLAY_DEVICE, DISPLAY_STAGE_FRAMEBUFFER_HIGH) == high
            && dev_recv(DISPLAY_DEVICE, DISPLAY_COLOR_FORMAT) == DISPLAY_RGB565
        {
            return;
        }
        let previous = dev_recv(DISPLAY_DEVICE, DISPLAY_FRAME_INDEX);
        frame_wait(previous);
        frames += 1;
        if frames == 8 {
            fail(5);
        }
    }
}

#[allow(clippy::eq_op)]
fn main() {
    dev_send(SYSTEM_CONTROL_DEVICE, SYSCTL_LED, 2);
    fill_surface(128, 0);
    fill_surface(134, 1);
    invalidate();
    if physical_read(0x20, 0) != 0xf800
        || physical_read(0x21, 0x8000) != 0x001f
        || physical_read(0x20, 0x0500) != 0x07e0
        || physical_read(0x21, 0x8500) != 0x07e0
        || physical_read(0x21, 0x76ff) != 0xffff
        || physical_read(0x22, 0xf6ff) != 0xffff
    {
        fail(2);
    }
    invalidate();
    let mut target: u16 = 1;
    let mut column: u16 = 0;
    let mut frame: u16 = 0;
    let mut previous_a: u16 = 25;
    let mut previous_b: u16 = 25;
    while 1 == 1 {
        // Row 7 is near the screen center. The moving white tile is changed
        // only in the back surface; the other surface remains the front.
        let first_page: u16 = if target == 0 { 128 } else { 134 };
        let previous: u16 = if target == 0 { previous_a } else { previous_b };
        if previous < 25 {
            let old_word = (175 + previous) << 8;
            fill_tile(
                first_page + (old_word >> 14),
                old_word & 0x3fff,
                color(previous, target),
            );
        }
        let word = (175 + column) << 8;
        fill_tile(first_page + (word >> 14), word & 0x3fff, 0xffff);
        invalidate();
        present(target);
        if target == 0 {
            previous_a = column;
        } else {
            previous_b = column;
        }
        dev_send(SYSTEM_CONTROL_DEVICE, SYSCTL_LED, 4 | (frame & 1));
        report(0);
        let mut pause: u16 = 0;
        while pause < 5 {
            let previous = dev_recv(DISPLAY_DEVICE, DISPLAY_FRAME_INDEX);
            frame_wait(previous);
            pause += 1;
        }
        target ^= 1;
        column += 1;
        if column == 25 {
            column = 0;
        }
        frame += 1;
    }
}
