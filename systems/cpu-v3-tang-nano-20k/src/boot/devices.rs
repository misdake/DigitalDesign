//! Host models of the boot devices for the `cpu_v3::sim::Machine` device bus:
//! the device-2 boot DMA engine and the device-0 system-control block.

use super::device_abi::{
    BOOT_SELECT_VALUE, DMA_COMMAND, DMA_COMMAND_START, DMA_COMPLETED_WORDS_LOW,
    DMA_DESTINATION_HIGH, DMA_DESTINATION_LOW, DMA_ERROR, DMA_ERROR_FILE_LARGER_THAN_MEMORY,
    DMA_ERROR_FLASH_RANGE, DMA_ERROR_MEMORY_RANGE, DMA_FILE_SIZE_HIGH, DMA_FILE_SIZE_LOW,
    DMA_FLASH_OFFSET_HIGH, DMA_FLASH_OFFSET_LOW, DMA_MEMORY_SIZE_HIGH, DMA_MEMORY_SIZE_LOW,
    DMA_STATUS, DMA_STATUS_BUSY, DMA_STATUS_DONE, DMA_STATUS_ERROR, DMA_STATUS_IDLE,
    D_INVALIDATE_ALL, ICACHE_INVALIDATE_ALL_DELAYED, SYSCTL_LED, SYSCTL_UART,
};
use super::loader::{DmaCommand, DmaError, DmaStatus, FlashToDramDma};
use crate::{Device, PhysicalWordAddress, Word};

fn dma_error_code(error: &DmaError) -> Word {
    match error {
        DmaError::Busy => 0,
        DmaError::FileLargerThanMemory { .. } => DMA_ERROR_FILE_LARGER_THAN_MEMORY,
        DmaError::FlashRangeExceeded { .. } => DMA_ERROR_FLASH_RANGE,
        DmaError::PhysicalMemoryExceeded { .. } => DMA_ERROR_MEMORY_RANGE,
    }
}

/// Device-2 model of the boot Flash-to-SDRAM DMA engine.
///
/// Wraps the transaction-level [`FlashToDramDma`]; every `DMA_STATUS` read
/// advances an active transfer by one word, so polling software makes
/// deterministic progress. Validation failures are latched on the error
/// channel at `DMA_COMMAND_START`, matching the hardware register contract.
pub struct BootDmaDevice {
    engine: FlashToDramDma,
    flash: Vec<u8>,
    physical_memory_words: usize,
    flash_offset: u32,
    destination: u32,
    file_size: u32,
    memory_size: u32,
    start_error: Option<Word>,
}

impl BootDmaDevice {
    pub fn new(flash: Vec<u8>, physical_memory_words: usize) -> Self {
        Self {
            engine: FlashToDramDma::default(),
            flash,
            physical_memory_words,
            flash_offset: 0,
            destination: 0,
            file_size: 0,
            memory_size: 0,
            start_error: None,
        }
    }
}

impl Device for BootDmaDevice {
    fn read(&mut self, memory: &mut [Word], channel: u8) -> Word {
        match channel {
            DMA_STATUS => {
                if self.start_error.is_some() {
                    return DMA_STATUS_ERROR;
                }
                match self.engine.tick(&self.flash, memory, true) {
                    DmaStatus::Idle => DMA_STATUS_IDLE,
                    DmaStatus::Busy => DMA_STATUS_BUSY,
                    DmaStatus::Done => DMA_STATUS_DONE,
                    DmaStatus::Error(_) => DMA_STATUS_ERROR,
                }
            }
            DMA_ERROR => match self.start_error {
                Some(code) => code,
                None => match self.engine.status() {
                    DmaStatus::Error(error) => dma_error_code(&error),
                    _ => 0,
                },
            },
            // Completion progress is not tracked by the transaction model.
            DMA_COMPLETED_WORDS_LOW => 0,
            _ => 0,
        }
    }

    fn write(&mut self, _memory: &mut [Word], channel: u8, value: Word) {
        match channel {
            DMA_FLASH_OFFSET_LOW => {
                self.flash_offset = (self.flash_offset & 0xffff_0000) | u32::from(value)
            }
            DMA_FLASH_OFFSET_HIGH => {
                self.flash_offset = (self.flash_offset & 0xffff) | (u32::from(value) << 16)
            }
            DMA_DESTINATION_LOW => {
                self.destination = (self.destination & 0xffff_0000) | u32::from(value)
            }
            DMA_DESTINATION_HIGH => {
                self.destination = (self.destination & 0xffff) | (u32::from(value) << 16)
            }
            DMA_FILE_SIZE_LOW => self.file_size = (self.file_size & 0xffff_0000) | u32::from(value),
            DMA_FILE_SIZE_HIGH => {
                self.file_size = (self.file_size & 0xffff) | (u32::from(value) << 16)
            }
            DMA_MEMORY_SIZE_LOW => {
                self.memory_size = (self.memory_size & 0xffff_0000) | u32::from(value)
            }
            DMA_MEMORY_SIZE_HIGH => {
                self.memory_size = (self.memory_size & 0xffff) | (u32::from(value) << 16)
            }
            DMA_COMMAND if value == DMA_COMMAND_START => {
                self.start_error = None;
                let command = DmaCommand {
                    flash_offset: self.flash_offset,
                    destination: PhysicalWordAddress::new(self.destination),
                    file_size_bytes: self.file_size,
                    memory_size_bytes: self.memory_size,
                };
                if let Err(error) =
                    self.engine
                        .start(command, self.flash.len(), self.physical_memory_words)
                {
                    // A busy engine keeps its transfer; validation failures
                    // latch onto the status/error channels.
                    if !matches!(error, DmaError::Busy) {
                        self.start_error = Some(dma_error_code(&error));
                    }
                }
            }
            _ => {}
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Device-0 model of the system-control block. The UART transmitter is never
/// busy; writes are captured for test assertions.
#[derive(Default)]
pub struct SystemControlDevice {
    pub led: Option<Word>,
    pub uart: Vec<u8>,
    pub icache_invalidations: u32,
    pub dcache_invalidations: u32,
}

impl Device for SystemControlDevice {
    fn read(&mut self, _memory: &mut [Word], _channel: u8) -> Word {
        0
    }

    fn write(&mut self, _memory: &mut [Word], channel: u8, value: Word) {
        match channel {
            ICACHE_INVALIDATE_ALL_DELAYED => self.icache_invalidations += 1,
            D_INVALIDATE_ALL => self.dcache_invalidations += 1,
            SYSCTL_LED => self.led = Some(value),
            SYSCTL_UART => self.uart.push(value as u8),
            _ => {}
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Device-1 model of the reset-time boot-selection latch.
#[derive(Default)]
pub struct BootSelectDevice {
    selection: Word,
}

impl BootSelectDevice {
    pub fn new(selection: Word) -> Self {
        Self {
            selection: selection & 0b11,
        }
    }
}

impl Device for BootSelectDevice {
    fn read(&mut self, _memory: &mut [Word], channel: u8) -> Word {
        if channel == BOOT_SELECT_VALUE {
            self.selection
        } else {
            0
        }
    }

    fn write(&mut self, _memory: &mut [Word], _channel: u8, _value: Word) {}

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
