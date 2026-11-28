//! Pickup activation, parachute descent, settling and ground spin.
use crate::dsl_rt::*;
use crate::{attitude, collision, ground, integer, terrain};

pub struct Pose {
    pub basis: Array<u16>,
    pub position: Array<u16>,
}
pub struct State {
    // angular XYZ, normal XYZ, blink time/count, delay, disabled,
    // parachute-present, ground, fall range and regeneration.
    pub core: Array<u16>,
    // Nine basis values followed by position XYZ, when present.
    pub parachute: Array<u16>,
}
pub struct Frame {
    pub dt: fix32,
    pub number: fix32,
    pub entity: fix32,
    pub player: Array<u16>,
    pub camera: Array<u16>,
    pub visibility: u16,
    pub auxiliary_active: bool,
    pub separation: fix32,
    pub diameter: fix32,
    pub sine: Array<i16>,
    pub angle: Array<i16>,
}
fn n(value: i16) -> fix32 {
    integer::from_i16(value)
}
fn g(data: Array<u16>, index: u16) -> fix32 {
    attitude::get(data, index)
}
fn p(data: Array<u16>, index: u16, value: fix32) {
    attitude::put(data, index, value);
}
fn magnitude(value: fix32) -> fix32 {
    if value < n(0) {
        return -value;
    }
    value
}
fn flag(data: Array<u16>, set: u16) {
    let value = g(data, 0);
    p(
        data,
        0,
        fix32::from_words(value.lo_bits() | set, value.hi_bits()),
    );
}
fn pivot(pose: &Pose, angular: Array<u16>, height: fix32, sine: Array<i16>) {
    let before: Buf<u16, 6> = Buf::new([0; 6]);
    let mut i = 0u16;
    while i < 3 {
        p(
            before.as_array(),
            i,
            integer::sar(fmul_lo32(g(pose.basis, i + 3), height), 12),
        );
        i += 1;
    }
    attitude::rotate(pose.basis, 0, 1, g(angular, 1), sine);
    attitude::rotate(pose.basis, 0, 0, g(angular, 0), sine);
    attitude::rotate(pose.basis, 0, 2, g(angular, 2), sine);
    i = 0;
    while i < 3 {
        let after = integer::sar(fmul_lo32(g(pose.basis, i + 3), height), 12);
        p(
            pose.position,
            i,
            g(pose.position, i) + g(before.as_array(), i) - after,
        );
        i += 1;
    }
}
fn cap_angle(value: fix32, cap: fix32) -> fix32 {
    if value != n(0) && magnitude(value) > cap {
        if value < n(0) {
            return -cap;
        }
        return cap;
    }
    value
}
// 0 None, 1 immediate activation, 2 immediate auxiliary update, negative error.
// The scene owner commits external-basis presence on every return and performs
// the selected immediate action before finish_frame reads the live flags.
pub fn phase(
    state: &State,
    pose: &Pose,
    flags: Array<u16>,
    frame: &Frame,
    map: &mut ground::World,
) -> fix32 {
    let s = state.core;
    let old = g(flags, 0).lo_bits();
    let dt = frame.dt;
    if old & 1 == 0 {
        if frame.auxiliary_active {
            return n(2);
        }
        if g(s, 9) != n(0) {
            return n(0);
        }
        if g(s, 13) != n(-1) && g(s, 8) != n(0) {
            p(s, 8, g(s, 8) - dt);
            if g(s, 8) > n(0) {
                return n(0);
            }
            p(s, 8, n(0));
        }
        if magnitude(g(pose.position, 0) - g(frame.player, 0)) < g(s, 12)
            && magnitude(g(pose.position, 2) - g(frame.player, 2)) < g(s, 12)
        {
            flag(flags, 1);
            return n(1);
        }
    } else if old & 8 == 0 {
        if old & 4 == 0 {
            let floor = g(s, 11) + frame.diameter;
            if floor < g(pose.position, 1) {
                p(
                    pose.position,
                    1,
                    g(pose.position, 1) + integer::sar(fmul_lo32(n(-1310), dt), 8),
                );
            } else {
                p(pose.position, 1, floor);
                let mut i = 0u16;
                while i < 9 {
                    p(state.parachute, i, g(pose.basis, i));
                    i += 1;
                }
                i = 0;
                while i < 3 {
                    p(state.parachute, 9 + i, g(pose.position, i));
                    i += 1;
                }
                p(s, 10, n(1));
                flag(flags, 4);
                let mut normal = terrain::Normal {
                    x: g(s, 3),
                    y: g(s, 4),
                    z: g(s, 5),
                };
                let plane =
                    map.normal_height(g(pose.position, 0), g(pose.position, 2), &mut normal);
                p(s, 3, normal.x);
                p(s, 4, normal.y);
                p(s, 5, normal.z);
                if plane.status == 2 {
                    return n(-2);
                }
                if plane.status > 2 {
                    return n(-1);
                }
                p(s, 11, plane.height);
            }
            let angle = integer::sar(dt, 1);
            let mut z = angle;
            if g(pose.basis, 3) > n(0) {
                z = -angle;
            }
            let mut x = -angle;
            if g(pose.basis, 5) > n(0) {
                x = angle;
            }
            p(s, 2, g(s, 2) + z);
            p(s, 0, g(s, 0) + x);
            pivot(pose, s, frame.separation, frame.sine);
        } else {
            let local: Buf<u16, 6> = Buf::new([0; 6]);
            let normal: Buf<u16, 6> = Buf::new([0; 6]);
            let mut i = 0u16;
            while i < 3 {
                p(normal.as_array(), i, g(s, 3 + i));
                i += 1;
            }
            collision::transform(local.as_array(), pose.basis, normal.as_array(), true);
            let pitch =
                attitude::angle(frame.angle, g(local.as_array(), 1), g(local.as_array(), 2));
            if pitch == n(-1) {
                return n(-2);
            }
            let roll =
                attitude::angle(frame.angle, -g(local.as_array(), 0), g(local.as_array(), 1));
            if roll == n(-1) {
                return n(-2);
            }
            let pitch = cap_angle(pitch - n(16384), fmul_lo32(dt, n(16)));
            let roll = cap_angle(-roll, fmul_lo32(dt, n(16)));
            if pitch != n(0) {
                attitude::rotate(pose.basis, 0, 0, pitch, frame.sine);
            }
            if roll != n(0) {
                attitude::rotate(pose.basis, 0, 2, roll, frame.sine);
            }
            if g(s, 10) == n(0) {
                return n(-4);
            }
            let parachute = state.parachute;
            p(
                parachute,
                10,
                g(parachute, 10) + integer::sar(fmul_lo32(n(-1310), dt), 10),
            );
            let mut turn = dt;
            if pitch > n(0) {
                turn = -dt;
            }
            attitude::rotate(parachute, 0, 0, turn, frame.sine);
            turn = dt;
            if roll > n(0) {
                turn = -dt;
            }
            attitude::rotate(parachute, 0, 2, turn, frame.sine);
            attitude::rotate(
                pose.basis,
                0,
                1,
                integer::sar(fmul_lo32(n(4659), dt), 8),
                frame.sine,
            );
        }
    } else {
        let cap = fmul_lo32(fix32::from_words(frame.visibility, 0), n(128));
        if magnitude(integer::sar(g(pose.position, 0), 3) - g(frame.camera, 0)) < cap
            && magnitude(integer::sar(g(pose.position, 2), 3) - g(frame.camera, 2)) < cap
        {
            attitude::rotate(
                pose.basis,
                0,
                1,
                integer::sar(fmul_lo32(n(4659), dt), 8),
                frame.sine,
            );
            if (frame.entity.lo_bits() ^ frame.number.lo_bits()) & 255 == 0
                && !attitude::normalize(pose.basis, 0)
            {
                return n(-2);
            }
        }
    }
    n(0)
}
pub fn finish_frame(state: &State, flags: Array<u16>, dt: fix32) {
    let s = state.core;
    let old = g(flags, 0);
    if old.lo_bits() & 8 == 0 && old.lo_bits() & 4 != 0 {
        p(s, 6, g(s, 6) + dt);
        if g(s, 6) > n(30) {
            p(s, 6, n(0));
            let before = g(s, 7);
            p(
                flags,
                0,
                fix32::from_words(old.lo_bits() ^ 2, old.hi_bits()),
            );
            p(s, 7, before + n(1));
            if before == n(10) {
                flag(flags, 8);
            }
        }
    }
}
