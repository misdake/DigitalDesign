//! Independent geometry, base-color source and lighting choices.
use crate::dsl_rt::*;
mod gpu_api;
use crate::common::state::*;
use crate::common::ui::*;
use gpu_api::*;
mod boot_resources;
use crate::pages::lab::lab_ui::*;
use boot_resources::*;

#[derive(Copy, Clone)]
pub struct SurfaceTab {
    pub cursor: u16,
    pub geometry: u16,
    pub color: u16,
    pub lit: bool,
}
impl SurfaceTab {
    pub const fn new() -> Self {
        Self {
            cursor: 0,
            geometry: 2,
            color: 2,
            lit: true,
        }
    }
    pub fn update(&mut self, pressed: u16) {
        self.cursor = lab_cursor(self.cursor, pressed, 3);
        let mut step = 0;
        if pressed & (BUTTON_RIGHT | BUTTON_CROSS) != 0 {
            step = 1;
        }
        if pressed & BUTTON_LEFT != 0 {
            step = 3;
        }
        if step != 0 {
            if self.cursor == 0 {
                self.geometry = (self.geometry + step) % 4;
            } else if self.cursor == 1 {
                let color_step = if step == 3 { 2 } else { 1 };
                self.color = (self.color + color_step) % 3;
            } else {
                self.lit = !self.lit;
            }
        }
    }
}

pub fn surface_labels(grid: &mut UiGrid, state: SurfaceTab) {
    lab_option(grid, 5, state.cursor == 0, b"GEOMETRY");
    if state.geometry == 0 {
        text(grid, 15, 5, b"QUAD");
    } else if state.geometry == 1 {
        text(grid, 15, 5, b"CUBE");
    } else if state.geometry == 2 {
        text(grid, 15, 5, b"SPHERE");
    } else {
        text(grid, 15, 5, b"TORUS");
    }
    lab_option(grid, 7, state.cursor == 1, b"COLOR");
    if state.color == 0 {
        text(grid, 15, 7, b"VERTEX RGB");
    } else if state.color == 1 {
        text(grid, 15, 7, b"TEXTURE");
    } else {
        text(grid, 15, 7, b"UNIFORM");
    }
    lab_option(grid, 9, state.cursor == 2, b"LIGHTING");
    if state.lit {
        text(grid, 15, 9, b"LIT");
    } else {
        text(grid, 15, 9, b"UNLIT");
    }
    hint_at(grid, 1, 12, KEY_DPAD_V, ICON_SELECT);
    hint_at(grid, 8, 12, KEY_DPAD_H, ICON_ADJUST);
}
