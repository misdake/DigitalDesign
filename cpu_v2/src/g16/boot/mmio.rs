//! Stable CPU register contract for the boot Flash-to-memory DMA engine.

/// Device-register page selected by `dev_recv(2, channel)` and
/// `dev_send(2, channel, value)`.
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
pub const DMA_EXPECTED_CRC_LOW: u8 = 10;
pub const DMA_EXPECTED_CRC_HIGH: u8 = 11;
pub const DMA_ACTUAL_CRC_LOW: u8 = 12;
pub const DMA_ACTUAL_CRC_HIGH: u8 = 13;
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
pub const DMA_ERROR_CRC_MISMATCH: u16 = 6;

/// A host-side view of the writable DMA register bank.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BootDmaRegisters {
    /// Absolute fitted-Flash byte address, not a package-relative offset.
    pub flash_offset: u32,
    pub destination: super::super::PhysicalWordAddress,
    pub file_size_bytes: u32,
    pub memory_size_bytes: u32,
    pub expected_crc32: u32,
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
            DMA_EXPECTED_CRC_LOW => Some(self.expected_crc32 as u16),
            DMA_EXPECTED_CRC_HIGH => Some((self.expected_crc32 >> 16) as u16),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::g16::PhysicalWordAddress;

    #[test]
    fn register_words_preserve_flash_memory_and_crc_fields() {
        let registers = BootDmaRegisters {
            flash_offset: 0x007a_bcde,
            destination: PhysicalWordAddress::new(0x0032_4567),
            file_size_bytes: 0x0001_ffff,
            memory_size_bytes: 0x0002_0000,
            expected_crc32: 0x89ab_cdef,
        };
        assert_eq!(registers.channel_value(DMA_FLASH_OFFSET_LOW), Some(0xbcde));
        assert_eq!(registers.channel_value(DMA_FLASH_OFFSET_HIGH), Some(0x007a));
        assert_eq!(registers.channel_value(DMA_DESTINATION_LOW), Some(0x4567));
        assert_eq!(registers.channel_value(DMA_DESTINATION_HIGH), Some(0x0032));
        assert_eq!(registers.channel_value(DMA_EXPECTED_CRC_LOW), Some(0xcdef));
        assert_eq!(registers.channel_value(DMA_EXPECTED_CRC_HIGH), Some(0x89ab));
        assert_eq!(registers.channel_value(DMA_STATUS), None);
    }
}
