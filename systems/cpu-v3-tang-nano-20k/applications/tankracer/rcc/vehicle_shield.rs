//! Shared shield orbits, using the world's seed and caller-owned banks.
use crate::dsl_rt::*;
use crate::{attitude, collision, integer, random, trig_lookup};
pub struct Input {
    pub enabled: bool,
    pub bank: fix32,
    pub dt: fix32,
    pub position: Array<u16>,
    pub basis: Array<u16>,
    pub has_basis: bool,
}
fn n(x: i16) -> fix32 {
    integer::from_i16(x)
}
fn g(v: Array<u16>, i: u16) -> fix32 {
    attitude::get(v, i)
}
fn p(v: Array<u16>, i: u16, x: fix32) {
    attitude::put(v, i, x);
}
fn at(v: Array<u16>, i: u16) -> Array<u16> {
    unsafe { v.as_ptr().add(i as i16).as_u16_array() }
}
fn draw(seed: Array<u16>, mask: u16, offset: fix32) -> fix32 {
    let value = random::next(g(seed, 0));
    p(seed, 0, value);
    fix32::from_words(((value.lo_bits() >> 4) | (value.hi_bits() << 12)) & mask, 0) + offset
}
// Nine banks, each four 11-column orbits followed by one tail column (1,620 B).
// Reinitialization preserves positions, matching the selective source stores.
pub fn initialize(state: Array<u16>, seed: Array<u16>) {
    let mut bank = 0u16;
    while bank < 9 {
        let b = at(state, bank * 90);
        p(b, 44, n(0));
        let mut i = 0u16;
        while i < 4 {
            let o = at(b, i * 22);
            p(o, 0, draw(seed, 16383, n(0)));
            p(o, 1, draw(seed, 16383, n(0)));
            p(o, 2, draw(seed, 65535, n(0)));
            p(o, 3, draw(seed, 65535, n(-32768)));
            p(o, 4, draw(seed, 65535, n(-32768)));
            p(
                o,
                5,
                n(200) + integer::mul(fix32::from_words(bank, 0), n(100)),
            );
            p(o, 6, n(0));
            p(o, 7, n(0));
            i += 1;
        }
        bank += 1;
    }
}
fn seek(o: Array<u16>, column: u16, step: fix32, mask: u16, offset: fix32, seed: Array<u16>) {
    let mut v = g(o, column);
    let target = g(o, column + 1);
    let mut reached = true;
    if v < target {
        v = v + step;
        if v >= target {
            v = target;
        } else {
            reached = false;
        }
    } else if v > target {
        v = v - step;
        if v <= target {
            v = target;
        } else {
            reached = false;
        }
    }
    p(o, column, v);
    if reached {
        p(o, column + 1, draw(seed, mask, offset));
    }
}
pub fn tick(state: Array<u16>, input: &Input, seed: Array<u16>, sine: Array<i16>) -> fix32 {
    if !input.enabled {
        return n(0);
    }
    if input.bank.hi_bits() != 0 || input.bank.lo_bits() >= 9 {
        return n(-1);
    }
    let b = at(state, input.bank.lo_bits() * 90);
    let point: Buf<u16, 6> = Buf::new([0; 6]);
    let v = point.as_array();
    let mut i = 0u16;
    while i < 4 {
        let o = at(b, i * 22);
        p(
            o,
            2,
            g(o, 2) + integer::trunc_shift(integer::mul(input.dt, g(o, 3)), 8),
        );
        seek(o, 3, integer::mul(input.dt, n(4)), 65535, n(-32768), seed);
        seek(o, 0, integer::mul(input.dt, n(16)), 16383, n(0), seed);
        let pitch = (g(o, 0).lo_bits() & 65532) >> 2;
        let yaw = (g(o, 2).lo_bits() & 65532) >> 2;
        let radius = integer::sar(
            integer::mul(trig_lookup::index(sine, pitch + 4096), n(1500)),
            12,
        );
        p(
            v,
            0,
            integer::sar(
                integer::mul(radius, trig_lookup::index(sine, yaw + 4096)),
                12,
            ),
        );
        p(
            v,
            1,
            integer::sar(integer::mul(trig_lookup::index(sine, pitch), n(1500)), 12),
        );
        p(
            v,
            2,
            integer::sar(integer::mul(radius, trig_lookup::index(sine, yaw)), 12),
        );
        if input.has_basis {
            collision::transform(v, input.basis, v, false);
        }
        let mut axis = 0u16;
        while axis < 3 {
            p(o, 8 + axis, g(v, axis) + g(input.position, axis));
            axis += 1;
        }
        let size = g(o, 5);
        if g(o, 6) == n(0) {
            if size < n(201) {
                p(o, 5, n(200));
                p(o, 6, n(1));
            } else {
                p(o, 5, size - integer::mul(input.dt, n(2)));
            }
        } else if size < n(500) {
            p(o, 5, size + integer::mul(input.dt, n(2)));
        } else {
            p(o, 5, n(500));
            p(o, 6, n(0));
        }
        p(
            o,
            7,
            fix32::from_words((g(o, 7) + integer::mul(input.dt, n(8))).lo_bits(), 0),
        );
        i += 1;
    }
    n(0)
}
