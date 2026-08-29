//! Stable CpuV3 global cache-maintenance ABI.
//!
//! The current data cache is write-through, so `D_INVALIDATE_ALL` is a
//! one-cycle valid-bit clear. When write-back caching is introduced, the same
//! command becomes blocking clean-plus-invalidate; its numeric ABI does not
//! change. `D_CLEAN_ALL` and the status register are reserved until that
//! maintenance engine exists.

/// Device index of the system-control cache-maintenance interface.
pub const CACHE_MAINTENANCE_DEVICE: u8 = 0;
/// Registered one-cycle-delayed complete instruction-cache invalidation.
pub const ICACHE_INVALIDATE_ALL_DELAYED: u8 = 0;
/// Complete data-cache invalidation; clean-plus-invalidate under write-back.
pub const D_INVALIDATE_ALL: u8 = 1;
/// Reserved complete data-cache clean command.
pub const D_CLEAN_ALL: u8 = 4;
/// Reserved final result of the most recently completed maintenance command.
pub const CACHE_MAINTENANCE_STATUS: u8 = 5;

pub const CACHE_MAINTENANCE_STATUS_SUCCESS: u16 = 0;
pub const CACHE_MAINTENANCE_STATUS_ERROR: u16 = 0x8000;
