//! Stage0 of the CpuV3 two-stage flash boot. Compiled for `--target cpu-v3 --code-base 0`
//! into the 0x400-word BSRAM boot window (reset state: CSEG=0, DSEG=0, PC=0).
//!
//! Mirrors the host reference `cpu_v3::boot::loader::run_stage0`: DMA the 64-byte
//! boot descriptor from flash byte 0x100000 into the reserved scratch range
//! (physical word 0x40), validate it, DMA Stage1 into SDRAM, mirror the
//! descriptor to the Stage1 handoff address, invalidate both caches, and enter
//! Stage1 with MTSR DSEG + JSEG. Every failure reports through the boot error
//! ABI: the `{stage, category}` LED word followed by repeating 10-byte `CV3B`
//! UART frames (see `BootErrorReport`).

use crate::dsl_rt::*;
mod device_abi;

// Flash byte address of the boot package is 0x0010_0000 (behind the FPGA
// configuration reserve); descriptor offsets are package-relative, so only
// the high DMA halfword carries the base.
const FLASH_BASE_HI: u16 = 0x0010;

// Boot error ABI codes for Stage0 (cpu_v3/boot/loader.rs `boot_report`).
const CATEGORY_DESCRIPTOR: u16 = 1;
const CATEGORY_DMA: u16 = 3;
const CATEGORY_ENTRY: u16 = 4;

// Descriptor word indices (byte offset / 2, format version 3).
const DW_VERSION: u16 = 4;
const DW_SIZE: u16 = 5;
const DW_TARGET_LO: u16 = 6;
const DW_TARGET_HI: u16 = 7;
const DW_PACKAGE_LO: u16 = 8;
const DW_PACKAGE_HI: u16 = 9;
const DW_S1_FLASH_LO: u16 = 10;
const DW_S1_FLASH_HI: u16 = 11;
const DW_S1_FILE_LO: u16 = 12;
const DW_S1_FILE_HI: u16 = 13;
const DW_S1_MEM_LO: u16 = 14;
const DW_S1_MEM_HI: u16 = 15;
const DW_S1_DEST_LO: u16 = 16;
const DW_S1_DEST_HI: u16 = 17;
const DW_S1_CSEG: u16 = 18;
const DW_S1_ENTRY: u16 = 19;
const DW_S1_DSEG: u16 = 20;
const DW_S1_STACK: u16 = 21;
const DW_MANIFEST_LO: u16 = 22;
const DW_MANIFEST_HI: u16 = 23;
const DW_MANIFEST_SIZE_LO: u16 = 24;
const DW_MANIFEST_SIZE_HI: u16 = 25;
const DW_HANDOFF_LO: u16 = 30;
const DW_HANDOFF_HI: u16 = 31;

/// Waits for the current DMA transfer; returns 0 on completion or the DMA
/// error code. Device 2 channels: 0 command, 1 status, 14 error.
fn dma_wait() -> u16 {
    let mut status = dev_recv(BOOT_DMA_DEVICE, DMA_STATUS);
    while status == DMA_STATUS_BUSY {
        // busy
        status = dev_recv(BOOT_DMA_DEVICE, DMA_STATUS);
    }
    if status == DMA_STATUS_ERROR {
        return dev_recv(BOOT_DMA_DEVICE, DMA_ERROR);
    }
    0
}

/// Programs the DMA flash/destination addresses (device 2, channels 2..=5).
fn dma_set_addrs(flash_hi: u16, flash_lo: u16, dest_hi: u16, dest_lo: u16) {
    dev_send(BOOT_DMA_DEVICE, DMA_FLASH_OFFSET_LOW, flash_lo);
    dev_send(BOOT_DMA_DEVICE, DMA_FLASH_OFFSET_HIGH, flash_hi);
    dev_send(BOOT_DMA_DEVICE, DMA_DESTINATION_LOW, dest_lo);
    dev_send(BOOT_DMA_DEVICE, DMA_DESTINATION_HIGH, dest_hi);
}

/// Programs the DMA file/memory sizes and starts the transfer (device 2,
/// channels 6..=9 and 0). The engine zero-fills `memory_size - file_size`.
fn dma_start(file_hi: u16, file_lo: u16, mem_hi: u16, mem_lo: u16) {
    dev_send(BOOT_DMA_DEVICE, DMA_FILE_SIZE_LOW, file_lo);
    dev_send(BOOT_DMA_DEVICE, DMA_FILE_SIZE_HIGH, file_hi);
    dev_send(BOOT_DMA_DEVICE, DMA_MEMORY_SIZE_LOW, mem_lo);
    dev_send(BOOT_DMA_DEVICE, DMA_MEMORY_SIZE_HIGH, mem_hi);
    dev_send(BOOT_DMA_DEVICE, DMA_COMMAND, DMA_COMMAND_START);
}

/// Transmits one byte through the device 0 UART (channel 3), polling the
/// busy bit first.
fn uart_byte(b: u16) {
    while dev_recv(SYSTEM_CONTROL_DEVICE, SYSCTL_UART_STATUS) & 1 != 0 { }
    dev_send(SYSTEM_CONTROL_DEVICE, SYSCTL_UART_TX_DATA, b);
}

/// Reports a boot failure: LED `{stage, category}` on device 0 channel 2,
/// then the 10-byte `CV3B` frame retransmitted forever.
#[allow(clippy::eq_op)] // `while 1 == 1` is the rcc spelling of an endless loop
fn boot_fail(stage: u16, category: u16, code: u16, detail: u16) {
    dev_send(SYSTEM_CONTROL_DEVICE, SYSCTL_LED, (stage << 4) | category);
    let checksum = 0x43 ^ 0x56 ^ 0x33 ^ 0x42 ^ stage ^ category ^ code ^ (detail & 0xff) ^ (detail >> 8);
    while 1 == 1 {
        uart_byte(0x43); // 'C'
        uart_byte(0x56); // 'V'
        uart_byte(0x33); // '3'
        uart_byte(0x42); // 'B'
        uart_byte(stage);
        uart_byte(category);
        uart_byte(code);
        uart_byte(detail & 0xff);
        uart_byte(detail >> 8);
        uart_byte(checksum);
    }
}

/// `(a_hi, a_lo) > (b_hi, b_lo)` for u32 values held as u16 pairs.
fn u32_above(a_hi: u16, a_lo: u16, b_hi: u16, b_lo: u16) -> u16 {
    if a_hi > b_hi {
        return 1;
    }
    if a_hi == b_hi && a_lo > b_lo {
        return 1;
    }
    0
}

/// Validates the scratch descriptor, mirroring `validate_stage0_descriptor`.
fn validate_descriptor() {
    let desc = Ptr::from_addr(0x40).as_u16_array();
    // magic "CPU3BOOT" (little-endian words)
    if desc[0u16] != 0x5043 || desc[1u16] != 0x3355 || desc[2u16] != 0x4f42 || desc[3u16] != 0x544f {
        boot_fail(1, CATEGORY_DESCRIPTOR, 1, 0);
    }
    if desc[DW_VERSION] != 3 || desc[DW_SIZE] != 64 {
        boot_fail(1, CATEGORY_DESCRIPTOR, 1, 0);
    }
    // target TangNano20K = 0x544e_3230
    if desc[DW_TARGET_LO] != 0x3230 || desc[DW_TARGET_HI] != 0x544e {
        boot_fail(1, CATEGORY_DESCRIPTOR, 2, 0);
    }
    // the package must fit the 7-MiB payload region (0x0070_0000 bytes)
    if u32_above(desc[DW_PACKAGE_HI], desc[DW_PACKAGE_LO], 0x0070, 0) != 0 {
        boot_fail(1, CATEGORY_DESCRIPTOR, 3, 0);
    }
    // the handoff address must be offset 0x0100 of the Stage1 data segment
    if desc[DW_HANDOFF_HI] != desc[DW_S1_DSEG] || desc[DW_HANDOFF_LO] != 0x0100 {
        boot_fail(1, CATEGORY_ENTRY, 3, 0);
    }
    // Stage1 flash extent inside the package
    let end_lo = desc[DW_S1_FLASH_LO] + desc[DW_S1_FILE_LO];
    let carry: u16 = if end_lo < desc[DW_S1_FLASH_LO] { 1 } else { 0 };
    let end_hi = desc[DW_S1_FLASH_HI] + desc[DW_S1_FILE_HI] + carry;
    if u32_above(end_hi, end_lo, desc[DW_PACKAGE_HI], desc[DW_PACKAGE_LO]) != 0 {
        boot_fail(1, CATEGORY_DESCRIPTOR, 4, 0);
    }
    // manifest extent inside the package
    let end_lo = desc[DW_MANIFEST_LO] + desc[DW_MANIFEST_SIZE_LO];
    let carry: u16 = if end_lo < desc[DW_MANIFEST_LO] { 1 } else { 0 };
    let end_hi = desc[DW_MANIFEST_HI] + desc[DW_MANIFEST_SIZE_HI] + carry;
    if u32_above(end_hi, end_lo, desc[DW_PACKAGE_HI], desc[DW_PACKAGE_LO]) != 0 {
        boot_fail(1, CATEGORY_DESCRIPTOR, 4, 0);
    }
    // Stage1 entry inside the Stage1 file extent (byte addresses)
    let entry_b_hi = (desc[DW_S1_CSEG] << 1) | (desc[DW_S1_ENTRY] >> 15);
    let entry_b_lo = desc[DW_S1_ENTRY] << 1;
    let dest_b_hi = (desc[DW_S1_DEST_HI] << 1) | (desc[DW_S1_DEST_LO] >> 15);
    let dest_b_lo = desc[DW_S1_DEST_LO] << 1;
    let end_b_lo = dest_b_lo + desc[DW_S1_FILE_LO];
    let carry: u16 = if end_b_lo < dest_b_lo { 1 } else { 0 };
    let end_b_hi = dest_b_hi + desc[DW_S1_FILE_HI] + carry;
    if u32_above(dest_b_hi, dest_b_lo, entry_b_hi, entry_b_lo) != 0 {
        boot_fail(1, CATEGORY_ENTRY, 2, 0);
    }
    if u32_above(end_b_hi, end_b_lo, entry_b_hi, entry_b_lo) == 0 {
        boot_fail(1, CATEGORY_ENTRY, 2, 0);
    }
    // Stage1 initial stack: inside the fitted
    // 4M-word SDRAM (segments 0x00..0x3f)
    if desc[DW_S1_DSEG] > 0x3f {
        boot_fail(1, CATEGORY_ENTRY, 1, 0);
    }
}

fn main() {
    // DMA the 64-byte descriptor from the package base into scratch word 0x40.
    dma_set_addrs(FLASH_BASE_HI, 0, 0, 0x40);
    dma_start(0, 64, 0, 64);
    let err = dma_wait();
    if err != 0 {
        boot_fail(1, CATEGORY_DMA, 1, err);
    }

    validate_descriptor();

    let desc = Ptr::from_addr(0x40).as_u16_array();
    // DMA Stage1 from flash into its destination.
    dma_set_addrs(
        desc[DW_S1_FLASH_HI] + FLASH_BASE_HI,
        desc[DW_S1_FLASH_LO],
        desc[DW_S1_DEST_HI],
        desc[DW_S1_DEST_LO],
    );
    dma_start(
        desc[DW_S1_FILE_HI],
        desc[DW_S1_FILE_LO],
        desc[DW_S1_MEM_HI],
        desc[DW_S1_MEM_LO],
    );
    let err = dma_wait();
    if err != 0 {
        boot_fail(1, CATEGORY_DMA, 1, err);
    }

    // Mirror the validated descriptor into the Stage1 handoff range. Source
    // and destination live in different data segments, so each word crosses
    // DSEG (only pure register work happens while DSEG is switched).
    let mut handoff = Ptr::from_addr(desc[DW_HANDOFF_LO]).as_u16_array();
    let handoff_segment = desc[DW_HANDOFF_HI];
    let mut i: u16 = 0;
    while i < 32 {
        let w = desc[i];
        mtsr_dseg(handoff_segment);
        handoff[i] = w;
        mtsr_dseg(0);
        i += 1;
    }

    // Invalidate both caches (device 0 channels 0 and 1), then switch DSEG
    // immediately before the inter-segment jump, mirroring
    // `ApplicationHandoff::instructions()`: every handoff field is read into
    // registers before MTSR, because the descriptor scratch lives in data
    // segment 0. Stage1 initializes its own stack pointer from its
    // compiled-in `--stack-init`.
    let dseg = desc[DW_S1_DSEG];
    let cseg = desc[DW_S1_CSEG];
    let entry = desc[DW_S1_ENTRY];
    dcache_invalidate_all();
    mtsr_dseg(dseg);
    icache_invalidate_delayed_and_jump(cseg, entry);
}
