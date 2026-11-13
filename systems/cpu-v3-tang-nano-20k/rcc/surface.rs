//! Shared physical RGB565 storage description; no GPU or canvas ownership state.
use crate::dsl_rt::*;
mod gpu_layout;
use gpu_layout::*;
mod static_buffer;
use static_buffer::*;

#[derive(Copy, Clone)]
pub struct Rgb565Surface {
    pub storage: StaticBuffer,
}
impl Rgb565Surface {
    pub const fn first_sector(self) -> u16 {
        self.storage.first_sector()
    }
    pub const fn sector_count(self) -> u16 {
        self.storage.sector_count()
    }
    pub const fn allocate() -> Self {
        Self {
            storage: StaticBuffer::new(GPU_FRAMEBUFFER_PHYSICAL_SECTORS as usize * 128, 64 * 1024),
        }
    }
    /// Caller guarantees the physical allocation and coordinates CPU/GPU/display use.
    pub const unsafe fn from_sectors(first_sector: u16, sector_count: u16) -> Self {
        debug_assert!(first_sector & 511 == 0);
        debug_assert!(sector_count >= GPU_FRAMEBUFFER_PHYSICAL_SECTORS);
        assert!(sector_count != 0);
        assert!(sector_count - 1 <= 65535 - first_sector);
        Self {
            storage: unsafe { StaticBuffer::from_sectors(first_sector, sector_count) },
        }
    }
}

#[derive(Copy, Clone)]
pub struct DepthSurface {
    pub storage: StaticBuffer,
}
impl DepthSurface {
    pub const fn allocate() -> Self {
        Self {
            storage: StaticBuffer::new(GPU_FRAMEBUFFER_PHYSICAL_SECTORS as usize * 128, 64 * 1024),
        }
    }
}

#[derive(Copy, Clone)]
pub struct UiGridStorage {
    pub storage: StaticBuffer,
}
impl UiGridStorage {
    pub const fn allocate() -> Self {
        // Native grid: 50 columns by 15 rows, one byte per glyph ID.
        Self {
            storage: StaticBuffer::new(50 * 15, 128),
        }
    }
}
