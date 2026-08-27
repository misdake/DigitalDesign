//! CPU V3 display-device ABI.

use crate::{Device, Word, FRAMEBUFFER_A_BASE_WORD, FRAMEBUFFER_WORDS};
use std::cell::Cell;

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

const MAX_FRAMEBUFFER_BASE: u32 = (1 << 22) - FRAMEBUFFER_WORDS;

/// Host-side model of the frame-index and framebuffer-swap registers.
pub struct DisplayDevice {
    frame_index: Cell<u16>,
    active_base: Cell<u32>,
    next_base: Cell<u32>,
    low_written: Cell<bool>,
    high_written: Cell<bool>,
    pending: Cell<bool>,
    invalid_address: Cell<bool>,
}

impl Default for DisplayDevice {
    fn default() -> Self {
        Self {
            frame_index: Cell::new(0),
            active_base: Cell::new(FRAMEBUFFER_A_BASE_WORD),
            next_base: Cell::new(FRAMEBUFFER_A_BASE_WORD),
            low_written: Cell::new(false),
            high_written: Cell::new(false),
            pending: Cell::new(false),
            invalid_address: Cell::new(false),
        }
    }
}

impl DisplayDevice {
    pub fn frame_index(&self) -> u16 {
        self.frame_index.get()
    }

    pub fn active_base(&self) -> u32 {
        self.active_base.get()
    }

    pub fn swap_pending(&self) -> bool {
        self.pending.get()
    }

    /// Advances through one vblank event and atomically applies a pending base.
    pub fn advance_frame(&self) {
        self.frame_index.set(self.frame_index.get().wrapping_add(1));
        if self.pending.replace(false) {
            self.active_base.set(self.next_base.get());
            self.low_written.set(false);
            self.high_written.set(false);
        }
    }

    fn accept_completed_address(&self) {
        if !self.low_written.get() || !self.high_written.get() {
            return;
        }
        let base = self.next_base.get();
        if base & 0xf == 0 && base <= MAX_FRAMEBUFFER_BASE {
            self.pending.set(true);
        } else {
            self.low_written.set(false);
            self.high_written.set(false);
            self.invalid_address.set(true);
        }
    }

    fn status(&self) -> u16 {
        (if self.pending.get() {
            DISPLAY_STATUS_PENDING
        } else {
            0
        }) | (if self.low_written.get() ^ self.high_written.get() {
            DISPLAY_STATUS_PARTIAL
        } else {
            0
        }) | (if self.invalid_address.get() {
            DISPLAY_STATUS_INVALID_ADDRESS
        } else {
            0
        })
    }
}

impl Device for DisplayDevice {
    fn read(&mut self, _memory: &mut [Word], channel: u8) -> Word {
        match channel {
            DISPLAY_FRAME_INDEX => self.frame_index.get(),
            DISPLAY_FRAMEBUFFER_LOW => self.active_base.get() as u16,
            DISPLAY_FRAMEBUFFER_HIGH => (self.active_base.get() >> 16) as u16,
            DISPLAY_STATUS => self.status(),
            _ => 0,
        }
    }

    fn write(&mut self, _memory: &mut [Word], channel: u8, value: Word) {
        if self.pending.get() {
            return;
        }
        match channel {
            DISPLAY_FRAMEBUFFER_LOW => {
                self.next_base
                    .set((self.next_base.get() & 0xffff_0000) | u32::from(value));
                self.low_written.set(true);
            }
            DISPLAY_FRAMEBUFFER_HIGH => {
                self.next_base
                    .set((self.next_base.get() & 0x0000_ffff) | (u32::from(value) << 16));
                self.high_written.set(true);
            }
            _ => return,
        }
        self.invalid_address.set(false);
        self.accept_completed_address();
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(device: &mut DisplayDevice, channel: u8, value: u16) {
        device.write(&mut [], channel, value);
    }

    #[test]
    fn address_halves_commit_atomically_at_vblank() {
        let mut device = DisplayDevice::default();
        write(&mut device, DISPLAY_FRAMEBUFFER_LOW, 0x2d00);
        assert_eq!(device.status(), DISPLAY_STATUS_PARTIAL);
        device.advance_frame();
        assert_eq!(device.active_base(), FRAMEBUFFER_A_BASE_WORD);
        write(&mut device, DISPLAY_FRAMEBUFFER_HIGH, 0x0021);
        assert_eq!(device.status(), DISPLAY_STATUS_PENDING);
        device.advance_frame();
        assert_eq!(device.active_base(), 0x0021_2d00);
        assert_eq!(device.frame_index(), 2);
    }

    #[test]
    fn frame_index_wraps_and_invalid_addresses_do_not_apply() {
        let mut device = DisplayDevice::default();
        device.frame_index.set(u16::MAX);
        write(&mut device, DISPLAY_FRAMEBUFFER_LOW, 0x0101);
        write(&mut device, DISPLAY_FRAMEBUFFER_HIGH, 0x0020);
        assert_eq!(device.status(), DISPLAY_STATUS_INVALID_ADDRESS);
        device.advance_frame();
        assert_eq!(device.frame_index(), 0);
        assert_eq!(device.active_base(), FRAMEBUFFER_A_BASE_WORD);
    }
}
