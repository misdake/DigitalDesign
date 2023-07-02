use crate::cpu_v1::devices::device_2_and_3_util::{GamepadButton, GamepadState};
use crate::cpu_v1::devices::{Device, DeviceReadResult, DeviceType};

#[derive(Default)]
enum ButtonQueryMode {
    #[default]
    Down,
    Press,
    Up,
}

pub struct DeviceGamepad {
    stack: Vec<u8>,
    button_query_mode: ButtonQueryMode,
    states: GamepadState,
}

#[repr(u8)]
#[allow(unused)]
pub enum DeviceGamepadOpcode {
    NextFrame = 0, // refresh prev/curr state

    // button control
    ButtonPop,       // pop value for buttons
    ButtonDownMode,  // prev=0 && curr=1
    ButtonPressMode, // curr=1
    ButtonUpMode,    // prev=1 && curr=0

    // buttons, pop order left to right
    ButtonDpad,          // left, right, up, down
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

impl DeviceGamepad {
    fn query_button(&mut self, button: GamepadButton) {
        let v = match self.button_query_mode {
            ButtonQueryMode::Down => self.states.is_down(button),
            ButtonQueryMode::Press => self.states.is_pressed(button),
            ButtonQueryMode::Up => self.states.is_up(button),
        };
        if v {
            self.stack.push(1)
        } else {
            self.stack.push(0)
        }
    }
    fn query_buttons(&mut self, buttons: [GamepadButton; 4]) {
        self.query_button(buttons[3]);
        self.query_button(buttons[2]);
        self.query_button(buttons[1]);
        self.query_button(buttons[0]);
    }
}

impl Device for DeviceGamepad {
    fn device_type(&self) -> DeviceType {
        DeviceType::Gamepad
    }
    fn exec(&mut self, opcode4: u8, reg0: u8, _reg1: u8) -> DeviceReadResult {
        let mut r = reg0;

        self.states.update();
        let opcode4: DeviceGamepadOpcode = unsafe { std::mem::transmute(opcode4) };
        match opcode4 {
            DeviceGamepadOpcode::NextFrame => {
                self.states.next_frame();
            }

            DeviceGamepadOpcode::ButtonPop => {
                if let Some(v) = self.stack.pop() {
                    r = v;
                }
            }
            DeviceGamepadOpcode::ButtonDownMode => {
                self.button_query_mode = ButtonQueryMode::Down;
            }
            DeviceGamepadOpcode::ButtonPressMode => {
                self.button_query_mode = ButtonQueryMode::Press;
            }
            DeviceGamepadOpcode::ButtonUpMode => {
                self.button_query_mode = ButtonQueryMode::Up;
            }

            DeviceGamepadOpcode::ButtonDpad => {
                self.query_buttons([
                    GamepadButton::Left,
                    GamepadButton::Right,
                    GamepadButton::Up,
                    GamepadButton::Down,
                ]);
            }
            DeviceGamepadOpcode::ButtonABXY => {
                self.query_buttons([
                    GamepadButton::A,
                    GamepadButton::B,
                    GamepadButton::X,
                    GamepadButton::Y,
                ]);
            }
            DeviceGamepadOpcode::ButtonLRStartOption => {
                self.query_buttons([
                    GamepadButton::LB,
                    GamepadButton::RB,
                    GamepadButton::Start,
                    GamepadButton::Option,
                ]);
            }

            //TODO
            DeviceGamepadOpcode::TriggerL => {}
            DeviceGamepadOpcode::TriggerR => {}
            DeviceGamepadOpcode::JoystickLX => {}
            DeviceGamepadOpcode::JoystickLY => {}
            DeviceGamepadOpcode::JoystickRX => {}
            DeviceGamepadOpcode::JoystickRY => {}
        }

        //pop value
        DeviceReadResult {
            reg0_write_data: r,
            self_latency: 4,
        }
    }
}
