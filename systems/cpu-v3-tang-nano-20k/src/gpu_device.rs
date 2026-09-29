//! Legacy GPU device ABI retained while the v2 cmodel replaces the implementation.
//!
//! Device 4 now reports submission rejection in both host and hardware shells.
//! The old command constants remain for source compatibility, not as v2 spec.
use crate::layout::{
    FRAMEBUFFER_A_BASE_WORD, FRAMEBUFFER_B_BASE_WORD, FRAMEBUFFER_TILE_COLUMNS,
    FRAMEBUFFER_TILE_ROWS,
};
use crate::{Device, Word};

/// GPU device selected by `dev_recv(GPU_DEVICE, channel)` and
/// `dev_send(GPU_DEVICE, channel, value)`.
pub const GPU_DEVICE: u8 = 4;

// ---- write channels ----

/// Command buffer word-address bits 15:0.
pub const GPU_CMD_BASE_LOW: u8 = 0;
/// Command buffer word-address bits 21:16; only bits 5:0 are valid.
pub const GPU_CMD_BASE_HIGH: u8 = 1;
/// Valid command length in 16-bit words, low 16 bits.
pub const GPU_CMD_WORDS_LOW: u8 = 2;
/// Valid command length high half; every bit is reserved and must be zero.
pub const GPU_CMD_WORDS_HIGH: u8 = 3;
/// Submit doorbell. A successful write atomically captures the staged base and
/// word count and increments `GPU_RECEIVED_COUNT`.
pub const GPU_SUBMIT: u8 = 4;
/// Control register. See [`GPU_CONTROL_RESET`] and [`GPU_CONTROL_CLEAR_ERRORS`].
pub const GPU_CONTROL: u8 = 5;

// ---- read channels ----

/// Accepted submission count, 16-bit wrapping.
pub const GPU_RECEIVED_COUNT: u8 = 0;
/// Retired submission count, 16-bit wrapping.
pub const GPU_EXECUTED_COUNT: u8 = 1;
/// Status word assembled from the `GPU_STATUS_*` bits.
pub const GPU_STATUS: u8 = 2;
/// Command FIFO occupancy, 0..[`GPU_FIFO_DEPTH`].
pub const GPU_QUEUE_LEVEL: u8 = 3;

// ---- status bits (channel GPU_STATUS) ----

/// Bit 0: at least one accepted submission is queued or active.
pub const GPU_STATUS_BUSY: u16 = 1 << 0;
/// Bit 1: the command FIFO is full; a submit now is rejected.
pub const GPU_STATUS_FIFO_FULL: u16 = 1 << 1;
/// Bit 2 (sticky): at least one submit has been rejected since the last clear.
pub const GPU_STATUS_SUBMIT_REJECTED: u16 = 1 << 2;
/// Bit 3 (sticky): at least one accepted submission terminated with an error.
pub const GPU_STATUS_COMMAND_ERROR: u16 = 1 << 3;
/// Every status bit implemented by this milestone.
pub const GPU_STATUS_MASK: u16 =
    GPU_STATUS_BUSY | GPU_STATUS_FIFO_FULL | GPU_STATUS_SUBMIT_REJECTED | GPU_STATUS_COMMAND_ERROR;

// ---- control bits (channel GPU_CONTROL) ----

/// Bit 0: soft reset. Accepted only while the device is idle (no queued or
/// active submission); clears the staging registers, FIFO, command state and
/// sticky error bits but preserves `received_count` and `executed_count`.
pub const GPU_CONTROL_RESET: u16 = 1 << 0;
/// Bit 1: write 1 to clear both sticky error bits. Every other bit is ignored.
pub const GPU_CONTROL_CLEAR_ERRORS: u16 = 1 << 1;

/// Submission FIFO depth (queued submissions, excluding the active one).
pub const GPU_FIFO_DEPTH: usize = 2;

// ---- temporary command shell ----

pub const GPU_OPCODE_SET_TARGET: u8 = 0xe0;
pub const GPU_OPCODE_FAKE_DRAW: u8 = 0xe1;
/// Temporary draw of one inline viewport-space triangle.
pub const GPU_OPCODE_TRIANGLE: u8 = 0xe2;
pub const GPU_OPCODE_END: u8 = 0xff;

/// `SET_TARGET.arg0` must be 32 KiB (2^14 words) aligned.
pub const GPU_TARGET_ALIGN_WORDS: u32 = 1 << 14;
/// Total number of tile-linear tiles; every `FAKE_DRAW` tile index must be
/// below this bound.
pub const GPU_TILE_TOTAL: u32 = FRAMEBUFFER_TILE_COLUMNS * FRAMEBUFFER_TILE_ROWS;
/// `FAKE_DRAW` is exactly three qwords: header, tile-list address, and
/// colors/row-mask/temporary draw flags.
pub const GPU_FAKE_DRAW_QWORDS: u8 = 3;
/// `TRIANGLE`: header with zero arg0, then three qwords whose low 32 bits
/// contain `{y:s12.4, x:s12.4}`. All other payload bits are reserved.
pub const GPU_TRIANGLE_QWORDS: u8 = 4;
/// `FAKE_DRAW.arg0[17:16] == GPU_LOAD_OP_LOAD`: refill each tile from
/// `gpu_fb_r` before applying the draw rows.
pub const GPU_LOAD_OP_LOAD: u16 = 0;
/// `FAKE_DRAW.arg0[17:16] == GPU_LOAD_OP_CLEAR`: initialize each tile to the
/// clear color before applying the draw rows.
pub const GPU_LOAD_OP_CLEAR: u16 = 1;
/// `FAKE_DRAW.payload1[48]`: generate an RGB565 gradient from tile-local
/// `(x, y)` instead of repeating the explicit draw color. Higher flag bits are
/// reserved. The draw color contributes the high channel bits, so three draws
/// can retain distinct red/green/blue identities with very little logic.
pub const GPU_DRAW_FLAG_GRADIENT_XY: u16 = 1;

/// Returns the fitted framebuffer slot base for a `SET_TARGET` word address, or
/// `None` when the address is not one of the two fitted 32 KiB-aligned slots.
pub const fn gpu_target_slot(base: u32) -> Option<u32> {
    if base & (GPU_TARGET_ALIGN_WORDS - 1) != 0 {
        return None;
    }
    if base == FRAMEBUFFER_A_BASE_WORD || base == FRAMEBUFFER_B_BASE_WORD {
        Some(base)
    } else {
        None
    }
}

/// One RGB565 pixel of the legacy dummy tile writer color formula. Retained as
/// a test/reference helper; the cache milestone writes explicit draw colors.
pub const fn gpu_dummy_pixel(
    tile_x: u32,
    tile_y: u32,
    phase: u16,
    r_bias: u16,
    g_bias: u16,
    b_bias: u16,
) -> u16 {
    let r5 = (tile_x + r_bias as u32 + phase as u32) & 0x1f;
    let g6 = ((tile_y << 2) + g_bias as u32 + (phase as u32 >> 2)) & 0x3f;
    let b5 = (tile_x + tile_y + b_bias as u32 + phase as u32) & 0x1f;
    ((r5 << 11) | (g6 << 5) | b5) as u16
}

/// The 64-bit beat value of a solid tile: the pixel repeated four times.
pub const fn gpu_dummy_beat(pixel: u16) -> u64 {
    let pixel = pixel as u64;
    pixel | (pixel << 16) | (pixel << 32) | (pixel << 48)
}

/// Temporary fake-draw RGB565 gradient. `x` and `y` are tile-local 0..15.
/// The channel layout deliberately uses mostly wiring: the draw color selects
/// high channel bits, x drives red, y drives green, and x+y drives blue.
pub const fn gpu_gradient_pixel(base: u16, x: u16, y: u16) -> u16 {
    let phase = base & 0xf;
    let r5 = ((base >> 15) & 1) << 4 | ((x + phase) & 0xf);
    let g6 = ((base >> 9) & 3) << 4 | ((y + phase) & 0xf);
    let b5 = ((base >> 4) & 1) << 4 | ((x + y + phase) & 0xf);
    (r5 << 11) | (g6 << 5) | b5
}

/// Four consecutive pixels for one 64-bit tile-cache beat.
pub const fn gpu_gradient_beat(base: u16, beat: usize) -> u64 {
    let y = ((beat >> 2) & 0xf) as u16;
    let x = ((beat & 3) << 2) as u16;
    let p0 = gpu_gradient_pixel(base, x, y) as u64;
    let p1 = gpu_gradient_pixel(base, x + 1, y) as u64;
    let p2 = gpu_gradient_pixel(base, x + 2, y) as u64;
    let p3 = gpu_gradient_pixel(base, x + 3, y) as u64;
    p0 | (p1 << 16) | (p2 << 32) | (p3 << 48)
}

/// Retired host device. It reports the legacy submit-rejected status and
/// performs no command or memory work.
#[derive(Default)]
pub struct GpuDevice;

impl GpuDevice {
    pub fn received_count(&self) -> u16 {
        0
    }
    pub fn executed_count(&self) -> u16 {
        0
    }
    pub fn busy(&self) -> bool {
        false
    }
    pub fn command_error(&self) -> bool {
        false
    }
    pub fn submit_rejected(&self) -> bool {
        true
    }
    pub fn queue_level(&self) -> u16 {
        0
    }
    pub fn run_until_idle(&mut self, _memory: &mut [Word], _maximum_cycles: usize) -> usize {
        0
    }
}

impl Device for GpuDevice {
    fn read(&mut self, _memory: &mut [Word], channel: u8) -> Word {
        if channel == GPU_STATUS {
            GPU_STATUS_SUBMIT_REJECTED
        } else {
            0
        }
    }

    fn write(&mut self, _memory: &mut [Word], _channel: u8, _value: Word) {}

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
