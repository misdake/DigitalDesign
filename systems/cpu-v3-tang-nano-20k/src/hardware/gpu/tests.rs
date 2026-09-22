//! GPU module tests: emulator behavior with a direct-memory line responder and
//! an explicit Icarus co-simulation of the handwritten command FSM.

use super::*;
use crate::gpu_device::{
    gpu_dummy_pixel, GPU_CMD_BASE_HIGH, GPU_CMD_BASE_LOW, GPU_CMD_WORDS_HIGH, GPU_CMD_WORDS_LOW,
    GPU_CONTROL, GPU_CONTROL_CLEAR_ERRORS, GPU_CONTROL_RESET, GPU_EXECUTED_COUNT, GPU_OPCODE_END,
    GPU_OPCODE_FAKE_DRAW, GPU_OPCODE_SET_TARGET, GPU_QUEUE_LEVEL, GPU_RECEIVED_COUNT, GPU_STATUS,
    GPU_STATUS_COMMAND_ERROR, GPU_STATUS_SUBMIT_REJECTED, GPU_SUBMIT,
};
use crate::layout::{
    FRAMEBUFFER_A_BASE_WORD, FRAMEBUFFER_B_BASE_WORD, FRAMEBUFFER_TILE_COLUMNS,
    FRAMEBUFFER_TILE_WORDS, FRAMEBUFFER_WORDS,
};
use crate::Device;

const CMD_BASE: u32 = 0x100;
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

fn three_draw_program() -> Vec<u64> {
    let payload = |(phase, r, g, b): (u16, u16, u16, u16)| {
        u64::from(phase) | (u64::from(r) << 16) | (u64::from(g) << 32) | (u64::from(b) << 48)
    };
    let mut words = vec![set_target(FRAMEBUFFER_A_BASE_WORD)];
    words.extend(fake_draw(125, 0, payload((3, 5, 7, 9))));
    words.extend(fake_draw(125, 0, payload((100, 1, 2, 3))));
    words.extend(fake_draw(125, 0, payload((200, 11, 12, 13))));
    words.push(END);
    words
}

#[test]
fn emulator_writes_three_draws_and_retires_after_the_final_response() {
    let mut harness = Harness::new();
    let program = three_draw_program();
    harness.set_qwords(CMD_BASE, &program);
    // Guard word between the live payload and the slot padding.
    let guard = FRAMEBUFFER_A_BASE_WORD as usize + FRAMEBUFFER_WORDS as usize;
    harness.words[guard] = 0xbeef;
    harness.submit((program.len() * 4) as u16);
    assert_eq!(harness.read(GPU_RECEIVED_COUNT), 1);

    let cycles = harness.run_until_idle(200_000);
    assert!(cycles < 200_000, "submission did not retire");
    assert_eq!(harness.read(GPU_EXECUTED_COUNT), 1);
    assert_eq!(harness.read(GPU_STATUS) & GPU_STATUS_COMMAND_ERROR, 0);
    assert_eq!(harness.read(GPU_QUEUE_LEVEL), 0);

    let draws = [(3u16, 5u16, 7u16, 9u16), (100, 1, 2, 3), (200, 11, 12, 13)];
    let slot = FRAMEBUFFER_A_BASE_WORD as usize;
    for index in 0..GPU_TILE_TOTAL {
        let tile_x = index % FRAMEBUFFER_TILE_COLUMNS;
        let tile_y = index / FRAMEBUFFER_TILE_COLUMNS;
        let (phase, r_bias, g_bias, b_bias) = draws[(index / 125) as usize];
        let expected = gpu_dummy_pixel(tile_x, tile_y, phase, r_bias, g_bias, b_bias);
        let base = slot + (index * FRAMEBUFFER_TILE_WORDS) as usize;
        for word in &harness.words[base..base + FRAMEBUFFER_TILE_WORDS as usize] {
            assert_eq!(*word, expected, "tile {index} pixel mismatch");
        }
    }
    assert_eq!(harness.words[guard], 0xbeef, "guard word was overwritten");
}

#[test]
fn emulator_rejects_bad_staging_and_full_fifo() {
    let mut harness = Harness::new();
    let program = three_draw_program();
    harness.set_qwords(CMD_BASE, &program);
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
    // A submission that ends with the wrong tile total is a command error.
    let mut program = vec![set_target(FRAMEBUFFER_A_BASE_WORD)];
    program.extend(fake_draw(10, 0, 0));
    program.push(END);
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
    let program = three_draw_program();
    harness.set_qwords(CMD_BASE, &program);
    harness.submit((program.len() * 4) as u16);
    // While the submission is active/busy a reset must be ignored.
    harness.write(GPU_CONTROL, GPU_CONTROL_RESET);
    assert!(harness.read(GPU_STATUS) & GPU_STATUS_BUSY != 0);
    harness.run_until_idle(200_000);
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
    // Three SET_TARGET qwords place the FAKE_DRAW header on qword 3 and the
    // payload on qword 4, so the command straddles the 32-byte line boundary.
    let payload = 3u64;
    let mut program = vec![
        set_target(FRAMEBUFFER_A_BASE_WORD),
        set_target(FRAMEBUFFER_B_BASE_WORD),
        set_target(FRAMEBUFFER_A_BASE_WORD),
    ];
    program.extend(fake_draw(375, 0, payload));
    program.push(END);
    harness.set_qwords(CMD_BASE, &program);
    harness.submit((program.len() * 4) as u16);
    let cycles = harness.run_until_idle(200_000);
    assert!(cycles < 200_000);
    assert_eq!(harness.read(GPU_EXECUTED_COUNT), 1);
    assert_eq!(harness.read(GPU_STATUS) & GPU_STATUS_COMMAND_ERROR, 0);
    // The final target was slot A, and the tiles landed there.
    assert_eq!(
        harness.words[FRAMEBUFFER_A_BASE_WORD as usize],
        gpu_dummy_pixel(0, 0, 3, 0, 0, 0)
    );
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
        let program = three_draw_program();
        let words = (program.len() * 4) as u16;

        let mut hardware = Harness::new();
        hardware.set_qwords(CMD_BASE, &program);
        hardware.submit(words);
        hardware.run_until_idle(200_000);

        let mut host = GpuDevice::default();
        let mut host_memory = vec![0u16; MEMORY_WORDS];
        let mut cursor = CMD_BASE as usize;
        for qword in &program {
            host_memory[cursor] = *qword as u16;
            host_memory[cursor + 1] = (*qword >> 16) as u16;
            host_memory[cursor + 2] = (*qword >> 32) as u16;
            host_memory[cursor + 3] = (*qword >> 48) as u16;
            cursor += 4;
        }
        host.write(&mut host_memory, GPU_CMD_BASE_LOW, CMD_BASE as u16);
        host.write(&mut host_memory, GPU_CMD_BASE_HIGH, (CMD_BASE >> 16) as u16);
        host.write(&mut host_memory, GPU_CMD_WORDS_LOW, words);
        host.write(&mut host_memory, GPU_CMD_WORDS_HIGH, 0);
        host.write(&mut host_memory, GPU_SUBMIT, 0);
        host.run_until_idle(&mut host_memory, 200_000);

        assert_eq!(hardware.read(GPU_RECEIVED_COUNT), host.received_count());
        assert_eq!(hardware.read(GPU_EXECUTED_COUNT), host.executed_count());
        assert_eq!(hardware.read(GPU_STATUS) & GPU_STATUS_COMMAND_ERROR, 0);
        assert!(!host.command_error());
        assert_eq!(hardware.words, host_memory);
    }
}
