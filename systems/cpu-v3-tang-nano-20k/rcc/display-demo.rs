//! GPU memory-interface bring-up demo.
//!
//! The CPU owns two fixed, heap-backed command buffers and alternates them.
//! Three permanent tile-index lists divide the framebuffer into 125-tile
//! batches. Each frame updates an 88-byte temporary command stream, cleans its
//! three cache lines, submits it to device 4, waits for cache-drained
//! retirement, and publishes the completed tile-linear framebuffer at display
//! vblank. The CPU never writes framebuffer pixels in this milestone.

use crate::dsl_rt::*;
use crate::rcc_std::*;
mod device_abi;

use device_abi::*;

const DISPLAY_TEST_ID: u16 = 0x0b;

const FB_A_WORD_LOW: u16 = 0x0000;
const FB_A_WORD_HIGH: u16 = 0x0020;
const FB_B_WORD_LOW: u16 = 0x8000;
const FB_B_WORD_HIGH: u16 = 0x0021;

const COMMAND_WORDS: u16 = 44;
const COMMAND_ALLOCATION_WORDS: u16 = 128 + 15;
const TILES_PER_DRAW: u16 = 125;
// Reserve eight complete cache lines after alignment even though only 125
// entries are live, so cleaning the final line never reaches another object.
const TILE_LIST_ALLOCATION_WORDS: u16 = 128 + 15;

const OPCODE_SET_TARGET: u16 = 0x01e0;
const OPCODE_FAKE_DRAW: u16 = 0x03e1;
const OPCODE_END: u16 = 0x01ff;
const LOAD_OP_LOAD: u16 = 0;
const LOAD_OP_CLEAR: u16 = 1;

fn aligned_command_buffer() -> Ptr {
    let raw = malloc(COMMAND_ALLOCATION_WORDS);
    Ptr::from_addr((raw.addr() + 15) & 0xfff0)
}

fn aligned_tile_list() -> Ptr {
    let raw = malloc(TILE_LIST_ALLOCATION_WORDS);
    Ptr::from_addr((raw.addr() + 15) & 0xfff0)
}

fn write_qword(buffer: Ptr, qword: u16, word0: u16, word1: u16, word2: u16, word3: u16) {
    let base = (qword << 2) as i16;
    unsafe {
        buffer.write(base, word0);
        buffer.write(base + 1, word1);
        buffer.write(base + 2, word2);
        buffer.write(base + 3, word3);
    }
}

fn write_fake_draw(
    buffer: Ptr,
    qword: u16,
    tile_list: Ptr,
    load_op: u16,
    clear_color: u16,
    draw_color: u16,
) {
    write_qword(
        buffer,
        qword,
        OPCODE_FAKE_DRAW,
        0,
        TILES_PER_DRAW,
        load_op,
    );
    // All permanent demo allocations remain in physical page zero.
    write_qword(buffer, qword + 1, tile_list.addr(), 0, 0, 0);
    write_qword(buffer, qword + 2, clear_color, draw_color, 0xffff, 0);
}

fn write_target(buffer: Ptr, target_low: u16, target_high: u16) {
    write_qword(
        buffer,
        0,
        OPCODE_SET_TARGET,
        0,
        target_low,
        target_high,
    );
}

/// Draw three starts at qword 7, so its payload crosses the following 32-byte
/// command-fetch boundary.
fn write_animated_draws(
    buffer: Ptr,
    list_0: Ptr,
    list_1: Ptr,
    list_2: Ptr,
    phase: u16,
) {
    let red = (((phase + 7) & 0x001f) << 11) | 0x001f;
    let green = (((phase + 19) & 0x003f) << 5) | 0xf800;
    let blue = ((phase + 3) & 0x001f) | 0x07e0;
    write_fake_draw(buffer, 1, list_0, LOAD_OP_CLEAR, 0, red);
    write_fake_draw(buffer, 4, list_1, LOAD_OP_LOAD, 0, green);
    write_fake_draw(buffer, 7, list_2, LOAD_OP_CLEAR, 0, blue);
    write_qword(buffer, 10, OPCODE_END, 0, 0, 0);
}

fn clean_commands(buffer: Ptr) {
    unsafe {
        dcache_clean_line(buffer);
        dcache_clean_line(buffer.add(16));
        dcache_clean_line(buffer.add(32));
    }
    dcache_wait();
}

fn initialize_tile_list(list: Ptr, first_tile: u16) {
    let mut index: u16 = 0;
    while index < TILES_PER_DRAW {
        unsafe {
            list.write(index as i16, first_tile + index);
        }
        index += 1;
    }
    let mut offset: u16 = 0;
    while offset < 128 {
        unsafe {
            dcache_clean_line(list.add(offset as i16));
        }
        offset += 16;
    }
    dcache_wait();
}

fn submit(buffer: Ptr) -> u16 {
    let previous = dev_recv(GPU_DEVICE, GPU_EXECUTED_COUNT);
    dev_send(GPU_DEVICE, GPU_CMD_BASE_LOW, buffer.addr());
    // The S2 application data segment is physical page zero; both permanent
    // command buffers therefore have zero physical word-address high bits.
    dev_send(GPU_DEVICE, GPU_CMD_BASE_HIGH, 0);
    dev_send(GPU_DEVICE, GPU_CMD_WORDS_LOW, COMMAND_WORDS);
    dev_send(GPU_DEVICE, GPU_CMD_WORDS_HIGH, 0);
    dev_send(GPU_DEVICE, GPU_SUBMIT, 1);
    if dev_recv(GPU_DEVICE, GPU_STATUS) & GPU_STATUS_SUBMIT_REJECTED != 0 {
        halt(0x0b01);
    }
    previous
}

fn wait_device_change(device: u16, channel: u16, previous: u16) {
    dev_send(
        SYSTEM_CONTROL_DEVICE,
        SYSCTL_WATCH_TARGET,
        sysctl_watch_target(device, channel),
    );
    dev_send(SYSTEM_CONTROL_DEVICE, SYSCTL_WATCH_EXPECTED, previous);
}

fn wait_gpu(previous: u16) {
    wait_device_change(GPU_DEVICE, GPU_EXECUTED_COUNT, previous);
    if dev_recv(GPU_DEVICE, GPU_EXECUTED_COUNT) != previous + 1 {
        halt(0x0b03);
    }
    if dev_recv(GPU_DEVICE, GPU_STATUS) & GPU_STATUS_COMMAND_ERROR != 0 {
        halt(0x0b02);
    }
}

fn request_display_swap(target_low: u16, target_high: u16) -> u16 {
    let frame = dev_recv(DISPLAY_DEVICE, DISPLAY_FRAME_INDEX);
    dev_send(DISPLAY_DEVICE, DISPLAY_STAGE_FRAMEBUFFER_LOW, target_low);
    dev_send(DISPLAY_DEVICE, DISPLAY_STAGE_FRAMEBUFFER_HIGH, target_high);
    dev_send(DISPLAY_DEVICE, DISPLAY_SWAP_COMMAND, DISPLAY_NEXT_SWAP);
    frame
}

fn wait_next_frame(frame: u16) {
    wait_device_change(DISPLAY_DEVICE, DISPLAY_FRAME_INDEX, frame);
}

fn uart_byte(byte: u16) {
    while dev_recv(SYSTEM_CONTROL_DEVICE, SYSCTL_UART_STATUS) & 1 != 0 {}
    dev_send(SYSTEM_CONTROL_DEVICE, SYSCTL_UART_TX_DATA, byte);
}

fn uart_success() {
    uart_byte(0x44);
    uart_byte(0x44);
    uart_byte(0x48);
    uart_byte(0x54);
    uart_byte(1);
    uart_byte(DISPLAY_TEST_ID);
    uart_byte(0);
    uart_byte(0x48 ^ 0x54 ^ 1 ^ DISPLAY_TEST_ID);
}

#[allow(clippy::eq_op)]
fn main() {
    let command_a = aligned_command_buffer();
    let command_b = aligned_command_buffer();
    let list_0 = aligned_tile_list();
    let list_1 = aligned_tile_list();
    let list_2 = aligned_tile_list();
    initialize_tile_list(list_0, 0);
    initialize_tile_list(list_1, 125);
    initialize_tile_list(list_2, 250);
    let mut use_a: u16 = 0;
    let mut phase: u16 = 0;

    // Display powers up on A, so the first completed render targets B.
    while 1 == 1 {
        let command = if use_a == 0 { command_a } else { command_b };
        let target_low = if use_a == 0 {
            FB_B_WORD_LOW
        } else {
            FB_A_WORD_LOW
        };
        let target_high = if use_a == 0 {
            FB_B_WORD_HIGH
        } else {
            FB_A_WORD_HIGH
        };

        write_target(command, target_low, target_high);
        write_animated_draws(command, list_0, list_1, list_2, phase);
        clean_commands(command);
        let previous = submit(command);
        wait_gpu(previous);
        let frame = request_display_swap(target_low, target_high);
        wait_next_frame(frame);
        uart_success();

        use_a ^= 1;
        phase += 1;
    }
}
