//! Explosion sprites, smoke and contact fragments on caller-owned state.
use crate::dsl_rt::*;
use crate::{attitude, collision, integer, particle_sprite, projectile, trig_lookup};
pub struct State {
    // 21 i32: slot, timer, angular, visible, smoke timer/count, band/ring,
    // owner presence/ID, velocity XYZ, attachment XYZ, fragments enabled/count,
    // fragment time and fragment-vehicle presence/ID. Two packed UV words follow.
    pub core: Array<u16>,
    pub uv: Array<u16>,
    pub sprite: Array<u16>,
    // Ten 34-word rows: sprite[22], velocity[6], lift[2], angular[2], active/pad.
    pub smoke: Array<u16>,
    // Fifteen 18-word rows: position[6], velocity[6], size[2], angular[2], rotation/pad.
    pub fragments: Array<u16>,
}
pub struct Context {
    // Main/smoke material, small flag, packed damping and map-fog flag (i32).
    pub resources: Array<u16>,
    pub seed: Array<u16>,
    pub sine: Array<i16>,
}
pub struct Frame {
    pub dt: fix32,
    pub origin: Array<u16>,
    pub has_owner_pose: bool,
    pub owner_basis: Array<u16>,
    pub owner_position: Array<u16>,
}
pub struct Contact {
    pub owner: fix32,
    pub vehicle_velocity: Array<u16>,
    pub local_point: Array<u16>,
    pub world_point: Array<u16>,
    pub velocity: Array<u16>,
    pub kind: u16,
    pub spin: fix32,
    pub has_body: bool,
    pub radius: i16,
}
fn n(v: i16) -> fix32 {
    integer::from_i16(v)
}
fn at(data: Array<u16>, offset: u16) -> Array<u16> {
    unsafe { data.as_ptr().add(offset as i16).as_u16_array() }
}
fn g(data: Array<u16>, i: u16) -> fix32 {
    attitude::get(data, i)
}
fn p(data: Array<u16>, i: u16, value: fix32) {
    attitude::put(data, i, value)
}
pub fn view(data: Array<u16>) -> State {
    State {
        core: data,
        uv: at(data, 42),
        sprite: at(data, 44),
        smoke: at(data, 66),
        fragments: at(data, 406),
    }
}
pub fn initialize(state: &State, origin: Array<u16>, first: fix32, ctx: &Context) {
    let s = state.sprite;
    let mut i = 0u16;
    while i < 3 {
        p(s, 1 + i, g(origin, i));
        i += 1;
    }
    p(state.core, 16, fix32::from_words(first.lo_bits() & 1, 0));
    particle_sprite::rectangle(s, 0, 0, 31, 31);
    let draw = projectile::random_word(ctx.seed);
    // The product must retain 32 bits before division.
    p(
        state.core,
        2,
        integer::div(
            fmul_lo32(
                fix32::from_words(draw % 40 + 55, 0),
                fix32::from_words(0xb600, 0),
            ),
            n(1000),
        ),
    );
    let mut sprite = s;
    sprite[20u16] = fmul_lo32(first, n(182)).lo_bits();
    let mut size = n(3000);
    if g(ctx.resources, 2) != n(0) {
        size = n(2000);
    }
    p(s, 6, size);
    p(s, 7, size);
    p(s, 5, n(-6));
    particle_sprite::rgb(s, 128, 128, 128);
    p(s, 4, n(1));
    p(s, 0, g(ctx.resources, 0));
}
pub fn finish_spawn(state: &State) {
    p(state.core, 3, n(1));
    p(state.core, 6, n(-1));
    p(state.core, 7, n(-1));
}
pub fn contact(state: &State, hit: &Contact, ctx: &Context) -> fix32 {
    let core = state.core;
    p(core, 8, n(1));
    p(core, 9, hit.owner);
    p(core, 10, integer::sar(g(hit.vehicle_velocity, 2), 6));
    let attachment = at(core, 26);
    let mut i = 0u16;
    while i < 3 {
        p(attachment, i, g(hit.local_point, i));
        i += 1;
    }
    collision::normalize(attachment);
    if !hit.has_body {
        return n(-4);
    }
    i = 0;
    while i < 3 {
        p(
            attachment,
            i,
            integer::sar(fmul_lo32(n(hit.radius), g(attachment, i)), 9),
        );
        i += 1;
    }
    let mut count = 0u16;
    if hit.kind == 1 {
        count = 15;
    } else if g(core, 16) != n(0) {
        if hit.kind == 2 {
            count = 11;
        } else if hit.kind == 0 || hit.kind == 3 {
            count = 7;
        }
    }
    if count == 0 {
        return n(0);
    }
    let mut a = projectile::random_word(ctx.seed);
    let mut b = projectile::random_word(ctx.seed);
    if hit.spin >= n(1) {
        return n(0);
    }
    p(core, 17, fix32::from_words(count, 0));
    p(core, 19, n(1));
    p(core, 20, hit.owner);
    let mut uv = state.uv;
    uv[0u16] = (a % 111) | ((b % 111) << 8);
    uv[1u16] = (a % 111 + 16) | ((b % 111 + 16) << 8);
    i = 0;
    while i < count {
        let mut row = at(state.fragments, i * 18);
        let mut axis = 0u16;
        while axis < 3 {
            p(row, axis, g(hit.world_point, axis));
            p(row, 3 + axis, g(hit.velocity, axis));
            axis += 1;
        }
        row[12u16] = 175;
        row[13u16] = 175;
        p(row, 4, n(0));
        let velocity = at(row, 6);
        collision::normalize(velocity);
        let spread = fix32::from_words(a % 2500, 0) - n(1250);
        p(
            velocity,
            0,
            integer::sar(fmul_lo32(spread + g(velocity, 0), n(12000)), 12),
        );
        p(
            velocity,
            2,
            integer::sar(fmul_lo32(g(velocity, 2) - spread, n(12000)), 12),
        );
        p(
            velocity,
            1,
            integer::sar(fmul_lo32(fix32::from_words(b % 30, 0) - n(15), n(1024)), 2),
        );
        let mut angular = fix32::from_words(16364, 1);
        if a & 1 == 0 {
            angular = -angular;
        }
        p(at(row, 14), 0, angular);
        a = projectile::random_word(ctx.seed);
        b = projectile::random_word(ctx.seed);
        i += 1;
    }
    n(0)
}
pub fn tick_fragments(state: &State, dt: fix32, ctx: &Context) {
    let count = g(state.core, 17).lo_bits();
    if count == 0 {
        return;
    }
    let time = g(state.core, 18) + dt;
    p(state.core, 18, time);
    let wave =
        integer::sar(trig_lookup::index(ctx.sine, (time.lo_bits() & 255) << 6), 6).lo_bits() as i16;
    let mut i = 0u16;
    while i < count {
        let mut row = at(state.fragments, i * 18);
        let mut axis = 0u16;
        while axis < 3 {
            p(
                row,
                axis,
                g(row, axis) + integer::sar(fmul_lo32(g(row, 3 + axis), dt), 8),
            );
            axis += 1;
        }
        p(row, 4, g(row, 4) - fmul_lo32(dt, n(4)));
        let omega = g(at(row, 14), 0);
        row[16u16] = row[16u16] + integer::sar(fmul_lo32(dt, omega), 8).lo_bits();
        let mut step = dt;
        if omega < n(1) {
            step = -step;
        }
        p(at(row, 14), 0, omega - fmul_lo32(step, n(64)));
        let mut width = (row[12u16] as i16) + wave;
        let mut height = 350i16 - width;
        if width < 30 {
            width = 30;
        } else if width > 350 {
            width = 350;
        }
        if height < 30 {
            height = 30;
        } else if height > 350 {
            height = 350;
        }
        row[12u16] = width as u16;
        row[13u16] = height as u16;
        i += 1;
    }
}
// True retires before motion, shared blast work or smoke/RNG consumption.
pub fn begin_tick(state: &State, frame: &Frame, ctx: &Context) -> bool {
    let dt = frame.dt;
    tick_fragments(state, dt, ctx);
    let timer = g(state.core, 1) + dt;
    p(state.core, 1, timer);
    let s = state.sprite;
    if timer > n(30) {
        p(state.core, 1, n(0));
        if particle_sprite::uv(s, 0) < 95 {
            particle_sprite::put_uv(s, 0, particle_sprite::uv(s, 0) + 32);
            particle_sprite::put_uv(s, 2, particle_sprite::uv(s, 2) + 32);
        } else if particle_sprite::uv(s, 1) < 95 {
            particle_sprite::put_uv(s, 0, 0);
            particle_sprite::put_uv(s, 1, particle_sprite::uv(s, 1) + 32);
            particle_sprite::put_uv(s, 2, 31);
            particle_sprite::put_uv(s, 3, particle_sprite::uv(s, 3) + 32);
        } else {
            p(state.core, 3, n(0));
            let mut alive = false;
            let mut i = 0u16;
            while i < 10 {
                if at(state.smoke, i * 34)[32u16] != 0 {
                    alive = true;
                }
                i += 1;
            }
            if !alive {
                return true;
            }
        }
    }
    let omega = g(state.core, 2) - dt;
    p(state.core, 2, omega);
    let mut sprite = s;
    sprite[20u16] = sprite[20u16] + integer::sar(fmul_lo32(dt, omega), 8).lo_bits();
    let mut i = 0u16;
    while i < 3 {
        p(
            s,
            1 + i,
            g(s, 1 + i) + integer::sar(fmul_lo32(g(state.core, 10 + i), dt), 8),
        );
        i += 1;
    }
    let packed = g(ctx.resources, 3);
    i = 0;
    while i < 3 {
        if i != 1 {
            p(
                state.core,
                10 + i,
                integer::decay(
                    integer::rate_step(g(state.core, 10 + i), n(packed.lo_bits() as i16), dt),
                    packed.hi_bits() >> 8,
                    dt,
                ),
            );
        }
        i += 1;
    }
    false
}
// Shared band/ring updates belong between begin_tick and finish_tick.
pub fn finish_tick(state: &State, frame: &Frame, ctx: &Context) {
    let dt = frame.dt;
    let core = state.core;
    let count = g(core, 5).lo_bits();
    if count < 10 {
        let timer = g(core, 4) + dt;
        p(core, 4, timer);
        let mut interval = n(97);
        if g(core, 8) != n(0) {
            interval = n(65);
        }
        if timer >= interval {
            let a = projectile::random_word(ctx.seed);
            let b = projectile::random_word(ctx.seed);
            p(core, 4, n(0));
            let puff = at(state.smoke, count * 34);
            if g(core, 8) == n(0) {
                p(puff, 1, g(frame.origin, 0));
                p(puff, 2, g(frame.origin, 1) - n(1024));
                p(puff, 3, g(frame.origin, 2));
            } else if frame.has_owner_pose {
                let offset: Buf<u16, 6> = Buf::new([0; 6]);
                collision::transform(offset.as_array(), frame.owner_basis, at(core, 26), false);
                let mut i = 0u16;
                while i < 3 {
                    p(
                        puff,
                        1 + i,
                        g(offset.as_array(), i) + g(frame.owner_position, i),
                    );
                    i += 1;
                }
            } else {
                let mut i = 0u16;
                while i < 3 {
                    p(puff, 1 + i, g(frame.origin, i));
                    i += 1;
                }
            }
            p(puff, 0, g(ctx.resources, 1));
            particle_sprite::rectangle(puff, 0, 0, 63, 63);
            p(puff, 4, n(1));
            p(puff, 6, n(300));
            p(puff, 7, n(300));
            p(puff, 5, n(-16));
            particle_sprite::rgb(puff, 128, 128, 128);
            let mut words = puff;
            words[20u16] = 0;
            p(
                at(puff, 22),
                0,
                integer::sar(fmul_lo32(fix32::from_words(a & 1, 0), n(1024)) - n(1024), 3),
            );
            p(
                at(puff, 22),
                1,
                integer::sar(fmul_lo32(fix32::from_words(a % 3 + 2, 0), n(3072)), 4),
            );
            p(
                at(puff, 22),
                2,
                integer::sar(fmul_lo32(fix32::from_words(b & 1, 0), n(1024)) - n(1024), 3),
            );
            p(
                at(puff, 30),
                0,
                integer::sar(fmul_lo32(fix32::from_words(b % 360, 0) - n(180), n(182)), 2),
            );
            words[32u16] = 1;
            if g(core, 8) == n(0) {
                p(at(puff, 28), 0, n(1536));
            }
            p(core, 5, fix32::from_words((count + 1) & 255, 0));
        }
    }
    let mut i = 0u16;
    while i < 10 {
        let mut puff = at(state.smoke, i * 34);
        if puff[32u16] != 0 {
            let mut axis = 0u16;
            while axis < 3 {
                let mut speed = g(at(puff, 22), axis);
                if axis == 1 {
                    speed = speed + g(at(puff, 28), 0);
                }
                p(
                    puff,
                    1 + axis,
                    g(puff, 1 + axis) + integer::sar(fmul_lo32(speed, dt), 8),
                );
                axis += 1;
            }
            let lift = g(at(puff, 28), 0);
            if lift != n(0) {
                let mut next = lift - fmul_lo32(dt, n(8));
                if next < n(0) {
                    next = n(0);
                }
                p(at(puff, 28), 0, next);
            }
            p(puff, 6, g(puff, 6) + integer::sar(dt, 1));
            p(puff, 7, g(puff, 7) + integer::sar(dt, 1));
            let omega = g(at(puff, 30), 0);
            puff[20u16] = puff[20u16] + integer::sar(fmul_lo32(dt, omega), 8).lo_bits();
            let mut step = fmul_lo32(dt, n(2));
            if omega < n(1) {
                step = -step;
            }
            p(at(puff, 30), 0, omega - step);
            let fade = integer::sar(dt, 4);
            if fade < fix32::from_words(particle_sprite::color(puff, 0), 0) {
                axis = 0;
                while axis < 3 {
                    particle_sprite::put_color(
                        puff,
                        axis,
                        (fix32::from_words(particle_sprite::color(puff, axis), 0) - fade).lo_bits(),
                    );
                    axis += 1;
                }
            } else {
                puff[32u16] = 0;
            }
        }
        i += 1;
    }
}
// Public ordinary-explosion kernel: nonempty shared blast references require
// the scene's middle stage and preserve the original -5 failure prefix.
pub fn tick(state: &State, frame: &Frame, ctx: &Context) -> fix32 {
    if begin_tick(state, frame, ctx) {
        return n(1);
    }
    if g(state.core, 6) != n(-1) || g(state.core, 7) != n(-1) {
        return n(-5);
    }
    finish_tick(state, frame, ctx);
    n(0)
}
