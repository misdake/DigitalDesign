//! Synchronous blast impulse and health writes on the shared random stream.
use crate::dsl_rt::*;
use crate::{attitude, collision, integer, random};
pub struct Target {
    // Health, health timer/color, protection timer, angular enabled, hit axes X/Z.
    pub state: Array<u16>,
    pub velocity: Array<u16>,
    pub angular: Array<u16>,
    pub marker: fix32,
}
pub struct Input {
    pub position: Array<u16>,
    pub source: Array<u16>,
    pub mode: fix32,
    pub damage: bool,
    pub max_health: fix32,
    pub record_axes: bool,
    pub mark: bool,
}
fn n(v: i16) -> fix32 {
    integer::from_i16(v)
}
fn signed_random(seed: Array<u16>) -> fix32 {
    let sign = random::next(attitude::get(seed, 0));
    let value = random::next(sign);
    attitude::put(seed, 0, value);
    let bits = (value.lo_bits() >> 5) | ((value.hi_bits() & 15) << 11);
    let magnitude = fix32::from_words(bits, 0) + n(20000);
    if sign.lo_bits() & 32 != 0 {
        return magnitude;
    }
    -magnitude
}
// 0 outside force range, 1 selects subsequent device feedback, 2 arithmetic.
// Errors retain health/timer writes and do not consume subsequent random values.
pub fn apply(target: &mut Target, input: &Input, seed: Array<u16>) -> u16 {
    let delta: Buf<u16, 6> = Buf::new([0; 6]);
    let d = delta.as_array();
    let mut i = 0u16;
    while i < 3 {
        attitude::put(
            d,
            i,
            attitude::get(input.position, i) - attitude::get(input.source, i),
        );
        i += 1;
    }
    let distance = collision::distance(d);
    let mut force = n(25);
    if input.mode == n(1) {
        force = n(40);
        if distance < n(5120) {
            force = n(80);
        } else if distance < n(20480) {
            force = n(60);
        }
    } else if distance >= n(7168) {
        return 0;
    }
    force = force - integer::trunc_shift(distance, 9);
    if force < n(0) {
        return 0;
    }
    let s = target.state;
    if input.damage {
        let health = attitude::get(s, 0) - fmul_lo32(force, n(128));
        attitude::put(s, 0, health);
        attitude::put(s, 1, n(220));
        let half = integer::sar(input.max_health, 1);
        if health <= half {
            let numerator = fmul_lo32(health, n(175));
            if half == n(0) || (numerator == fix32::from_words(0, 32768) && half == n(-1)) {
                return 2;
            }
            let color = integer::div(numerator, half);
            attitude::put(s, 2, fix32::from_words(color.lo_bits(), 0));
        }
    }
    if attitude::get(s, 0) < n(0) {
        attitude::put(s, 0, n(0));
    }
    collision::normalize(d);
    i = 0;
    while i < 3 {
        attitude::put(
            target.velocity,
            i,
            attitude::get(target.velocity, i) + fmul_lo32(attitude::get(d, i), force),
        );
        i += 1;
    }
    signed_random(seed);
    attitude::put(
        target.angular,
        1,
        attitude::get(target.angular, 1) + fmul_lo32(force, signed_random(seed)),
    );
    let rates: Buf<u16, 4> = Buf::new([0; 4]);
    i = 0;
    while i < 2 {
        let negative = fmul_lo32(force, signed_random(seed)) < n(0);
        let product = fmul_lo32(force, signed_random(seed));
        let mut rate = integer::sar(product, 1);
        if negative {
            rate = -integer::sar(-product, 1);
        }
        attitude::put(rates.as_array(), i, rate);
        i += 1;
    }
    let pitch = attitude::get(rates.as_array(), 0);
    let roll = attitude::get(rates.as_array(), 1);
    if attitude::get(s, 4) != n(0) {
        attitude::put(target.angular, 2, attitude::get(target.angular, 2) + pitch);
        attitude::put(target.angular, 0, attitude::get(target.angular, 0) + roll);
    }
    if input.record_axes {
        let mut x = n(2);
        let mut z = n(2);
        if roll > n(0) {
            x = n(1);
        }
        if pitch > n(0) {
            z = n(1);
        }
        attitude::put(s, 5, x);
        attitude::put(s, 6, z);
    }
    if input.mark {
        target.marker = n(300);
    }
    1
}
