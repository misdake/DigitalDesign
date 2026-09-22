//! CPU V3 GPU device ABI and host/SystemEmu model.
//!
//! The GPU is device 4 on the CpuV3 device port. Writes are one-cycle pulses
//! and reads are combinational, so a full submission FIFO is reported through
//! `GPU_STATUS.fifo_full` instead of stalling the bus.
//!
//! This module owns the stable device register contract and a cycle model used
//! by device-API tests and by `CpuV3SystemSim`. The model drives the *same*
//! [`GpuCore`] state machine as the fitted hardware command processor (see
//! `crate::hardware::gpu`), so the register contract, the command shell and the
//! tile-linear framebuffer formula cannot drift between the two.

use crate::hardware::gpu::{GpuCore, GpuDeviceBus, HostGpuMemory};
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
pub const GPU_OPCODE_END: u8 = 0xff;

/// `SET_TARGET.arg0` must be 32 KiB (2^14 words) aligned.
pub const GPU_TARGET_ALIGN_WORDS: u32 = 1 << 14;
/// A submission must accumulate exactly this many tiles before `END`.
pub const GPU_TILE_TOTAL: u32 = FRAMEBUFFER_TILE_COLUMNS * FRAMEBUFFER_TILE_ROWS;
/// `FAKE_DRAW.arg0[31:16]` must be zero in this milestone.
pub const GPU_COLOR_MODE_ZERO: u32 = 0;

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

/// One RGB565 pixel of the dummy tile writer. `tile_x`/`tile_y` are the tile's
/// own indices (0..24 and 0..14); every pixel of a tile is this solid color.
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

/// Cycle model of the GPU device.
///
/// Every `read` and `write` advances the machine one main clock, so a polling
/// driver always makes progress. The submission FIFO, counters, sticky errors,
/// command shell and tile-linear writes match the fitted hardware.
#[derive(Default)]
pub struct GpuDevice {
    core: GpuCore,
    memory: HostGpuMemory,
}

impl GpuDevice {
    pub fn received_count(&self) -> u16 {
        self.core.received_count()
    }

    pub fn executed_count(&self) -> u16 {
        self.core.executed_count()
    }

    pub fn busy(&self) -> bool {
        self.core.busy()
    }

    pub fn command_error(&self) -> bool {
        self.core.command_error()
    }

    pub fn submit_rejected(&self) -> bool {
        self.core.submit_rejected()
    }

    pub fn queue_level(&self) -> u16 {
        self.core.queue_level()
    }

    /// Runs one main clock against `memory` and returns the combinational read
    /// value for `channel` sampled before the edge.
    fn cycle(&mut self, memory: &mut [Word], read: Option<u8>, write: Option<(u8, Word)>) -> Word {
        let (read_enable, channel, write_enable, write_data) = match (read, write) {
            (Some(channel), None) => (true, channel, false, 0),
            (None, Some((channel, data))) => (false, channel, true, data),
            _ => (false, 0, false, 0),
        };
        let dev = GpuDeviceBus {
            index: GPU_DEVICE,
            channel,
            read_enable,
            write_enable,
            write_data,
        };
        let mem = self.memory.inputs(memory);
        let outputs = self.core.combine(dev, mem);
        self.core.advance(false, dev, mem);
        self.memory.advance(&outputs, memory);
        outputs.read_data
    }

    /// Runs clocks until the device is idle or `maximum_cycles` is reached.
    /// Returns the number of clocks spent. Never loops forever.
    pub fn run_until_idle(&mut self, memory: &mut [Word], maximum_cycles: usize) -> usize {
        let mut cycles = 0;
        while self.busy() && cycles < maximum_cycles {
            self.cycle(memory, Some(GPU_STATUS), None);
            cycles += 1;
        }
        cycles
    }
}

impl Device for GpuDevice {
    fn read(&mut self, memory: &mut [Word], channel: u8) -> Word {
        self.cycle(memory, Some(channel), None)
    }

    fn write(&mut self, memory: &mut [Word], channel: u8, value: Word) {
        self.cycle(memory, None, Some((channel, value)));
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{FRAMEBUFFER_TILE_WORDS, FRAMEBUFFER_WORDS};

    /// Disjoint framebuffer slices for slot A and its guard words. The test
    /// memory is large enough for both slots and an explicit guard after A.
    const MEMORY_WORDS: usize = (FRAMEBUFFER_B_BASE_WORD + 0x1_8000) as usize;

    fn program(words: &[u64]) -> (Vec<Word>, u32) {
        let base = 0x100u32;
        let mut memory = vec![0u16; MEMORY_WORDS];
        let mut cursor = base as usize;
        for qword in words {
            memory[cursor] = *qword as u16;
            memory[cursor + 1] = (*qword >> 16) as u16;
            memory[cursor + 2] = (*qword >> 32) as u16;
            memory[cursor + 3] = (*qword >> 48) as u16;
            cursor += 4;
        }
        (memory, base)
    }

    fn word_count(words: &[u64]) -> u16 {
        (words.len() * 4) as u16
    }

    fn set_target(base: u32) -> u64 {
        GPU_OPCODE_SET_TARGET as u64 | (1u64 << 8) | (u64::from(base) << 32)
    }

    fn fake_draw(tiles: u16, color_mode: u16, payload: u64) -> [u64; 2] {
        let arg0 = u64::from(tiles) | (u64::from(color_mode) << 16);
        [
            GPU_OPCODE_FAKE_DRAW as u64 | (2u64 << 8) | (arg0 << 32),
            payload,
        ]
    }

    const END: u64 = GPU_OPCODE_END as u64 | (1u64 << 8);

    fn stage_and_submit(device: &mut GpuDevice, memory: &mut [Word], base: u32, words: u16) {
        device.write(memory, GPU_CMD_BASE_LOW, base as u16);
        device.write(memory, GPU_CMD_BASE_HIGH, (base >> 16) as u16);
        device.write(memory, GPU_CMD_WORDS_LOW, words);
        device.write(memory, GPU_CMD_WORDS_HIGH, 0);
        device.write(memory, GPU_SUBMIT, 0);
    }

    #[test]
    fn host_device_rejects_incomplete_and_misaligned_submissions() {
        let mut memory = vec![0u16; 0x1000];
        let mut device = GpuDevice::default();
        device.write(&mut memory, GPU_SUBMIT, 0);
        assert_eq!(device.received_count(), 0);
        assert!(device.submit_rejected());
        device.write(&mut memory, GPU_CMD_BASE_LOW, 0x101);
        device.write(&mut memory, GPU_CMD_BASE_HIGH, 0x0020);
        device.write(&mut memory, GPU_CMD_WORDS_LOW, 4);
        device.write(&mut memory, GPU_CMD_WORDS_HIGH, 0);
        assert_eq!(
            device.read(&mut memory, GPU_STATUS) & GPU_STATUS_SUBMIT_REJECTED,
            GPU_STATUS_SUBMIT_REJECTED
        );
    }

    #[test]
    fn host_device_fills_the_same_tile_linear_framebuffer() {
        // Three draws with distinct parameters: tile 0..124, 125..249, 250..374.
        let draws = [(3u16, 5u16, 7u16, 9u16), (100, 1, 2, 3), (200, 11, 12, 13)];
        let payload = |(phase, r, g, b): (u16, u16, u16, u16)| {
            u64::from(phase) | (u64::from(r) << 16) | (u64::from(g) << 32) | (u64::from(b) << 48)
        };
        let mut words = vec![set_target(FRAMEBUFFER_A_BASE_WORD)];
        words.extend(fake_draw(125, 0, payload(draws[0])));
        words.extend(fake_draw(125, 0, payload(draws[1])));
        words.extend(fake_draw(125, 0, payload(draws[2])));
        words.push(END);
        let (mut memory, base) = program(&words);
        // Guard word after the 375 tiles of slot A's live payload.
        let guard = FRAMEBUFFER_A_BASE_WORD as usize + FRAMEBUFFER_WORDS as usize;
        memory[guard] = 0xbeef;
        let mut device = GpuDevice::default();
        stage_and_submit(&mut device, &mut memory, base, word_count(&words));
        assert_eq!(device.received_count(), 1);
        let cycles = device.run_until_idle(&mut memory, 200_000);
        assert!(cycles < 200_000, "submission did not retire");
        assert_eq!(device.executed_count(), 1);
        assert!(!device.command_error());

        let slot = FRAMEBUFFER_A_BASE_WORD as usize;
        for index in 0..GPU_TILE_TOTAL {
            let tile_x = index % FRAMEBUFFER_TILE_COLUMNS;
            let tile_y = index / FRAMEBUFFER_TILE_COLUMNS;
            let (phase, r_bias, g_bias, b_bias) = draws[(index / 125) as usize];
            let expected = gpu_dummy_pixel(tile_x, tile_y, phase, r_bias, g_bias, b_bias);
            let base = slot + (index * FRAMEBUFFER_TILE_WORDS) as usize;
            for word in &memory[base..base + FRAMEBUFFER_TILE_WORDS as usize] {
                assert_eq!(*word, expected, "tile {index} pixel mismatch");
            }
        }
        assert_eq!(memory[guard], 0xbeef, "guard word was overwritten");
    }

    #[test]
    fn host_device_end_mismatch_is_a_command_error() {
        let mut words = vec![set_target(FRAMEBUFFER_A_BASE_WORD)];
        words.extend(fake_draw(10, 0, 0));
        words.push(END);
        let (mut memory, base) = program(&words);
        let mut device = GpuDevice::default();
        stage_and_submit(&mut device, &mut memory, base, word_count(&words));
        device.run_until_idle(&mut memory, 100_000);
        assert!(device.command_error());
        assert_eq!(device.executed_count(), 1);
        device.write(&mut memory, GPU_CONTROL, GPU_CONTROL_CLEAR_ERRORS);
        assert_eq!(
            device.read(&mut memory, GPU_STATUS) & GPU_STATUS_COMMAND_ERROR,
            0
        );
    }

    /// Runs one submission and returns whether it terminated with a command
    /// error, checking the retire/counter contract either way.
    fn run_one(words: &[u64]) -> bool {
        let (mut memory, base) = program(words);
        let mut device = GpuDevice::default();
        stage_and_submit(&mut device, &mut memory, base, word_count(words));
        assert_eq!(device.received_count(), 1);
        let cycles = device.run_until_idle(&mut memory, 200_000);
        assert!(cycles < 200_000, "submission did not retire");
        assert_eq!(device.executed_count(), 1, "every submission retires once");
        device.command_error()
    }

    #[test]
    fn host_device_rejects_each_invalid_command_class() {
        let payload = 0u64;
        // Unknown opcode.
        assert!(run_one(&[0x17u64 | (1u64 << 8)]));
        // Wrong qword count for SET_TARGET.
        assert!(run_one(&[GPU_OPCODE_SET_TARGET as u64 | (2u64 << 8)]));
        // Nonzero reserved flags.
        assert!(run_one(&[GPU_OPCODE_END as u64
            | (1u64 << 8)
            | (1u64 << 16)]));
        // Missing target before FAKE_DRAW.
        assert!(run_one(&[fake_draw(1, 0, payload)[0], payload]));
        // Unsupported color mode.
        let arg0 = 1u64 | (1u64 << 16);
        let draw = [
            GPU_OPCODE_FAKE_DRAW as u64 | (2u64 << 8) | (arg0 << 32),
            payload,
        ];
        assert!(run_one(&[
            set_target(FRAMEBUFFER_A_BASE_WORD),
            draw[0],
            draw[1]
        ]));
        // END before any target.
        assert!(run_one(&[END]));
        // Tile total overflow within one submission.
        assert!(run_one(&[
            set_target(FRAMEBUFFER_A_BASE_WORD),
            fake_draw(376, 0, payload)[0],
            payload,
        ]));
        // END with the wrong accumulated total.
        assert!(run_one(&[
            set_target(FRAMEBUFFER_A_BASE_WORD),
            fake_draw(374, 0, payload)[0],
            payload,
            END,
        ]));
    }

    #[test]
    fn host_device_accepts_a_command_crossing_a_32_byte_line() {
        // Three SET_TARGET qwords put the FAKE_DRAW header on qword 3 and its
        // payload on qword 4, i.e. across the 32-byte (4-qword) line boundary.
        let payload = 0x0003u64;
        let words = [
            set_target(FRAMEBUFFER_A_BASE_WORD),
            set_target(FRAMEBUFFER_B_BASE_WORD),
            set_target(FRAMEBUFFER_A_BASE_WORD),
            fake_draw(375, 0, payload)[0],
            payload,
            END,
        ];
        assert!(!run_one(&words));
    }

    #[test]
    fn host_device_fifo_full_rejects_without_touching_received_count() {
        let mut words = vec![set_target(FRAMEBUFFER_A_BASE_WORD)];
        words.extend(fake_draw(375, 0, 0));
        words.push(END);
        let (mut memory, base) = program(&words);
        let mut device = GpuDevice::default();
        // One submission activates immediately; the two FIFO slots then fill,
        // and the following doorbell is rejected without changing the count.
        for _ in 0..(GPU_FIFO_DEPTH + 2) {
            stage_and_submit(&mut device, &mut memory, base, word_count(&words));
        }
        assert_eq!(device.received_count(), (GPU_FIFO_DEPTH + 1) as u16);
        assert!(device.submit_rejected());
        assert!(device.busy());
    }
}
