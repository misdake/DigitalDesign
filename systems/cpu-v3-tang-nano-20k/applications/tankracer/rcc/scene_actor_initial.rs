//! Authored pickup setup and pose construction over caller-owned state.
use crate::dsl_rt::*;
use crate::{attitude, integer, scene_initial, trig_lookup};
fn n(value: i16) -> fix32 {
    integer::from_i16(value)
}
fn g(data: Array<u16>, i: u16) -> fix32 {
    attitude::get(data, i)
}
fn p(data: Array<u16>, i: u16, value: fix32) {
    attitude::put(data, i, value);
}

// Input i32 columns: authored pose12, ground, model kind, range-present/range,
// CUP extra range, Circuit extra, increment-lap, aspect-present/aspect3,
// model height, parachute height, model radius, range-value-present.
// Shared setup: map, CUP, tilt byte, lap byte, separation, diameter.
// Output: pose12, powerup flight14/parachute12, kind, phase, added lap,
// mass, restitution2 and auxiliary6 (50 i32 columns).
pub fn pickup(
    out: Array<u16>,
    data: Array<u16>,
    setup: Array<u16>,
    sine: Array<i16>,
    tilts: Array<i16>,
) {
    let mut column = 0u16;
    while column < 50 {
        p(out, column, n(0));
        column += 1;
    }
    column = 0;
    while column < 3 {
        p(out, 9 + column, g(data, 9 + column));
        column += 1;
    }
    p(out, 10, g(out, 10) + n(20480));
    let mut kind = g(data, 13);
    let cup = g(setup, 1);
    if kind == n(9) || (kind == n(2) && cup == n(0)) || (kind == n(10) && cup != n(2)) {
        kind = n(0);
    }
    if g(setup, 4) == n(0) {
        p(setup, 4, integer::mul(g(data, 23) + g(data, 24), n(8)));
    }
    if g(setup, 5) == n(0) {
        p(setup, 5, integer::mul(g(data, 25), n(2)));
    }
    let extra = g(data, 16);
    let mut range = n(130) + extra;
    if g(setup, 0) == n(7) {
        range = range + g(data, 17);
    }
    range = integer::shl(range, 10);
    if g(data, 14) != n(0) {
        let mut value = range;
        if g(data, 26) != n(0) {
            value = g(data, 15);
        }
        range = integer::shl(value, 10) + integer::shl(extra, 10);
    }
    let tilt = g(setup, 2).lo_bits();
    let byte = trig_lookup::index(tilts, tilt).lo_bits();
    let angle = ((byte & 3) + 8) * 182;
    scene_initial::basis(out, angle, 0, angle, sine);
    p(setup, 2, fix32::from_words((tilt + 1) % 31, 0));
    if g(data, 19) != n(0) {
        scene_initial::basis(
            out,
            g(data, 20).lo_bits(),
            g(data, 21).lo_bits(),
            g(data, 22).lo_bits(),
            sine,
        );
    }
    let phase = g(setup, 3).lo_bits();
    if g(data, 18) != n(0) {
        p(setup, 3, fix32::from_words((phase + 1) & 255, 0));
    }
    p(out, 23, g(data, 12));
    p(out, 24, range);
    p(out, 25, n(-1));
    p(out, 38, kind);
    p(out, 39, fix32::from_words(phase, 0));
    if phase == 2 || phase == 3 {
        p(out, 40, fix32::from_words(phase - 1, 0));
    }
    p(out, 41, n(75));
    p(out, 42, fix32::from_words(500, 10));
    p(out, 43, fix32::from_words(800, 10));
    let auxiliary: Buf<u16, 6> = Buf::new([150, 8, 1, 10, 5, 18]);
    column = 0;
    while column < 6 {
        p(out, 44 + column, fix32::from_words(auxiliary[column], 0));
        column += 1;
    }
}
