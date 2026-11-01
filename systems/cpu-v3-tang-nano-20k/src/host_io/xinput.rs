//! Windows XInput is a source of software snapshots, not a DS2 wire peer.
use super::ControllerSource;
use crate::input_device::{ControllerSnapshot, InputMode};

#[derive(Clone, Copy, Debug, Default)]
pub struct XInputSample {
    pub buttons: u16,
    pub left_trigger: u8,
    pub right_trigger: u8,
    pub left: [i16; 2],
    pub right: [i16; 2],
}

pub fn map_xinput(sample: XInputSample) -> ControllerSnapshot {
    let mut buttons = 0_u16;
    for (source, destination) in [
        (0x0020, 0), // Back -> Select
        (0x0040, 1), // left/right stick clicks
        (0x0080, 2),
        (0x0010, 3), // Start
        (0x0001, 4), // Up, Right, Down, Left
        (0x0008, 5),
        (0x0002, 6),
        (0x0004, 7),
        (0x0100, 10), // shoulders
        (0x0200, 11),
        (0x8000, 12), // Y -> Triangle
        (0x2000, 13), // B -> Circle
        (0x1000, 14), // A -> Cross
        (0x4000, 15), // X -> Square
    ] {
        if sample.buttons & source != 0 {
            buttons |= 1 << destination;
        }
    }
    buttons |= u16::from(sample.left_trigger > 30) << 8;
    buttons |= u16::from(sample.right_trigger > 30) << 9;
    let axes = |[x, y]: [i16; 2]| {
        [
            ((32768 + i32::from(x)) >> 8) as u8,
            ((32768 - i32::from(y)).clamp(0, 65535) >> 8) as u8,
        ]
    };
    ControllerSnapshot {
        mode: InputMode::Analog,
        buttons_active_low: !buttons,
        left: axes(sample.left),
        right: axes(sample.right),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XInputError {
    InvalidSlot,
    UnsupportedPlatform,
}

impl std::fmt::Display for XInputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidSlot => "XInput slot must be between 0 and 3",
            Self::UnsupportedPlatform => "XInput host input is supported only on Windows",
        })
    }
}

impl std::error::Error for XInputError {}

pub struct XInputSource {
    #[cfg(windows)]
    slot: u32,
}

impl XInputSource {
    pub fn new(slot: u32) -> Result<Self, XInputError> {
        if slot > 3 {
            return Err(XInputError::InvalidSlot);
        }
        #[cfg(windows)]
        {
            Ok(Self { slot })
        }
        #[cfg(not(windows))]
        {
            Err(XInputError::UnsupportedPlatform)
        }
    }
}

impl ControllerSource for XInputSource {
    fn poll(&mut self) -> Option<ControllerSnapshot> {
        #[cfg(windows)]
        {
            use windows_sys::Win32::UI::Input::XboxController::{XInputGetState, XINPUT_STATE};
            // XINPUT_STATE contains only integer fields; zero is a valid initial value.
            let mut state: XINPUT_STATE = unsafe { std::mem::zeroed() };
            // The initialized output lives through the synchronous official call.
            if unsafe { XInputGetState(self.slot, &mut state) } != 0 {
                return None;
            }
            let g = state.Gamepad;
            Some(map_xinput(XInputSample {
                buttons: g.wButtons,
                left_trigger: g.bLeftTrigger,
                right_trigger: g.bRightTrigger,
                left: [g.sThumbLX, g.sThumbLY],
                right: [g.sThumbRX, g.sThumbRY],
            }))
        }
        #[cfg(not(windows))]
        {
            None
        }
    }
}
