//! Sixteen chimney particles driven by the scene-wide ten-value sequence.
use crate::dsl_rt::*;
use crate::{attitude, integer, particle_sprite};

pub struct State {
    // white, billow, timer, visible, material: five i32 columns.
    pub header: Array<u16>,
    // Sixteen rows: spin, velocity3, size2, rotation, position3,
    // active, clockwise, life. Narrow values are stored zero-extended.
    pub particles: Array<u16>,
    pub sequence: Array<u16>,
}
static VALUES: Buf<u16, 10> = Buf::new([6, 8, 3, 5, 9, 1, 4, 2, 0, 7]);
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
fn next(mut sequence: Array<u16>) -> fix32 {
    let cursor = sequence[0u16];
    let value = VALUES[cursor];
    let mut following = cursor + 1;
    if cursor == 9 {
        following = 0;
    }
    sequence[0u16] = following;
    fix32::from_words(value, 0)
}
pub fn near(position: Array<u16>, camera: Array<u16>, threshold: fix32) -> bool {
    let mut axis = 0u16;
    while axis < 3 {
        let mut delta = integer::sar(g(position, axis), 3) - g(camera, axis);
        if delta < n(0) {
            delta = -delta;
        }
        if delta >= threshold {
            return false;
        }
        axis += 2;
    }
    true
}
pub fn tick(state: &State, position: Array<u16>, camera: Array<u16>, dt: fix32) {
    let visible = near(position, camera, n(19200));
    let mut flag = n(0);
    if visible {
        flag = n(1);
    }
    p(state.header, 3, flag);
    if !visible {
        return;
    }
    let delta = integer::sar(dt, 4);
    let mut available = 65535u16;
    let mut index = 0u16;
    while index < 16 {
        let row = at(state.particles, index * 26);
        if g(row, 10) != n(0) {
            if g(row, 12) < delta {
                p(row, 10, n(0));
            } else {
                p(
                    row,
                    12,
                    fix32::from_words((g(row, 12) - delta).lo_bits() & 255, 0),
                );
                let mut spin = integer::mul(g(row, 0), dt);
                if g(row, 11) == n(0) {
                    spin = -spin;
                }
                p(row, 6, fix32::from_words((g(row, 6) + spin).lo_bits(), 0));
                let mut axis = 0u16;
                while axis < 2 {
                    p(
                        row,
                        4 + axis,
                        g(row, 4 + axis) + next(state.sequence) + integer::sar(dt, 2),
                    );
                    axis += 1;
                }
                axis = 0;
                while axis < 3 {
                    let mut noise = next(state.sequence);
                    if axis != 1 {
                        noise = -noise;
                    }
                    p(
                        row,
                        7 + axis,
                        g(row, 7 + axis) + integer::mul(g(row, 1 + axis), dt) + noise,
                    );
                    axis += 1;
                }
                index += 1;
                continue;
            }
        }
        available = index;
        index += 1;
    }
    if g(state.header, 2) < dt {
        if available != 65535 {
            let row = at(state.particles, available * 26);
            let mut axis = 0u16;
            while axis < 3 {
                p(
                    row,
                    7 + axis,
                    g(position, axis) + integer::mul(next(state.sequence), n(128)) - n(576),
                );
                axis += 1;
            }
            p(row, 1, n(-1));
            let mut lift = n(2);
            if g(state.header, 1) != n(0) {
                lift = integer::sar(next(state.sequence), 2);
            }
            p(row, 2, lift);
            p(row, 3, n(-1));
            p(row, 10, n(1));
            p(
                row,
                11,
                fix32::from_words(next(state.sequence).lo_bits() & 1, 0),
            );
            p(row, 0, integer::shl(next(state.sequence), 1));
            p(
                row,
                6,
                fix32::from_words(integer::shl(next(state.sequence), 11).lo_bits(), 0),
            );
            p(row, 12, integer::mul(next(state.sequence) + n(55), n(2)));
            p(row, 4, integer::mul(next(state.sequence) + n(25), n(16)));
            p(row, 5, integer::mul(next(state.sequence) + n(25), n(16)));
            p(state.header, 2, next(state.sequence) + n(150));
        }
    } else {
        p(state.header, 2, g(state.header, 2) - dt);
    }
}
// Rendering derives sprites without advancing particles or either random stream.
pub fn sprites(state: &State, output: Array<u16>) -> u16 {
    let mut count = 0u16;
    if g(state.header, 3) == n(0) {
        return count;
    }
    let mut index = 0u16;
    while index < 16 {
        let particle = at(state.particles, index * 26);
        if g(particle, 10) != n(0) {
            let row = at(output, count * 22);
            let mut data = row;
            let mut i = 0u16;
            while i < 22 {
                data[i] = 0;
                i += 1;
            }
            p(row, 0, g(state.header, 4));
            i = 0;
            while i < 3 {
                p(row, 1 + i, g(particle, 7 + i));
                i += 1;
            }
            p(row, 4, g(state.header, 0));
            p(row, 6, g(particle, 4));
            p(row, 7, g(particle, 5));
            particle_sprite::rectangle(row, 1, 1, 63, 63);
            let life = g(particle, 12).lo_bits();
            particle_sprite::rgb(row, life, life, life);
            data[20u16] = g(particle, 6).lo_bits();
            count += 1;
        }
        index += 1;
    }
    count
}
