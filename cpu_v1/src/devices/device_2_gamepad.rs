#![allow(unused)]

use crate::devices::device_0_terminal::DeviceTerminalOp;
use crate::devices::device_2_and_3_util::{GamepadButton, GamepadState};
use crate::devices::{Device, DeviceReadResult, DeviceType};

#[repr(u8)]
pub enum ButtonQueryMode {
    Down,
    Press,
    Up,
}
#[repr(u8)]
pub enum ButtonQueryType {
    PlaceHolder = 0,
    ButtonUp,
    ButtonDown,
    ButtonLeft,
    ButtonRight,
    ButtonA,
    ButtonB,
    ButtonX,
    ButtonY,
    ButtonLB,
    ButtonRB,
    ButtonStart,
    ButtonOption,
}
#[repr(u8)]
pub enum AnalogQueryType {
    PlaceHolder = 0,
    TriggerL,   // LT (0 to 7)
    TriggerR,   // RT (0 to 7)
    JoystickLX, // Left/Right -7 to 7
    JoystickLY, // Up/Down -7 to 7
    JoystickRX, // Left/Right -7 to 7
    JoystickRY, // Up/Down -7 to 7
}

pub struct DeviceGamepad {
    button_query_mode: ButtonQueryMode,
    gamepad_state: GamepadState,
}
impl DeviceGamepad {
    pub fn create(gamepad_state: GamepadState) -> Self {
        Self {
            button_query_mode: ButtonQueryMode::Down,
            gamepad_state,
        }
    }
}

#[repr(u8)]
#[allow(unused)]
pub enum DeviceGamepadOpcode {
    NextFrame = 0, // refresh prev/curr state
    // button
    SetButtonQueryMode, // reg0 = ButtonQueryMode
    QueryButton,        // reg0 = ButtonQueryType
    // analog
    QueryAnalog, // reg0 = AnalogQueryType
}

impl DeviceGamepad {
    fn query_button(&mut self, button: GamepadButton) -> u8 {
        let v = match self.button_query_mode {
            ButtonQueryMode::Down => self.gamepad_state.is_down(button),
            ButtonQueryMode::Press => self.gamepad_state.is_pressed(button),
            ButtonQueryMode::Up => self.gamepad_state.is_up(button),
        };
        v as u8
    }
}

impl Device for DeviceGamepad {
    fn device_type(&self) -> DeviceType {
        DeviceType::Gamepad
    }
    fn exec(&mut self, opcode3: u8, reg0: u8, _reg1: u8) -> DeviceReadResult {
        let mut r = reg0;

        self.gamepad_state.update();
        let opcode3: DeviceGamepadOpcode = unsafe { std::mem::transmute(opcode3) };
        match opcode3 {
            DeviceGamepadOpcode::NextFrame => {
                self.gamepad_state.next_frame();
            }
            DeviceGamepadOpcode::SetButtonQueryMode => {
                self.button_query_mode = unsafe { std::mem::transmute(reg0) };
            }
            DeviceGamepadOpcode::QueryButton => {
                let button: ButtonQueryType = unsafe { std::mem::transmute(reg0) };
                r = match button {
                    ButtonQueryType::PlaceHolder => 0,
                    ButtonQueryType::ButtonUp => self.query_button(GamepadButton::Up),
                    ButtonQueryType::ButtonDown => self.query_button(GamepadButton::Down),
                    ButtonQueryType::ButtonLeft => self.query_button(GamepadButton::Left),
                    ButtonQueryType::ButtonRight => self.query_button(GamepadButton::Right),
                    ButtonQueryType::ButtonA => self.query_button(GamepadButton::A),
                    ButtonQueryType::ButtonB => self.query_button(GamepadButton::B),
                    ButtonQueryType::ButtonX => self.query_button(GamepadButton::X),
                    ButtonQueryType::ButtonY => self.query_button(GamepadButton::Y),
                    ButtonQueryType::ButtonLB => self.query_button(GamepadButton::LB),
                    ButtonQueryType::ButtonRB => self.query_button(GamepadButton::RB),
                    ButtonQueryType::ButtonStart => self.query_button(GamepadButton::Start),
                    ButtonQueryType::ButtonOption => self.query_button(GamepadButton::Option),
                }
            }
            DeviceGamepadOpcode::QueryAnalog => {
                // let ty: AnalogQueryType = unsafe { std::mem::transmute(reg0) };
                todo!()
            }
        }

        DeviceReadResult {
            reg0_write_data: r,
            self_latency: 4,
        }
    }
}

#[test]
fn test_gamepad() {
    use crate::devices::*;
    use crate::isa::Instruction::*;
    use crate::isa::RegisterIndex::*;

    test_device(
        &[
            load_imm(DeviceType::Gamepad as u8),
            set_bus_addr0(()),
            load_imm(DeviceType::Terminal as u8),
            set_bus_addr1(()),
            load_imm(ButtonQueryMode::Press as u8),
            bus0(DeviceGamepadOpcode::SetButtonQueryMode as u8), // set press mode
            load_imm(ButtonQueryType::ButtonStart as u8),
            bus0(DeviceGamepadOpcode::QueryButton as u8), // query start button
            bus1(DeviceTerminalOp::Print as u8),          // print 0
            jmp_offset(16 - 3),
        ],
        100000000, // just big enough to keep it running
        [0, 0, 0, 0],
    );
}
