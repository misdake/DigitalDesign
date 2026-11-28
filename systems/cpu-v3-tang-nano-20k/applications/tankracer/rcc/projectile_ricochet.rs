//! Shield deflection, ordered peer redirection and projectile owner transfer.
use crate::dsl_rt::*;
use crate::{attitude, collision, integer, projectile, projectile_sweep, random};

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
fn slow(state: &projectile::State) {
    let mut i = 5u16;
    while i < 8 {
        p(
            state.core,
            i,
            integer::trunc_shift(fmul_lo32(g(state.core, i), n(700)), 10),
        );
        i += 1;
    }
}
fn deflect(state: &projectile::State, ctx: &projectile::Context) {
    let mut angle = n(projectile::random_word(ctx.seed) as i16);
    let s = state.core;
    if angle < n(-24570) || angle > n(24570) {
        p(s, 6, g(s, 6) + n(9175));
        if angle < n(0) {
            angle = n(-24570);
        } else {
            angle = n(24570);
        }
    } else {
        p(
            s,
            6,
            g(s, 6) + fix32::from_words(projectile::random_word(ctx.seed) % 5242, 0) + n(2621),
        );
    }
    collision::yaw(at(s, 10), angle, ctx.tables.sine);
}
pub fn ownerless(state: &projectile::State, player: fix32, ctx: &projectile::Context) -> fix32 {
    let s = state.core;
    if g(s, 2) != n(4) || g(s, 0) != n(0) {
        return n(-5);
    }
    p(ctx.seed, 0, random::next(g(ctx.seed, 0)));
    deflect(state, ctx);
    p(s, 0, n(1));
    p(s, 1, player);
    slow(state);
    n(0)
}
pub fn reflect(
    state: &projectile::State,
    pose: &projectile::Pose,
    vehicle: u16,
    cars: &projectile_sweep::Cars,
    ctx: &projectile::Context,
) -> fix32 {
    if !cars.present || vehicle >= cars.count || !cars.records[vehicle].present {
        return n(-4);
    }
    let target = cars.records[vehicle];
    let s = state.core;
    let mut owner = 65535u16;
    if g(s, 0) != n(0) {
        owner = projectile_sweep::find(cars, g(s, 1));
        if owner == 65535 {
            return n(-4);
        }
    }
    let roll = projectile::random_word(ctx.seed) % 10;
    let mut redirect = false;
    if roll == 0 && owner != 65535 {
        redirect = attitude::length2(
            g(cars.records[owner].position, 0) - g(target.position, 0),
            g(cars.records[owner].position, 2) - g(target.position, 2),
        ) < fix32::from_words(35840, 0);
    }
    if redirect {
        let mut closest = fix32::from_words(8192, 78);
        let mut chosen = 65535u16;
        let mut slot = 2u16;
        while slot < 9 {
            let id = cars.slots[slot];
            if id != 65535 && id != vehicle {
                if id >= cars.count || !cars.records[id].present {
                    return n(-4);
                }
                let car = cars.records[id];
                if car.visible {
                    let x = g(target.position, 0) - g(car.position, 0);
                    if abs(x) < closest {
                        let z = g(target.position, 2) - g(car.position, 2);
                        if abs(z) < closest {
                            closest = integer::sar(x + z, 1);
                            chosen = id;
                        }
                    }
                }
            }
            slot += 1;
        }
        if chosen != 65535 {
            let direction: Buf<u16, 6> = Buf::new([0; 6]);
            let d = direction.as_array();
            let mut i = 0u16;
            while i < 3 {
                p(d, i, g(s, 5 + i));
                i += 1;
            }
            let mut shifts = 0u16;
            while abs(g(d, 0)) >= fix32::from_words(32768, 0)
                || abs(g(d, 1)) >= fix32::from_words(32768, 0)
                || abs(g(d, 2)) >= fix32::from_words(32768, 0)
            {
                if shifts >= 32 {
                    return n(-3);
                }
                i = 0;
                while i < 3 {
                    p(d, i, integer::sar(g(d, i), 1));
                    i += 1;
                }
                shifts += 1;
            }
            let yaw = attitude::angle(ctx.tables.angle, -g(d, 0), g(d, 2));
            if yaw == n(-1) {
                return n(-2);
            }
            let pitch = attitude::angle(
                ctx.tables.angle,
                g(d, 1),
                attitude::length2(g(d, 0), g(d, 2)),
            );
            if pitch == n(-1) {
                return n(-2);
            }
            i = 0;
            while i < 9 {
                let mut v = n(0);
                if i == 0 || i == 4 || i == 8 {
                    v = n(4096);
                }
                p(pose.basis, i, v);
                i += 1;
            }
            attitude::rotate(pose.basis, 0, 1, yaw, ctx.tables.sine);
            attitude::rotate(pose.basis, 0, 0, pitch, ctx.tables.sine);
            p(
                d,
                0,
                g(cars.records[chosen].position, 0) - g(target.position, 0),
            );
            p(d, 1, n(0));
            p(
                d,
                2,
                g(cars.records[chosen].position, 2) - g(target.position, 2),
            );
            let local: Buf<u16, 6> = Buf::new([0; 6]);
            collision::transform(local.as_array(), pose.basis, d, true);
            let turn = attitude::angle(
                ctx.tables.angle,
                g(local.as_array(), 0),
                g(local.as_array(), 2),
            );
            if turn == n(-1) {
                return n(-2);
            }
            collision::yaw(at(s, 10), -turn, ctx.tables.sine);
            p(s, 6, g(s, 6) + n(1310));
        }
    } else {
        deflect(state, ctx);
    }
    // Primary lookup is required only for the concussion ownership exception.
    let mut transfer = true;
    if g(s, 2) == n(1) {
        let primary = cars.slots[0u16];
        if primary == 65535 || primary >= cars.count || !cars.records[primary].present {
            return n(-4);
        }
        if g(s, 0) != n(0) && g(s, 1) == cars.records[primary].entity {
            transfer = false;
        }
    }
    if transfer {
        p(s, 0, n(1));
        p(s, 1, target.entity);
    }
    slow(state);
    n(0)
}
