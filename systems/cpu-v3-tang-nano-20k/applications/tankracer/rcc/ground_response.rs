//! Ordered fallback-plane response with immediate impact and damage services.
use crate::dsl_rt::*;
use crate::{attitude, collision, damage, impact, integer};
pub struct State {
    pub velocity: Array<u16>,
    pub angular: Array<u16>,
    // Four ground::Point records (17 i32), and spring records (6 i32).
    pub points: Array<u16>,
    pub springs: Array<u16>,
    pub position: Array<u16>,
    pub delta: Array<u16>,
    pub impact: impact::Vehicle,
    pub damage: damage::Target,
    pub mode: fix32,
}
fn n(v: i16) -> fix32 {
    integer::from_i16(v)
}
fn g(v: Array<u16>, i: u16) -> fix32 {
    attitude::get(v, i)
}
fn p(v: Array<u16>, i: u16, x: fix32) {
    attitude::put(v, i, x);
}
fn abs(v: fix32) -> fix32 {
    if v < n(0) {
        return -v;
    }
    v
}
pub fn step(v: &mut State, impacts: &mut impact::State, c: &mut impact::Context) -> u16 {
    let initial: Buf<u16, 6> = Buf::new([0; 6]);
    let before = initial.as_array();
    let mut i = 0u16;
    while i < 3 {
        p(before, i, g(v.velocity, i));
        i += 1;
    }
    let mut last = n(-1);
    let mut flipped = false;
    let input: Buf<u16, 12> = Buf::new([0; 12]);
    let a = input.as_array();
    let config: Buf<u16, 16> = Buf::new([0; 16]);
    let cfg = config.as_array();
    p(cfg, 6, v.mode);
    if v.damage.player {
        p(cfg, 7, n(1));
    }
    let output: Buf<u16, 16> = Buf::new([0; 16]);
    let out = output.as_array();
    i = 0;
    while i < 4 {
        let kind = g(v.points, i * 17 + 6);
        let corner = g(v.springs, i * 6 + 2);
        if kind == n(4) || kind == n(5) {
            if kind == n(4) {
                if v.damage.player
                    && abs(v.impact.forward) > fix32::from_words(18928, 2)
                    && v.mode != n(2)
                    && v.mode != n(3)
                {
                    let mut effect = n(3);
                    if i >= 2 {
                        effect = n(4);
                    }
                    let status = impact::emit(impacts, &mut v.impact, corner, effect, c);
                    if status >= 2 {
                        return status;
                    }
                }
                last = corner;
            }
            let mut axis = 0u16;
            while axis < 3 {
                p(a, axis, g(v.points, i * 17 + 12 + axis));
                p(a, 3 + axis, g(v.velocity, axis));
                axis += 1;
            }
            let status = collision::respond(a, cfg, c.sine, c.angle, out);
            if status != 0 {
                return 2;
            }
            axis = 0;
            while axis < 3 {
                p(v.points, i * 17 + 12 + axis, g(out, axis));
                p(v.velocity, axis, g(out, 3 + axis));
                axis += 1;
            }
            let approach = g(out, 6);
            if approach > n(0) && corner.hi_bits() == 0 && corner.lo_bits() <= 3 {
                let negative = corner == n(0) || corner == n(3);
                let yaw = g(v.angular, 1);
                if (negative && yaw > n(0)) || (!negative && yaw < n(0)) {
                    p(
                        v.angular,
                        1,
                        -integer::trunc_shift(fmul_lo32(yaw, n(300)), 10),
                    );
                } else {
                    let amount =
                        integer::trunc_shift(fmul_lo32(collision::distance(before), n(300)), 10);
                    let amount = integer::trunc_shift(fmul_lo32(amount, approach), 12);
                    let mut next = yaw + amount;
                    if negative {
                        next = yaw - amount;
                    }
                    p(v.angular, 1, next);
                }
            }
        } else if kind == n(3) && !flipped {
            p(v.velocity, 0, -g(v.velocity, 0));
            p(v.velocity, 2, -g(v.velocity, 2));
            flipped = true;
        }
        i += 1;
    }
    if last != n(-1) && abs(v.impact.forward) > fix32::from_words(56784, 6) {
        let mut x = n(1024);
        if last == n(0) || last == n(1) {
            x = n(-1024);
        }
        let mut z = n(-1024);
        if last == n(0) || last == n(2) {
            z = n(1024);
        }
        p(a, 0, x);
        p(a, 1, n(0));
        p(a, 2, z);
        if v.impact.has_basis {
            collision::transform(a, v.impact.basis, a, false);
        }
        i = 0;
        while i < 3 {
            p(a, i, g(a, i) + g(v.position, i));
            i += 1;
        }
        let mut kind = n(3);
        if v.impact.forward > n(0) {
            kind = n(2);
        }
        if damage::track(&mut v.damage, kind, a) >= 2 {
            return 5;
        }
    }
    i = 0;
    while i < 3 {
        p(v.delta, i, g(v.velocity, i) - g(before, i));
        i += 1;
    }
    0
}
