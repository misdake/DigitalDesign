//! Build-time allocated, zero-initialized external working storage.
use crate::dsl_rt::*;

#[derive(Copy, Clone)]
pub struct StaticBuffer {
    first_sector: u16,
    sector_count: u16,
}
impl StaticBuffer {
    /// Allocation request, permitted only within a #[static_buffer] declaration.
    pub const fn new(bytes: usize, alignment: usize) -> Self {
        assert!(bytes != 0 && bytes <= 65535 * 128);
        assert!(alignment >= 128 && alignment & (alignment - 1) == 0);
        Self {
            first_sector: 0,
            sector_count: ((bytes + 127) / 128) as u16,
        }
    }
    pub const fn first_sector(self) -> u16 {
        self.first_sector
    }
    pub const fn sector_count(self) -> u16 {
        self.sector_count
    }
    /// Caller guarantees allocated backing storage and coordinates its users.
    pub const unsafe fn from_sectors(first_sector: u16, sector_count: u16) -> Self {
        assert!(sector_count != 0 && sector_count - 1 <= 65535 - first_sector);
        Self {
            first_sector,
            sector_count,
        }
    }
}
