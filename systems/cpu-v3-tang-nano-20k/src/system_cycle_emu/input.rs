//! Fixed bounded Device5 snapshot peer, not the DS2 serial engine.
use crate::input_device::{ControllerSnapshot, InputMode};
#[derive(Clone, Copy, Debug, Default)]
pub struct InputPeer {
    channels: [u8; 8],
    valid: bool,
    changed: bool,
    pub buttons: u8,
    button_meta: u8,
    button_sync: u8,
}
impl InputPeer {
    pub fn publish(&mut self, snapshot: Option<ControllerSnapshot>) {
        if let Some(s) = snapshot {
            self.channels = [
                s.left[0],
                s.left[1],
                match s.mode {
                    InputMode::Digital => 0x41,
                    InputMode::Analog => 0x73,
                    InputMode::Pressure => 0x79,
                },
                0x5a,
                s.buttons_active_low as u8,
                (s.buttons_active_low >> 8) as u8,
                s.right[0],
                s.right[1],
            ];
            self.valid = true;
            self.changed = true;
        } else if self.valid {
            self.valid = false;
            self.changed = true;
        }
    }
    pub fn read(&self, device: u8, channel: u8) -> Option<u16> {
        match device {
            0 => Some(if channel == 8 {
                u16::from(self.button_sync)
            } else {
                0
            }),
            5 => Some(match channel {
                0..=7 => self.channels[usize::from(channel)].into(),
                8 | 13..=15 => u16::from(self.valid) | u16::from(self.changed) << 1,
                _ => 0,
            }),
            7 => Some(0),
            _ => None,
        }
    }
    pub fn clock(&mut self, device: u8, channel: u8, write: bool) {
        let old = self.button_meta;
        self.button_meta = self.buttons & 3;
        self.button_sync = old;
        if write && device == 5 && (8..=15).contains(&channel) {
            self.changed = false;
        }
    }
}
