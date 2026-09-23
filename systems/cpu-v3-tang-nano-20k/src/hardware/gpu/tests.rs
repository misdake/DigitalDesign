//! GPU module tests: emulator behavior of the tile framebuffer cache with a
//! direct-memory line responder and an explicit Icarus co-simulation of the
//! handwritten command FSM.

use super::*;
use crate::gpu_device::{
    gpu_gradient_pixel, GPU_CMD_BASE_HIGH, GPU_CMD_BASE_LOW, GPU_CMD_WORDS_HIGH, GPU_CMD_WORDS_LOW,
    GPU_CONTROL, GPU_CONTROL_CLEAR_ERRORS, GPU_CONTROL_RESET, GPU_DRAW_FLAG_GRADIENT_XY,
    GPU_EXECUTED_COUNT, GPU_FAKE_DRAW_QWORDS, GPU_LOAD_OP_CLEAR, GPU_LOAD_OP_LOAD, GPU_OPCODE_END,
    GPU_OPCODE_FAKE_DRAW, GPU_OPCODE_SET_TARGET, GPU_QUEUE_LEVEL, GPU_RECEIVED_COUNT, GPU_STATUS,
    GPU_STATUS_COMMAND_ERROR, GPU_STATUS_SUBMIT_REJECTED, GPU_SUBMIT, GPU_TILE_TOTAL,
};
use crate::layout::{FRAMEBUFFER_A_BASE_WORD, FRAMEBUFFER_B_BASE_WORD, FRAMEBUFFER_WORDS};
use crate::Device;

const CMD_BASE: u32 = 0x100;
/// 32-byte aligned tile-list region; each draw uses a distinct 32-byte slot.
const LIST_BASE: u32 = 0x400;
const LIST_STRIDE: u32 = 16;
const MEMORY_WORDS: usize = (FRAMEBUFFER_B_BASE_WORD + 0x1_8000) as usize;

/// A command buffer and the physical memory it lives in.
struct Harness {
    core: GpuCore,
    memory: HostGpuMemory,
    words: Vec<u16>,
}

impl Harness {
    fn new() -> Self {
        Self {
            core: GpuCore::default(),
            memory: HostGpuMemory::default(),
            words: vec![0u16; MEMORY_WORDS],
        }
    }

    fn set_qwords(&mut self, base: u32, qwords: &[u64]) {
        let mut cursor = base as usize;
        for qword in qwords {
            self.words[cursor] = *qword as u16;
            self.words[cursor + 1] = (*qword >> 16) as u16;
            self.words[cursor + 2] = (*qword >> 32) as u16;
            self.words[cursor + 3] = (*qword >> 48) as u16;
            cursor += 4;
        }
    }

    fn set_words(&mut self, base: u32, values: &[u16]) {
        for (offset, value) in values.iter().enumerate() {
            self.words[base as usize + offset] = *value;
        }
    }

    fn cycle(&mut self, read: Option<u8>, write: Option<(u8, u16)>) -> u16 {
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
        let mem = self.memory.inputs(&self.words);
        let outputs = self.core.combine(dev, mem);
        self.core.advance(false, dev, mem);
        self.memory.advance(&outputs, &mut self.words);
        outputs.read_data
    }

    fn write(&mut self, channel: u8, value: u16) {
        self.cycle(None, Some((channel, value)));
    }

    fn read(&mut self, channel: u8) -> u16 {
        self.cycle(Some(channel), None)
    }

    fn run_until_idle(&mut self, maximum_cycles: usize) -> usize {
        let mut cycles = 0;
        while self.core.busy() && cycles < maximum_cycles {
            self.cycle(Some(GPU_STATUS), None);
            cycles += 1;
        }
        cycles
    }

    fn submit(&mut self, words: u16) {
        self.write(GPU_CMD_BASE_LOW, CMD_BASE as u16);
        self.write(GPU_CMD_BASE_HIGH, (CMD_BASE >> 16) as u16);
        self.write(GPU_CMD_WORDS_LOW, words);
        self.write(GPU_CMD_WORDS_HIGH, 0);
        self.write(GPU_SUBMIT, 0);
    }

    /// Programs `program`, writes one tile list per 32-byte slot, submits and
    /// runs to idle.
    fn run_program(&mut self, program: &[u64], lists: &[&[u16]]) {
        self.set_qwords(CMD_BASE, program);
        for (index, list) in lists.iter().enumerate() {
            self.set_words(LIST_BASE + index as u32 * LIST_STRIDE, list);
        }
        self.submit((program.len() * 4) as u16);
        let cycles = self.run_until_idle(500_000);
        assert!(cycles < 500_000, "submission did not retire");
    }
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

/// Word address of pixel `(row, col)` of `tile` within `target`.
fn tile_word(target: u32, tile: u16, row: usize, col: usize) -> usize {
    (target + u32::from(tile) * 256) as usize + row * 16 + col
}

#[test]
fn emulator_loads_and_clears_tiles_with_partial_rows() {
    let mut harness = Harness::new();
    let target = FRAMEBUFFER_A_BASE_WORD;
    // Preload tile 2's DRAM payload with a distinguishable pattern.
    for offset in 0..256usize {
        harness.words[(target + 2 * 256) as usize + offset] = 0x1000 + offset as u16;
    }
    let guard = target as usize + FRAMEBUFFER_WORDS as usize;
    harness.words[guard] = 0xbeef;

    // Draw 1: LOAD tile 2, overwrite rows 0..3, keep the loaded rows.
    let load = fake_draw(LIST_BASE, 1, GPU_LOAD_OP_LOAD, 0xffff, 0x1234, 0x000f);
    // Draw 2: CLEAR tile 5 to 0x0abc, then overwrite rows 4..7.
    let clear = fake_draw(
        LIST_BASE + LIST_STRIDE,
        1,
        GPU_LOAD_OP_CLEAR,
        0x0abc,
        0x5678,
        0x00f0,
    );
    let mut program = vec![set_target(target)];
    program.extend(load);
    program.extend(clear);
    program.push(END);
    harness.run_program(&program, &[&[2], &[5]]);

    assert_eq!(harness.read(GPU_EXECUTED_COUNT), 1);
    assert_eq!(harness.read(GPU_STATUS) & GPU_STATUS_COMMAND_ERROR, 0);

    for row in 0..16usize {
        for col in 0..16usize {
            let expected = if row < 4 {
                0x1234
            } else {
                0x1000 + (row * 16 + col) as u16
            };
            assert_eq!(
                harness.words[tile_word(target, 2, row, col)],
                expected,
                "loaded tile 2 row {row} col {col}"
            );
        }
    }
    for row in 0..16usize {
        for col in 0..16usize {
            let expected = if (4..8).contains(&row) {
                0x5678
            } else {
                0x0abc
            };
            assert_eq!(
                harness.words[tile_word(target, 5, row, col)],
                expected,
                "cleared tile 5 row {row} col {col}"
            );
        }
    }
    assert_eq!(harness.words[guard], 0xbeef, "guard word was overwritten");
}

#[test]
fn emulator_generates_tile_local_xy_gradient() {
    let mut harness = Harness::new();
    let target = FRAMEBUFFER_A_BASE_WORD;
    let base = 0x8215;
    let draw = fake_draw_with_flags(
        LIST_BASE,
        1,
        GPU_LOAD_OP_CLEAR,
        0,
        base,
        0xffff,
        GPU_DRAW_FLAG_GRADIENT_XY,
    );
    let mut program = vec![set_target(target)];
    program.extend(draw);
    program.push(END);
    harness.run_program(&program, &[&[7]]);

    for y in 0..16 {
        for x in 0..16 {
            assert_eq!(
                harness.words[tile_word(target, 7, y, x)],
                gpu_gradient_pixel(base, x as u16, y as u16),
                "gradient mismatch at ({x}, {y})"
            );
        }
    }
}

#[test]
fn emulator_handles_duplicate_and_unordered_tile_indices() {
    let mut harness = Harness::new();
    let target = FRAMEBUFFER_A_BASE_WORD;
    // Duplicate indices in one list are legal.
    let clear = fake_draw(LIST_BASE, 4, GPU_LOAD_OP_CLEAR, 0, 0x1111, 0xffff);
    // Reordered indices with a partial row mask hit the same two entries.
    let load = fake_draw(
        LIST_BASE + LIST_STRIDE,
        2,
        GPU_LOAD_OP_LOAD,
        0,
        0x2222,
        0x0001,
    );
    let mut program = vec![set_target(target)];
    program.extend(clear);
    program.extend(load);
    program.push(END);
    harness.run_program(&program, &[&[5, 2, 5, 2], &[2, 5]]);

    assert_eq!(harness.read(GPU_STATUS) & GPU_STATUS_COMMAND_ERROR, 0);
    for tile in [2u16, 5] {
        for row in 0..16usize {
            for col in 0..16usize {
                let expected = if row == 0 { 0x2222 } else { 0x1111 };
                assert_eq!(
                    harness.words[tile_word(target, tile, row, col)],
                    expected,
                    "tile {tile} row {row} col {col}"
                );
            }
        }
    }
}

#[test]
fn emulator_cleans_a_dirty_victim_before_reusing_its_entry() {
    let mut harness = Harness::new();
    let target = FRAMEBUFFER_A_BASE_WORD;
    // Tiles 0 and 8 map to the same direct-mapped entry.
    for offset in 0..256usize {
        harness.words[(target + 8 * 256) as usize + offset] = 0x2000 + offset as u16;
    }
    // Draw 0 makes tile 0 entirely 0xaaaa and dirty.
    let a = fake_draw(LIST_BASE, 1, GPU_LOAD_OP_LOAD, 0, 0xaaaa, 0xffff);
    // Draw 1 evicts it (cleaning 0xaaaa back to DRAM), refills tile 8 and
    // writes 0xbbbb.
    let b = fake_draw(
        LIST_BASE + LIST_STRIDE,
        1,
        GPU_LOAD_OP_LOAD,
        0,
        0xbbbb,
        0xffff,
    );
    // Draw 2 evicts tile 8 and reloads tile 0 from the cleaned DRAM.
    let c = fake_draw(
        LIST_BASE + 2 * LIST_STRIDE,
        1,
        GPU_LOAD_OP_LOAD,
        0,
        0xcccc,
        0x0001,
    );
    let mut program = vec![set_target(target)];
    program.extend(a);
    program.extend(b);
    program.extend(c);
    program.push(END);
    harness.run_program(&program, &[&[0], &[8], &[0]]);

    assert_eq!(harness.read(GPU_STATUS) & GPU_STATUS_COMMAND_ERROR, 0);
    // Tile 8 keeps the 0xbbbb that its own draw wrote.
    for row in 0..16usize {
        for col in 0..16usize {
            assert_eq!(
                harness.words[tile_word(target, 8, row, col)],
                0xbbbb,
                "tile 8 row {row} col {col}"
            );
        }
    }
    // Tile 0 reloaded the eviction-cleaned 0xaaaa, then row 0 was overwritten.
    for row in 0..16usize {
        for col in 0..16usize {
            let expected = if row == 0 { 0xcccc } else { 0xaaaa };
            assert_eq!(
                harness.words[tile_word(target, 0, row, col)],
                expected,
                "tile 0 row {row} col {col}"
            );
        }
    }
}

#[test]
fn emulator_retires_only_after_draining_every_dirty_entry() {
    let mut harness = Harness::new();
    let target = FRAMEBUFFER_A_BASE_WORD;
    let draw = fake_draw(LIST_BASE, 3, GPU_LOAD_OP_CLEAR, 0, 0x0f0f, 0xffff);
    let mut program = vec![set_target(target)];
    program.extend(draw);
    program.push(END);
    harness.set_qwords(CMD_BASE, &program);
    harness.set_words(LIST_BASE, &[0, 1, 2]);
    harness.submit((program.len() * 4) as u16);
    // The END drain must keep the device busy until every dirty entry is clean.
    let cycles = harness.run_until_idle(500_000);
    assert!(cycles > 0);
    assert_eq!(harness.read(GPU_EXECUTED_COUNT), 1);
    assert_eq!(harness.read(GPU_QUEUE_LEVEL), 0);
    let (_, fb_r_requests, fb_w_requests) = harness.memory.request_counts();
    assert_eq!(fb_r_requests, 0, "CLEAR must not refill framebuffer data");
    assert_eq!(fb_w_requests, 12, "three dirty tiles need four cleans each");
    for tile in 0..3u16 {
        for row in 0..16usize {
            for col in 0..16usize {
                assert_eq!(harness.words[tile_word(target, tile, row, col)], 0x0f0f);
            }
        }
    }
}

#[test]
fn emulator_retires_once_on_each_memory_port_error() {
    fn run_with_error(kind: u8) {
        let mut harness = Harness::new();
        let load_op = if kind == 2 {
            GPU_LOAD_OP_CLEAR
        } else {
            GPU_LOAD_OP_LOAD
        };
        let draw = fake_draw(LIST_BASE, 1, load_op, 0, 0x5a5a, 0xffff);
        let mut program = vec![set_target(FRAMEBUFFER_A_BASE_WORD)];
        program.extend(draw);
        program.push(END);
        harness.set_qwords(CMD_BASE, &program);
        harness.set_words(LIST_BASE, &[0]);
        match kind {
            0 => harness.memory.inject_next_ro_error(),
            1 => harness.memory.inject_next_fb_r_error(),
            2 => harness.memory.inject_next_fb_w_error(),
            _ => unreachable!(),
        }
        harness.submit((program.len() * 4) as u16);
        let cycles = harness.run_until_idle(500_000);
        assert!(cycles < 500_000, "error did not retire");
        assert_eq!(harness.read(GPU_EXECUTED_COUNT), 1);
        assert_ne!(harness.read(GPU_STATUS) & GPU_STATUS_COMMAND_ERROR, 0);
        // Extra clocks must not publish a second completion.
        for _ in 0..32 {
            harness.cycle(None, None);
        }
        assert_eq!(harness.read(GPU_EXECUTED_COUNT), 1);
    }

    run_with_error(0);
    run_with_error(1);
    run_with_error(2);
}

#[test]
fn emulator_rejects_invalid_draw_fields() {
    let target = set_target(FRAMEBUFFER_A_BASE_WORD);
    let run = |program: &[u64], list: &[u16]| -> bool {
        let mut harness = Harness::new();
        harness.set_qwords(CMD_BASE, program);
        harness.set_words(LIST_BASE, list);
        harness.submit((program.len() * 4) as u16);
        let cycles = harness.run_until_idle(500_000);
        assert!(cycles < 500_000, "submission did not retire");
        assert_eq!(
            harness.read(GPU_EXECUTED_COUNT),
            1,
            "every submission retires once"
        );
        harness.read(GPU_STATUS) & GPU_STATUS_COMMAND_ERROR != 0
    };

    // Wrong qword count (two qwords instead of three).
    let short = [GPU_OPCODE_FAKE_DRAW as u64 | (2u64 << 8), 0];
    assert!(run(&[target, short[0], short[1]], &[]));
    // Reserved load op.
    let mut bad_op = fake_draw(LIST_BASE, 0, GPU_LOAD_OP_LOAD, 0, 0, 0);
    bad_op[0] |= 2u64 << 48; // arg0[17:16] = 2
    assert!(run(&[target, bad_op[0], bad_op[1], bad_op[2]], &[]));
    // Nonzero reserved arg0 bits.
    let mut bad_arg = fake_draw(LIST_BASE, 0, GPU_LOAD_OP_LOAD, 0, 0, 0);
    bad_arg[0] |= 1u64 << 50; // arg0[18]
    assert!(run(&[target, bad_arg[0], bad_arg[1], bad_arg[2]], &[]));
    // Nonzero payload-0 high half.
    let mut bad_p0 = fake_draw(LIST_BASE, 0, GPU_LOAD_OP_LOAD, 0, 0, 0);
    bad_p0[1] = 1u64 << 32;
    assert!(run(&[target, bad_p0[0], bad_p0[1], bad_p0[2]], &[]));
    // Nonzero payload-1 reserved bits.
    let mut bad_p1 = fake_draw(LIST_BASE, 0, GPU_LOAD_OP_LOAD, 0, 0, 0);
    bad_p1[2] |= 1u64 << 48;
    assert!(run(&[target, bad_p1[0], bad_p1[1], bad_p1[2]], &[]));
    // Unaligned tile list.
    let draw = fake_draw(LIST_BASE + 1, 1, GPU_LOAD_OP_CLEAR, 0, 0, 0);
    assert!(run(&[target, draw[0], draw[1], draw[2]], &[0]));
    // Tile list past 22-bit word memory.
    let draw = fake_draw((1 << 22) - 16, 32, GPU_LOAD_OP_CLEAR, 0, 0, 0);
    assert!(run(&[target, draw[0], draw[1], draw[2]], &[]));
    // Tile index at the tile limit.
    let draw = fake_draw(LIST_BASE, 1, GPU_LOAD_OP_CLEAR, 0, 0, 0);
    assert!(run(
        &[target, draw[0], draw[1], draw[2]],
        &[GPU_TILE_TOTAL as u16],
    ));
    // FAKE_DRAW before any SET_TARGET.
    let draw = fake_draw(LIST_BASE, 0, GPU_LOAD_OP_CLEAR, 0, 0, 0);
    assert!(run(&[draw[0], draw[1], draw[2]], &[]));
    // A target switch with a resident tile would make its relative cache tag
    // ambiguous and is rejected. Re-selecting the same target remains legal.
    let draw = fake_draw(LIST_BASE, 1, GPU_LOAD_OP_CLEAR, 0, 0x1234, 0xffff);
    assert!(run(
        &[
            target,
            draw[0],
            draw[1],
            draw[2],
            set_target(FRAMEBUFFER_B_BASE_WORD),
            END,
        ],
        &[0],
    ));
    // A legal empty list performs no read and no error.
    let empty = fake_draw(LIST_BASE, 0, GPU_LOAD_OP_LOAD, 0, 0, 0);
    assert!(!run(&[target, empty[0], empty[1], empty[2], END], &[]));
}

#[test]
fn emulator_rejects_bad_staging_and_full_fifo() {
    let mut harness = Harness::new();
    let program = [
        set_target(FRAMEBUFFER_A_BASE_WORD),
        fake_draw(LIST_BASE, 1, GPU_LOAD_OP_CLEAR, 0, 0x1234, 0xffff)[0],
        fake_draw(LIST_BASE, 1, GPU_LOAD_OP_CLEAR, 0, 0x1234, 0xffff)[1],
        fake_draw(LIST_BASE, 1, GPU_LOAD_OP_CLEAR, 0, 0x1234, 0xffff)[2],
        END,
    ];
    harness.set_qwords(CMD_BASE, &program);
    harness.set_words(LIST_BASE, &[0]);
    let words = (program.len() * 4) as u16;

    // Missing staging fields.
    harness.write(GPU_SUBMIT, 0);
    assert_eq!(harness.read(GPU_RECEIVED_COUNT), 0);
    assert_eq!(
        harness.read(GPU_STATUS) & GPU_STATUS_SUBMIT_REJECTED,
        GPU_STATUS_SUBMIT_REJECTED
    );
    // Misaligned base.
    harness.write(GPU_CMD_BASE_LOW, 0x101);
    harness.write(GPU_CMD_BASE_HIGH, 0);
    harness.write(GPU_CMD_WORDS_LOW, words);
    harness.write(GPU_CMD_WORDS_HIGH, 0);
    harness.write(GPU_SUBMIT, 0);
    assert_eq!(harness.read(GPU_RECEIVED_COUNT), 0);
    // Odd (non-qword) word count.
    harness.write(GPU_CMD_BASE_LOW, CMD_BASE as u16);
    harness.write(GPU_CMD_WORDS_LOW, 2);
    harness.write(GPU_SUBMIT, 0);
    assert_eq!(harness.read(GPU_RECEIVED_COUNT), 0);

    // Fill the FIFO: one active plus two queued, then reject.
    for _ in 0..(GPU_FIFO_DEPTH + 2) {
        harness.submit(words);
    }
    assert_eq!(
        harness.read(GPU_RECEIVED_COUNT),
        (GPU_FIFO_DEPTH + 1) as u16
    );
    assert_eq!(
        harness.read(GPU_STATUS) & GPU_STATUS_SUBMIT_REJECTED,
        GPU_STATUS_SUBMIT_REJECTED
    );
}

#[test]
fn emulator_reset_is_idle_only_and_clears_sticky_errors() {
    let mut harness = Harness::new();
    // END without a target is a command error.
    let program = [END];
    harness.set_qwords(CMD_BASE, &program);
    harness.submit((program.len() * 4) as u16);
    harness.run_until_idle(100_000);
    assert_eq!(
        harness.read(GPU_STATUS) & GPU_STATUS_COMMAND_ERROR,
        GPU_STATUS_COMMAND_ERROR
    );
    assert_eq!(harness.read(GPU_EXECUTED_COUNT), 1);

    // The clear is sticky-error specific and leaves the counters intact.
    harness.write(GPU_CONTROL, GPU_CONTROL_CLEAR_ERRORS);
    assert_eq!(harness.read(GPU_STATUS) & GPU_STATUS_COMMAND_ERROR, 0);
    assert_eq!(harness.read(GPU_EXECUTED_COUNT), 1);
    assert_eq!(harness.read(GPU_RECEIVED_COUNT), 1);

    // Reset retires a finished submission but is ignored while busy.
    let program = [
        set_target(FRAMEBUFFER_A_BASE_WORD),
        fake_draw(LIST_BASE, 2, GPU_LOAD_OP_CLEAR, 0, 0x0f0f, 0xffff)[0],
        fake_draw(LIST_BASE, 2, GPU_LOAD_OP_CLEAR, 0, 0x0f0f, 0xffff)[1],
        fake_draw(LIST_BASE, 2, GPU_LOAD_OP_CLEAR, 0, 0x0f0f, 0xffff)[2],
        END,
    ];
    harness.set_qwords(CMD_BASE, &program);
    harness.set_words(LIST_BASE, &[0, 1]);
    harness.submit((program.len() * 4) as u16);
    // While the submission is active/busy a reset must be ignored.
    harness.write(GPU_CONTROL, GPU_CONTROL_RESET);
    assert!(harness.read(GPU_STATUS) & GPU_STATUS_BUSY != 0);
    harness.run_until_idle(500_000);
    // Create a rejected submit while idle, then check that reset clears it.
    harness.write(GPU_SUBMIT, 0);
    assert_ne!(harness.read(GPU_STATUS) & GPU_STATUS_SUBMIT_REJECTED, 0);
    harness.write(GPU_CONTROL, GPU_CONTROL_RESET);
    assert_eq!(harness.read(GPU_STATUS) & GPU_STATUS_BUSY, 0);
    assert_eq!(harness.read(GPU_STATUS) & GPU_STATUS_SUBMIT_REJECTED, 0);
}

#[test]
fn emulator_handles_a_command_that_crosses_a_32_byte_line() {
    let mut harness = Harness::new();
    // Three SET_TARGET qwords place the FAKE_DRAW header on qword 3 and its
    // two payloads on qwords 4 and 5, so the command straddles the line.
    let draw = fake_draw(LIST_BASE, 1, GPU_LOAD_OP_CLEAR, 0, 0x2222, 0xffff);
    let program = [
        set_target(FRAMEBUFFER_A_BASE_WORD),
        set_target(FRAMEBUFFER_B_BASE_WORD),
        set_target(FRAMEBUFFER_A_BASE_WORD),
        draw[0],
        draw[1],
        draw[2],
        END,
    ];
    harness.set_qwords(CMD_BASE, &program);
    harness.set_words(LIST_BASE, &[0]);
    harness.submit((program.len() * 4) as u16);
    let cycles = harness.run_until_idle(500_000);
    assert!(cycles < 500_000);
    assert_eq!(harness.read(GPU_EXECUTED_COUNT), 1);
    assert_eq!(harness.read(GPU_STATUS) & GPU_STATUS_COMMAND_ERROR, 0);
    // The final target was slot A, and the tile landed there.
    assert_eq!(harness.words[FRAMEBUFFER_A_BASE_WORD as usize], 0x2222);
}

#[test]
#[ignore = "explicit external simulation of the GPU command FSM"]
fn gpu_command_fsm_runs_in_iverilog() {
    digital_design_hardware::verify_verilog_with_iverilog::<CpuV3Gpu>().unwrap();
}

mod host_model_parity {
    use super::*;
    use crate::gpu_device::GpuDevice;

    /// The hardware emulator and the host transaction model must agree on the
    /// device register contract and the framebuffer contents for the same
    /// command buffer.
    #[test]
    fn hardware_emulator_and_host_model_agree() {
        let clear = fake_draw(LIST_BASE, 2, GPU_LOAD_OP_CLEAR, 0x0102, 0x1234, 0x00ff);
        let load = fake_draw(
            LIST_BASE + LIST_STRIDE,
            2,
            GPU_LOAD_OP_LOAD,
            0,
            0x4321,
            0xff00,
        );
        let mut program = vec![set_target(FRAMEBUFFER_A_BASE_WORD)];
        program.extend(clear);
        program.extend(load);
        program.push(END);
        let words = (program.len() * 4) as u16;

        let mut hardware = Harness::new();
        hardware.set_qwords(CMD_BASE, &program);
        hardware.set_words(LIST_BASE, &[0, 1]);
        hardware.set_words(LIST_BASE + LIST_STRIDE, &[1, 0]);
        let mut host_memory = hardware.words.clone();
        hardware.submit(words);
        hardware.run_until_idle(500_000);

        let mut host = GpuDevice::default();
        host.write(&mut host_memory, GPU_CMD_BASE_LOW, CMD_BASE as u16);
        host.write(&mut host_memory, GPU_CMD_BASE_HIGH, (CMD_BASE >> 16) as u16);
        host.write(&mut host_memory, GPU_CMD_WORDS_LOW, words);
        host.write(&mut host_memory, GPU_CMD_WORDS_HIGH, 0);
        host.write(&mut host_memory, GPU_SUBMIT, 0);
        host.run_until_idle(&mut host_memory, 500_000);

        assert_eq!(hardware.read(GPU_RECEIVED_COUNT), host.received_count());
        assert_eq!(hardware.read(GPU_EXECUTED_COUNT), host.executed_count());
        assert_eq!(hardware.read(GPU_STATUS) & GPU_STATUS_COMMAND_ERROR, 0);
        assert!(!host.command_error());
        assert_eq!(hardware.words, host_memory);
    }
}
