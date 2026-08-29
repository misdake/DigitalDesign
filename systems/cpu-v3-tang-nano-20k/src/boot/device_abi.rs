//! Stable CPU register contract for the boot Flash-to-memory DMA engine and
//! the system-control device.

/// System-control device selected by `dev_recv(SYSTEM_CONTROL_DEVICE, channel)`
/// and `dev_send(SYSTEM_CONTROL_DEVICE, channel, value)`.
pub const SYSTEM_CONTROL_DEVICE: u8 = cpu_v3::CACHE_MAINTENANCE_DEVICE;

pub use cpu_v3::{
    CACHE_MAINTENANCE_STATUS, CACHE_MAINTENANCE_STATUS_ERROR, CACHE_MAINTENANCE_STATUS_SUCCESS,
    D_CLEAN_ALL, D_INVALIDATE_ALL, ICACHE_INVALIDATE_ALL_DELAYED,
};
/// The low six written bits drive the board LEDs.
pub const SYSCTL_LED: u8 = 2;
/// Writes queue one UART transmit byte (8N1); reads report bit 0 set while
/// the transmitter is busy.
pub const SYSCTL_UART: u8 = 3;

/// Boot-selection strap device. The fitted system latches a stable one-hot
/// button value during reset and exposes it to Stage1 after button release.
pub const BOOT_SELECT_DEVICE: u8 = 1;
pub const BOOT_SELECT_VALUE: u8 = 0;

/// Boot DMA device selected by `dev_recv(BOOT_DMA_DEVICE, channel)` and
/// `dev_send(BOOT_DMA_DEVICE, channel, value)`.
pub const BOOT_DMA_DEVICE: u8 = 2;

pub const DMA_COMMAND: u8 = 0;
pub const DMA_STATUS: u8 = 1;
pub const DMA_FLASH_OFFSET_LOW: u8 = 2;
pub const DMA_FLASH_OFFSET_HIGH: u8 = 3;
pub const DMA_DESTINATION_LOW: u8 = 4;
pub const DMA_DESTINATION_HIGH: u8 = 5;
pub const DMA_FILE_SIZE_LOW: u8 = 6;
pub const DMA_FILE_SIZE_HIGH: u8 = 7;
pub const DMA_MEMORY_SIZE_LOW: u8 = 8;
pub const DMA_MEMORY_SIZE_HIGH: u8 = 9;
// Channels 10 through 13 held the CRC32 registers before format version 3
// and are now free.
pub const DMA_ERROR: u8 = 14;
pub const DMA_COMPLETED_WORDS_LOW: u8 = 15;

pub const DMA_COMMAND_START: u16 = 1;
pub const DMA_STATUS_IDLE: u16 = 0;
pub const DMA_STATUS_BUSY: u16 = 1;
pub const DMA_STATUS_DONE: u16 = 2;
pub const DMA_STATUS_ERROR: u16 = 0x8000;

pub const DMA_ERROR_FILE_LARGER_THAN_MEMORY: u16 = 1;
pub const DMA_ERROR_FLASH_RANGE: u16 = 2;
pub const DMA_ERROR_MEMORY_RANGE: u16 = 3;
pub const DMA_ERROR_FLASH_IO: u16 = 4;
pub const DMA_ERROR_MEMORY_IO: u16 = 5;

/// A host-side view of the writable DMA register bank.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BootDmaRegisters {
    /// Absolute fitted-Flash byte address, not a package-relative offset.
    pub flash_offset: u32,
    pub destination: super::super::PhysicalWordAddress,
    pub file_size_bytes: u32,
    pub memory_size_bytes: u32,
}

impl BootDmaRegisters {
    pub fn channel_value(self, channel: u8) -> Option<u16> {
        match channel {
            DMA_FLASH_OFFSET_LOW => Some(self.flash_offset as u16),
            DMA_FLASH_OFFSET_HIGH => Some((self.flash_offset >> 16) as u16),
            DMA_DESTINATION_LOW => Some(self.destination.get() as u16),
            DMA_DESTINATION_HIGH => Some((self.destination.get() >> 16) as u16),
            DMA_FILE_SIZE_LOW => Some(self.file_size_bytes as u16),
            DMA_FILE_SIZE_HIGH => Some((self.file_size_bytes >> 16) as u16),
            DMA_MEMORY_SIZE_LOW => Some(self.memory_size_bytes as u16),
            DMA_MEMORY_SIZE_HIGH => Some((self.memory_size_bytes >> 16) as u16),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PhysicalWordAddress;

    #[test]
    fn register_words_preserve_flash_and_memory_fields() {
        let registers = BootDmaRegisters {
            flash_offset: 0x007a_bcde,
            destination: PhysicalWordAddress::new(0x0032_4567),
            file_size_bytes: 0x0001_ffff,
            memory_size_bytes: 0x0002_0000,
        };
        assert_eq!(registers.channel_value(DMA_FLASH_OFFSET_LOW), Some(0xbcde));
        assert_eq!(registers.channel_value(DMA_FLASH_OFFSET_HIGH), Some(0x007a));
        assert_eq!(registers.channel_value(DMA_DESTINATION_LOW), Some(0x4567));
        assert_eq!(registers.channel_value(DMA_DESTINATION_HIGH), Some(0x0032));
        assert_eq!(registers.channel_value(DMA_STATUS), None);
    }
}
