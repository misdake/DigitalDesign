//! Standalone framebuffer-cache GPU simulation and image dump.
//!
//! This deliberately bypasses the CPU and display. It submits one handwritten
//! temporary command buffer to the same `GpuDevice`/`GpuCore` model used by
//! SystemSim, then decodes tile-linear RGB565 memory into a binary PPM image.

use cpu_v3::Device;
use cpu_v3_tang_nano_20k::{
    GpuDevice, FRAMEBUFFER_A_BASE_WORD, FRAMEBUFFER_TILE_COLUMNS, GPU_CMD_BASE_HIGH,
    GPU_CMD_BASE_LOW, GPU_CMD_WORDS_HIGH, GPU_CMD_WORDS_LOW, GPU_OPCODE_END, GPU_OPCODE_FAKE_DRAW,
    GPU_OPCODE_SET_TARGET, GPU_STATUS, GPU_STATUS_COMMAND_ERROR, GPU_SUBMIT,
};
use std::{env, fs, io::Write, path::PathBuf};

const WIDTH: usize = 400;
const HEIGHT: usize = 240;
const MEMORY_WORDS: usize = 0x23_0000;
const COMMAND_BASE: u32 = 0x100;
const LIST_BASES: [u32; 3] = [0x400, 0x500, 0x600];
const TILES_PER_DRAW: u16 = 125;
const LOAD_OP_LOAD: u16 = 0;
const LOAD_OP_CLEAR: u16 = 1;

fn set_target(base: u32) -> u64 {
    GPU_OPCODE_SET_TARGET as u64 | (1u64 << 8) | (u64::from(base) << 32)
}

fn fake_draw(list: u32, load_op: u16, clear: u16, color: u16) -> [u64; 3] {
    let arg0 = u64::from(TILES_PER_DRAW) | (u64::from(load_op) << 16);
    [
        GPU_OPCODE_FAKE_DRAW as u64 | (3u64 << 8) | (arg0 << 32),
        u64::from(list),
        u64::from(clear) | (u64::from(color) << 16) | (0xffffu64 << 32),
    ]
}

fn store_qwords(memory: &mut [u16], base: u32, qwords: &[u64]) {
    for (index, qword) in qwords.iter().enumerate() {
        let address = base as usize + index * 4;
        for word in 0..4 {
            memory[address + word] = (qword >> (word * 16)) as u16;
        }
    }
}

fn rgb565_to_rgb888(pixel: u16) -> [u8; 3] {
    let r = ((pixel >> 11) & 0x1f) as u8;
    let g = ((pixel >> 5) & 0x3f) as u8;
    let b = (pixel & 0x1f) as u8;
    [
        (u16::from(r) * 255 / 31) as u8,
        (u16::from(g) * 255 / 63) as u8,
        (u16::from(b) * 255 / 31) as u8,
    ]
}

fn main() {
    let output = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/gpu-framebuffer.ppm"));
    let mut memory = vec![0u16; MEMORY_WORDS];

    for (batch, base) in LIST_BASES.iter().enumerate() {
        for offset in 0..TILES_PER_DRAW as usize {
            memory[*base as usize + offset] = batch as u16 * TILES_PER_DRAW + offset as u16;
        }
    }

    let mut commands = vec![set_target(FRAMEBUFFER_A_BASE_WORD)];
    commands.extend(fake_draw(LIST_BASES[0], LOAD_OP_CLEAR, 0, 0xf81f));
    commands.extend(fake_draw(LIST_BASES[1], LOAD_OP_LOAD, 0, 0xffe0));
    commands.extend(fake_draw(LIST_BASES[2], LOAD_OP_CLEAR, 0, 0x07ff));
    commands.push(GPU_OPCODE_END as u64 | (1u64 << 8));
    store_qwords(&mut memory, COMMAND_BASE, &commands);

    let mut gpu = GpuDevice::default();
    gpu.write(&mut memory, GPU_CMD_BASE_LOW, COMMAND_BASE as u16);
    gpu.write(&mut memory, GPU_CMD_BASE_HIGH, (COMMAND_BASE >> 16) as u16);
    gpu.write(&mut memory, GPU_CMD_WORDS_LOW, (commands.len() * 4) as u16);
    gpu.write(&mut memory, GPU_CMD_WORDS_HIGH, 0);
    gpu.write(&mut memory, GPU_SUBMIT, 1);
    let cycles = gpu.run_until_idle(&mut memory, 500_000);
    assert!(cycles < 500_000, "GPU simulation exceeded its cycle limit");
    assert_eq!(
        gpu.read(&mut memory, GPU_STATUS) & GPU_STATUS_COMMAND_ERROR,
        0,
        "GPU command error"
    );

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).expect("create output directory");
    }
    let mut file = fs::File::create(&output).expect("create PPM output");
    write!(file, "P6\n{WIDTH} {HEIGHT}\n255\n").expect("write PPM header");
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let tile = (y / 16) * FRAMEBUFFER_TILE_COLUMNS as usize + x / 16;
            let local_word = (y % 16) * 16 + x % 16;
            let address = FRAMEBUFFER_A_BASE_WORD as usize + tile * 256 + local_word;
            file.write_all(&rgb565_to_rgb888(memory[address]))
                .expect("write PPM pixel");
        }
    }
    println!("wrote {} after {cycles} GPU cycles", output.display());
}
