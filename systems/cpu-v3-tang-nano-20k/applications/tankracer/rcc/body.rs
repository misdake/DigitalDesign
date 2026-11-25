//! Recoil/pitch, player lean and the four-contact physical hull.
use crate::dsl_rt::*;
use crate::{attitude, integer};
fn n(v: i16) -> fix32 {
    integer::from_i16(v)
}
fn g(v: Array<u16>, i: u16) -> fix32 {
    attitude::get(v, i)
}
fn p(v: Array<u16>, i: u16, x: fix32) {
    attitude::put(v, i, x);
}
fn clamp(v: fix32, low: fix32, high: fix32) -> fix32 {
    if v < low {
        return low;
    }
    if v > high {
        return high;
    }
    v
}
// State: recoil/rate/pitch/rate. Tuning: recoil decay/step, pitch damping
// step/fraction/shift, lean decay, dt. Binding: present, vertex count, four
// vertex indices, four original signed Z values. Errors keep the written prefix.
pub fn articulate(
    state: Array<u16>,
    config: Array<u16>,
    mut mesh: Array<i16>,
    binding: Array<u16>,
) -> u16 {
    if binding[0u16] == 0 {
        return 0;
    }
    let dt = g(config, 6);
    let recoil = g(state, 0);
    let mut shift = 14u16;
    if recoil < n(1) {
        shift = 16;
    }
    let force = integer::trunc_shift(fmul_lo32(recoil, recoil), shift);
    let mut rate = g(state, 1);
    if recoil < n(1) {
        rate = rate + fmul_lo32(dt, force);
    } else {
        rate = rate - fmul_lo32(dt, force);
    }
    rate = integer::decay(rate, g(config, 0).lo_bits(), dt);
    let mut value = recoil + rate;
    if value < n(-500) {
        value = n(-500);
        rate = n(0);
    } else if value > n(200) {
        value = n(200);
        rate = n(0);
    }
    p(state, 1, rate);
    p(state, 0, integer::rate_step(value, g(config, 1), dt));
    let pitch = g(state, 2);
    shift = 14;
    if pitch < n(1) {
        shift = 15;
    }
    let pitch_force = integer::trunc_shift(fmul_lo32(pitch, pitch), shift);
    rate = g(state, 3);
    if pitch < n(1) {
        rate = rate + fmul_lo32(dt, pitch_force);
    } else {
        rate = rate - fmul_lo32(dt, pitch_force);
    }
    rate = integer::decay(
        integer::rate_step(rate, g(config, 2), dt),
        g(config, 4).lo_bits(),
        dt,
    );
    value = pitch + rate;
    if value < n(-1456) {
        value = n(-1456);
        rate = n(0);
    } else if value > n(364) {
        value = n(364);
        rate = n(0);
    }
    p(state, 2, value);
    p(state, 3, rate);
    let mut i = 0u16;
    while i < 4 {
        let vertex = binding[2 + i];
        if vertex >= binding[1u16] {
            return 2;
        }
        let original = integer::from_i16(binding[6 + i] as i16);
        mesh[vertex * 3 + 2] = (original - integer::sar(g(state, 0), 3)).lo_bits() as i16;
        i += 1;
    }
    0
}
// State: two [position,rate] springs. Acceleration: lateral,longitudinal.
pub fn lean(state: Array<u16>, acceleration: Array<u16>, config: Array<u16>) {
    let dt = g(config, 6);
    let mut i = 0u16;
    while i < 2 {
        let mut sign = n(-60);
        if i == 1 {
            sign = n(60);
        }
        let delta = integer::trunc_shift(fmul_lo32(g(acceleration, i), sign), 10);
        let mut rate = g(state, i * 2 + 1) + delta;
        let position = clamp(g(state, i * 2), n(-2730), n(2730));
        let force = integer::trunc_shift(fmul_lo32(position, position), 15);
        if position < n(1) {
            rate = rate + fmul_lo32(dt, force);
        } else {
            rate = rate - fmul_lo32(dt, force);
        }
        rate = integer::decay(rate, g(config, 5).lo_bits(), dt);
        p(state, i * 2 + 1, rate);
        p(state, i * 2, position + rate);
        i += 1;
    }
}
// Contacts: four [world XYZ,gap] records. Output words: position(6), identity
// matrix(18), twelve signed i16 vertex coordinates, u16 radius (37 total).
pub fn hull(out: Array<u16>, current: Array<u16>, previous: Array<u16>, contacts: Array<u16>) {
    hull_points(out, current, previous, contacts, 4, 3);
}
pub fn hull_points(
    mut out: Array<u16>,
    current: Array<u16>,
    previous: Array<u16>,
    contacts: Array<u16>,
    stride: u16,
    gap: u16,
) {
    let mut i = 0u16;
    while i < 3 {
        p(out, i, g(current, i));
        i += 1;
    }
    i = 0;
    while i < 9 {
        let mut value = n(0);
        if i % 4 == 0 {
            value = n(4096);
        }
        p(out, 3 + i, value);
        i += 1;
    }
    let order: Buf<u16, 4> = Buf::new([1, 3, 2, 0]);
    let mut maximum = n(0);
    i = 0;
    while i < 4 {
        let index = order[i];
        let mut distance = n(0);
        let mut j = 0u16;
        while j < 3 {
            let mut value = g(contacts, index * stride + j) - g(previous, j);
            if j == 1 {
                value = value - g(contacts, index * stride + gap);
            }
            let mut magnitude = value;
            if magnitude < n(0) {
                magnitude = -magnitude;
            }
            distance = distance + magnitude;
            out[24 + i * 3 + j] = integer::trunc_shift(value, 3).lo_bits();
            j += 1;
        }
        if distance > maximum {
            maximum = distance;
        }
        i += 1;
    }
    let radius = integer::sar(maximum, 4).lo_bits();
    out[36u16] = radius + (radius >> 5);
}
