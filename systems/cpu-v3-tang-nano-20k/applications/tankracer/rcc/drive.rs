//! Drive/powertrain scalar stages. Raw i32 pairs; no fixed-point `*` operator.
//! State: velocity0..3 angular3..6 track6 boost_target7 engine8 slip9
//! was_active10 gear11 lateral12 acceleration13..15 load15.
//! Config: Drive0..49, gear limits49..54 offsets54..59 gains59..69,
//! engine/boost approach/decay damping69..78, surfaces78..258 (18 slots each).
//! Frame: dt0 mode1 player2 active3 launch4 materials5..7 kinds7..11
//! contact_y11..15 airborne15 boost_charge16 boost_power17 extra_drive18
//! throttle19 brake20 steering21 action_flags22 alternate23 alt_lateral24 alt_rotation25.
//! Config0..20: rpm,power,turn,initial_power,power_mul,brakes,turn_gain,
//! hill_gain,understeer_gain,rearweight_decel_gain,rearweight_accel_gain,
//! traction,catchup_traction,sidegrip_gain. Gains use two i32 slots.
//! Damping blocks20,23,26,29,32,35,38,41,44 are [step,fraction,shift].
//! Turbo amount47/power_mul48; each surface is drag/grip/seven slide gains.
use crate::dsl_rt::*;
use crate::{attitude, integer};
fn n(v: i16) -> fix32 {
    integer::from_i16(v)
}
fn g(a: Array<u16>, i: u16) -> fix32 {
    attitude::get(a, i)
}
fn p(a: Array<u16>, i: u16, v: fix32) {
    attitude::put(a, i, v);
}
fn abs(v: fix32) -> fix32 {
    if v < n(0) {
        return -v;
    }
    v
}
fn minimum(a: fix32, b: fix32) -> fix32 {
    if a < b {
        return a;
    }
    b
}
fn maximum(a: fix32, b: fix32) -> fix32 {
    if a > b {
        return a;
    }
    b
}
fn gain(v: fix32, c: Array<u16>, i: u16) -> fix32 {
    integer::trunc_shift(integer::mul(v, g(c, i)), g(c, i + 1).lo_bits() & 255)
}
fn average(v: fix32, c: Array<u16>, a: u16, b: u16, channel: u16) -> fix32 {
    integer::sar(gain(v, c, a + channel) + gain(v, c, b + channel), 1)
}
fn rate(v: fix32, force: fix32, dt: fix32) -> fix32 {
    let amount = integer::mul(force, dt);
    if v > amount {
        return v - amount;
    }
    if v < -amount {
        return v + amount;
    }
    n(0)
}
fn decay(v: fix32, shift: u16, dt: fix32) -> fix32 {
    let result = v - integer::sar(integer::mul(v, dt), shift);
    if (v < n(0) && result > n(0)) || (v > n(0) && result < n(0)) {
        return n(0);
    }
    result
}
fn damp(v: fix32, c: Array<u16>, i: u16, dt: fix32) -> fix32 {
    decay(rate(v, g(c, i), dt), g(c, i + 2).lo_bits(), dt)
}
fn damp_force(v: fix32, force: fix32, c: Array<u16>, i: u16, dt: fix32) -> fix32 {
    decay(
        rate(v, n(force.lo_bits() as i16), dt),
        g(c, i + 2).lo_bits(),
        dt,
    )
}
fn approach(v: fix32, target: fix32, c: Array<u16>, i: u16, dt: fix32) -> fix32 {
    let scaled = n(((g(c, i + 1).lo_bits() * dt.lo_bits()) & 255) as i16);
    let force = n(integer::mul(g(c, i), dt).lo_bits() as i16);
    let remaining = rate(v - target, force, n(1));
    let mut result =
        remaining - integer::trunc_shift(integer::mul(remaining, scaled), g(c, i + 2).lo_bits());
    if (remaining < n(0) && result > n(0)) || (remaining > n(0) && result < n(0)) {
        result = n(0);
    }
    result + target
}
fn gears(s: Array<u16>, c: Array<u16>, dt: fix32) -> bool {
    let mut remaining = abs(g(s, 6));
    let mut gear = 0u16;
    let mut count = n(0);
    if remaining > g(c, 49) {
        let mut more = true;
        while more {
            remaining = remaining - g(c, 49 + gear);
            if gear < 4 {
                gear += 1;
            }
            count = count + n(1);
            if count > fix32::from_words(0, 1) {
                return false;
            }
            more = remaining > g(c, 49 + gear);
        }
    }
    p(s, 11, n(gear as i16));
    let target = remaining + g(c, 54 + gear);
    p(s, 8, target + damp(g(s, 8) - target, c, 69, dt));
    true
}
/// 0 success, 1 missing surface, 2 invalid weather, 3 gear limit.
// Preserve the original damping, hill roll, track decay and launch order.
fn damping_step(s: Array<u16>, c: Array<u16>, f: Array<u16>) -> u16 {
    let ma = g(f, 5);
    let mb = g(f, 6);
    if ma < n(0) || ma >= n(10) || mb < n(0) || mb >= n(10) {
        return 1;
    }
    let a = 78 + ma.lo_bits() * 18;
    let b = 78 + mb.lo_bits() * 18;
    let dt = g(f, 0);
    let old_x = g(s, 0);
    let old_z = g(s, 2);
    let airborne = g(f, 15);
    let flags = g(f, 22).lo_bits();
    let player = g(f, 2) != n(0);
    let alternate = g(f, 23) != n(0);
    let active = g(f, 3) != n(0);
    let throttle = g(f, 19);
    let brake = g(f, 20);
    let mode = g(f, 1);
    let mut lateral = n(0);
    if player && alternate {
        lateral = integer::sar(
            integer::mul(g(c, 41), g(f, 24)),
            g(c, a + 5).lo_bits() & 255,
        );
    } else {
        let mut slide = 0u16;
        if player && flags & 3 != 0 {
            if flags & 56 == 0 {
                return 2;
            }
            slide = 4;
            if flags & 1 != 0 {
                slide = 1;
            }
            if flags & 8 == 0 {
                if flags & 16 != 0 {
                    slide += 1;
                } else {
                    slide += 2;
                }
            }
        }
        lateral = average(g(c, 41), c, a, b, 4 + slide * 2);
    }
    let force = integer::trunc_shift(
        integer::mul(integer::mul(lateral, g(c, 18)), g(s, 15)),
        g(c, 19).lo_bits() & 255,
    );
    p(s, 0, damp_force(old_x, force, c, 41, dt));
    p(s, 1, rate(g(s, 1), g(c, 32), dt));
    let force = average(g(c, 20), c, a, b, 0);
    p(s, 2, damp_force(g(s, 2), force, c, 20, dt));
    p(s, 3, damp(g(s, 3), c, 38, dt));
    p(s, 5, damp(g(s, 5), c, 38, dt));
    let mut descriptor = 35u16;
    let mut force = average(g(c, 35), c, a, b, 2);
    if alternate {
        descriptor = 20;
        force = gain(g(f, 25), c, a + 2);
    }
    force = integer::trunc_shift(integer::mul(n(4) - airborne, force), 2);
    p(s, 4, damp_force(g(s, 4), force, c, descriptor, dt));
    p(s, 12, g(s, 0));
    if active && throttle == n(0) && brake == n(0) {
        let difference = g(f, 12) + g(f, 14) - g(f, 11) - g(f, 13);
        p(s, 2, g(s, 2) + gain(difference, c, 8));
    }
    descriptor = 23;
    if airborne == n(4) {
        descriptor = 26;
    } else {
        if g(s, 9) != n(0) {
            descriptor = 29;
        }
    }
    p(s, 6, decay(g(s, 6), g(c, descriptor + 2).lo_bits(), dt));
    let mut active_word = n(0);
    if active {
        active_word = n(1);
    }
    if g(s, 10) != active_word {
        p(s, 10, active_word);
        if active && g(f, 4) != n(0) {
            let launch = fix32::from_words(52344, 4);
            p(s, 6, launch);
            p(s, 7, launch);
            p(s, 2, n(19687));
        }
    }
    0
}
// Catchup/boost, gear selection and drive/brake cap commit before traction.
fn engine_step(s: Array<u16>, c: Array<u16>, f: Array<u16>) -> u16 {
    let a = 78 + g(f, 5).lo_bits() * 18;
    let b = 78 + g(f, 6).lo_bits() * 18;
    let dt = g(f, 0);
    let airborne = g(f, 15);
    let throttle = g(f, 19);
    let brake = g(f, 20);
    let mut catchup = g(c, 17);
    if g(s, 7) != n(0) {
        catchup = n(20000);
    }
    let budget = average(integer::mul(catchup, dt), c, a, b, 2);
    if g(s, 7) != n(0) {
        if g(s, 6) < g(s, 7) {
            p(s, 6, approach(g(s, 6), g(s, 7), c, 72, dt));
        }
        p(s, 7, damp(g(s, 7), c, 75, dt));
        let ceiling = fix32::from_words(52344, 4);
        if g(s, 2) < ceiling {
            p(s, 2, minimum(g(s, 2) + integer::mul(dt, n(512)), ceiling));
        }
    }
    let v = g(s, 2);
    let mut drive = g(s, 6);
    if v > n(0) && v > drive {
        drive = drive + minimum(v - drive, budget);
    } else {
        if v < n(0) && -v > -drive {
            drive = drive - minimum(drive - v, budget);
        }
    }
    p(s, 6, drive);
    if !gears(s, c, dt) {
        return 3;
    }
    let q = integer::sar(drive, 12);
    let square = integer::mul(q, q);
    let mut power_mul = g(c, 4);
    if g(f, 16) >= n(2501) {
        power_mul = g(c, 48);
    }
    let mut cap = g(c, 3) - integer::mul(power_mul, square);
    if g(f, 16) >= n(2501) {
        cap = cap + g(f, 17);
    }
    cap = minimum(maximum(cap, n(0)), g(c, 1));
    let mut term = integer::mul(
        integer::mul(integer::sar(cap, 8), integer::sar(throttle, 8)),
        dt,
    );
    if airborne == n(4) {
        term = integer::mul(term, n(4));
    }
    drive = g(s, 6) + gain(integer::sar(term, 15), c, 59 + g(s, 11).lo_bits() * 2);
    let mut base = n(0);
    if abs(throttle) < n(1024) {
        base = integer::mul(dt, n(16));
    }
    let braking = integer::trunc_shift(
        integer::mul(
            integer::mul(
                integer::mul(n(4) - airborne, integer::sar(g(c, 5), 23)),
                integer::sar(brake, 8),
            ),
            dt,
        ),
        2,
    );
    if drive > n(0) {
        drive = maximum(drive - base - braking, n(0));
    } else {
        drive = minimum(drive + base + braking, n(0));
    }
    p(s, 6, minimum(maximum(drive, -g(c, 0)), g(c, 0)));
    0
}
// Traction publishes slip/acceleration before the final steering mutation.
fn traction_step(s: Array<u16>, c: Array<u16>, f: Array<u16>, old_x: fix32, old_z: fix32) -> u16 {
    let a = 78 + g(f, 5).lo_bits() * 18;
    let b = 78 + g(f, 6).lo_bits() * 18;
    let dt = g(f, 0);
    let airborne = g(f, 15);
    let brake = g(f, 20);
    let mode = g(f, 1);
    let player = g(f, 2) != n(0);
    let active = g(f, 3) != n(0);
    let mut delta = n(0);
    if airborne != n(4) {
        delta = g(s, 6) - g(s, 2);
    }
    let mut fallback = false;
    let mut i = 0u16;
    while i < 4 {
        if g(f, 7 + i) == n(4) {
            fallback = true;
        }
        i += 1;
    }
    let mut traction = g(c, 16);
    if (mode == n(4) || mode == n(5)) && fallback {
        traction = n(400);
    }
    let budget = average(integer::mul(traction, dt), c, a, b, 2);
    let mut applied = delta;
    if player
        && (mode == n(0) || mode == n(1) || mode == n(5))
        && abs(g(s, 2)) < fix32::from_words(37856, 4)
        && brake == n(0)
        && g(s, 7) == n(0)
        && !fallback
    {
        if delta > budget {
            applied = integer::mul(budget, n(2));
        } else {
            if delta < -budget {
                applied = -integer::mul(budget, n(2));
            }
        }
    } else {
        if delta > budget {
            applied = budget;
        } else {
            if delta < -budget {
                applied = -budget;
            }
        }
    }
    p(s, 9, delta - applied);
    if active {
        applied = integer::trunc_shift(integer::mul(n(4) - airborne, applied + g(f, 18)), 2);
    }
    p(s, 2, g(s, 2) + applied);
    p(s, 13, g(s, 0) - old_x);
    p(s, 14, g(s, 2) - old_z);
    p(s, 15, integer::trunc_shift(g(s, 15), 8));
    let mut term = integer::trunc_shift(integer::mul(g(f, 21), g(c, 2)), 8);
    term = integer::trunc_shift(
        integer::mul(integer::mul(term, g(c, 6)), g(s, 15)),
        g(c, 7).lo_bits() & 255,
    );
    p(
        s,
        4,
        g(s, 4) - integer::mul(dt, term) + gain(-g(s, 0), c, 10),
    );
    0
}

pub fn step(s: Array<u16>, c: Array<u16>, f: Array<u16>) -> u16 {
    let old_x = g(s, 0);
    let old_z = g(s, 2);
    let status = damping_step(s, c, f);
    if status != 0 {
        return status;
    }
    let status = engine_step(s, c, f);
    if status != 0 {
        return status;
    }
    traction_step(s, c, f, old_x, old_z)
}

pub fn large(v: Array<u16>) -> bool {
    let limit = fix32::from_words(41248, 7);
    abs(g(v, 0)) > limit || abs(g(v, 2)) > limit
}
pub fn transform(src: Array<u16>, matrix: Array<u16>, dst: Array<u16>, inverse: bool, big: bool) {
    let x_large = g(src, 0) > g(src, 2);
    let mut xs = 4u16;
    let mut zs = 4u16;
    if big {
        if x_large {
            xs = 8;
        } else {
            zs = 8;
        }
    }
    let x = integer::sar(g(src, 0), xs);
    let y = integer::sar(g(src, 1), 4);
    let z = integer::sar(g(src, 2), zs);
    let temporary: Buf<u16, 6> = Buf::new([0; 6]);
    let t = temporary.as_array();
    let mut i = 0u16;
    while i < 3 {
        let mut a = i * 3;
        let mut b = a + 1;
        let mut c = a + 2;
        if inverse {
            a = i;
            b = i + 3;
            c = i + 6;
        }
        let mut tx = integer::mul(g(matrix, a), x);
        let mut ty = integer::mul(g(matrix, b), y);
        let mut tz = integer::mul(g(matrix, c), z);
        let mut shift = 8u16;
        if big {
            shift = 4;
            if !x_large {
                tx = integer::sar(tx, 4);
            }
            ty = integer::sar(ty, 4);
            if x_large {
                tz = integer::sar(tz, 4);
            }
        }
        p(t, i, integer::sar(tx + tz + ty, shift));
        i += 1;
    }
    i = 0;
    while i < 3 {
        p(dst, i, g(t, i));
        i += 1;
    }
}
pub fn correct_yaw(s: Array<u16>, c: Array<u16>) {
    let mut x = g(s, 0);
    let mut z = g(s, 2);
    let a = abs(x);
    let b = n(0);
    let d = abs(z);
    let mut length = n(0);
    if b > a && b > d {
        length = b + integer::sar(a, 2) + integer::sar(d, 2);
    } else {
        if a >= b && a > d {
            length = a + integer::sar(b, 2) + integer::sar(d, 2);
        } else {
            length = d + integer::sar(a, 2) + integer::sar(b, 2);
        }
    }
    if length > n(0) {
        if length > fix32::from_words(0, 4) {
            let denominator = integer::sar(length, 12);
            x = integer::div(x, denominator);
            z = integer::div(z, denominator);
        } else {
            x = integer::div(integer::mul(x, n(4096)), length);
            z = integer::div(integer::mul(z, n(4096)), length);
        }
    }
    let cross = integer::sar(integer::mul(n(-4096), x), 12);
    let delta = g(s, 14);
    let mut change = integer::trunc_shift(integer::mul(abs(delta), cross), 12);
    let mut index = 14u16;
    if delta < n(0) {
        index = 12;
    }
    change = gain(change, c, index);
    if delta < n(0) {
        p(s, 4, g(s, 4) - change);
    } else {
        p(s, 4, g(s, 4) + change);
    }
}
