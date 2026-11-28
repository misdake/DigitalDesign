//! Original wall reflection and asymmetric two-car impulse arithmetic.
use crate::dsl_rt::*;
use crate::{attitude, integer, trig_lookup};
fn n(v: i16) -> fix32 {
    integer::from_i16(v)
}
fn g(v: Array<u16>, i: u16) -> fix32 {
    attitude::get(v, i)
}
fn p(v: Array<u16>, i: u16, x: fix32) {
    attitude::put(v, i, x);
}
fn at(v: Array<u16>, words: i16) -> Array<u16> {
    unsafe { v.as_ptr().add(words).as_u16_array() }
}
fn abs(v: fix32) -> fix32 {
    if v < n(0) {
        return -v;
    }
    v
}
fn above(a: fix32, b: fix32) -> bool {
    a.hi_bits() > b.hi_bits() || (a.hi_bits() == b.hi_bits() && a.lo_bits() > b.lo_bits())
}
fn quarter(a: fix32) -> fix32 {
    fix32::from_words((a.lo_bits() >> 2) | (a.hi_bits() << 14), a.hi_bits() >> 2)
}
pub fn distance(v: Array<u16>) -> fix32 {
    let x = abs(g(v, 0));
    let y = abs(g(v, 1));
    let z = abs(g(v, 2));
    if above(x, y) && above(x, z) {
        return x + quarter(y) + quarter(z);
    }
    if !above(x, y) && above(y, z) {
        return y + quarter(x) + quarter(z);
    }
    z + quarter(x) + quarter(y)
}
pub fn normalize(v: Array<u16>) {
    let x = abs(g(v, 0));
    let y = abs(g(v, 1));
    let z = abs(g(v, 2));
    let mut length = z + integer::sar(x, 2) + integer::sar(y, 2);
    if y > x && y > z {
        length = y + integer::sar(x, 2) + integer::sar(z, 2);
    } else if x >= y && x > z {
        length = x + integer::sar(y, 2) + integer::sar(z, 2);
    }
    if length <= n(0) {
        return;
    }
    let mut i = 0u16;
    while i < 3 {
        let mut numerator = g(v, i);
        let mut denominator = integer::sar(length, 12);
        if length <= fix32::from_words(0, 4) {
            numerator = fmul_lo32(numerator, n(4096));
            denominator = length;
        }
        p(v, i, integer::div(numerator, denominator));
        i += 1;
    }
}
pub fn transform(out: Array<u16>, basis: Array<u16>, v: Array<u16>, inverse: bool) {
    let x = g(v, 0);
    let y = g(v, 1);
    let z = g(v, 2);
    let mut i = 0u16;
    while i < 3 {
        let mut a = i;
        let mut b = i + 3;
        let mut c = i + 6;
        if inverse {
            a = i * 3;
            b = a + 1;
            c = a + 2;
        }
        p(
            out,
            i,
            integer::sar(
                fmul_lo32(g(basis, a), x) + fmul_lo32(g(basis, b), y) + fmul_lo32(g(basis, c), z),
                12,
            ),
        );
        i += 1;
    }
}
pub fn yaw(v: Array<u16>, a: fix32, table: Array<i16>) {
    let index = (a.lo_bits() & 65532) >> 2;
    let sine = trig_lookup::index(table, index);
    let cosine = trig_lookup::index(table, 4096 + index);
    let x = g(v, 0);
    let z = g(v, 2);
    p(
        v,
        0,
        integer::trunc_shift(fmul_lo32(x, cosine) - fmul_lo32(z, sine), 12),
    );
    p(
        v,
        2,
        integer::trunc_shift(fmul_lo32(x, sine) + fmul_lo32(z, cosine), 12),
    );
}
// Input normal[0..3], velocity[3..6]. Config is local-scale present/mul/shift,
// final-scale present/mul/shift, mode, player. Output normal/velocity/approach/speed.
pub fn respond(
    input: Array<u16>,
    config: Array<u16>,
    sine: Array<i16>,
    angle: Array<i16>,
    out: Array<u16>,
) -> u16 {
    let scratch: Buf<u16, 18> = Buf::new([0; 18]);
    let normal = scratch.as_array();
    let toward = at(normal, 6);
    let reflected = at(normal, 12);
    p(normal, 0, g(input, 0));
    p(normal, 2, g(input, 2));
    p(toward, 0, -g(input, 3));
    p(toward, 2, -g(input, 5));
    let velocity = at(input, 6);
    let speed = integer::trunc_shift(distance(velocity), 12);
    normalize(normal);
    normalize(toward);
    let mut d = integer::trunc_shift(fmul_lo32(g(toward, 0), g(normal, 0)), 12)
        + integer::trunc_shift(fmul_lo32(g(toward, 2), g(normal, 2)), 12);
    if d > n(0) {
        if d > n(4096) {
            d = n(4096);
        }
        p(
            reflected,
            0,
            integer::trunc_shift(fmul_lo32(d, fmul_lo32(g(normal, 0), n(2))), 12) - g(toward, 0),
        );
        p(
            reflected,
            2,
            integer::trunc_shift(fmul_lo32(d, fmul_lo32(g(normal, 2), n(2))), 12) - g(toward, 2),
        );
        let a = attitude::angle(angle, -g(normal, 0), g(normal, 2));
        if a == n(-1) {
            return 2;
        }
        yaw(reflected, -a, sine);
        let mut multiplier = n(1);
        let mut shift = 3u16;
        if g(config, 0) != n(0) {
            multiplier = g(config, 1);
            shift = g(config, 2).lo_bits();
        }
        p(
            reflected,
            2,
            integer::trunc_shift(fmul_lo32(g(reflected, 2), multiplier), shift),
        );
        yaw(reflected, a, sine);
        multiplier = n(900);
        shift = 10;
        if g(config, 6) == n(4) || g(config, 6) == n(5) {
            multiplier = n(870);
        } else if g(config, 7) != n(0) {
            multiplier = n(930);
        }
        if g(config, 3) != n(0) {
            multiplier = g(config, 4);
            shift = g(config, 5).lo_bits();
        }
        p(
            reflected,
            0,
            integer::trunc_shift(
                fmul_lo32(fmul_lo32(speed, g(reflected, 0)), multiplier),
                shift,
            ),
        );
        p(
            reflected,
            2,
            integer::trunc_shift(
                fmul_lo32(fmul_lo32(speed, g(reflected, 2)), multiplier),
                shift,
            ),
        );
    } else {
        p(reflected, 0, g(input, 3));
        p(reflected, 2, g(input, 5));
    }
    let mut i = 0u16;
    while i < 3 {
        p(out, i, g(normal, i));
        p(out, 3 + i, g(reflected, i));
        i += 1;
    }
    p(out, 4, g(input, 4));
    p(out, 3, g(out, 3) + fmul_lo32(g(normal, 0), n(4)));
    p(out, 5, g(out, 5) + fmul_lo32(g(normal, 2), n(4)));
    p(out, 6, d);
    p(out, 7, speed);
    0
}
/// Exact binary64 1.7 / x87 truncation, expressed with quotient and remainder.
pub fn one_point_seven(value: fix32) -> fix32 {
    let magnitude = abs(value);
    let mut quotient = fix32::from_words(0xcccc, 0x0ccc);
    let mut remainder = 8u16;
    if magnitude != fix32::from_words(0, 32768) {
        quotient = integer::div(magnitude, n(10));
        remainder = (magnitude - fmul_lo32(quotient, n(10))).lo_bits();
    }
    let mut result = fmul_lo32(quotient, n(17)) + fix32::from_words((remainder * 17) / 10, 0);
    if magnitude != n(0) && remainder == 0 {
        result = result - n(1);
    }
    if value < n(0) {
        result = -result;
    }
    result
}
fn rate(value: fix32) -> fix32 {
    if value > n(1000) {
        return value - n(1000);
    }
    if value < n(-1000) {
        return value + n(1000);
    }
    n(0)
}
// Input basis[0..9], local velocity[9..12], mine[12..15], other[15..18],
// normal[18..21], saved incoming-other[21..24]. Output mine/other/approach.
pub fn exchange(input: Array<u16>, out: Array<u16>) {
    let storage: Buf<u16, 48> = Buf::new([0; 48]);
    let normal = storage.as_array();
    let relative = at(normal, 6);
    let local = at(normal, 12);
    let impulse = at(normal, 18);
    let world = at(normal, 24);
    let a = at(normal, 30);
    let b = at(normal, 36);
    let old = at(normal, 42);
    transform(normal, input, at(input, 36), true);
    p(normal, 1, n(0));
    normalize(normal);
    let mut i = 0u16;
    while i < 3 {
        p(local, i, integer::trunc_shift(g(input, 21 + i), 4));
        p(out, i, g(input, 12 + i));
        p(out, 3 + i, g(input, 15 + i));
        i += 1;
    }
    transform(relative, input, local, true);
    p(
        relative,
        0,
        g(relative, 0) - integer::trunc_shift(g(input, 9), 4),
    );
    p(relative, 1, n(0));
    p(
        relative,
        2,
        g(relative, 2) - integer::trunc_shift(g(input, 11), 4),
    );
    let mut speed = integer::trunc_shift(distance(relative), 12);
    normalize(relative);
    let mut d = integer::trunc_shift(fmul_lo32(g(normal, 0), g(relative, 0)), 12)
        + integer::trunc_shift(fmul_lo32(g(normal, 2), g(relative, 2)), 12);
    if d <= n(2048) {
        p(impulse, 0, integer::trunc_shift(g(normal, 0), 2));
        p(impulse, 2, integer::trunc_shift(g(normal, 2), 2));
        transform(world, input, impulse, false);
        i = 0;
        while i < 3 {
            let value = fmul_lo32(g(world, i), n(4));
            p(out, i, g(out, i) + value);
            p(out, 3 + i, g(out, 3 + i) - value);
            i += 1;
        }
        p(out, 6, d);
        return;
    }
    if d > n(4096) {
        d = n(4096);
    }
    p(
        impulse,
        0,
        integer::trunc_shift(fmul_lo32(fmul_lo32(d, g(normal, 0)), n(2)), 12) - g(relative, 0),
    );
    p(
        impulse,
        2,
        integer::trunc_shift(fmul_lo32(fmul_lo32(d, g(normal, 2)), n(2)), 12) - g(relative, 2),
    );
    p(a, 0, g(input, 12));
    p(a, 2, g(input, 14));
    p(b, 0, g(input, 15));
    p(b, 2, g(input, 17));
    normalize(a);
    normalize(b);
    let dot = fmul_lo32(g(a, 0), g(b, 0)) + fmul_lo32(g(a, 2), g(b, 2));
    let threshold = fix32::from_words(43275, 65350);
    if dot >= threshold && speed > n(3) {
        speed = n(3);
    }
    i = 0;
    while i < 3 {
        let value = integer::trunc_shift(fmul_lo32(fmul_lo32(speed, g(impulse, i)), n(600)), 10);
        p(impulse, i, value);
        p(old, i, value);
        i += 1;
    }
    let capped = dot >= threshold && distance(impulse) > n(7000);
    if capped {
        normalize(impulse);
        i = 0;
        while i < 3 {
            p(impulse, i, one_point_seven(g(impulse, i)));
            i += 1;
        }
    }
    transform(world, input, impulse, false);
    i = 0;
    while i < 3 {
        p(out, i, g(out, i) + fmul_lo32(g(world, i), n(16)));
        i += 1;
    }
    p(out, 3, g(out, 3) - g(old, 0));
    p(out, 5, g(out, 5) - g(old, 2));
    if capped {
        p(out, 0, rate(g(out, 0)));
        p(out, 2, rate(g(out, 2)));
        p(out, 3, rate(rate(g(out, 3))));
    }
    p(out, 6, d);
}
pub fn direction(x: fix32, z: fix32) -> u16 {
    if z > n(0) {
        if x > n(0) {
            if x <= z {
                return 2;
            }
            return 1;
        }
        if -x <= z {
            return 2;
        }
        return 0;
    }
    if x > n(0) {
        if x <= -z {
            return 3;
        }
        return 1;
    }
    if -x <= -z {
        return 3;
    }
    0
}
// Two position/velocity pairs, twelve raw i32 values; dt is a raw integer.
pub fn approaching(input: Array<u16>, dt: fix32) -> bool {
    let mut now = n(0);
    let mut future = n(0);
    let mut i = 0u16;
    while i < 3 {
        let delta = g(input, i) - g(input, 6 + i);
        let a = integer::trunc_shift(fmul_lo32(g(input, 3 + i), dt), 14);
        let b = integer::trunc_shift(fmul_lo32(g(input, 9 + i), dt), 14);
        let predicted = g(input, i) + a - (g(input, 6 + i) + b);
        now = now + fmul_lo32(delta, delta);
        future = future + fmul_lo32(predicted, predicted);
        i += 1;
    }
    future < now
}
