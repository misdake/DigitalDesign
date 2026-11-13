//! Small application menu helpers; no renderer or input ownership.
use crate::dsl_rt::*;
mod gpu_api;
use crate::common::state::*;
use crate::common::ui::*;
use gpu_api::*;
mod ui_text;
use ui_text::*;

pub fn lab_clamp(value: fix32, minimum: fix32, maximum: fix32) -> fix32 {
    if value.less_than(minimum) {
        return minimum;
    }
    if value.greater_than(maximum) {
        return maximum;
    }
    value
}
pub fn lab_cursor(cursor: u16, pressed: u16, count: u16) -> u16 {
    let mut next = cursor;
    if pressed & BUTTON_DOWN != 0 {
        next = (next + 1) % count;
    }
    if pressed & BUTTON_UP != 0 {
        next = (next + count - 1) % count;
    }
    next
}
pub fn lab_adjust(value: u16, pressed: u16, step: u16, maximum: u16) -> u16 {
    let mut next = value;
    if pressed & BUTTON_RIGHT != 0 {
        next = if maximum - next < step {
            maximum
        } else {
            next + step
        };
    }
    if pressed & BUTTON_LEFT != 0 {
        next = if next < step { 0 } else { next - step };
    }
    next
}
pub fn lab_option(grid: &mut UiGrid, row: u16, selected: bool, label: &[u8]) {
    if selected {
        grid.write_glyph(1, row, 62);
    }
    text(grid, 3, row, label);
}
pub fn lab_value(grid: &mut UiGrid, row: u16, value: u16) {
    text_u16_field(grid, 15, row, 3, value);
}
pub fn lab_distance(grid: &mut UiGrid, row: u16, value: fix32) {
    let tenths = (value * fix!(10)).round().to_int() as u16;
    let whole = tenths / 10;
    text_u16_field(grid, 15, row, 2, whole);
    let decimal = if whole < 10 { 16u16 } else { 17u16 };
    grid.write_glyph(decimal, row, 46);
    grid.write_glyph(decimal + 1, row, (48 + tenths % 10) as u8);
}
