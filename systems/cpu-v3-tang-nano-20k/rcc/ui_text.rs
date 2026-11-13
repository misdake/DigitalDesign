//! Stateless ASCII decimal/hex formatting over the public GPU UI API.
//! The caller owns layout, glyph coverage, clearing and synchronization.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;

pub fn text_u16(grid: &mut UiGrid, x: u16, y: u16, value: u16) {
    let mut divisor: u16 = 10000;
    let mut column = x;
    let mut started = false;
    while divisor != 0 {
        let digit = (value / divisor) % 10;
        if digit != 0 || started || divisor == 1 {
            grid.write_glyph(column, y, (48 + digit) as u8);
            column += 1;
            started = true;
        }
        divisor /= 10;
    }
}
/// Left-aligned decimal field, truncating at width and clearing trailing glyphs.
pub fn text_u16_field(grid: &mut UiGrid, x: u16, y: u16, width: u16, value: u16) {
    let mut divisor: u16 = 1;
    while value / divisor >= 10 {
        divisor *= 10;
    }
    let mut column: u16 = 0;
    while column < width {
        grid.write_glyph(x + column, y, (48 + (value / divisor) % 10) as u8);
        column += 1;
        if divisor == 1 {
            break;
        }
        divisor /= 10;
    }
    while column < width {
        grid.write_glyph(x + column, y, 255);
        column += 1;
    }
}
pub fn text_i16(grid: &mut UiGrid, x: u16, y: u16, value: i16) {
    if value < 0 {
        grid.write_glyph(x, y, 45);
        text_u16(grid, x + 1, y, 0u16 - value as u16);
    } else {
        text_u16(grid, x, y, value as u16);
    }
}
pub fn text_hex16(grid: &mut UiGrid, x: u16, y: u16, value: u16) {
    let mut i: u16 = 0;
    while i < 4 {
        let digit = (value >> ((3 - i) * 4)) & 15;
        let mut glyph = 48 + digit;
        if digit >= 10 {
            glyph = 65 + digit - 10;
        }
        grid.write_glyph(x + i, y, glyph as u8);
        i += 1;
    }
}
