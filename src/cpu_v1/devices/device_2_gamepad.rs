use crate::cpu_v1::devices::{Device, DeviceReadResult, DeviceType};

#[derive(Default)]
enum ButtonQueryMode {
    #[default]
    Down,
    Press,
    Up,
}

#[derive(Default)]
pub struct DeviceGamepad {
    //TODO receiver
    button_query_mode: ButtonQueryMode,
    //TODO all the buttons
}

#[repr(u8)]
#[allow(unused)]
pub enum DeviceGamepadOpcode {
    // control
    NextFrame = 0,   // refresh prev/curr state
    ButtonDownMode,  // prev=0 && curr=1
    ButtonPressMode, // curr=1
    ButtonUpMode,    // prev=1 && curr=0

    // buttons, set values (4bits)
    ButtonArrowKey,      // up, right, down, left
    ButtonABXY,          // A, B, X, Y
    ButtonLRStartOption, // LB, RB, Start, Option

    // analog, set value
    TriggerL,   // LT (0 to 7)
    TriggerR,   // RT (0 to 7)
    JoystickLX, // Left/Right -7 to 7
    JoystickLY, // Up/Down -7 to 7
    JoystickRX, // Left/Right -7 to 7
    JoystickRY, // Up/Down -7 to 7
}

impl Device for DeviceGamepad {
    fn device_type(&self) -> DeviceType {
        DeviceType::Gamepad
    }
    fn exec(&mut self, _opcode4: u8, _reg0: u8, _reg1: u8) -> DeviceReadResult {
        //TODO poll receiver, update values
        //pop value
        todo!()
    }
}
