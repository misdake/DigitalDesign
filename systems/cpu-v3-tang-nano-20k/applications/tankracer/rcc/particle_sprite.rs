//! Shared 44-byte sprite: eight i32 values, UV/RGB bytes and a u16 rotation.
use crate::dsl_rt::*;

// Words 0..15: material, position XYZ, flags, depth bias and size XY.
// Words 16/17: UV; 18/19: RGB; 20: rotation; 21: reserved zero.
pub fn uv(data: Array<u16>, index: u16) -> u16 {
    let value = data[16 + (index >> 1)];
    if index & 1 == 0 {
        return value & 255;
    }
    value >> 8
}
pub fn put_uv(mut data: Array<u16>, index: u16, value: u16) {
    let at = 16 + (index >> 1);
    if index & 1 == 0 {
        data[at] = (data[at] & 65280) | (value & 255);
    } else {
        data[at] = (data[at] & 255) | ((value & 255) << 8);
    }
}
pub fn color(data: Array<u16>, index: u16) -> u16 {
    let value = data[18 + (index >> 1)];
    if index & 1 == 0 {
        return value & 255;
    }
    value >> 8
}
pub fn put_color(mut data: Array<u16>, index: u16, value: u16) {
    let at = 18 + (index >> 1);
    if index & 1 == 0 {
        data[at] = (data[at] & 65280) | (value & 255);
    } else {
        data[at] = (data[at] & 255) | ((value & 255) << 8);
    }
}
pub fn rgb(mut data: Array<u16>, r: u16, g: u16, b: u16) {
    data[18u16] = (r & 255) | ((g & 255) << 8);
    data[19u16] = (data[19u16] & 65280) | (b & 255);
}
pub fn rectangle(mut data: Array<u16>, a: u16, b: u16, c: u16, d: u16) {
    data[16u16] = (a & 255) | ((b & 255) << 8);
    data[17u16] = (c & 255) | ((d & 255) << 8);
}
