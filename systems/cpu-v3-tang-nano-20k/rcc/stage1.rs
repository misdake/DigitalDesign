//! Stage1 of the CpuV3 two-stage flash boot. Loaded into SDRAM by Stage0 and
//! entered with CSEG/DSEG/SP from the boot descriptor; the mirrored 64-byte
//! descriptor sits at offset 0x0100 of its data segment (the Stage0-to-Stage1
//! handoff).
//!
//! Mirrors the host reference `cpu_v3::boot::loader::run_stage1`: DMA the
//! manifest into the own static buffer, validate it, DMA every section except
//! the Stage1 self-section (Load copies file bytes and zero-fills the tail;
//! Zero sections carry no file data), invalidate both caches, and enter the
//! application with MTSR DSEG + JSEG. Failures report through the boot error
//! ABI with stage code 2 (see `BootErrorReport`).

use crate::dsl_rt::*;
mod device_abi;

/// Manifest buffer: 192 words = 384 bytes, holding the 48-byte header plus
/// up to ten 32-byte section records. Zero-initialized statics emit no
/// __data_init code; the DMA fills the buffer before it is read.
static MANIFEST: [u16; 192] = [0; 192];

// Boot error ABI categories for Stage1 (cpu_v3/boot/loader.rs `boot_report`).
// Codes: 1 package size mismatch, 3 invalid section, 5 manifest larger than
// the static buffer, 6 manifest header malformed (the last two are specific
// to this on-hardware Stage1; the host reference decodes from a flash slice
// and needs neither).
const CATEGORY_MANIFEST: u16 = 2;
const CATEGORY_DMA: u16 = 3;

// Flash byte address of the boot package is 0x0010_0000; manifest/section
// offsets are package-relative, so only the high DMA halfword carries it.
const FLASH_BASE_HI: u16 = 0x0010;

// Mirrored descriptor field words (byte offset / 2, format version 3).
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
const DW_S1_DSEG: u16 = 20;
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

fn main() {
    let desc = Ptr::from_addr(0x0100).as_u16_array();
    let size_lo = desc[DW_MANIFEST_SIZE_LO];
    let size_hi = desc[DW_MANIFEST_SIZE_HI];
    if size_hi != 0 || size_lo > 384 {
        boot_fail(2, CATEGORY_MANIFEST, 5, size_lo);
    }

    // DMA the manifest into the own static buffer.
    dev_send(BOOT_DMA_DEVICE, DMA_FLASH_OFFSET_LOW, desc[DW_MANIFEST_LO]);
    dev_send(BOOT_DMA_DEVICE, DMA_FLASH_OFFSET_HIGH, desc[DW_MANIFEST_HI] + FLASH_BASE_HI);
    dev_send(BOOT_DMA_DEVICE, DMA_DESTINATION_LOW, addr_of(&MANIFEST).addr());
    dev_send(BOOT_DMA_DEVICE, DMA_DESTINATION_HIGH, desc[DW_S1_DSEG]);
    dev_send(BOOT_DMA_DEVICE, DMA_FILE_SIZE_LOW, size_lo);
    dev_send(BOOT_DMA_DEVICE, DMA_FILE_SIZE_HIGH, size_hi);
    dev_send(BOOT_DMA_DEVICE, DMA_MEMORY_SIZE_LOW, size_lo);
    dev_send(BOOT_DMA_DEVICE, DMA_MEMORY_SIZE_HIGH, size_hi);
    dev_send(BOOT_DMA_DEVICE, DMA_COMMAND, DMA_COMMAND_START);
    let err = dma_wait();
    if err != 0 {
        boot_fail(2, CATEGORY_DMA, 1, err);
    }

    let m = MANIFEST.as_array();
    // magic "CPU3SECT" (little-endian words), format version, fixed sizes
    if m[0u16] != 0x5043 || m[1u16] != 0x3355 || m[2u16] != 0x4553 || m[3u16] != 0x5443 {
        boot_fail(2, CATEGORY_MANIFEST, 6, 0);
    }
    if m[MW_VERSION] != 3 || m[MW_HEADER_SIZE] != 48 || m[MW_RECORD_SIZE] != 32 {
        boot_fail(2, CATEGORY_MANIFEST, 6, 0);
    }
    if m[MW_TABLE_LO] != 48 || m[MW_TABLE_HI] != 0 {
        boot_fail(2, CATEGORY_MANIFEST, 6, 0);
    }
    let count = m[MW_COUNT];
    // the manifest size must be exactly 48 + count * 32 bytes
    if size_lo != 48 + (count << 5) {
        boot_fail(2, CATEGORY_MANIFEST, 6, count);
    }
    if m[MW_PACKAGE_LO] != desc[DW_PACKAGE_LO] || m[MW_PACKAGE_HI] != desc[DW_PACKAGE_HI] {
        boot_fail(2, CATEGORY_MANIFEST, 1, 0);
    }

    // Read the reset-time choice before loading application sections. Button
    // 10 selects application segment 5; button 01 and default 00 select the
    // primary application segment 3.
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
            boot_fail(2, CATEGORY_MANIFEST, 3, i);
        }
        if kind == 2 && (file_lo != 0 || file_hi != 0) {
            boot_fail(2, CATEGORY_MANIFEST, 3, i);
        }

        // skip the Stage1 self-section (same rule as `matches_stage1`)
        let is_self = if kind == 1
            && (flags & 4) != 0
            && f_lo == desc[DW_S1_FLASH_LO]
            && f_hi == desc[DW_S1_FLASH_HI]
            && d_lo == desc[DW_S1_DEST_LO]
            && d_hi == desc[DW_S1_DEST_HI]
            && file_lo == desc[DW_S1_FILE_LO]
            && file_hi == desc[DW_S1_FILE_HI]
            && mem_lo == desc[DW_S1_MEM_LO]
            && mem_hi == desc[DW_S1_MEM_HI]
        {
            1
        } else {
            0
        };

        let is_primary_application = if kind == 1 && (flags & 4) != 0 && d_hi == 3 {
            1
        } else {
            0
        };
        let is_alternate_application = if kind == 1 && (flags & 4) != 0 && d_hi == 5 {
            1
        } else {
            0
        };
        let mut skip_unselected: u16 = 0;
        if selection == 2 && is_primary_application == 1 {
            skip_unselected = 1;
        }
        if selection != 2 && is_alternate_application == 1 {
            skip_unselected = 1;
        }

        if is_self == 0 && skip_unselected == 0 {
            // Load: copy file bytes; Zero: file size 0 zero-fills the extent.
            let file_lo2 = if kind == 2 { 0 } else { file_lo };
            let file_hi2 = if kind == 2 { 0 } else { file_hi };
            dev_send(BOOT_DMA_DEVICE, DMA_FLASH_OFFSET_LOW, f_lo);
            dev_send(BOOT_DMA_DEVICE, DMA_FLASH_OFFSET_HIGH, f_hi + FLASH_BASE_HI);
            dev_send(BOOT_DMA_DEVICE, DMA_DESTINATION_LOW, d_lo);
            dev_send(BOOT_DMA_DEVICE, DMA_DESTINATION_HIGH, d_hi);
            dev_send(BOOT_DMA_DEVICE, DMA_FILE_SIZE_LOW, file_lo2);
            dev_send(BOOT_DMA_DEVICE, DMA_FILE_SIZE_HIGH, file_hi2);
            dev_send(BOOT_DMA_DEVICE, DMA_MEMORY_SIZE_LOW, mem_lo);
            dev_send(BOOT_DMA_DEVICE, DMA_MEMORY_SIZE_HIGH, mem_hi);
            dev_send(BOOT_DMA_DEVICE, DMA_COMMAND, DMA_COMMAND_START);
            let err = dma_wait();
            if err != 0 {
                boot_fail(2, CATEGORY_DMA, 1, err);
            }
        }
        i += 1;
    }

    // Invalidate both caches, then switch DSEG immediately before the
    // inter-segment jump, mirroring `ApplicationHandoff::instructions()`:
    // every handoff field is read into registers before MTSR, because the
    // manifest buffer lives in the Stage1 data segment. The application
    // initializes its own stack pointer from its compiled-in `--stack-init`.
    let mut dseg = m[MW_APP_DSEG];
    let mut cseg = m[MW_APP_CSEG];
    let mut entry = m[MW_APP_ENTRY];
    // Button 10 selects the second application fitted at 0005:0200 with its
    // independent stack/data segment 0006. Button 01 and the power-on default
    // 00 use the manifest's primary application entry.
    if selection == 2 {
        dseg = 6;
        cseg = 5;
        entry = 0x0200;
    }
    dev_send(SYSTEM_CONTROL_DEVICE, SYSCTL_INVALIDATE_ICACHE, 0);
    dev_send(SYSTEM_CONTROL_DEVICE, SYSCTL_INVALIDATE_DCACHE, 0);
    mtsr_dseg(dseg);
    jseg(cseg, entry);
}
