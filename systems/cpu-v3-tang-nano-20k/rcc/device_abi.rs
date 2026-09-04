//! Device indices, channels, and commands shared by CPU V3 RCC firmware.

pub const SYSTEM_CONTROL_DEVICE: u16 = 0;
pub const ICACHE_INVALIDATE_ALL_DELAYED: u16 = 0;
pub const D_INVALIDATE_ALL: u16 = 1;
pub const SYSCTL_LED: u16 = 2;
pub const SYSCTL_UART_STATUS: u16 = 3;
pub const SYSCTL_UART_TX_DATA: u16 = 3;
pub const D_CLEAN_ALL: u16 = 4;
pub const CACHE_MAINTENANCE_STATUS: u16 = 5;
pub const CACHE_MAINTENANCE_STATUS_SUCCESS: u16 = 0;
pub const CACHE_MAINTENANCE_STATUS_ERROR: u16 = 0x8000;

pub const BOOT_SELECT_DEVICE: u16 = 1;
pub const BOOT_SELECT_VALUE: u16 = 0;

pub const BOOT_DMA_DEVICE: u16 = 2;
pub const DMA_COMMAND: u16 = 0;
pub const DMA_STATUS: u16 = 1;
pub const DMA_FLASH_OFFSET_LOW: u16 = 2;
pub const DMA_FLASH_OFFSET_HIGH: u16 = 3;
pub const DMA_DESTINATION_LOW: u16 = 4;
pub const DMA_DESTINATION_HIGH: u16 = 5;
pub const DMA_FILE_SIZE_LOW: u16 = 6;
pub const DMA_FILE_SIZE_HIGH: u16 = 7;
pub const DMA_MEMORY_SIZE_LOW: u16 = 8;
pub const DMA_MEMORY_SIZE_HIGH: u16 = 9;
pub const DMA_ERROR: u16 = 14;
pub const DMA_COMMAND_START: u16 = 1;
pub const DMA_STATUS_BUSY: u16 = 1;
pub const DMA_STATUS_ERROR: u16 = 0x8000;

pub const DISPLAY_DEVICE: u16 = 3;
pub const DISPLAY_FRAME_INDEX: u16 = 0;
pub const DISPLAY_STAGE_FRAMEBUFFER_LOW: u16 = 1;
pub const DISPLAY_STAGE_FRAMEBUFFER_HIGH: u16 = 2;
pub const DISPLAY_SWAP_COMMAND: u16 = 3;
pub const DISPLAY_NEXT_SWAP: u16 = 1;
