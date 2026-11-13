//! App-owned layout and corner controls; the picture is the main content.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
mod boot_resources;
use boot_resources::*;
mod state;
use state::*;

pub fn text(grid: &mut UiGrid, x: u16, y: u16, value: &[u8]) {
    grid.write_text(x, y, value);
}
fn hint(grid: &mut UiGrid, x: u16, key: u8, action: u8) {
    let mut cell: u16 = 0;
    while cell < 3 {
        grid.write_glyph(x + cell, 14, key + cell as u8);
        cell += 1;
    }
    grid.write_glyph(x + 4, 14, action);
}
pub fn page_start(grid: &mut UiGrid, state: DemoState, title: &[u8]) {
    grid.fill(255);
    text(grid, 2, 1, title);
    hint(grid, 1, KEY_L1, ICON_PREVIOUS);
    hint(grid, 8, KEY_R1, ICON_NEXT);
    if state.page == 0 {
        hint(grid, 38, KEY_DPAD_V, ICON_SELECT);
        hint(grid, 44, KEY_CROSS, ICON_ACCEPT);
    } else {
        hint(grid, 44, KEY_START, ICON_HOME);
        if state.page == 1 || state.page == 2 || state.page == 5 {
            hint(grid, 26, KEY_DPAD_H, ICON_ADJUST);
            if state.paused {
                hint(grid, 32, KEY_CROSS, ICON_PLAY);
            } else {
                hint(grid, 32, KEY_CROSS, ICON_PAUSE);
            }
            hint(grid, 38, KEY_TRIANGLE, ICON_RESET);
        } else if state.page == 4 {
            hint(grid, 32, KEY_DPAD_V, ICON_SELECT);
            hint(grid, 38, KEY_CROSS, ICON_ACCEPT);
        }
    }
}
pub fn menu_entry(grid: &mut UiGrid, index: u16, selected: u16, label: &[u8]) {
    if index == selected {
        grid.write_glyph(3, 2 + index * 2, 62);
    }
    text(grid, 6, 2 + index * 2, label);
}
