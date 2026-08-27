//! Host-side reference model for framebuffer scanout and line buffering.

use crate::{
    framebuffer_word_at, rgb565_to_rgb888, Machine, PhysicalWordAddress, FRAMEBUFFER_A_BASE_WORD,
    FRAMEBUFFER_WIDTH,
};

pub const HDMI_WIDTH: usize = 1280;
pub const HDMI_HEIGHT: usize = 720;
pub const DISPLAY_SCALE: usize = 3;
pub const DISPLAY_SIDE_BORDER: usize = 160;
pub const DISPLAY_LINE_SLOTS: usize = 3;
pub const DISPLAY_LINE_WORDS: usize = FRAMEBUFFER_WIDTH as usize;
pub const DISPLAY_LINE_BUFFER_WORDS: usize = DISPLAY_LINE_SLOTS * DISPLAY_LINE_WORDS;
pub const DISPLAY_BURST_PIXELS: usize = 16;
pub const DISPLAY_BURSTS_PER_LINE: usize = DISPLAY_LINE_WORDS / DISPLAY_BURST_PIXELS;
pub const MEMORY_CYCLES_PER_SOURCE_LINE: usize = 3_600;

pub fn render_frame(machine: &Machine) -> Vec<u32> {
    render_frame_at(machine, FRAMEBUFFER_A_BASE_WORD)
}

pub fn render_frame_at(machine: &Machine, framebuffer_base: u32) -> Vec<u32> {
    let mut frame = vec![0; HDMI_WIDTH * HDMI_HEIGHT];
    for output_y in 0..HDMI_HEIGHT {
        let source_y = output_y / DISPLAY_SCALE;
        for output_x in DISPLAY_SIDE_BORDER..(HDMI_WIDTH - DISPLAY_SIDE_BORDER) {
            let source_x = (output_x - DISPLAY_SIDE_BORDER) / DISPLAY_SCALE;
            let address = framebuffer_word_at(framebuffer_base, source_x as u32, source_y as u32);
            let pixel = machine.physical_memory(PhysicalWordAddress::new(address));
            let (red, green, blue) = rgb565_to_rgb888(pixel, true);
            frame[output_y * HDMI_WIDTH + output_x] =
                (u32::from(red) << 16) | (u32::from(green) << 8) | u32::from(blue);
        }
    }
    frame
}

pub fn write_ppm(path: &std::path::Path, pixels: &[u32]) -> std::io::Result<()> {
    assert_eq!(pixels.len(), HDMI_WIDTH * HDMI_HEIGHT);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut bytes = format!("P6\n{} {}\n255\n", HDMI_WIDTH, HDMI_HEIGHT).into_bytes();
    bytes.reserve(pixels.len() * 3);
    for pixel in pixels {
        bytes.extend_from_slice(&[(pixel >> 16) as u8, (pixel >> 8) as u8, *pixel as u8]);
    }
    std::fs::write(path, bytes)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BufferSimulation {
    pub underflows: usize,
    pub minimum_ready_lines: usize,
}

/// Conservative line-level model. A display line fetch costs 20 bursts of 16
/// memory clocks each. The caller can inject a complete SDRAM blackout.
pub fn simulate_line_buffers(
    slots: usize,
    source_lines: usize,
    blackout_cycles: usize,
) -> BufferSimulation {
    if slots < 2 {
        return BufferSimulation {
            underflows: source_lines,
            minimum_ready_lines: 0,
        };
    }
    let fetch_cycles = DISPLAY_BURSTS_PER_LINE * 16;
    let mut ready = slots;
    let mut minimum_ready = ready;
    let mut available_cycles = 0usize;
    let mut blackout = blackout_cycles;
    let mut underflows = 0;
    for _ in 0..source_lines {
        if ready == 0 {
            underflows += 1;
        } else {
            ready -= 1;
        }
        available_cycles += MEMORY_CYCLES_PER_SOURCE_LINE;
        let blocked = blackout.min(available_cycles);
        blackout -= blocked;
        available_cycles -= blocked;
        while ready < slots && available_cycles >= fetch_cycles {
            available_cycles -= fetch_cycles;
            ready += 1;
        }
        minimum_ready = minimum_ready.min(ready);
    }
    BufferSimulation {
        underflows,
        minimum_ready_lines: minimum_ready,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_rgb565_lines_fit_one_18k_block_and_four_do_not() {
        assert_eq!(DISPLAY_LINE_BUFFER_WORDS, 960);
        assert_eq!(1024 - DISPLAY_LINE_BUFFER_WORDS, 64);
        assert_eq!(4 * DISPLAY_LINE_WORDS, 1280);
        assert_eq!(DISPLAY_BURSTS_PER_LINE, 20);
    }

    #[test]
    fn triple_buffer_absorbs_two_source_line_blackout() {
        let result = simulate_line_buffers(3, 240 * 100, 7_200);
        assert_eq!(result.underflows, 0);
        assert!(result.minimum_ready_lines >= 1);
        assert!(simulate_line_buffers(1, 240, 0).underflows > 0);
    }
}
