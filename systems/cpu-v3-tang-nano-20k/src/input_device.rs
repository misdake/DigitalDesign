//! Latest-only Device5 software snapshots, independent of serial acquisition.
use cpu_v3::{Device, Word};
use std::cell::Cell;

pub const INPUT_DEVICE: u8 = 5;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputMode {
    Digital,
    Analog,
    Pressure,
}

/// Raw software-ABI input; RCC owns button edges, calibration and dead zones.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ControllerSnapshot {
    pub mode: InputMode,
    pub buttons_active_low: u16,
    pub right: [u8; 2],
    pub left: [u8; 2],
}

#[derive(Clone, Copy, Default)]
struct State {
    channels: [u8; 8],
    valid: bool,
    changed: bool,
}

#[derive(Default)]
pub struct InputSnapshotDevice {
    state: Cell<State>,
}

impl InputSnapshotDevice {
    /// Replace the complete latest sample. Begin-read never freezes an old one.
    pub fn publish(&self, snapshot: Option<ControllerSnapshot>) {
        let mut state = self.state.get();
        if let Some(snapshot) = snapshot {
            state.channels = [
                snapshot.left[0],
                snapshot.left[1],
                match snapshot.mode {
                    InputMode::Digital => 0x41,
                    InputMode::Analog => 0x73,
                    InputMode::Pressure => 0x79,
                },
                0x5a,
                snapshot.buttons_active_low as u8,
                (snapshot.buttons_active_low >> 8) as u8,
                snapshot.right[0],
                snapshot.right[1],
            ];
            state.valid = true;
            state.changed = true;
        } else if state.valid {
            state.valid = false;
            state.changed = true;
        }
        self.state.set(state);
    }
}

impl Device for InputSnapshotDevice {
    fn read(&mut self, _memory: &mut [Word], channel: u8) -> Word {
        let state = self.state.get();
        match channel {
            0..=7 => u16::from(state.channels[usize::from(channel)]),
            8 | 13..=15 => u16::from(state.valid) | (u16::from(state.changed) << 1),
            // Background bytes and wire slot/phase/DAT/ATT are not modeled.
            _ => 0,
        }
    }

    fn write(&mut self, _memory: &mut [Word], channel: u8, _value: Word) {
        if (8..=15).contains(&channel) {
            let mut state = self.state.get();
            state.changed = false;
            self.state.set(state);
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
