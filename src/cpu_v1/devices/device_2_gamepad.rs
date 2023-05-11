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
    //values to pop
    stack: Vec<u8>,
}

#[repr(u8)]
#[allow(unused)]
pub enum DeviceGamepadOpcode {
    Next = 0, // write next bit to reg0, value 0 or 1

    // buttons
    ButtonDownMode = 1,
    ButtonPressMode = 2,
    ButtonUpMode = 3,
    ButtonArrowKey = 4,      // up, right, down left
    ButtonABXY = 5,          // A, B, X, Y
    ButtonLRStartOption = 6, // LB, RB, Start, Option

    // analog
    TriggerL = 8,    // LT (0 to 15)
    TriggerR = 9,    // RT (0 to 15)
    JoystickLX = 10, // Left/Right -7 to 7
    JoystickLY = 11, // Up/Down -7 to 7
    JoystickRX = 12, // Left/Right -7 to 7
    JoystickRY = 13, // Up/Down -7 to 7
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
