//! Authored camera/BOX triggers and one-draw emitter launch arithmetic.
use crate::dsl_rt::*;
use crate::{attitude, integer, projectile};

pub struct Frame {
    pub camera: Array<u16>,
    pub player: Array<u16>,
    pub player_present: bool,
    pub position: Array<u16>,
    pub basis: Array<u16>,
    pub dt: fix32,
    pub seed: Array<u16>,
    pub sine: Array<i16>,
}
fn n(value: i16) -> fix32 {
    integer::from_i16(value)
}
fn g(values: Array<u16>, index: u16) -> fix32 {
    attitude::get(values, index)
}
fn p(values: Array<u16>, index: u16, value: fix32) {
    attitude::put(values, index, value);
}
fn at(values: Array<u16>, word: u16) -> Array<u16> {
    unsafe { values.as_ptr().add(word as i16).as_u16_array() }
}

// The constructor draws this timer before preparing its authored parameters.
pub fn initialize_timer(data: Array<u16>, seed: Array<u16>) {
    p(
        data,
        0,
        fix32::from_words(projectile::random_word(seed) & 1023, 0),
    );
}

// 25 i32: timer, gun, size, offset3, pitch, trigger, bounds4, period,
// RGB3, alpha, glow3, asteroid, optional mesh flag/ID, optional material flag/ID.
// Output: position3, basis9, velocity3, flags. It is private unless true returns.
pub fn tick(data: Array<u16>, frame: &Frame, out: Array<u16>) -> bool {
    let trigger = g(data, 7);
    let mut active = false;
    if trigger == n(0) {
        active = true;
        let mut axis = 0u16;
        while axis < 3 {
            let mut delta = integer::sar(g(frame.position, axis), 3) - g(frame.camera, axis);
            if delta < n(0) {
                delta = -delta;
            }
            if delta >= n(19200) {
                active = false;
            }
            axis += 2;
        }
        if !active {
            p(data, 0, n(0));
        }
    } else if trigger == n(1) && frame.player_present {
        active = g(data, 8) < g(frame.player, 0)
            && g(frame.player, 0) < g(data, 10)
            && g(data, 9) < g(frame.player, 2)
            && g(frame.player, 2) < g(data, 11);
        if !active {
            p(data, 0, n(0));
        }
    }
    if !active {
        return false;
    }
    if g(data, 0) >= frame.dt {
        p(data, 0, g(data, 0) - frame.dt);
        return false;
    }
    let wait = projectile::random_word(frame.seed) & 2047;
    p(data, 0, g(data, 12) + fix32::from_words(wait, 0));
    let mut basis = at(out, 6);
    let gun = g(data, 1) != n(0);
    if gun {
        let mut i = 0u16;
        while i < 18 {
            basis[i] = frame.basis[i];
            i += 1;
        }
        attitude::rotate(basis, 0, 0, g(data, 6), frame.sine);
    } else {
        projectile::basis(basis, n(-16380), n(0), frame.sine);
    }
    let mut axis = 0u16;
    while axis < 3 {
        p(out, axis, g(frame.position, axis) + g(data, 3 + axis));
        let mut velocity = n(0);
        if gun {
            velocity = integer::sar(integer::mul(g(basis, 6 + axis), n(5242)), 12);
        } else if axis == 0 {
            velocity = n(4194);
        }
        p(out, 12 + axis, velocity);
        axis += 1;
    }
    let mut flags = n(0);
    if !gun && g(data, 20) != n(0) {
        flags = n(16);
    }
    p(out, 15, flags);
    true
}
