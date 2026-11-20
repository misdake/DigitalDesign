//! App-owned layout and corner controls; the picture is the main content.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
mod boot_resources;
use crate::common::state::*;
use boot_resources::*;

pub fn text(grid: &mut UiGrid, x: u16, y: u16, value: &[u8]) {
    grid.write_text(x, y, value);
}
pub fn hint(grid: &mut UiGrid, x: u16, key: u8, action: u8) {
    hint_at(grid, x, 14, key, action);
}
pub fn hint_at(grid: &mut UiGrid, x: u16, y: u16, key: u8, action: u8) {
    let mut cell: u16 = 0;
    while cell < 3 {
        grid.write_glyph(x + cell, y, key + cell as u8);
        cell += 1;
    }
    grid.write_glyph(x + 4, y, action);
}
pub fn page_start(grid: &mut UiGrid, page: PageId, paused: bool, title: &[u8]) {
    grid.fill(255);
    text(grid, 2, 1, title);
    hint(grid, 1, KEY_L1, ICON_PREVIOUS);
    hint(grid, 8, KEY_R1, ICON_NEXT);
    if page == PageId::Home {
        hint(grid, 38, KEY_DPAD_V, ICON_SELECT);
        hint(grid, 44, KEY_CROSS, ICON_ACCEPT);
    } else {
        hint(grid, 44, KEY_START, ICON_HOME);
        if page.is_animated() {
            hint(grid, 26, KEY_DPAD_H, ICON_ADJUST);
            if paused {
                hint(grid, 32, KEY_CROSS, ICON_PLAY);
            } else {
                hint(grid, 32, KEY_CROSS, ICON_PAUSE);
            }
            hint(grid, 38, KEY_TRIANGLE, ICON_RESET);
        } else if page == PageId::Color {
            hint(grid, 32, KEY_DPAD_V, ICON_SELECT);
            hint(grid, 38, KEY_CROSS, ICON_ACCEPT);
        }
    }
}
pub fn menu_entry(grid: &mut UiGrid, index: u16, selected: u16, label: &[u8]) {
    if index == selected {
        grid.write_glyph(3, 3 + index, 62);
    }
    text(grid, 6, 3 + index, label);
}
