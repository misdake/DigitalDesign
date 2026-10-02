//! Host-side reference model for framebuffer scanout and line buffering.

use crate::{
    framebuffer_word_at, rgb565_to_rgb888, CpuV3Sim, PhysicalWordAddress, FRAMEBUFFER_A_BASE_WORD,
    FRAMEBUFFER_HEIGHT, FRAMEBUFFER_WIDTH,
};

/// One fitted HDMI output mode. The scanout RTL, the host renderer, and the
/// board video PLL all derive from a single [`DisplayConfig`]; exactly one
/// mode is compiled in at a time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DisplayConfig {
    /// Short mode name used in diagnostics and preview windows.
    pub name: &'static str,
    /// Active HDMI pixel dimensions.
    pub hdmi_width: usize,
    pub hdmi_height: usize,
    /// Integer framebuffer upscale factor, uniform in both axes.
    pub scale: usize,
    /// One-sided horizontal black-border width in output pixels. The
    /// framebuffer is centered, so `2*side_border + width*scale == hdmi_width`.
    pub side_border: usize,
    /// 54-MHz logic-clock cycles available to fetch one source line.
    pub memory_cycles_per_source_line: usize,
    /// Scanout timing totals and active window. The sync pulse is always the
    /// first blanking segment, so `active_start == sync + back_porch`.
    pub h_total: u32,
    pub v_total: u32,
    pub h_sync_end: u32,
    pub h_active_start: u32,
    pub h_active_end: u32,
    pub v_sync_end: u32,
    pub v_active_start: u32,
    pub v_active_end: u32,
    /// `vertical_repeat` value that publishes the final repeat of a source line.
    pub vertical_repeat_last: u32,
    /// Board video PLL pixel-clock frequency.
    pub pixel_clock_hz: u64,
}

pub const VGA_720P_3X: DisplayConfig = DisplayConfig {
    name: "1280x720p60 3x",
    hdmi_width: 1280,
    hdmi_height: 720,
    scale: 3,
    side_border: 40,
    memory_cycles_per_source_line: memory_cycles_per_source_line(1650, 3, 74_250_000),
    h_total: 1650,
    h_sync_end: 40,
    h_active_start: 260,
    h_active_end: 1540,
    v_total: 750,
    v_sync_end: 5,
    v_active_start: 25,
    v_active_end: 745,
    vertical_repeat_last: 2,
    pixel_clock_hz: 74_250_000,
};

pub const VGA_800X480_2X: DisplayConfig = DisplayConfig {
    name: "800x480@60 2x",
    hdmi_width: 800,
    hdmi_height: 480,
    scale: 2,
    side_border: 0,
    memory_cycles_per_source_line: memory_cycles_per_source_line(1056, 2, 33_300_000),
    h_total: 1056,
    h_sync_end: 48,
    h_active_start: 216,
    h_active_end: 1016,
    v_total: 525,
    v_sync_end: 3,
    v_active_start: 32,
    v_active_end: 512,
    vertical_repeat_last: 1,
    pixel_clock_hz: 33_300_000,
};

pub const DISPLAY_MODES: [DisplayConfig; 2] = [VGA_720P_3X, VGA_800X480_2X];

/// The compiled-in display mode. `display-2x` (also the default) and
/// `display-3x` select the scanout schedule, host renderer and board PLL
/// together; both features cannot be enabled in the same build.
#[cfg(all(feature = "display-2x", feature = "display-3x"))]
compile_error!("display-2x and display-3x are mutually exclusive");

#[cfg(not(feature = "display-3x"))]
pub const ACTIVE_DISPLAY_CONFIG: DisplayConfig = VGA_800X480_2X;
#[cfg(feature = "display-3x")]
pub const ACTIVE_DISPLAY_CONFIG: DisplayConfig = VGA_720P_3X;

pub const HDMI_WIDTH: usize = ACTIVE_DISPLAY_CONFIG.hdmi_width;
pub const HDMI_HEIGHT: usize = ACTIVE_DISPLAY_CONFIG.hdmi_height;
pub const DISPLAY_SCALE: usize = ACTIVE_DISPLAY_CONFIG.scale;
pub const DISPLAY_SIDE_BORDER: usize = ACTIVE_DISPLAY_CONFIG.side_border;
/// IEC sRGB transfer at i/63, rounded to the nearest 8-bit value.
pub const LINEAR6_TO_SRGB8: [u8; 64] = [
    0, 34, 50, 62, 71, 80, 87, 94, 100, 106, 111, 116, 121, 125, 130, 134, 138, 142, 146, 149, 153,
    156, 160, 163, 166, 169, 172, 175, 178, 181, 183, 186, 189, 191, 194, 197, 199, 201, 204, 206,
    209, 211, 213, 215, 218, 220, 222, 224, 226, 228, 230, 232, 234, 236, 238, 240, 242, 244, 246,
    248, 250, 251, 253, 255,
];

/// Linear RGB565 uses bit replication from five to six bits for red/blue.
pub fn linear_rgb565_to_srgb888(pixel: u16) -> (u8, u8, u8) {
    let r = ((pixel >> 11) & 31) as usize;
    let g = ((pixel >> 5) & 63) as usize;
    let b = (pixel & 31) as usize;
    (
        LINEAR6_TO_SRGB8[(r << 1) | (r >> 4)],
        LINEAR6_TO_SRGB8[g],
        LINEAR6_TO_SRGB8[(b << 1) | (b >> 4)],
    )
}
pub const DISPLAY_LINE_SLOTS: usize = 4;
pub const DISPLAY_LINE_WORDS: usize = FRAMEBUFFER_WIDTH as usize;
pub const DISPLAY_LINE_BUFFER_WORDS: usize = DISPLAY_LINE_SLOTS * DISPLAY_LINE_WORDS;
pub const DISPLAY_BURST_PIXELS: usize = 16;
pub const DISPLAY_BURSTS_PER_LINE: usize = DISPLAY_LINE_WORDS / DISPLAY_BURST_PIXELS;
pub const MEMORY_CYCLES_PER_SOURCE_LINE: usize =
    ACTIVE_DISPLAY_CONFIG.memory_cycles_per_source_line;

/// Conservative floor of 54-MHz logic cycles available across the repeated output lines.
const fn memory_cycles_per_source_line(h_total: u32, scale: usize, pixel_clock_hz: u64) -> usize {
    (scale as u64 * h_total as u64 * 54_000_000 / pixel_clock_hz) as usize
}

impl DisplayConfig {
    /// Verilog `localparam` block injected into the scanout RTL and its
    /// testbench so both derive from this single source of truth.
    pub fn verilog_localparams(&self) -> String {
        format!(
            "localparam [10:0] H_TOTAL={0}; localparam [10:0] H_SYNC_END={1};\n\
             localparam [10:0] H_ACTIVE_START={2}; localparam [10:0] H_ACTIVE_END={3};\n\
             localparam [9:0] V_TOTAL={4}; localparam [9:0] V_SYNC_END={5};\n\
             localparam [9:0] V_ACTIVE_START={6}; localparam [9:0] V_ACTIVE_END={7};\n\
             localparam [9:0] FB_WIDTH={8}; localparam [9:0] FB_HEIGHT={9};\n\
             localparam [8:0] SIDE_BORDER={10}; localparam [1:0] SCALE={11}; localparam [1:0] LAST_REPEAT={12};\n\
             localparam [9:0] LINE_SLOT_WORDS=FB_WIDTH/2; localparam [4:0] BURSTS_PER_LINE=FB_WIDTH/16;\n\
             localparam [4:0] LAST_BURST=BURSTS_PER_LINE-1; localparam [7:0] LAST_FILL_Y=FB_HEIGHT-1;\n\
             localparam [12:0] TILE_ROW_STRIDE=FB_WIDTH*16;",
            self.h_total,
            self.h_sync_end,
            self.h_active_start,
            self.h_active_end,
            self.v_total,
            self.v_sync_end,
            self.v_active_start,
            self.v_active_end,
            FRAMEBUFFER_WIDTH,
            FRAMEBUFFER_HEIGHT,
            self.side_border,
            self.scale,
            self.vertical_repeat_last,
        )
    }
}

pub fn render_frame(machine: &CpuV3Sim) -> Vec<u32> {
    render_frame_at(machine, FRAMEBUFFER_A_BASE_WORD)
}

/// Renders the logical 400x240 RGB565 framebuffer as packed 0x00RRGGBB pixels.
pub fn render_framebuffer_at(machine: &CpuV3Sim, framebuffer_base: u32) -> Vec<u32> {
    let mut frame = vec![0; FRAMEBUFFER_WIDTH as usize * FRAMEBUFFER_HEIGHT as usize];
    let linear = machine
        .device::<crate::DisplayDevice>(crate::DISPLAY_DEVICE)
        .is_some_and(crate::DisplayDevice::linear_rgb565);
    for y in 0..FRAMEBUFFER_HEIGHT {
        for x in 0..FRAMEBUFFER_WIDTH {
            let address = framebuffer_word_at(framebuffer_base, x, y);
            let pixel = machine.physical_memory(PhysicalWordAddress::new(address));
            let (red, green, blue) = if linear {
                linear_rgb565_to_srgb888(pixel)
            } else {
                rgb565_to_rgb888(pixel, true)
            };
            frame[(y * FRAMEBUFFER_WIDTH + x) as usize] =
                (u32::from(red) << 16) | (u32::from(green) << 8) | u32::from(blue);
        }
    }
    frame
}

pub fn render_frame_at(machine: &CpuV3Sim, framebuffer_base: u32) -> Vec<u32> {
    render_frame_at_config(&ACTIVE_DISPLAY_CONFIG, machine, framebuffer_base)
}

/// Renders the active framebuffer through a specific [`DisplayConfig`]. The
/// framebuffer is upscaled by `config.scale` and centered on the HDMI output
/// with `config.side_border` black pixels on each side.
pub fn render_frame_at_config(
    config: &DisplayConfig,
    machine: &CpuV3Sim,
    framebuffer_base: u32,
) -> Vec<u32> {
    let framebuffer = render_framebuffer_at(machine, framebuffer_base);
    let mut frame = vec![0; config.hdmi_width * config.hdmi_height];
    for output_y in 0..config.hdmi_height {
        let source_y = output_y / config.scale;
        for output_x in config.side_border..(config.hdmi_width - config.side_border) {
            let source_x = (output_x - config.side_border) / config.scale;
            frame[output_y * config.hdmi_width + output_x] =
                framebuffer[source_y * FRAMEBUFFER_WIDTH as usize + source_x];
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

/// Conservative line-level model. A display line fetch costs 25 bursts of 16
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
    fn four_rgb565_lines_and_tables_fit_two_blocks() {
        assert_eq!(DISPLAY_LINE_WORDS, 400);
        assert_eq!(DISPLAY_LINE_BUFFER_WORDS, 1600);
        assert_eq!(DISPLAY_LINE_SLOTS * (DISPLAY_LINE_WORDS / 4), 400);
        assert_eq!(DISPLAY_BURSTS_PER_LINE, 25);
    }

    #[test]
    fn srgb_table_and_all_rgb565_pixels_match_the_transfer_function() {
        let transfer = |v: usize| {
            let linear = v as f64 / 63.0;
            let encoded = if linear <= 0.0031308 {
                linear * 12.92
            } else {
                1.055 * linear.powf(1.0 / 2.4) - 0.055
            };
            (encoded * 255.0).round() as u8
        };
        for pixel in 0..=u16::MAX {
            let r = usize::from(pixel >> 11);
            let g = usize::from((pixel >> 5) & 63);
            let b = usize::from(pixel & 31);
            assert_eq!(
                linear_rgb565_to_srgb888(pixel),
                (
                    transfer((r << 1) | (r >> 4)),
                    transfer(g),
                    transfer((b << 1) | (b >> 4))
                )
            );
        }
    }

    #[test]
    fn two_slots_absorb_a_source_line_blackout() {
        // The display is arbiter-urgent whenever it requests (it only requests
        // once a slot is free, when one line is buffered), so a single line of
        // lead covers the realistic stall of refresh plus one in-flight
        // transaction. The model's 0-underflow blackout ceiling for two slots
        // is ~6400 cycles; 6000 leaves the tested margin below it.
        let result = simulate_line_buffers(2, 240 * 100, 6_000);
        assert_eq!(result.underflows, 0);
        assert!(result.minimum_ready_lines >= 1);
        assert!(simulate_line_buffers(1, 240, 0).underflows > 0);
    }

    #[test]
    fn every_mode_geometry_is_centered_and_timing_consistent() {
        for config in DISPLAY_MODES {
            let framebuffer_width = FRAMEBUFFER_WIDTH as usize;
            let framebuffer_height = FRAMEBUFFER_HEIGHT as usize;
            assert_eq!(
                2 * config.side_border + framebuffer_width * config.scale,
                config.hdmi_width,
                "{} horizontal centering",
                config.name
            );
            assert_eq!(
                framebuffer_height * config.scale,
                config.hdmi_height,
                "{} vertical fit",
                config.name
            );
            assert_eq!(
                config.h_active_end - config.h_active_start,
                config.hdmi_width as u32,
                "{} active scan width",
                config.name
            );
            assert_eq!(
                config.v_active_end - config.v_active_start,
                config.hdmi_height as u32,
                "{} active scan height",
                config.name
            );
            assert!(
                config.h_sync_end < config.h_active_start
                    && config.h_active_start < config.h_active_end,
                "{} horizontal blanking order",
                config.name
            );
            assert!(
                config.h_active_end < config.h_total && config.v_active_end < config.v_total,
                "{} front-porch present",
                config.name
            );
        }
        assert_eq!(VGA_720P_3X.pixel_clock_hz, 74_250_000);
        assert_eq!(VGA_800X480_2X.pixel_clock_hz, 33_300_000);
        assert_eq!(VGA_800X480_2X.h_total, 1056);
        assert_eq!(VGA_800X480_2X.v_total, 525);
        assert_eq!(VGA_720P_3X.memory_cycles_per_source_line, 3600);
        assert_eq!(VGA_800X480_2X.memory_cycles_per_source_line, 3424);
    }

    #[test]
    fn active_mode_renders_the_framebuffer_centered() {
        let config = ACTIVE_DISPLAY_CONFIG;
        let mut machine = CpuV3Sim::default();
        let base = PhysicalWordAddress::new(FRAMEBUFFER_A_BASE_WORD);
        machine.physical_memory_mut()[base.get() as usize] = 0xf800;
        machine.physical_memory_mut()[base.get() as usize + 1] = 0x07e0;
        let frame = render_frame_at_config(&config, &machine, FRAMEBUFFER_A_BASE_WORD);
        assert_eq!(frame.len(), config.hdmi_width * config.hdmi_height);

        // Black side borders on both horizontal edges of every scaled row.
        for y in 0..config.hdmi_height {
            for x in 0..config.side_border {
                assert_eq!(frame[y * config.hdmi_width + x], 0);
                assert_eq!(frame[y * config.hdmi_width + config.hdmi_width - 1 - x], 0);
            }
        }
        // Red and green framebuffer pixels land at the scaled column offsets.
        let red = frame[config.hdmi_width + config.side_border];
        let green = frame[config.hdmi_width + config.side_border + config.scale];
        assert_eq!(red, 0xff0000);
        assert_eq!(green, 0x00ff00);
    }
}
