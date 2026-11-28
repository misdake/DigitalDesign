//! Mine arming, repeat lifetime, terrain alignment and direct vehicle impulse.
use crate::dsl_rt::*;
use crate::{attitude, collision, ground, integer, projectile, terrain};

pub struct State {
    // Owner presence/ID, kind, u8 count, repeat, armed, born/last frame, elapsed.
    pub core: Array<u16>,
}
pub struct Vehicle {
    pub base_position: Array<u16>,
    pub velocity: Array<u16>,
    pub angular: Array<u16>,
    pub external: Array<u16>,
    pub has_external: bool,
    pub health: Array<u16>,
    pub collision_size: Array<u16>,
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

pub fn contact(state: &State, frame: fix32) -> bool {
    let s = state.core;
    if g(s, 5) != n(0) {
        return true;
    }
    if frame != g(s, 6) && g(s, 7) != frame - n(1) {
        p(s, 5, n(1));
    } else {
        p(s, 7, frame);
    }
    false
}
// 0 None, 1 Repeat, 2 Retire. A repeat discards elapsed remainder.
pub fn tick(state: &State, dt: fix32) -> u16 {
    let s = state.core;
    let elapsed = g(s, 8) + dt;
    p(s, 8, elapsed);
    let count = g(s, 3).lo_bits() & 255;
    if g(s, 4) == n(0) || (count > 2 && count < 128) {
        if elapsed > n(20000) {
            return 2;
        }
    } else if elapsed > n(250) {
        p(s, 8, n(0));
        p(s, 3, fix32::from_words((count + 1) & 255, 0));
        return 1;
    }
    0
}
pub fn align(
    pose: &projectile::Pose,
    map: &mut ground::World,
    tables: &projectile::Tables,
) -> fix32 {
    let mut normal = terrain::Normal {
        x: n(0),
        y: n(0),
        z: n(0),
    };
    let result = map.normal_height(g(pose.position, 0), g(pose.position, 2), &mut normal);
    if result.status > 1 {
        return n(-2);
    }
    if result.status != 1 {
        return n(-4);
    }
    let vector: Buf<u16, 6> = Buf::new([0; 6]);
    let local: Buf<u16, 6> = Buf::new([0; 6]);
    p(vector.as_array(), 0, normal.x);
    p(vector.as_array(), 1, normal.y);
    p(vector.as_array(), 2, normal.z);
    collision::transform(local.as_array(), pose.basis, vector.as_array(), true);
    let pitch = attitude::angle(tables.angle, g(local.as_array(), 1), g(local.as_array(), 2));
    if pitch == n(-1) {
        return n(-2);
    }
    let roll = attitude::angle(
        tables.angle,
        -g(local.as_array(), 0),
        g(local.as_array(), 1),
    );
    if roll == n(-1) {
        return n(-2);
    }
    attitude::rotate(pose.basis, 0, 0, pitch - n(16384), tables.sine);
    attitude::rotate(pose.basis, 0, 2, -roll, tables.sine);
    n(0)
}
pub fn point_hit(vehicle: &Vehicle, point: Array<u16>) -> fix32 {
    let delta: Buf<u16, 6> = Buf::new([0; 6]);
    let impulse: Buf<u16, 6> = Buf::new([0; 6]);
    let local: Buf<u16, 6> = Buf::new([0; 6]);
    let mut i = 0u16;
    while i < 3 {
        let v = g(point, i) - g(vehicle.base_position, i);
        p(delta.as_array(), i, v);
        p(impulse.as_array(), i, fmul_lo32(-v, n(4)));
        p(local.as_array(), i, v);
        i += 1;
    }
    if vehicle.has_external {
        collision::transform(local.as_array(), vehicle.external, delta.as_array(), true);
    }
    let mut value = g(vehicle.angular, 2) - fmul_lo32(g(local.as_array(), 0), n(700));
    if abs(value) < fix32::from_words(13568, 12) {
        if value > n(0) {
            value = fix32::from_words(13568, 12);
        } else {
            value = -fix32::from_words(13568, 12);
        }
    }
    if abs(value) > fix32::from_words(20352, 18) {
        if value > n(0) {
            value = fix32::from_words(20352, 18);
        } else {
            value = -fix32::from_words(20352, 18);
        }
    }
    p(vehicle.angular, 2, value);
    p(
        vehicle.angular,
        0,
        g(vehicle.angular, 0) + fmul_lo32(g(local.as_array(), 2), n(200)),
    );
    let x = g(local.as_array(), 0);
    let z = g(local.as_array(), 2);
    if (x < n(0) && z > n(0)) || (x > n(0) && z < n(0)) {
        p(
            vehicle.angular,
            1,
            g(vehicle.angular, 1) + fix32::from_words(18928, 2),
        );
    } else if (x < n(0) && z < n(0)) || (x > n(0) && z > n(0)) {
        p(
            vehicle.angular,
            1,
            g(vehicle.angular, 1) - fix32::from_words(18928, 2),
        );
    }
    i = 0;
    while i < 3 {
        p(
            vehicle.velocity,
            i,
            integer::trunc_shift(fmul_lo32(g(vehicle.velocity, i), n(850)), 10),
        );
        i += 1;
    }
    // The source publishes angular response and damping before this lookup.
    if !vehicle.has_external {
        return n(-4);
    }
    collision::transform(local.as_array(), vehicle.external, impulse.as_array(), true);
    let mut lateral = g(local.as_array(), 0);
    if abs(lateral) < n(8000) {
        if lateral > n(0) {
            lateral = n(8000);
        } else {
            lateral = n(-8000);
        }
    }
    i = 0;
    while i < 3 {
        p(delta.as_array(), i, integer::sar(g(vehicle.velocity, i), 6));
        i += 1;
    }
    collision::transform(local.as_array(), vehicle.external, delta.as_array(), true);
    p(
        local.as_array(),
        0,
        g(local.as_array(), 0) + integer::sar(lateral, 1),
    );
    collision::transform(delta.as_array(), vehicle.external, local.as_array(), false);
    i = 0;
    while i < 3 {
        p(
            vehicle.velocity,
            i,
            fmul_lo32(g(delta.as_array(), i), n(64)),
        );
        i += 1;
    }
    if g(vehicle.health, 0) < n(0) {
        p(vehicle.health, 0, n(0));
    }
    p(vehicle.collision_size, 0, n(200));
    n(0)
}
