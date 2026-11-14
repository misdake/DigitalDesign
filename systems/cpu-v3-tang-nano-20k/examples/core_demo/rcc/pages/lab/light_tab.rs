//! Direction and intensities act on the shared object and floor.
use crate::dsl_rt::*;
mod gpu_api;
use crate::common::state::*;
use crate::common::ui::*;
use gpu_api::*;
mod boot_resources;
use crate::pages::lab::lab_scene::*;
use crate::pages::lab::lab_ui::*;
use boot_resources::*;
mod gamepad;
use gamepad::*;

#[derive(Copy, Clone)]
pub struct LightTab {
    pub light: LabLight,
    pub cursor: u16,
}
impl LightTab {
    pub const fn new() -> Self {
        Self {
            light: LabLight::new(),
            cursor: 0,
        }
    }
    pub fn update(&mut self, input: GamepadFrame, seconds: fix32) {
        self.cursor = lab_cursor(self.cursor, input.pressed, 2);
        self.light.yaw = lab_wrap_angle(self.light.yaw - input.left_x * seconds * fix!(2));
        self.light.pitch = lab_clamp(
            self.light.pitch - input.left_y * seconds * fix!(2),
            fix!(-1.2),
            fix!(1.2),
        );
        self.light.ambient = lab_clamp(
            self.light.ambient + input.right_x * seconds,
            fix!(0),
            fix!(1),
        );
        self.light.directional = lab_clamp(
            self.light.directional - input.right_y * seconds,
            fix!(0),
            fix!(1),
        );
        let mut delta = fix!(0);
        if input.pressed & BUTTON_RIGHT != 0 {
            delta = fix!(0.1);
        }
        if input.pressed & BUTTON_LEFT != 0 {
            delta = fix!(-0.1);
        }
        if self.cursor == 0 {
            self.light.ambient = lab_clamp(self.light.ambient + delta, fix!(0), fix!(1));
        } else {
            self.light.directional = lab_clamp(self.light.directional + delta, fix!(0), fix!(1));
        }
    }
}

pub fn light_labels(grid: &mut UiGrid, state: LightTab) {
    lab_option(grid, 5, state.cursor == 0, b"AMBIENT");
    lab_value(
        grid,
        5,
        (state.light.ambient * fix!(100)).round().to_int() as u16,
    );
    lab_option(grid, 7, state.cursor == 1, b"DIRECTIONAL");
    lab_value(
        grid,
        7,
        (state.light.directional * fix!(100)).round().to_int() as u16,
    );
    text(grid, 3, 9, b"LS DIRECTION");
    text(grid, 3, 11, b"RS STRENGTH");
    hint_at(grid, 1, 12, KEY_LS, ICON_ADJUST);
    hint_at(grid, 8, 12, KEY_RS, ICON_ADJUST);
    hint_at(grid, 17, 12, KEY_DPAD_H, ICON_ADJUST);
}
