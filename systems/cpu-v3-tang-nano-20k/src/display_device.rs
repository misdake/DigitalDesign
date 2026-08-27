//! CPU V3 display-device ABI.

/// Display controller device selected by `dev_recv(3, channel)` and
/// `dev_send(3, channel, value)`.
pub const DISPLAY_DEVICE: u8 = 3;

/// Read-only frame index. It increments once when active scanout enters
/// vertical blanking, independently of whether a framebuffer swap is pending.
pub const DISPLAY_FRAME_INDEX: u8 = 0;
/// Read: active framebuffer word-address bits 15:0. Write: next base bits 15:0.
pub const DISPLAY_FRAMEBUFFER_LOW: u8 = 1;
/// Read: active framebuffer word-address bits 31:16. Write: next base bits 31:16.
pub const DISPLAY_FRAMEBUFFER_HIGH: u8 = 2;
/// Read-only status bits described by the constants below.
pub const DISPLAY_STATUS: u8 = 3;

pub const DISPLAY_STATUS_PENDING: u16 = 1 << 0;
pub const DISPLAY_STATUS_PARTIAL: u16 = 1 << 1;
pub const DISPLAY_STATUS_INVALID_ADDRESS: u16 = 1 << 2;
pub const DISPLAY_STATUS_UNDERFLOW: u16 = 1 << 3;
pub const DISPLAY_STATUS_MEMORY_ERROR: u16 = 1 << 4;
