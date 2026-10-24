//! CPU register contract for the fitted system-control device.

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
/// Stage a device/channel selector for the generic watch-change sleep.
/// Bits 2:0 select the device and bits 6:3 select its channel; other bits are
/// ignored.
pub const SYSCTL_WATCH_TARGET: u8 = 6;
/// Arm the staged watch while the selected channel equals the written value.
pub const SYSCTL_WATCH_EXPECTED: u8 = 7;
/// Free-running millisecond timestamp modulo 65536; read-only.
pub const SYSCTL_MILLISECONDS: u8 = 10;
/// Saturating microsecond interval; write bit 0 clears count and prescaler.
pub const SYSCTL_MICROSECONDS: u8 = 11;

pub const fn sysctl_watch_target(device: u8, channel: u8) -> u16 {
    ((channel as u16 & 0x0f) << 3) | (device as u16 & 0x07)
}
