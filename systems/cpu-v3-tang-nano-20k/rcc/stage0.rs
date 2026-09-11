//! Stage0 of the CpuV3 single-stage flash boot. Compiled for `--target cpu-v3 --code-base 0`
//! into the 0x400-word BSRAM boot window (reset state: CSEG=0, DSEG=0, PC=0).
//!
//! Mirrors the host reference `cpu_v3::boot::loader::run_boot`: DMA the 64-byte
//! boot descriptor from flash byte 0x100000 into the reserved scratch range
//! (physical word 0x40), validate it, DMA the section manifest into the own
//! static buffer, validate it, DMA every section of the reset-selected
//! application (Load copies file bytes and zero-fills the tail; Zero sections
//! carry no file data), invalidate both caches, and enter the application with
//! MTSR DSEG + JSEG. Every failure reports through the boot error ABI: the
//! `{stage, category}` LED word (stage is always 1 for this single first
//! stage) followed by repeating 10-byte `CV3B` UART frames (see
//! `BootErrorReport`).

use crate::dsl_rt::*;
mod device_abi;
mod boot_selection;
use boot_selection::*;

// Flash byte address of the boot package is 0x0010_0000 (behind the FPGA
// configuration reserve); descriptor/manifest/section offsets are
// package-relative, so only the high DMA halfword carries the base.
const FLASH_BASE_HI: u16 = 0x0010;

/// Manifest buffer: 192 words = 384 bytes, holding the 48-byte header plus up
/// to ten 32-byte section records. Zero-initialized statics emit no
/// __data_init code; the DMA fills the buffer before it is read.
static MANIFEST: [u16; 192] = [0; 192];

// Boot error ABI codes for the single first stage
// (cpu_v3/boot/loader.rs `boot_report`).
const CATEGORY_DESCRIPTOR: u16 = 1;
const CATEGORY_MANIFEST: u16 = 2;
const CATEGORY_DMA: u16 = 3;

// Section flag bit that marks an executable load section (see
// `cpu_v3::boot::SECTION_EXECUTE`).
const SECTION_EXECUTE: u16 = 4;

// The largest section count the 192-word manifest buffer can hold:
// (192 - 24 header words) / 16 words per record = 10.
const MANIFEST_MAX_SECTIONS: u16 = 10;

// Descriptor word indices (byte offset / 2, format version 4).
const DW_VERSION: u16 = 4;
const DW_SIZE: u16 = 5;
const DW_TARGET_LO: u16 = 6;
const DW_TARGET_HI: u16 = 7;
const DW_PACKAGE_LO: u16 = 8;
const DW_PACKAGE_HI: u16 = 9;
const DW_MANIFEST_LO: u16 = 22;
const DW_MANIFEST_HI: u16 = 23;
const DW_MANIFEST_SIZE_LO: u16 = 24;
const DW_MANIFEST_SIZE_HI: u16 = 25;

// Manifest header word indices (byte offset / 2).
const MW_VERSION: u16 = 4;
const MW_HEADER_SIZE: u16 = 5;
const MW_RECORD_SIZE: u16 = 6;
const MW_COUNT: u16 = 7;
const MW_PACKAGE_LO: u16 = 8;
const MW_PACKAGE_HI: u16 = 9;
const MW_APP_CSEG: u16 = 10;
const MW_APP_ENTRY: u16 = 11;
const MW_APP_DSEG: u16 = 12;
const MW_TABLE_LO: u16 = 14;
const MW_TABLE_HI: u16 = 15;
// First section record word: 48 header bytes / 2.
const MW_RECORDS: u16 = 24;

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

/// Validates the scratch descriptor, mirroring `validate_boot_descriptor`.
fn validate_descriptor() {
    let desc = Ptr::from_addr(0x40).as_u16_array();
    // magic "CPU3BOOT" (little-endian words)
    if desc[0u16] != 0x5043 || desc[1u16] != 0x3355 || desc[2u16] != 0x4f42 || desc[3u16] != 0x544f {
        boot_fail(1, CATEGORY_DESCRIPTOR, 1, 0);
    }
    if desc[DW_VERSION] != 4 || desc[DW_SIZE] != 64 {
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
    // manifest extent inside the package
    let end_lo = desc[DW_MANIFEST_LO] + desc[DW_MANIFEST_SIZE_LO];
    let carry: u16 = if end_lo < desc[DW_MANIFEST_LO] { 1 } else { 0 };
    let end_hi = desc[DW_MANIFEST_HI] + desc[DW_MANIFEST_SIZE_HI] + carry;
    if u32_above(end_hi, end_lo, desc[DW_PACKAGE_HI], desc[DW_PACKAGE_LO]) != 0 {
        boot_fail(1, CATEGORY_DESCRIPTOR, 4, 0);
    }
}

/// Validates the buffered manifest, mirroring `validate_manifest`.
fn validate_manifest(package_lo: u16, package_hi: u16, size_lo: u16) {
    let m = MANIFEST.as_array();
    // magic "CPU3SECT" (little-endian words), format version, fixed sizes
    if m[0u16] != 0x5043 || m[1u16] != 0x3355 || m[2u16] != 0x4553 || m[3u16] != 0x5443 {
        boot_fail(1, CATEGORY_MANIFEST, 6, 0);
    }
    if m[MW_VERSION] != 4 || m[MW_HEADER_SIZE] != 48 || m[MW_RECORD_SIZE] != 32 {
        boot_fail(1, CATEGORY_MANIFEST, 6, 0);
    }
    if m[MW_TABLE_LO] != 48 || m[MW_TABLE_HI] != 0 {
        boot_fail(1, CATEGORY_MANIFEST, 6, 0);
    }
    let count = m[MW_COUNT];
    // `count << 5` wraps in 16-bit arithmetic, so bound the count explicitly
    // before the size equation and the section loop use it. Without this a
    // manifest with `count = 0x0800` and `size_lo = 48` would pass the size
    // check and read section records past the 192-word buffer.
    if count > MANIFEST_MAX_SECTIONS {
        boot_fail(1, CATEGORY_MANIFEST, 6, count);
    }
    // the manifest size must be exactly 48 + count * 32 bytes
    if size_lo != 48 + (count << 5) {
        boot_fail(1, CATEGORY_MANIFEST, 6, count);
    }
    if m[MW_PACKAGE_LO] != package_lo || m[MW_PACKAGE_HI] != package_hi {
        boot_fail(1, CATEGORY_MANIFEST, 1, 0);
    }
    if m[MW_APP_CSEG] != S1_CODE_SEGMENT
        || m[MW_APP_ENTRY] != S1_ENTRY_OFFSET
        || m[MW_APP_DSEG] != S1_DATA_SEGMENT
    {
        boot_fail(1, CATEGORY_MANIFEST, 3, 0);
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
    let size_lo = desc[DW_MANIFEST_SIZE_LO];
    let size_hi = desc[DW_MANIFEST_SIZE_HI];
    // The manifest buffer starts at data-segment word 0, so a large manifest
    // DMA would overwrite the descriptor scratch words at 0x40. Capture the
    // descriptor package size before starting that transfer.
    let package_lo = desc[DW_PACKAGE_LO];
    let package_hi = desc[DW_PACKAGE_HI];
    if size_hi != 0 || size_lo > 384 {
        boot_fail(1, CATEGORY_MANIFEST, 5, size_lo);
    }

    // DMA the manifest into the own static buffer (Stage0's data segment is 0).
    dma_set_addrs(
        desc[DW_MANIFEST_HI] + FLASH_BASE_HI,
        desc[DW_MANIFEST_LO],
        0,
        addr_of(&MANIFEST).addr(),
    );
    dma_start(size_hi, size_lo, size_hi, size_lo);
    let err = dma_wait();
    if err != 0 {
        boot_fail(1, CATEGORY_DMA, 1, err);
    }

    validate_manifest(package_lo, package_hi, size_lo);

    // Read the reset-time choice before loading application sections. The
    // generated selection module defines both application slots.
    let m = MANIFEST.as_array();
    let count = m[MW_COUNT];
    let selection = dev_recv(BOOT_SELECT_DEVICE, BOOT_SELECT_VALUE) & 3;
    let mut i: u16 = 0;
    while i < count {
        // section record i: 16 words at MW_RECORDS + i * 16
        let r = MW_RECORDS + (i << 4);
        let kind = m[r];
        let flags = m[r + 1];
        let f_lo = m[r + 2];
        let f_hi = m[r + 3];
        let d_lo = m[r + 4];
        let d_hi = m[r + 5];
        let file_lo = m[r + 6];
        let file_hi = m[r + 7];
        let mem_lo = m[r + 8];
        let mem_hi = m[r + 9];

        if kind != 1 && kind != 2 {
            boot_fail(1, CATEGORY_MANIFEST, 3, i);
        }
        if kind == 2 && (file_lo != 0 || file_hi != 0) {
            boot_fail(1, CATEGORY_MANIFEST, 3, i);
        }

        let is_s1_application = if kind == 1
            && (flags & SECTION_EXECUTE) != 0
            && d_hi == S1_CODE_SEGMENT
            && d_lo == S1_ENTRY_OFFSET
        {
            1
        } else {
            0
        };
        let is_s2_application = if kind == 1
            && (flags & SECTION_EXECUTE) != 0
            && d_hi == S2_CODE_SEGMENT
            && d_lo == S2_ENTRY_OFFSET
        {
            1
        } else {
            0
        };
        let mut skip_unselected: u16 = 0;
        if selection == 2 && is_s1_application == 1 {
            skip_unselected = 1;
        }
        if selection != 2 && is_s2_application == 1 {
            skip_unselected = 1;
        }

        if skip_unselected == 0 {
            // Load: copy file bytes; Zero: file size 0 zero-fills the extent.
            let file_lo2 = if kind == 2 { 0 } else { file_lo };
            let file_hi2 = if kind == 2 { 0 } else { file_hi };
            dma_set_addrs(f_hi + FLASH_BASE_HI, f_lo, d_hi, d_lo);
            dma_start(file_hi2, file_lo2, mem_hi, mem_lo);
            let err = dma_wait();
            if err != 0 {
                boot_fail(1, CATEGORY_DMA, 1, err);
            }
        }
        i += 1;
    }

    // Invalidate both caches, then switch DSEG immediately before the
    // inter-segment jump, mirroring `ApplicationHandoff::instructions()`.
    // The application initializes its own stack pointer from its compiled-in
    // `--stack-init`.
    let mut dseg = S1_DATA_SEGMENT;
    let mut cseg = S1_CODE_SEGMENT;
    let mut entry = S1_ENTRY_OFFSET;
    if selection == 2 {
        dseg = S2_DATA_SEGMENT;
        cseg = S2_CODE_SEGMENT;
        entry = S2_ENTRY_OFFSET;
    }
    dcache_invalidate_all();
    mtsr_dseg(dseg);
    icache_invalidate_delayed_and_jump(cseg, entry);
}
