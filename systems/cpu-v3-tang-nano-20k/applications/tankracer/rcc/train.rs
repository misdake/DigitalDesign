//! First-map Train path motion on live travel and endpoint columns.
use crate::dsl_rt::*;
use crate::{attitude, collision, integer, random};

pub struct State {
    // active, running, waiting, wait; remaining original Travel fields follow.
    pub travel: Array<u16>,
    // start XYZ, end XYZ, velocity XYZ, all mutable across a reversal.
    pub path: Array<u16>,
    pub position: Array<u16>,
    pub basis: Array<u16>,
    pub speed: fix32,
    pub range: fix32,
    pub sound: bool,
    pub seed: Array<u16>,
    pub sine: Array<i16>,
}
pub struct Frame {
    pub player: Array<u16>,
    pub entity: fix32,
    pub number: fix32,
    pub dt: fix32,
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
// Decoded endpoints preserve authored presence: without :start_pos the object
// retains its world-file position. Only X/Z velocity receives the speed gain.
pub fn initialize(state: &State, start: Array<u16>, end: Array<u16>, has_start: bool) {
    let mut travel = state.travel;
    let mut word = 0u16;
    while word < 38 {
        travel[word] = 0;
        word += 1;
    }
    let mut axis = 0u16;
    while axis < 3 {
        let first = g(start, axis);
        let last = g(end, axis);
        p(state.path, axis, first);
        p(state.path, 3 + axis, last);
        p(state.path, 6 + axis, last - first);
        if has_start {
            p(state.position, axis, first);
        }
        axis += 1;
    }
    collision::normalize(at(state.path, 12));
    axis = 0;
    while axis < 3 {
        p(
            state.path,
            6 + axis,
            integer::mul(g(state.path, 6 + axis), state.speed),
        );
        axis += 2;
    }
}
pub fn tick(state: &State, frame: &Frame) -> fix32 {
    if g(state.travel, 2) != n(0) {
        if g(state.travel, 3) <= frame.dt {
            p(state.travel, 2, n(0));
            p(state.travel, 1, n(0));
        } else {
            p(state.travel, 3, g(state.travel, 3) - frame.dt);
        }
        return n(0);
    }
    if g(state.travel, 1) == n(0) && (frame.entity.lo_bits() ^ frame.number.lo_bits()) & 7 != 0 {
        return n(0);
    }
    let mut near = true;
    let mut axis = 0u16;
    while axis < 3 {
        let mut delta = g(frame.player, axis) - g(state.position, axis);
        if delta < n(0) {
            delta = -delta;
        }
        if delta >= state.range {
            near = false;
        }
        axis += 2;
    }
    if g(state.travel, 1) == n(0) {
        if near {
            p(state.travel, 0, n(1));
            p(state.travel, 1, n(1));
            if state.sound {
                p(state.travel, 11, n(0));
            }
        }
        return n(0);
    }
    if !near {
        p(state.travel, 0, n(0));
        p(state.travel, 1, n(0));
        return n(0);
    }
    axis = 0;
    while axis < 3 {
        p(
            state.position,
            axis,
            g(state.position, axis)
                + integer::sar(integer::mul(frame.dt, g(state.path, 6 + axis)), 8),
        );
        axis += 2;
    }
    let mut arrived = false;
    axis = 0;
    while axis < 3 {
        let velocity = g(state.path, 6 + axis);
        let position = g(state.position, axis);
        let end = g(state.path, 3 + axis);
        if (velocity > n(0) && position >= end) || (velocity < n(0) && position <= end) {
            arrived = true;
        }
        axis += 2;
    }
    if arrived {
        let seed = random::next(g(state.seed, 0));
        p(state.seed, 0, seed);
        if seed.lo_bits() & 16 != 0 {
            axis = 0;
            while axis < 3 {
                p(state.position, axis, g(state.path, axis));
                axis += 1;
            }
        } else {
            axis = 0;
            while axis < 3 {
                let start = g(state.path, axis);
                p(state.path, axis, g(state.path, 3 + axis));
                p(state.path, 3 + axis, start);
                p(state.path, 6 + axis, start - g(state.position, axis));
                axis += 1;
            }
            collision::normalize(at(state.path, 12));
            axis = 0;
            while axis < 3 {
                p(
                    state.path,
                    6 + axis,
                    integer::mul(g(state.path, 6 + axis), state.speed),
                );
                axis += 1;
            }
            attitude::rotate(state.basis, 0, 1, n(32760), state.sine);
            if !attitude::normalize(state.basis, 0) {
                return n(-2);
            }
        }
        p(state.travel, 3, n(5000));
        p(state.travel, 2, n(1));
        p(state.travel, 0, n(0));
    }
    n(0)
}
