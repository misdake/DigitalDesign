//! Uniform color and ambient/diffuse/specular response belong to the material.
use crate::dsl_rt::*;
mod gpu_api;
use crate::common::state::*;
use crate::common::ui::*;
use gpu_api::*;
mod boot_resources;
use crate::pages::lab::lab_scene::*;
use crate::pages::lab::lab_ui::*;
use boot_resources::*;

#[derive(Copy, Clone)]
pub struct MaterialTab {
    pub material: LabMaterial,
    pub cursor: u16,
}
impl MaterialTab {
    pub const fn new() -> Self {
        Self {
            material: LabMaterial::new(),
            cursor: 0,
        }
    }
    pub fn update(&mut self, pressed: u16) {
        self.cursor = lab_cursor(self.cursor, pressed, 7);
        if self.cursor == 0 {
            self.material.red = lab_adjust(self.material.red, pressed, 16, 255);
        } else if self.cursor == 1 {
            self.material.green = lab_adjust(self.material.green, pressed, 16, 255);
        } else if self.cursor == 2 {
            self.material.blue = lab_adjust(self.material.blue, pressed, 16, 255);
        } else if self.cursor == 3 {
            self.material.ambient = lab_adjust(self.material.ambient, pressed, 10, 100);
        } else if self.cursor == 4 {
            self.material.diffuse = lab_adjust(self.material.diffuse, pressed, 10, 100);
        } else if self.cursor == 5 {
            self.material.specular = lab_adjust(self.material.specular, pressed, 16, 255);
        } else {
            self.material.gloss = lab_adjust(self.material.gloss, pressed, 1, 4);
        }
        if pressed & BUTTON_TRIANGLE != 0 {
            *self = Self::new();
        }
    }
}

pub fn material_labels(grid: &mut UiGrid, state: MaterialTab) {
    lab_option(grid, 5, state.cursor == 0, b"RED");
    lab_value(grid, 5, state.material.red);
    lab_option(grid, 6, state.cursor == 1, b"GREEN");
    lab_value(grid, 6, state.material.green);
    lab_option(grid, 7, state.cursor == 2, b"BLUE");
    lab_value(grid, 7, state.material.blue);
    lab_option(grid, 8, state.cursor == 3, b"AMBIENT");
    lab_value(grid, 8, state.material.ambient);
    lab_option(grid, 9, state.cursor == 4, b"DIFFUSE");
    lab_value(grid, 9, state.material.diffuse);
    lab_option(grid, 10, state.cursor == 5, b"SPECULAR");
    lab_value(grid, 10, state.material.specular);
    lab_option(grid, 11, state.cursor == 6, b"SHININESS");
    lab_value(grid, 11, 4u16 << state.material.gloss);
    hint_at(grid, 1, 12, KEY_DPAD_V, ICON_SELECT);
    hint_at(grid, 8, 12, KEY_DPAD_H, ICON_ADJUST);
}
