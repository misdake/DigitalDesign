//! Four real contact springs, pre-response load and contact-dependent pivot.
//! State i32 slots: points[4][kind,gap,corner,travel,rate,output] 0..24;
//! angular24..26 tilt26..28 penetration28 travel_sum29 vertical30 vy31 y32
//! timer33 auxiliary_type34..36 cached36..38 max38 last39 load40 pivot41..44.
//! Config i32 slots: bounds0..2; positive/negative force gains2..6;
//! output/tilt gains6..10; shape shift10; positive/negative spring gains11..15;
//! damping shift15, dead step16; load base/floor/gain17..21; geometry layout21..26.
//! Every gain is [unsigned multiplier, arithmetic shift]; contact stride is 6.
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
fn gain(v: fix32, c: Array<u16>, i: u16) -> fix32 {
    integer::sar(integer::mul(v, g(c, i)), g(c, i + 1).lo_bits())
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
fn agree(a: fix32, b: fix32) -> fix32 {
    if a > n(0) && b > n(0) {
        return minimum(a, b);
    }
    if a < n(0) && b < n(0) {
        return maximum(a, b);
    }
    n(0)
}
fn grounded(s: Array<u16>, i: u16) -> bool {
    let k = g(s, i * 6);
    k == n(0) || k == n(1)
}
pub fn contact(
    s: Array<u16>,
    at: u16,
    c: Array<u16>,
    mut force: fix32,
    dt: fix32,
    auxiliary: fix32,
) {
    let mut travel = g(s, at + 3);
    let mut rate = g(s, at + 4);
    if force > n(0) {
        rate = rate + gain(force, c, 2);
    } else {
        if force < n(0) {
            rate = rate + gain(force, c, 4);
        } else {
            p(s, at + 5, n(0));
        }
    }
    let shaped = integer::sar(travel, g(c, 10).lo_bits());
    let mut index = 11u16;
    if travel <= n(0) {
        index = 13;
    }
    let restoring = integer::mul(gain(integer::mul(shaped, shaped), c, index), dt);
    if travel <= n(0) {
        rate = rate + restoring;
    } else {
        rate = rate - restoring;
    }
    let previous = rate;
    rate = previous - integer::sar(integer::mul(previous, dt), g(c, 15).lo_bits());
    if (previous < n(0) && rate > n(0)) || (previous > n(0) && rate < n(0)) {
        rate = n(0);
    }
    travel = travel + rate;
    let dead = integer::mul(g(c, 16), dt);
    if dead < travel {
        travel = travel - dead;
    } else {
        if travel < -dead {
            travel = travel + dead;
        } else {
            travel = n(0);
        }
    }
    travel = minimum(maximum(travel, g(c, 0)), g(c, 1));
    p(s, at + 3, travel);
    p(s, at + 4, rate);
    if force > n(0) {
        force = gain(travel, c, 6);
    }
    p(s, at + 5, integer::sar(force, 14));
    let tilt = gain(force, c, 8);
    let corner = g(s, at + 2);
    if corner >= n(4) || (tilt <= n(0) && auxiliary != n(2)) {
        return;
    }
    let tilt = integer::sar(tilt, 9);
    let mut x = tilt;
    let mut z = tilt;
    if corner == n(0) || corner == n(2) {
        x = -x;
    }
    if corner < n(2) {
        z = -z;
    }
    p(s, 26, g(s, 26) + x);
    p(s, 27, g(s, 27) + z);
}
pub fn conditions(e: Array<u16>) -> u16 {
    let half = g(e, 1) != n(0) && g(e, 2) == n(0);
    let region =
        g(e, 5) >= g(e, 7) && g(e, 5) <= g(e, 9) && g(e, 6) >= g(e, 8) && g(e, 6) <= g(e, 10);
    let modified = (g(e, 3) == n(5) && region) || half;
    let mut flags = 0u16;
    if modified {
        flags |= 1;
    }
    if half {
        flags |= 2;
    }
    flags
}
pub fn prepare(s: Array<u16>, c: Array<u16>, mut events: Array<u16>) {
    events[0u16] = 0;
    events[1u16] = 0;
    events[2u16] = 0;
    // PRE-response load reads the old spring travel.
    let mut load = n(0);
    let mut i = 0u16;
    while i < 4 {
        if grounded(s, i) {
            let value = g(c, 17) + g(s, i * 6 + 3);
            if value > n(0) {
                load =
                    load + integer::trunc_shift(integer::mul(value, g(c, 19)), g(c, 20).lo_bits());
            }
            load = load + g(c, 18);
        }
        i += 1;
    }
    p(s, 40, load);
    // Pivot uses grounded sides and the model-derived min/max geometry.
    let left = grounded(s, 0) || grounded(s, 1);
    let right = grounded(s, 2) || grounded(s, 3);
    let front = grounded(s, 0) || grounded(s, 2);
    let rear = grounded(s, 1) || grounded(s, 3);
    let mut x = n(0);
    let mut y = n(0);
    let mut z = n(0);
    if left && !right {
        x = g(c, 21);
    } else {
        if right && !left {
            x = g(c, 24);
        }
    }
    if left || right {
        y = g(c, 22);
    }
    if rear && !front {
        z = g(c, 23);
    } else {
        if front && !rear {
            z = g(c, 25);
        }
    }
    p(s, 41, x);
    p(s, 42, y);
    p(s, 43, z);
    p(s, 26, n(0));
    p(s, 27, n(0));
    p(s, 28, n(0));
    p(s, 29, n(0));
    p(s, 38, n(0));
    p(s, 39, n(0));
}
pub fn force(
    s: Array<u16>,
    c: Array<u16>,
    e: Array<u16>,
    i: u16,
    flags: u16,
    mut events: Array<u16>,
) -> fix32 {
    let dt = g(e, 0);
    let modified = flags & 1 != 0;
    let half = flags & 2 != 0;
    let at = i * 6;
    let kind = g(s, at);
    let mut force = n(0);
    if kind == n(2) {
        events[2u16] |= 1u16 << i;
        if modified {
            let mut index = 13u16;
            if g(e, 4) != n(0) {
                index = 11;
            }
            force = -integer::mul(
                integer::sar(
                    integer::mul(g(e, index), fix32::from_words(0, 75)),
                    g(e, index + 1).lo_bits(),
                ),
                dt,
            );
            if half {
                force = integer::sar(force, 1);
            }
        } else {
            force = integer::mul(fix32::from_words(0, 0xffb5), dt);
        }
    }
    if kind == n(0) {
        events[2u16] |= 16u16 << i;
        let penetration = n(-75) - g(s, at + 1);
        let mut travel = g(s, at + 3) + integer::shl(penetration, g(e, 15).lo_bits());
        if travel > g(c, 1) {
            let last = integer::trunc_shift(travel - g(c, 1), g(e, 15).lo_bits());
            p(s, 39, last);
            p(s, 38, maximum(g(s, 38), last));
            travel = g(c, 1);
        }
        p(s, at + 3, travel);
        force = force + integer::mul(minimum(penetration, n(400)), n(10240));
        p(s, 28, g(s, 28) + force);
    }
    force
}
pub fn complete(s: Array<u16>, e: Array<u16>, mut events: Array<u16>) {
    let dt = g(e, 0);
    p(s, 24, g(s, 24) - g(s, 26));
    p(s, 25, g(s, 25) - g(s, 27));
    let a = g(s, 5);
    let b = g(s, 11);
    let c0 = g(s, 17);
    let d = g(s, 23);
    p(s, 30, agree(a + c0, b + d) + agree(a + b, c0 + d));
    if g(s, 38) > n(0) {
        events[1u16] = 1;
        p(s, 30, maximum(g(s, 30), n(0)));
        if g(s, 31) < n(0) {
            p(s, 31, integer::trunc_shift(g(s, 31), 1));
        }
        p(s, 32, g(s, 32) + g(s, 39) + n(1));
    }
    if g(s, 33) > n(0) {
        p(s, 33, g(s, 33) - dt);
        events[1u16] |= 2;
    }
}

pub fn step(s: Array<u16>, c: Array<u16>, e: Array<u16>, mut events: Array<u16>) {
    let dt = g(e, 0);
    prepare(s, c, events);
    let flags = conditions(e);
    let mut i = 0u16;
    while i < 4 {
        let at = i * 6;
        let force = force(s, c, e, i, flags, events);
        let side = i / 2;
        if force <= n(0) && g(s, 36 + side) == n(0) {
            p(s, 34 + side, g(e, 16 + side));
            p(s, 36 + side, n(1));
            events[0u16] |= 1u16 << side;
        }
        contact(s, at, c, force, dt, g(s, 34 + side));
        p(s, 29, g(s, 29) + g(s, at + 3));
        i += 1;
    }
    complete(s, e, events);
}
