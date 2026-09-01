//! Stable CpuV3 global cache-maintenance ABI.
//!
//! The data cache is write-back. `D_CLEAN_ALL` and `D_INVALIDATE_ALL` start the
//! blocking global maintenance engine; invalidate is always clean-plus-
//! invalidate and therefore never discards dirty data.

/// Device index of the system-control cache-maintenance interface.
pub const CACHE_MAINTENANCE_DEVICE: u8 = 0;
/// Registered one-cycle-delayed complete instruction-cache invalidation.
pub const ICACHE_INVALIDATE_ALL_DELAYED: u8 = 0;
/// Complete data-cache invalidation; clean-plus-invalidate under write-back.
pub const D_INVALIDATE_ALL: u8 = 1;
/// Complete data-cache clean command.
pub const D_CLEAN_ALL: u8 = 4;
/// Final result of the most recently completed maintenance command.
pub const CACHE_MAINTENANCE_STATUS: u8 = 5;

pub const CACHE_MAINTENANCE_STATUS_SUCCESS: u16 = 0;
pub const CACHE_MAINTENANCE_STATUS_ERROR: u16 = 0x8000;
