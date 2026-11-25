//! Orbit and pan with calibrated sticks; projection options remain independent.
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
use gamepad::{GamepadFrame, DS2_LEFT, DS2_RIGHT};

#[derive(Copy, Clone)]
pub struct CameraTab {
    pub view: LabView,
    pub cursor: u16,
}
impl CameraTab {
    pub const fn new() -> Self {
        Self {
            view: LabView::new(),
            cursor: 0,
        }
    }
    pub fn update(&mut self, input: GamepadFrame, seconds: fix32) {
        self.cursor = lab_cursor(self.cursor, input.pressed, 2);
        self.view.yaw = lab_wrap_angle(self.view.yaw - input.left_x * seconds);
        self.view.pitch = lab_clamp(
            self.view.pitch - input.left_y * seconds,
            fix!(0.15),
            fix!(1.2),
        );
        self.view.target_x = lab_clamp(
            self.view.target_x - input.right_x * seconds * fix!(2),
            fix!(-1),
            fix!(3),
        );
        self.view.target_y = lab_clamp(
            self.view.target_y - input.right_y * seconds * fix!(2),
            fix!(-2),
            fix!(2),
        );
        if self.cursor == 0 {
            let step = if input.pressed & DS2_RIGHT != 0 {
                fix!(0.5)
            } else {
                fix!(0)
            };
            let back = if input.pressed & DS2_LEFT != 0 {
                fix!(-0.5)
            } else {
                fix!(0)
            };
            self.view.distance = lab_clamp(self.view.distance + step + back, fix!(4), fix!(12));
        } else {
            self.view.fov = lab_adjust(self.view.fov, input.pressed, 5, 80);
            if self.view.fov < 35 {
                self.view.fov = 35;
            }
        }
        let direction = self.view.direction();
        // Support of the floor/object AABB relative to the orbit target. Keep
        // its nearest point at least 0.75 world units in front of the camera.
        // Pitch stays positive, so the AABB's upper Y is the object's top.
        let nearest = LAB_FLOOR_HALF_SIZE * (direction.x.abs() + direction.z.abs())
            + (LAB_OBJECT_HALF_SIZE - self.view.target_y) * direction.y
            - self.view.target_x * direction.x
            + fix!(0.75);
        if self.view.distance.less_than(nearest) {
            self.view.distance = nearest;
        }
        // Panning cannot move the eye through the floor at y=-1.1.
        let minimum_y = LAB_FLOOR_Y + fix!(0.35) - direction.y * self.view.distance;
        if self.view.target_y.less_than(minimum_y) {
            self.view.target_y = minimum_y;
        }
    }
}

pub fn camera_labels(grid: &mut UiGrid, state: CameraTab) {
    lab_option(grid, 5, state.cursor == 0, b"DISTANCE");
    lab_distance(grid, 5, state.view.distance);
    lab_option(grid, 7, state.cursor == 1, b"FOV DEG");
    lab_value(grid, 7, state.view.fov);
    text(grid, 3, 9, b"LS ORBIT");
    text(grid, 3, 11, b"RS PAN");
    hint_at(grid, 1, 12, KEY_LS, ICON_ADJUST);
    hint_at(grid, 8, 12, KEY_RS, ICON_ADJUST);
    hint_at(grid, 17, 12, KEY_DPAD_H, ICON_ADJUST);
}

fn camera_fixed_bits(grid: &mut UiGrid, row: u16, value: fix32) {
    text_hex16(grid, 15, row, value.hi_bits());
    text_hex16(grid, 19, row, value.lo_bits());
}

/// Raw halves preserve the exact signed Q16.16 values used by this frame.
pub fn camera_parameters(grid: &mut UiGrid, view: LabView) {
    text(grid, 3, 4, b"CAMERA HEX Q16.16");
    text(grid, 3, 5, b"YAW RAD");
    camera_fixed_bits(grid, 5, view.yaw);
    text(grid, 3, 6, b"PITCH RAD");
    camera_fixed_bits(grid, 6, view.pitch);
    text(grid, 3, 7, b"DISTANCE");
    camera_fixed_bits(grid, 7, view.distance);
    text(grid, 3, 8, b"TARGET X");
    camera_fixed_bits(grid, 8, view.target_x);
    text(grid, 3, 9, b"TARGET Y");
    camera_fixed_bits(grid, 9, view.target_y);
    text(grid, 3, 10, b"FOV DEG");
    text_u16_field(grid, 15, 10, 3, view.fov);
}
