//! Live projectile state and authored launch arithmetic, using direct views.
use crate::dsl_rt::*;
use crate::{attitude, collision, integer, particle_sprite, projectile_sweep, random, trig_lookup};

pub struct State {
    // 30 i32: owner presence/ID, kind, time, flags, velocity XYZ;
    // trail count/cursor/time, two flash views/alpha, target presence/ID/height;
    // previous XYZ, reference presence, pitch/roll/wobble, UV time,
    // attached-UFO presence/ID and attachment XYZ. Narrow fields are extended.
    pub core: Array<u16>,
    pub reference: Array<u16>,
    pub sprite: Array<u16>,
    // Fifteen 34-word records: active, i16 time, velocity XYZ, angular,
    // 22-word sprite and u32 channel. Total state: 610 words / 1220 bytes.
    pub trails: Array<u16>,
}
pub struct Pose {
    pub basis: Array<u16>,
    pub position: Array<u16>,
}
pub struct Shared {
    // sequence, homing drift/frame, glow size/color direction.
    pub core: Array<u16>,
    pub homing: Array<u16>,
    pub concussion: Array<u16>,
}
pub struct Resources {
    // gravity2, speed2, elevation2, snap6, homing speed/seek time/max steer.
    pub tuning: Array<u16>,
    pub main: Array<u16>,
    pub trail: Array<u16>,
    pub homing_tail: Array<u16>,
    pub cup: u16,
}
pub struct Tables {
    pub sine: Array<i16>,
    pub angle: Array<i16>,
}
pub struct Owner {
    pub entity: fix32,
    pub turret: Array<u16>,
    pub gun: Array<u16>,
    pub velocity: Array<u16>,
}
pub struct Context {
    pub shared: Shared,
    pub resources: Resources,
    pub tables: Tables,
    pub seed: Array<u16>,
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
pub fn view(data: Array<u16>) -> State {
    State {
        core: data,
        reference: at(data, 60),
        sprite: at(data, 78),
        trails: at(data, 100),
    }
}
pub fn shared(data: Array<u16>) -> Shared {
    Shared {
        core: data,
        homing: at(data, 10),
        concussion: at(data, 32),
    }
}
pub fn random_word(seed: Array<u16>) -> u16 {
    let value = random::next(g(seed, 0));
    p(seed, 0, value);
    (value.lo_bits() >> 4) | (value.hi_bits() << 12)
}
pub fn basis(out: Array<u16>, pitch: fix32, yaw: fix32, sine: Array<i16>) {
    let x = (pitch.lo_bits() & 65532) >> 2;
    let y = (yaw.lo_bits() & 65532) >> 2;
    let cx = trig_lookup::index(sine, 4096 + x);
    let sx = trig_lookup::index(sine, x);
    let cy = trig_lookup::index(sine, 4096 + y);
    let sy = trig_lookup::index(sine, y);
    p(out, 0, cy);
    p(out, 1, n(0));
    p(out, 2, sy);
    p(out, 3, integer::sar(fmul_lo32(sy, sx), 12));
    p(out, 4, cx);
    p(out, 5, -integer::sar(fmul_lo32(cy, sx), 12));
    p(out, 6, integer::sar(-fmul_lo32(cx, sy), 12));
    p(out, 7, sx);
    p(out, 8, integer::sar(fmul_lo32(cy, cx), 12));
}
// Homing drift is published after the complete launch, as in the caller.
pub fn launch(state: &State, pose: &Pose, owner: &Owner, kind: u16, ctx: &Context) -> fix32 {
    let s = state.core;
    let tuning = ctx.resources.tuning;
    p(s, 0, n(1));
    p(s, 1, owner.entity);
    p(s, 2, fix32::from_words(kind, 0));
    let mut pitch = attitude::angle(
        ctx.tables.angle,
        g(owner.turret, 7),
        attitude::length2(g(owner.turret, 6), g(owner.turret, 8)),
    );
    if pitch == n(-1) {
        return n(-2);
    }
    let mut drift = n(0);
    if kind == 2 {
        p(s, 21, pitch);
        copy(pose.basis, owner.turret, 18);
        copy(state.reference, pose.basis, 18);
        p(s, 20, n(1));
        let velocity: Buf<u16, 6> = Buf::new([0; 6]);
        p(velocity.as_array(), 2, g(tuning, 12));
        collision::transform(at(s, 10), pose.basis, velocity.as_array(), false);
        drift = fix32::from_words(random_word(ctx.seed) % 2000, 0) - n(1000);
        if drift >= n(-500) && drift < n(0) {
            drift = n(-500);
        } else if drift > n(0) && drift < n(500) {
            drift = n(500);
        }
    } else {
        let mut index = 0u16;
        if kind == 1 {
            index = 1;
        }
        let yaw = attitude::angle(ctx.tables.angle, g(owner.turret, 6), g(owner.turret, 8));
        if yaw == n(-1) {
            return n(-2);
        }
        if abs(pitch) < g(tuning, 6) {
            pitch = n(0);
        } else {
            let mut i = 1u16;
            while i < 6 {
                if abs(pitch) < g(tuning, 6 + i) {
                    if pitch > n(0) {
                        pitch = g(tuning, 5 + i);
                    } else {
                        pitch = -g(tuning, 5 + i);
                    }
                    break;
                }
                i += 1;
            }
        }
        let angle = g(tuning, 4 + index) + pitch;
        let lookup = (angle.lo_bits() & 65532) >> 2;
        p(s, 5, n(0));
        p(
            s,
            6,
            integer::trunc_shift(
                fmul_lo32(
                    g(tuning, 2 + index),
                    trig_lookup::index(ctx.tables.sine, lookup),
                ),
                12,
            ),
        );
        p(
            s,
            7,
            integer::trunc_shift(
                fmul_lo32(
                    g(tuning, 2 + index),
                    trig_lookup::index(ctx.tables.sine, 4096 + lookup),
                ),
                12,
            ),
        );
        collision::yaw(at(s, 10), -yaw, ctx.tables.sine);
        let mut i = 0u16;
        while i < 3 {
            p(
                s,
                5 + i,
                g(s, 5 + i) + integer::sar(g(owner.velocity, i), 6),
            );
            i += 1;
        }
    }
    copy(pose.position, owner.gun, 6);
    p(pose.position, 1, g(pose.position, 1) - n(150));
    if kind == 2 {
        p(s, 16, g(pose.position, 1));
        copy(at(s, 34), pose.position, 6);
    }
    if kind == 5 {
        copy(pose.basis, owner.turret, 18);
    }
    if kind == 2 {
        p(ctx.shared.core, 1, drift);
    }
    n(0)
}
pub fn sprite(state: &State, ctx: &Context) {
    let kind = g(state.core, 2).lo_bits();
    let s = state.sprite;
    p(s, 4, n(1));
    p(s, 0, g(ctx.resources.main, kind));
    let mut size = n(400);
    if kind == 0 {
        particle_sprite::rectangle(s, 0, 0, 63, 63);
        if ctx.resources.cup == 1 {
            size = n(430);
        } else if ctx.resources.cup == 2 {
            size = n(460);
        }
    } else if kind == 1 {
        p(s, 4, n(-1));
        particle_sprite::rectangle(s, 0, 0, 31, 31);
        size = n(450);
    } else if kind == 2 || kind == 4 {
        return;
    } else if kind == 3 {
        p(s, 4, n(-1));
        particle_sprite::rectangle(s, 0, 0, 63, 63);
        size = n(650);
    } else {
        particle_sprite::rectangle(s, 0, 0, 63, 63);
        p(s, 6, n(1400));
        p(s, 7, n(1400));
        particle_sprite::rgb(s, 0, 0, 128);
        let mut s = s;
        s[20u16] = random_word(ctx.seed);
        return;
    }
    p(s, 6, size);
    p(s, 7, size);
    particle_sprite::rgb(s, 128, 128, 128);
}
pub fn select_target(state: &State, cars: &projectile_sweep::Cars, tables: &Tables) -> fix32 {
    let s = state.core;
    let kind = g(s, 2).lo_bits();
    if kind != 2 && kind != 5 {
        return n(0);
    }
    if g(s, 0) == n(0) {
        return n(-4);
    }
    let owner = projectile_sweep::find(cars, g(s, 1));
    if owner == 65535 {
        return n(-4);
    }
    let own = &cars.records[owner];
    let mut best = fix32::from_words(40960, 15);
    p(s, 14, n(0));
    let mut slot = 0u16;
    while slot < 9 {
        let id = cars.slots[slot];
        if id == 65535 {
            slot += 1;
            continue;
        }
        if id >= cars.count || !cars.records[id].present {
            return n(-4);
        }
        let peer = &cars.records[id];
        if peer.entity == g(s, 1) || peer.ai_disabled {
            slot += 1;
            continue;
        }
        let delta: Buf<u16, 6> = Buf::new([0; 6]);
        let local: Buf<u16, 6> = Buf::new([0; 6]);
        let mut i = 0u16;
        while i < 3 {
            p(
                delta.as_array(),
                i,
                g(peer.position, i) - g(own.position, i),
            );
            i += 1;
        }
        collision::transform(local.as_array(), own.turret, delta.as_array(), true);
        if g(local.as_array(), 2) <= n(0) || g(local.as_array(), 2) >= fix32::from_words(36864, 1) {
            slot += 1;
            continue;
        }
        let mut count = 0u16;
        while abs(g(local.as_array(), 0)) > fix32::from_words(32768, 0)
            || abs(g(local.as_array(), 1)) > fix32::from_words(32768, 0)
            || abs(g(local.as_array(), 2)) > fix32::from_words(32768, 0)
        {
            if count >= 32 {
                return n(-3);
            }
            i = 0;
            while i < 3 {
                p(local.as_array(), i, integer::sar(g(local.as_array(), i), 1));
                i += 1;
            }
            count += 1;
        }
        let yaw = attitude::angle(tables.angle, g(local.as_array(), 0), g(local.as_array(), 2));
        if yaw == n(-1) {
            return n(-2);
        }
        if abs(yaw) < n(5460) && g(local.as_array(), 2) < best {
            best = g(local.as_array(), 2);
            p(s, 14, n(1));
            p(s, 15, peer.entity);
        }
        slot += 1;
    }
    n(0)
}
