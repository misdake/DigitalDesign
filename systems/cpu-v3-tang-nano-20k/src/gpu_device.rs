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
//! tile-linear framebuffer cache cannot drift between the two.

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
/// Total number of tile-linear tiles; every `FAKE_DRAW` tile index must be
/// below this bound.
pub const GPU_TILE_TOTAL: u32 = FRAMEBUFFER_TILE_COLUMNS * FRAMEBUFFER_TILE_ROWS;
/// `FAKE_DRAW` is exactly three qwords: header, tile-list address, and
/// colors/row-mask/temporary draw flags.
pub const GPU_FAKE_DRAW_QWORDS: u8 = 3;
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

/// Cycle model of the GPU device.
///
/// Every `read` and `write` advances the machine one main clock, so a polling
/// driver always makes progress. The submission FIFO, counters, sticky errors,
/// command shell and tile-cache writes match the fitted hardware.
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
    /// 32-byte aligned word address reserved for the tile lists.
    const LIST_BASE: u32 = 0x400;

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

    fn write_tile_list(memory: &mut [Word], indices: &[u16]) {
        for (offset, index) in indices.iter().enumerate() {
            memory[LIST_BASE as usize + offset] = *index;
        }
    }

    fn word_count(words: &[u64]) -> u16 {
        (words.len() * 4) as u16
    }

    fn set_target(base: u32) -> u64 {
        GPU_OPCODE_SET_TARGET as u64 | (1u64 << 8) | (u64::from(base) << 32)
    }

    fn fake_draw(
        list_addr: u32,
        tile_count: u16,
        load_op: u16,
        clear_color: u16,
        draw_color: u16,
        row_mask: u16,
    ) -> [u64; 3] {
        fake_draw_with_flags(
            list_addr,
            tile_count,
            load_op,
            clear_color,
            draw_color,
            row_mask,
            0,
        )
    }

    fn fake_draw_with_flags(
        list_addr: u32,
        tile_count: u16,
        load_op: u16,
        clear_color: u16,
        draw_color: u16,
        row_mask: u16,
        flags: u16,
    ) -> [u64; 3] {
        let arg0 = u64::from(tile_count) | (u64::from(load_op) << 16);
        let payload1 = u64::from(clear_color)
            | (u64::from(draw_color) << 16)
            | (u64::from(row_mask) << 32)
            | (u64::from(flags) << 48);
        [
            GPU_OPCODE_FAKE_DRAW as u64 | (u64::from(GPU_FAKE_DRAW_QWORDS) << 8) | (arg0 << 32),
            u64::from(list_addr),
            payload1,
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
    fn host_device_clears_every_tile_through_the_cache() {
        let indices: Vec<u16> = (0..GPU_TILE_TOTAL as u16).collect();
        let mut words = vec![set_target(FRAMEBUFFER_A_BASE_WORD)];
        words.extend(fake_draw(
            LIST_BASE,
            GPU_TILE_TOTAL as u16,
            GPU_LOAD_OP_CLEAR,
            0,
            0xabcd,
            0xffff,
        ));
        words.push(END);
        let (mut memory, base) = program(&words);
        write_tile_list(&mut memory, &indices);
        let guard = FRAMEBUFFER_A_BASE_WORD as usize + FRAMEBUFFER_WORDS as usize;
        memory[guard] = 0xbeef;
        let mut device = GpuDevice::default();
        stage_and_submit(&mut device, &mut memory, base, word_count(&words));
        assert_eq!(device.received_count(), 1);
        let cycles = device.run_until_idle(&mut memory, 500_000);
        assert!(cycles < 500_000, "submission did not retire");
        assert_eq!(device.executed_count(), 1);
        assert!(!device.command_error());

        let slot = FRAMEBUFFER_A_BASE_WORD as usize;
        for index in 0..GPU_TILE_TOTAL {
            let tile_base = slot + (index * FRAMEBUFFER_TILE_WORDS) as usize;
            for word in &memory[tile_base..tile_base + FRAMEBUFFER_TILE_WORDS as usize] {
                assert_eq!(*word, 0xabcd, "tile {index} pixel mismatch");
            }
        }
        assert_eq!(memory[guard], 0xbeef, "guard word was overwritten");
    }

    #[test]
    fn host_device_generates_tile_local_xy_gradient() {
        let base_color = 0x8215;
        let mut words = vec![set_target(FRAMEBUFFER_A_BASE_WORD)];
        words.extend(fake_draw_with_flags(
            LIST_BASE,
            1,
            GPU_LOAD_OP_CLEAR,
            0,
            base_color,
            0xffff,
            GPU_DRAW_FLAG_GRADIENT_XY,
        ));
        words.push(END);
        let (mut memory, base) = program(&words);
        write_tile_list(&mut memory, &[7]);
        let mut device = GpuDevice::default();
        stage_and_submit(&mut device, &mut memory, base, word_count(&words));
        assert!(device.run_until_idle(&mut memory, 500_000) < 500_000);
        assert!(!device.command_error());

        let tile_base = (FRAMEBUFFER_A_BASE_WORD + 7 * FRAMEBUFFER_TILE_WORDS) as usize;
        for y in 0..16usize {
            for x in 0..16usize {
                assert_eq!(
                    memory[tile_base + y * 16 + x],
                    gpu_gradient_pixel(base_color, x as u16, y as u16)
                );
            }
        }
    }

    #[test]
    fn host_device_end_requires_a_target() {
        // END before any SET_TARGET is a command error.
        assert!(run_one(&[END]));
        // SET_TARGET followed by END with no draws is legal: nothing to drain.
        assert!(!run_one(&[set_target(FRAMEBUFFER_A_BASE_WORD), END]));
    }

    /// Runs one submission and returns whether it terminated with a command
    /// error, checking the retire/counter contract either way.
    fn run_one(words: &[u64]) -> bool {
        run_one_with_list(words, &[])
    }

    fn run_one_with_list(words: &[u64], list: &[u16]) -> bool {
        let (mut memory, base) = program(words);
        write_tile_list(&mut memory, list);
        let mut device = GpuDevice::default();
        stage_and_submit(&mut device, &mut memory, base, word_count(words));
        assert_eq!(device.received_count(), 1);
        let cycles = device.run_until_idle(&mut memory, 500_000);
        assert!(cycles < 500_000, "submission did not retire");
        assert_eq!(device.executed_count(), 1, "every submission retires once");
        device.command_error()
    }

    #[test]
    fn host_device_rejects_each_invalid_command_class() {
        // Unknown opcode.
        assert!(run_one(&[0x17u64 | (1u64 << 8)]));
        // Wrong qword count for SET_TARGET.
        assert!(run_one(&[GPU_OPCODE_SET_TARGET as u64 | (2u64 << 8)]));
        // Nonzero reserved header flags.
        assert!(run_one(&[GPU_OPCODE_END as u64
            | (1u64 << 8)
            | (1u64 << 16)]));
        // FAKE_DRAW without a target.
        assert!(run_one(&fake_draw(
            LIST_BASE,
            0,
            GPU_LOAD_OP_CLEAR,
            0,
            0,
            0
        )));
        let target = set_target(FRAMEBUFFER_A_BASE_WORD);
        // Wrong qword count for FAKE_DRAW.
        let short = [GPU_OPCODE_FAKE_DRAW as u64 | (2u64 << 8), 0];
        assert!(run_one(&[target, short[0], short[1]]));
        // Reserved load-op value.
        let mut reserved_load_op = fake_draw(LIST_BASE, 0, 2, 0, 0, 0);
        reserved_load_op[0] = GPU_OPCODE_FAKE_DRAW as u64
            | (u64::from(GPU_FAKE_DRAW_QWORDS) << 8)
            | ((2u64 << 16) << 32);
        assert!(run_one(&[
            target,
            reserved_load_op[0],
            reserved_load_op[1],
            reserved_load_op[2]
        ]));
        // Nonzero reserved bits above the load op.
        let mut reserved_bits = fake_draw(LIST_BASE, 0, GPU_LOAD_OP_LOAD, 0, 0, 0);
        reserved_bits[0] |= 1u64 << 50; // arg0 bit 18
        assert!(run_one(&[
            target,
            reserved_bits[0],
            reserved_bits[1],
            reserved_bits[2]
        ]));
        // Nonzero payload-0 high half.
        let mut bad_high = fake_draw(LIST_BASE, 0, GPU_LOAD_OP_LOAD, 0, 0, 0);
        bad_high[1] = 1u64 << 32;
        assert!(run_one(&[target, bad_high[0], bad_high[1], bad_high[2]]));
        // Nonzero payload-1 reserved bits.
        let mut bad_color = fake_draw(LIST_BASE, 0, GPU_LOAD_OP_LOAD, 0, 0, 0);
        bad_color[2] |= 1u64 << 48;
        assert!(run_one(&[target, bad_color[0], bad_color[1], bad_color[2]]));
        // Unaligned tile list.
        assert!(run_one(&[
            target,
            fake_draw(LIST_BASE + 1, 1, GPU_LOAD_OP_CLEAR, 0, 0, 0)[0],
            fake_draw(LIST_BASE + 1, 1, GPU_LOAD_OP_CLEAR, 0, 0, 0)[1],
            fake_draw(LIST_BASE + 1, 1, GPU_LOAD_OP_CLEAR, 0, 0, 0)[2],
        ]));
        // Tile list that leaves 22-bit word memory.
        let draw = fake_draw((1 << 22) - 16, 32, GPU_LOAD_OP_CLEAR, 0, 0, 0);
        assert!(run_one(&[target, draw[0], draw[1], draw[2]]));
        // Tile index at or above the tile count.
        let draw = fake_draw(LIST_BASE, 1, GPU_LOAD_OP_CLEAR, 0, 0, 0);
        assert!(run_one_with_list(
            &[target, draw[0], draw[1], draw[2]],
            &[GPU_TILE_TOTAL as u16],
        ));
    }

    #[test]
    fn host_device_accepts_a_command_crossing_a_32_byte_line() {
        // Three SET_TARGET qwords put the FAKE_DRAW header on qword 3 and its
        // payloads on qwords 4 and 5, i.e. across the 32-byte line boundary.
        let draw = fake_draw(LIST_BASE, 1, GPU_LOAD_OP_CLEAR, 0, 0x2222, 0xffff);
        let words = [
            set_target(FRAMEBUFFER_A_BASE_WORD),
            set_target(FRAMEBUFFER_B_BASE_WORD),
            set_target(FRAMEBUFFER_A_BASE_WORD),
            draw[0],
            draw[1],
            draw[2],
            END,
        ];
        let (mut memory, base) = program(&words);
        write_tile_list(&mut memory, &[0]);
        let mut device = GpuDevice::default();
        stage_and_submit(&mut device, &mut memory, base, word_count(&words));
        device.run_until_idle(&mut memory, 500_000);
        assert!(!device.command_error());
        assert_eq!(memory[FRAMEBUFFER_A_BASE_WORD as usize], 0x2222);
    }

    #[test]
    fn host_device_fifo_full_rejects_without_touching_received_count() {
        let indices: Vec<u16> = (0..GPU_TILE_TOTAL as u16).collect();
        let mut words = vec![set_target(FRAMEBUFFER_A_BASE_WORD)];
        words.extend(fake_draw(
            LIST_BASE,
            GPU_TILE_TOTAL as u16,
            GPU_LOAD_OP_CLEAR,
            0,
            0x1234,
            0xffff,
        ));
        words.push(END);
        let (mut memory, base) = program(&words);
        write_tile_list(&mut memory, &indices);
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
