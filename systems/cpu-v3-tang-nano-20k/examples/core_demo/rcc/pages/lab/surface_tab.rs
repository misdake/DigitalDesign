//! Four real shader paths over one plane, selected through radio options.
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
    pub mode: u16,
}
impl SurfaceTab {
    pub const fn new() -> Self {
        Self { cursor: 0, mode: 0 }
    }
    pub fn update(&mut self, pressed: u16) {
        self.cursor = lab_cursor(self.cursor, pressed, 4);
        if pressed & BUTTON_CROSS != 0 {
            self.mode = self.cursor;
        }
        if pressed & BUTTON_TRIANGLE != 0 {
            *self = Self::new();
        }
    }
}

pub fn surface_labels(grid: &mut UiGrid, state: SurfaceTab) {
    lab_option(grid, 5, state.cursor == 0, b"RGB");
    lab_option(grid, 7, state.cursor == 1, b"TEXTURE");
    lab_option(grid, 9, state.cursor == 2, b"LIT");
    lab_option(grid, 11, state.cursor == 3, b"TEXTURE+LIT");
    let mut option: u16 = 0;
    while option < 4 {
        grid.write_glyph(16, 5 + option * 2, ICON_RADIO_OFF);
        option += 1;
    }
    grid.write_glyph(16, 5 + state.mode * 2, ICON_RADIO_ON);
    hint_at(grid, 1, 12, KEY_DPAD_V, ICON_SELECT);
    hint_at(grid, 8, 12, KEY_CROSS, ICON_ACCEPT);
}
