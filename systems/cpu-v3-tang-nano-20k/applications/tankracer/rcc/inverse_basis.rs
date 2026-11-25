//! Original inverse Q12 basis lookup with transactional angle outputs.
use crate::dsl_rt::*;
use crate::{attitude, integer, trig_lookup};

fn n(value: i16) -> fix32 {
    integer::from_i16(value)
}
fn magnitude(value: fix32) -> fix32 {
    if value < n(0) {
        return -value;
    }
    value
}
fn cosine_angle(table: Array<i16>, value: fix32) -> fix32 {
    let index = magnitude(value).lo_bits();
    if index == 4096 {
        return n(0);
    }
    trig_lookup::index(table, index)
}
fn division_valid(numerator: fix32, denominator: fix32) -> bool {
    denominator != n(0) && (numerator != fix32::from_words(0, 32768) || denominator != n(-1))
}

// On success out contains pitch, yaw, roll and cosine(pitch) != 0 as four i32s.
// On arithmetic failure it is unchanged. Tables are the exact source constants.
pub fn angles(
    out: Array<u16>,
    matrix: Array<u16>,
    sine: Array<i16>,
    inverse_cosine: Array<i16>,
) -> bool {
    let v = attitude::get(matrix, 7);
    if v < n(-4096) || v > n(4096) {
        return false;
    }
    let a = cosine_angle(inverse_cosine, v);
    let mut pitch = n(16384) - a;
    if v < n(0) {
        pitch = a - n(16384);
    }
    let cosine = trig_lookup::index(sine, 4096 + ((pitch.lo_bits() & 65532) >> 2));
    let values: Buf<u16, 8> = Buf::new([0; 8]);
    let pending = values.as_array();
    attitude::put(pending, 0, pitch);
    if cosine != n(0) {
        attitude::put(pending, 3, n(1));
        let mut slot = 1u16;
        while slot < 3 {
            let mut component = 8u16;
            let mut sign = 6u16;
            if slot == 2 {
                component = 4;
                sign = 1;
            }
            let numerator = integer::shl(attitude::get(matrix, component), 12);
            if !division_valid(numerator, cosine) {
                return false;
            }
            let mut q = integer::div(numerator, cosine);
            if q < n(-4096) {
                q = n(-4096);
            }
            if q > n(4096) {
                q = n(4096);
            }
            let mut angle = cosine_angle(inverse_cosine, q);
            if q < n(0) {
                angle = fix32::from_words(32768, 0) - angle;
            }
            let numerator = integer::shl(attitude::get(matrix, sign), 12);
            if !division_valid(numerator, cosine) {
                return false;
            }
            let side = integer::div(numerator, cosine);
            if -side < n(0) {
                angle = fix32::from_words(0, 1) - angle;
            }
            attitude::put(pending, slot, angle);
            slot += 1;
        }
    }
    let mut i = 0u16;
    while i < 4 {
        attitude::put(out, i, attitude::get(pending, i));
        i += 1;
    }
    true
}
