//! Weapon spin and packed-color chase, in original shared RNG order.
use crate::dsl_rt::*;
use crate::{attitude, integer, random};

// Nine i32 columns: flags, visible, hold, color timer, packed color,
// target color, angle, speed, disabled. Shared count/pulse are two u8 words.
fn n(value: i16) -> fix32 {
    integer::from_i16(value)
}
fn g(data: Array<u16>, i: u16) -> fix32 {
    attitude::get(data, i)
}
fn p(data: Array<u16>, i: u16, value: fix32) {
    attitude::put(data, i, value);
}
fn roll(seed: Array<u16>) -> u16 {
    let value = random::next(g(seed, 0));
    p(seed, 0, value);
    (value.lo_bits() >> 4) | (value.hi_bits() << 12)
}
fn choose(state: Array<u16>, seed: Array<u16>) -> fix32 {
    let mut target = g(state, 5).lo_bits();
    let mut value = roll(seed);
    target ^= ((value % 12 * 2 + 6) ^ target) & 62;
    value = roll(seed);
    target ^= (((value % 12 + 3) * 64) ^ target) & 1984;
    value = roll(seed);
    target ^= (((value % 12 + 3) * 2048) ^ target) & 63488;
    p(state, 5, fix32::from_words(target, g(state, 5).hi_bits()));
    fix32::from_words(value / 12, 0)
}
pub fn start(
    state: Array<u16>,
    player: bool,
    mode: fix32,
    shared: Array<u16>,
    seed: Array<u16>,
) -> fix32 {
    let mut words = shared;
    if g(state, 7) == n(0) {
        words[0u16] = (words[0u16] + 1) & 255;
    }
    let mut hold = n(0);
    if player && mode != n(4) && mode != n(5) {
        hold = n(4000);
    }
    p(state, 2, hold);
    let color = g(state, 4);
    p(
        state,
        4,
        fix32::from_words(color.lo_bits() & 1, color.hi_bits()),
    );
    p(state, 7, n(1));
    p(state, 1, n(1));
    let result = choose(state, seed);
    p(state, 0, n(1));
    words[1u16] = 0;
    result
}
fn approach(value: fix32, goal: fix32, packed: fix32, dt: fix32) -> fix32 {
    let amount =
        integer::from_i16(fmul_lo32(packed, fix32::from_words(dt.lo_bits(), 0)).lo_bits() as i16);
    let factor = fix32::from_words(((packed.hi_bits() & 255) * (dt.lo_bits() & 255)) & 255, 0);
    let mut delta = value - goal;
    if delta > amount {
        delta = delta - amount;
    } else if delta < -amount {
        delta = delta + amount;
    } else {
        delta = n(0);
    }
    let mut result = delta - integer::trunc_shift(fmul_lo32(delta, factor), packed.hi_bits() >> 8);
    if (delta < n(0) && result > n(0)) || (delta > n(0) && result < n(0)) {
        result = n(0);
    }
    result + goal
}
fn chase(value: u16, target: u16, mask: u16, base: u16, up: u16, down: u16) -> u16 {
    let goal = target & mask;
    let mut result = value;
    if (result ^ target) & mask == 0 {
        return result;
    }
    if result & mask < goal {
        result ^= (((result & base) + up) ^ result) & mask;
        if result & mask > goal {
            result ^= (result ^ target) & mask;
        }
    }
    if result & mask > goal {
        result ^= (((result & base) - down) ^ result) & mask;
        if result & mask < goal {
            result ^= (result ^ target) & mask;
        }
    }
    result
}
pub fn step(
    state: Array<u16>,
    dt: fix32,
    tuning: Array<u16>,
    limit: fix32,
    shared: Array<u16>,
    seed: Array<u16>,
) -> fix32 {
    if limit == n(0) {
        return n(-1);
    }
    if g(state, 8) == n(0) {
        p(state, 8, n(1));
    }
    let flags = g(state, 0).lo_bits();
    let mut goal = n(0);
    let mut packed = n(0);
    if flags & 1 != 0 {
        goal = g(tuning, 0);
        packed = g(tuning, 3);
    } else if flags & 2 != 0 {
        goal = g(tuning, 1);
        if g(state, 7) == goal {
            p(state, 2, g(state, 2) - dt);
            if g(state, 2) < n(0) {
                p(state, 0, n(4));
            }
        } else {
            packed = g(tuning, 4);
        }
    } else if flags & 4 != 0 && g(state, 7) > g(tuning, 2) {
        goal = g(tuning, 2);
        packed = g(tuning, 3);
    }
    if flags & 1 != 0
        || (flags & 2 != 0 && g(state, 7) != goal)
        || (flags & 3 == 0 && flags & 4 != 0 && g(state, 7) > g(tuning, 2))
    {
        p(state, 7, approach(g(state, 7), goal, packed, dt));
        if flags & 1 != 0 && g(state, 7) == g(tuning, 0) {
            p(state, 0, n(2));
        }
    }
    p(
        state,
        6,
        g(state, 6) + integer::sar(fmul_lo32(dt, g(state, 7)), 8),
    );
    let mut work = 0u16;
    while g(state, 6) > n(32760) || g(state, 6) < n(-32759) {
        if limit.hi_bits() == 0 && work >= limit.lo_bits() {
            return n(-3);
        }
        work += 1;
        let mut angle = g(state, 6);
        if angle > n(32760) {
            angle = angle - fix32::from_words(65520, 0);
        } else {
            angle = angle + fix32::from_words(65520, 0);
        }
        p(state, 6, angle);
    }
    p(state, 3, g(state, 3) + dt);
    let mut words = shared;
    if g(state, 3) <= n(60) {
        words[1u16] = 0;
        return n(0);
    }
    p(state, 3, n(0));
    words[1u16] = 1;
    let color = g(state, 4);
    let mut value = color.lo_bits();
    if g(state, 0).lo_bits() & 4 != 0 && g(state, 7) <= g(tuning, 2) {
        let speed = approach(g(state, 7), n(0), g(tuning, 5), dt);
        p(state, 7, speed);
        if speed == n(0) {
            p(state, 7, n(1));
        }
        if value & 62 != 0 {
            value ^= (((value & 65534) - 5) ^ value) & 62;
        }
        if value & 1984 != 0 {
            value ^= (((value & 65472) - 129) ^ value) & 1984;
        }
        if value & 63488 != 0 {
            value ^= (((value & 63488) - 4097) ^ value) & 63488;
        }
        if value & 65534 == 0 && g(state, 7) == n(1) {
            if g(state, 0).lo_bits() & 8 != 0 {
                value = (value & 65505) | 32;
            } else {
                p(state, 7, n(0));
                p(state, 8, n(0));
                words[0u16] = (words[0u16] - 1) & 255;
            }
        }
    } else {
        let target = g(state, 5).lo_bits();
        value = chase(value, target, 62, 65534, 6, 5);
        value = chase(value, target, 1984, 65472, 192, 129);
        value = chase(value, target, 63488, 63488, 6144, 4097);
        if (value ^ target) & 65534 == 0 {
            choose(state, seed);
        }
    }
    p(state, 4, fix32::from_words(value, color.hi_bits()));
    n(0)
}
