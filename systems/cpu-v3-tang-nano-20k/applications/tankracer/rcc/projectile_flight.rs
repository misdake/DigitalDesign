//! Complete flight phase: steering, carrier, car sweep, terrain cadence and trails.
use crate::dsl_rt::*;
use crate::{
    attitude, collision, ground, integer, particle_sprite, projectile, projectile_sweep,
    projectile_trail, trig_lookup,
};
pub struct Frame {
    pub dt: fix32,
    pub number: fix32,
    pub entity: fix32,
    pub carrier_present: bool,
    pub carrier_basis: Array<u16>,
    pub carrier_position: Array<u16>,
    pub carrier_released: bool,
    pub cars: projectile_sweep::Cars,
    // Presence followed by four bytes in u16 lanes.
    pub overlay: Array<u16>,
}
pub struct Action {
    // 0 flying, 1 trails, 2 retire, 3 expired, 4 impact.
    pub kind: u16,
    pub origin: Buf<u16, 6>,
    pub mark: Buf<u16, 6>,
    pub has_mark: bool,
    pub contact: projectile_sweep::Hit,
    pub has_contact: bool,
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
fn at(v: Array<u16>, words: u16) -> Array<u16> {
    unsafe { v.as_ptr().add(words as i16).as_u16_array() }
}
fn abs(v: fix32) -> fix32 {
    if v < n(0) {
        return -v;
    }
    v
}
fn copy(mut to: Array<u16>, from: Array<u16>, count: u16) {
    let mut i = 0u16;
    while i < count {
        to[i] = from[i];
        i += 1;
    }
}
pub fn zero() -> Action {
    Action {
        kind: 0,
        origin: Buf::new([0; 6]),
        mark: Buf::new([0; 6]),
        has_mark: false,
        contact: projectile_sweep::zero(),
        has_contact: false,
    }
}
fn steer(
    matrix: Array<u16>,
    velocity: Array<u16>,
    position: Array<u16>,
    target: Array<u16>,
    offset: fix32,
    ctx: &projectile::Context,
) -> fix32 {
    let x = g(target, 0) - g(position, 0);
    let z = g(target, 2) - g(position, 2);
    let mut a = integer::sar(fmul_lo32(x, g(matrix, 0)) + fmul_lo32(z, g(matrix, 2)), 12) + offset;
    let mut b = integer::sar(fmul_lo32(x, g(matrix, 6)) + fmul_lo32(z, g(matrix, 8)), 12);
    if abs(a) > fix32::from_words(32768, 0) || abs(b) > fix32::from_words(32768, 0) {
        a = integer::sar(a, 1);
        b = integer::sar(b, 1);
    }
    let angle = attitude::angle(ctx.tables.angle, a, b);
    if angle == n(-1) {
        return n(-2);
    }
    let mut angle = -angle;
    let limit = g(ctx.resources.tuning, 14);
    if angle < -limit {
        angle = -limit;
    }
    if angle > limit {
        angle = limit;
    }
    attitude::rotate(matrix, 0, 1, angle, ctx.tables.sine);
    collision::yaw(velocity, angle, ctx.tables.sine);
    n(0)
}
pub fn advance(
    state: &projectile::State,
    pose: &projectile::Pose,
    frame: &Frame,
    ctx: &projectile::Context,
    map: &mut ground::World,
    out: &mut Action,
) -> fix32 {
    out.kind = 0;
    out.has_mark = false;
    out.has_contact = false;
    let s = state.core;
    let sprite = state.sprite;
    let dt = frame.dt;
    let kind = g(s, 2).lo_bits();
    let mut gravity = g(ctx.resources.tuning, 0);
    if kind == 1 {
        p(s, 24, g(s, 24) + dt);
        if g(s, 24) > n(60) {
            p(s, 24, n(0));
            let u = particle_sprite::uv(sprite, 0);
            if u < 31 {
                let a = u + 32;
                let b = particle_sprite::uv(sprite, 2) + 32;
                particle_sprite::put_uv(sprite, 0, a);
                particle_sprite::put_uv(sprite, 2, b);
                particle_sprite::put_uv(ctx.shared.concussion, 0, a);
                particle_sprite::put_uv(ctx.shared.concussion, 2, b);
            } else {
                particle_sprite::put_uv(sprite, 0, 0);
                if particle_sprite::uv(sprite, 1) < 31 {
                    particle_sprite::put_uv(sprite, 2, 31);
                    particle_sprite::put_uv(sprite, 1, particle_sprite::uv(sprite, 1) + 32);
                    particle_sprite::put_uv(sprite, 3, particle_sprite::uv(sprite, 3) + 32);
                } else {
                    particle_sprite::rectangle(sprite, 0, 0, 31, 31);
                }
                let mut i = 0u16;
                while i < 4 {
                    particle_sprite::put_uv(
                        ctx.shared.concussion,
                        i,
                        particle_sprite::uv(sprite, i),
                    );
                    i += 1;
                }
            }
        }
        gravity = g(ctx.resources.tuning, 1);
    } else if kind == 4 {
        if g(s, 4).lo_bits() & 16 != 0 {
            attitude::rotate(
                pose.basis,
                0,
                0,
                integer::shl(dt, (frame.entity.lo_bits() & 3) + 2),
                ctx.tables.sine,
            );
            attitude::rotate(
                pose.basis,
                0,
                2,
                integer::shl(dt, ((frame.entity.lo_bits() >> 2) & 3) + 1),
                ctx.tables.sine,
            );
            if (frame.entity.lo_bits() ^ frame.number.lo_bits()) & 7 != 0
                && !attitude::normalize(pose.basis, 0)
            {
                return n(-2);
            }
            gravity = integer::sar(gravity, 2);
        } else {
            let pitch = attitude::angle(ctx.tables.angle, g(s, 6), g(s, 5));
            if pitch == n(-1) {
                return n(-2);
            }
            let yaw = attitude::angle(ctx.tables.angle, g(s, 5), g(s, 7));
            if yaw == n(-1) {
                return n(-2);
            }
            projectile::basis(pose.basis, pitch, -yaw, ctx.tables.sine);
        }
    }
    if kind == 0 || kind == 4 || kind == 5 {
        let mut sprite = sprite;
        sprite[20u16] += integer::sar(fmul_lo32(n(9318), dt), 8).lo_bits();
    }
    let mut target = 65535u16;
    if g(s, 14) != n(0) {
        target = projectile_sweep::find(&frame.cars, g(s, 15));
    }
    if kind == 5 && target != 65535 {
        let status = steer(
            pose.basis,
            at(s, 10),
            pose.position,
            frame.cars.records[target].position,
            n(0),
            ctx,
        );
        if status != n(0) {
            return status;
        }
    }
    if g(s, 4).lo_bits() & 3 != 0 {
        if g(s, 4).lo_bits() & 2 != 0 {
            let mut view = 0u16;
            while view < 2 {
                if g(s, 11 + view) != n(0) {
                    let mut overlay = frame.overlay;
                    if g(s, 13) >= n(0) {
                        overlay[0u16] = 1;
                        overlay[1u16] = g(s, 13).lo_bits() & 255;
                        overlay[2u16] = 255;
                        overlay[3u16] = 200;
                        if kind == 4 {
                            overlay[3u16] = 128;
                        }
                        overlay[4u16] = 128;
                    } else {
                        overlay[0u16] = 0;
                        let value = g(s, 4);
                        p(
                            s,
                            4,
                            fix32::from_words(value.lo_bits() & 65533, value.hi_bits()),
                        );
                    }
                }
                view += 1;
            }
            p(
                s,
                13,
                integer::from_i16((g(s, 13).lo_bits() - integer::sar(dt, 1).lo_bits()) as i16),
            );
        }
        out.kind = 1;
        if g(s, 8) == n(0) && ((kind != 1 && kind != 4) || g(s, 4).lo_bits() & 2 == 0) {
            out.kind = 2;
        }
        return n(0);
    }
    let old: Buf<u16, 6> = Buf::new([0; 6]);
    copy(old.as_array(), pose.position, 6);
    let mut height = fix32::from_words(1, 32768);
    if g(s, 4).lo_bits() & 4 != 0 {
        if !frame.carrier_present {
            return n(-4);
        }
        collision::transform(pose.position, frame.carrier_basis, at(s, 54), false);
        let mut i = 0u16;
        while i < 3 {
            p(
                pose.position,
                i,
                g(pose.position, i) + g(frame.carrier_position, i),
            );
            i += 1;
        }
        if frame.carrier_released {
            let value = g(s, 4);
            p(
                s,
                4,
                fix32::from_words(value.lo_bits() | 1, value.hi_bits()),
            );
        }
    } else {
        p(
            pose.position,
            0,
            g(pose.position, 0) + integer::sar(fmul_lo32(dt, g(s, 5)), 8),
        );
        p(
            pose.position,
            2,
            g(pose.position, 2) + integer::sar(fmul_lo32(dt, g(s, 7)), 8),
        );
        if kind == 2 {
            if (frame.entity.lo_bits() ^ frame.number.lo_bits()) & 1 == 0 || g(s, 3) == n(0) {
                let plane = map.height(g(pose.position, 0), g(pose.position, 2));
                if plane.status >= 2 {
                    return n(-2);
                }
                height = plane.height;
                if height != fix32::from_words(1, 32768) {
                    p(s, 16, height + n(2048));
                    p(
                        pose.position,
                        1,
                        g(pose.position, 1) + integer::sar(g(s, 16) - g(pose.position, 1), 1),
                    );
                }
            } else {
                height = g(s, 16);
                p(pose.position, 1, height);
            }
            if g(s, 20) == n(0) {
                return n(-4);
            }
            if g(pose.position, 1) != g(s, 18) {
                let distance = attitude::length2(
                    g(pose.position, 0) - g(s, 17),
                    g(pose.position, 2) - g(s, 19),
                );
                let pitch =
                    attitude::angle(ctx.tables.angle, g(s, 18) - g(pose.position, 1), distance);
                if pitch == n(-1) {
                    return n(-2);
                }
                attitude::rotate(state.reference, 0, 0, -pitch - g(s, 21), ctx.tables.sine);
                p(s, 21, -pitch);
            }
            copy(at(s, 34), pose.position, 6);
            if g(ctx.shared.core, 2) != frame.number {
                let mut size = g(ctx.shared.homing, 6);
                let step = integer::shl(dt, 4);
                if g(ctx.shared.core, 3) != n(0) {
                    size = size + step;
                    if size >= n(500) {
                        p(ctx.shared.core, 3, n(0));
                        size = n(500);
                    }
                } else {
                    size = size - step;
                    if size <= n(300) {
                        p(ctx.shared.core, 3, n(1));
                        size = n(300);
                    }
                }
                if size < n(0) {
                    size = n(0);
                }
                p(ctx.shared.homing, 6, size);
                p(ctx.shared.homing, 7, size);
                let color = fix32::from_words(particle_sprite::color(ctx.shared.homing, 1), 0);
                let mut value = n(0);
                if g(ctx.shared.core, 4) != n(0) {
                    value = color + integer::sar(dt, 1);
                    if value > n(80) {
                        p(ctx.shared.core, 4, n(0));
                        value = n(80);
                    }
                } else {
                    value = color - integer::sar(dt, 1);
                    if value < n(0) {
                        p(ctx.shared.core, 4, n(1));
                        value = n(0);
                    }
                }
                particle_sprite::put_color(ctx.shared.homing, 1, value.lo_bits());
            }
            p(ctx.shared.core, 2, frame.number);
            let lookup = ((g(s, 3).lo_bits() * 48) & 65532) >> 2;
            let wobble = integer::sar(trig_lookup::index(ctx.tables.sine, lookup) - g(s, 23), 5);
            p(s, 23, integer::from_i16(wobble.lo_bits() as i16));
            p(
                pose.position,
                0,
                g(pose.position, 0) + integer::sar(fmul_lo32(g(s, 23), g(state.reference, 0)), 12),
            );
            p(
                pose.position,
                2,
                g(pose.position, 2) + integer::sar(fmul_lo32(g(s, 23), g(state.reference, 2)), 12),
            );
            if g(s, 3) < g(ctx.resources.tuning, 13) && target != 65535 {
                let status = steer(
                    state.reference,
                    at(s, 10),
                    pose.position,
                    frame.cars.records[target].position,
                    g(ctx.shared.core, 1),
                    ctx,
                );
                if status != n(0) {
                    return status;
                }
            }
            copy(pose.basis, state.reference, 18);
            p(
                s,
                22,
                integer::from_i16(
                    (g(s, 22).lo_bits() + integer::sar(fmul_lo32(dt, n(16773)), 8).lo_bits())
                        as i16,
                ),
            );
            attitude::rotate(pose.basis, 0, 2, g(s, 22), ctx.tables.sine);
        } else {
            p(s, 6, g(s, 6) - integer::sar(fmul_lo32(gravity, dt), 8));
            p(
                pose.position,
                1,
                g(pose.position, 1) + integer::sar(fmul_lo32(dt, g(s, 6)), 8),
            );
        }
    }
    let mut sweep = projectile_sweep::Frame {
        dt: dt,
        hit: view_of(&mut out.contact),
    };
    let status = projectile_sweep::cars(
        &frame.cars,
        g(s, 1),
        g(s, 0) != n(0),
        old.as_array(),
        pose.position,
        &mut sweep,
    );
    if status < n(0) {
        return status;
    }
    if status == n(1) {
        out.kind = 4;
        out.has_contact = true;
        copy(out.origin.as_array(), old.as_array(), 6);
        return n(0);
    }
    if (frame.entity.lo_bits() ^ frame.number.lo_bits()) & 3 == 0 {
        if kind != 2 {
            let plane = map.height(g(pose.position, 0), g(pose.position, 2));
            if plane.status >= 2 {
                return n(-2);
            }
            height = plane.height;
        }
        if height == fix32::from_words(1, 32768) && kind != 4 {
            out.kind = 4;
            copy(out.origin.as_array(), old.as_array(), 6);
            return n(0);
        }
        if g(pose.position, 1) < height {
            out.kind = 4;
            copy(out.origin.as_array(), old.as_array(), 6);
            p(out.origin.as_array(), 1, height + n(1024));
            if kind != 4 {
                out.has_mark = true;
                copy(out.mark.as_array(), old.as_array(), 6);
                p(out.mark.as_array(), 1, height);
            }
            return n(0);
        }
    }
    p(s, 3, g(s, 3) + dt);
    if g(s, 3) > n(6000) {
        let flags = g(s, 4);
        p(
            s,
            4,
            fix32::from_words(flags.lo_bits() | 1, flags.hi_bits()),
        );
        out.kind = 3;
        return n(0);
    }
    let status = projectile_trail::emit(state, pose, ctx, dt);
    if status != n(0) {
        return status;
    }
    projectile_trail::fade(state, dt)
}
