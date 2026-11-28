//! Deterministic fifteen-slot trail bank; emission uses its own fixed sequence.
use crate::dsl_rt::*;
use crate::{attitude, collision, integer, particle_sprite, projectile};
fn n(v: i16) -> fix32 {
    integer::from_i16(v)
}
fn g(v: Array<u16>, i: u16) -> fix32 {
    attitude::get(v, i)
}
fn p(v: Array<u16>, i: u16, x: fix32) {
    attitude::put(v, i, x);
}
fn at(v: Array<u16>, words: u16) -> Array<u16> {
    unsafe { v.as_ptr().add(words as i16).as_u16_array() }
}
pub fn emit(
    state: &projectile::State,
    pose: &projectile::Pose,
    ctx: &projectile::Context,
    dt: fix32,
) -> fix32 {
    let s = state.core;
    p(s, 10, g(s, 10) + dt);
    if g(s, 10) <= n(32) {
        return n(0);
    }
    p(s, 10, n(0));
    let noise: Buf<u16, 11> = Buf::new([
        48074, 62583, 14493, 46770, 58722, 10100, 51972, 35184, 42635, 8767, 23456,
    ]);
    let mut sequence = g(ctx.shared.core, 0);
    if sequence.hi_bits() != 0 || sequence.lo_bits() >= 11 {
        return n(-4);
    }
    let first = noise[sequence.lo_bits()];
    let mut index = sequence.lo_bits() + 1;
    if index == 11 {
        index = 0;
    }
    let second = noise[index];
    index += 1;
    if index == 11 {
        index = 0;
    }
    p(ctx.shared.core, 0, fix32::from_words(index, 0));
    let cursor = g(s, 9);
    if cursor < n(0) || cursor >= n(15) {
        return n(-4);
    }
    let t = at(state.trails, cursor.lo_bits() * 34);
    let sprite = at(t, 10);
    let kind = g(s, 2).lo_bits();
    let fades: Buf<i16, 6> = Buf::new([128, 50, 30, 128, 50, 128]);
    let spreads: Buf<u16, 6> = Buf::new([4, 8, 3, 1, 2, 1]);
    let lifts: Buf<u16, 6> = Buf::new([2, 6, 2, 2, 3, 2]);
    let sizes: Buf<i16, 6> = Buf::new([250, 400, 300, 100, 1000, 560]);
    p(sprite, 0, g(ctx.resources.trail, kind));
    if kind == 5 {
        particle_sprite::rgb(sprite, 100, 100, 128);
    } else {
        let lift = integer::shl(
            integer::shl(fix32::from_words(lifts[kind], 0), 10),
            8 - first % 2,
        );
        p(at(t, 2), 1, integer::div(lift, n(1000)));
        let color = fades[kind] as u16;
        if kind == 3 {
            let channel = first % 3;
            p(at(t, 32), 0, fix32::from_words(channel, 0));
            let a = first % color;
            let b = second % color;
            if channel == 0 {
                particle_sprite::rgb(sprite, color, a, b);
            } else if channel == 1 {
                particle_sprite::rgb(sprite, b, a, color);
            } else {
                particle_sprite::rgb(sprite, a, color, b);
            }
        } else {
            particle_sprite::rgb(sprite, color, color, color);
        }
    }
    p(sprite, 4, n(1));
    particle_sprite::rectangle(sprite, 0, 0, 63, 63);
    let delta: Buf<u16, 6> = Buf::new([0; 6]);
    if kind == 2 {
        collision::transform(
            delta.as_array(),
            pose.basis,
            ctx.resources.homing_tail,
            false,
        );
    } else if kind == 4 {
        collision::transform(delta.as_array(), pose.basis, at(s, 54), false);
    }
    let mut i = 0u16;
    while i < 3 {
        p(sprite, 1 + i, g(pose.position, i) + g(delta.as_array(), i));
        i += 1;
    }
    let spread = spreads[kind];
    let x = fmul_lo32(
        integer::shl(fix32::from_words(second % spread, 0), 17),
        n(1) - fix32::from_words(first % 3, 0),
    );
    let z = fmul_lo32(
        integer::shl(fix32::from_words(first % spread, 0), 17),
        n(1) - fix32::from_words(second % 3, 0),
    );
    p(at(t, 2), 0, integer::div(x, n(1000)));
    p(at(t, 2), 2, integer::div(z, n(1000)));
    p(sprite, 6, integer::from_i16(sizes[kind]));
    p(sprite, 7, integer::from_i16(sizes[kind]));
    let mut sprite = sprite;
    sprite[20u16] = 0;
    p(
        at(t, 8),
        0,
        integer::sar(
            fmul_lo32(fix32::from_words(first % 720, 0) - n(360), n(182)),
            2,
        ),
    );
    let mut t = t;
    t[0u16] = 1;
    let mut next = cursor.lo_bits() + 1;
    if next == 15 {
        next = 0;
    }
    p(s, 9, fix32::from_words(next, 0));
    if g(s, 8) < n(15) {
        p(s, 8, g(s, 8) + n(1));
    }
    n(0)
}
fn clamp_color(sprite: Array<u16>, channel: u16, fade: u16) {
    let value = particle_sprite::color(sprite, channel);
    if value == 0 {
        return;
    }
    let mut next = 0u16;
    if value > fade {
        next = value - fade;
    }
    particle_sprite::put_color(sprite, channel, next);
}
pub fn fade(state: &projectile::State, dt: fix32) -> fix32 {
    let kind = g(state.core, 2).lo_bits();
    let fades: Buf<u16, 6> = Buf::new([10, 5, 1, 10, 5, 8]);
    let decrement = fades[kind];
    let mut selected = 65535u16;
    let mut component = 0u16;
    let mut i = 0u16;
    while i < 15 {
        let mut t = at(state.trails, i * 34);
        if t[0u16] == 0 {
            i += 1;
            continue;
        }
        t[1u16] = (t[1u16] as i16 + dt.lo_bits() as i16) as u16;
        if (t[1u16] as i16) <= 30 {
            i += 1;
            continue;
        }
        let channel = if kind == 3 { g(at(t, 32), 0) } else { n(0) };
        if channel >= n(0) && channel <= n(2) {
            selected = i;
            component = channel.lo_bits();
            if component == 1 {
                component = 2;
            } else if component == 2 {
                component = 1;
            }
        }
        if selected == 65535 {
            return n(-4);
        }
        let chosen = at(state.trails, selected * 34 + 10);
        let value = (particle_sprite::color(chosen, component) - decrement) & 255;
        particle_sprite::put_color(chosen, component, value);
        if value < decrement {
            t[0u16] = 0;
            let count = g(state.core, 8).lo_bits() - 1;
            p(state.core, 8, integer::from_i16(count as i16));
            i += 1;
            continue;
        }
        let sprite = at(t, 10);
        if kind == 5 {
            p(at(t, 2), 1, g(at(t, 2), 1) - integer::sar(dt, 2));
        }
        if kind != 3 {
            particle_sprite::put_color(sprite, 1, particle_sprite::color(sprite, 1) - decrement);
            particle_sprite::put_color(sprite, 2, particle_sprite::color(sprite, 2) - decrement);
        } else if channel >= n(0) && channel <= n(2) {
            if channel != n(2) {
                clamp_color(sprite, 1, decrement);
            }
            if channel == n(1) {
                clamp_color(sprite, 0, decrement);
            } else {
                if channel == n(2) {
                    clamp_color(sprite, 0, decrement);
                }
                clamp_color(sprite, 2, decrement);
            }
        }
        t[1u16] = 0;
        let mut words = sprite;
        words[20u16] += integer::sar(fmul_lo32(dt, g(at(t, 8), 0)), 8).lo_bits();
        let mut j = 0u16;
        while j < 3 {
            p(
                sprite,
                1 + j,
                g(sprite, 1 + j) + integer::sar(fmul_lo32(g(at(t, 2), j), dt), 8),
            );
            j += 1;
        }
        if kind != 5 {
            p(sprite, 6, g(sprite, 6) + fmul_lo32(dt, n(2)));
            p(sprite, 7, g(sprite, 7) + fmul_lo32(dt, n(2)));
        }
        let angular = g(at(t, 8), 0);
        let mut delta = dt;
        if angular <= n(0) {
            delta = -dt;
        }
        p(at(t, 8), 0, angular - integer::shl(delta, 3));
        i += 1;
    }
    n(0)
}
