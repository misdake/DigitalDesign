//! Single-car physical tick with real ground queries and persistent track poses.
use crate::dsl_rt::*;
use crate::{attitude, body, deformation, ground, integer, sampling, suspension, vehicle};
pub struct Car {
    pub entity: fix32,
    // Dense vehicle storage index; a finish clone retains its original racer slot.
    pub owner: u16,
    // Immutable resource profile; a finish clone shares its source profile.
    pub profile: u16,
    pub slot: u16,
    pub player: bool,
    pub visible: bool,
    pub blast: Array<u16>,
    pub marker: Array<u16>,
    pub attitude: Array<u16>,
    pub suspension: Array<u16>,
    pub drive: Array<u16>,
    pub motion: Array<u16>,
    pub boost: Array<u16>,
    pub tracks: Array<u16>,
    pub ground: ground::State,
    pub articulation: Buf<u16, 8>,
    pub lean: Buf<u16, 8>,
    pub hull: Buf<u16, 37>,
    pub ready: bool,
    pub turret_z: fix32,
    pub gun: Array<i16>,
    pub gun_binding: Array<u16>,
}
pub struct Config {
    pub spring: Array<u16>,
    pub drive: Array<u16>,
    pub attitude: Array<u16>,
    // travel gain/shift, track length, two track offsets.
    pub pose: Array<u16>,
    pub sampling: Array<u16>,
    pub auxiliary: Array<u16>,
    pub environment: Array<u16>,
    pub body: Array<u16>,
    pub sine: Array<i16>,
    pub angle: Array<i16>,
}
fn n(v: i16) -> fix32 {
    integer::from_i16(v)
}
fn copy(mut to: Array<u16>, from: Array<u16>, count: u16) {
    let mut i = 0u16;
    while i < count {
        to[i] = from[i];
        i += 1;
    }
}
pub fn tracks(car: &mut Car, c: &Config) -> u16 {
    let mut side = 0u16;
    while side < 2 {
        let t = unsafe { car.tracks.as_ptr().add((side * 24) as i16).as_u16_array() };
        let shift = attitude::get(c.pose, 1).lo_bits();
        let front = integer::trunc_shift(attitude::get(car.suspension, side * 12 + 3), shift);
        let rear = integer::trunc_shift(attitude::get(car.suspension, side * 12 + 9), shift);
        let mut difference = front - rear;
        if difference < n(0) {
            difference = -difference;
        }
        let mut angle = attitude::angle(c.angle, difference, attitude::get(c.pose, 2));
        if angle < n(0) {
            return 6;
        }
        if front < rear {
            angle = -angle;
        }
        let mut i = 0u16;
        while i < 9 {
            attitude::put(t, i, attitude::get(car.attitude, 9 + i));
            i += 1;
        }
        i = 9;
        while i < 12 {
            attitude::put(t, i, n(0));
            i += 1;
        }
        let x = attitude::get(c.pose, 3 + side * 3);
        let y = attitude::get(c.pose, 4 + side * 3) + integer::sar(front + rear, 1);
        let z = attitude::get(c.pose, 5 + side * 3);
        // Position uses the unrotated body basis; pitch rotates the track afterwards.
        let wx = sampling::axis(t, 0, x, y, z) + attitude::get(car.motion, 3);
        let wy = sampling::axis(t, 1, x, y, z) + attitude::get(car.motion, 4);
        let wz = sampling::axis(t, 2, x, y, z) + attitude::get(car.motion, 5);
        attitude::put(t, 9, wx);
        attitude::put(t, 10, wy);
        attitude::put(t, 11, wz);
        attitude::rotate(t, 0, 0, angle, c.sine);
        side += 1;
    }
    0
}
pub fn tick(
    car: &mut Car,
    c: &Config,
    world: &mut ground::World,
    f: Array<u16>,
    events: Array<u16>,
    surface: &mut deformation::Surface,
) -> u16 {
    vehicle::boost(car.boost, c.drive, f);
    let status = contact_and_power(car, c, world, f, events, surface);
    if status != 0 {
        return status;
    }
    let status = integrate_and_pose(car, c, f);
    if status != 0 {
        return status;
    }
    finish(car, f);
    0
}
pub fn contact_and_power(
    car: &mut Car,
    c: &Config,
    world: &mut ground::World,
    f: Array<u16>,
    mut events: Array<u16>,
    surface: &mut deformation::Surface,
) -> u16 {
    // The original contact adapter commits sampling/response/RNG only on success.
    let mut pending = ground::zero();
    let saved = unsafe { view_of(&mut car.ground).as_ptr().as_u16_array() };
    let candidate = unsafe { view_of(&mut pending).as_ptr().as_u16_array() };
    copy(candidate, saved, 206);
    attitude::put(
        pending.effect.as_array(),
        4,
        attitude::get(car.articulation.as_array(), 1),
    );
    attitude::put(
        pending.effect.as_array(),
        5,
        attitude::get(car.articulation.as_array(), 3),
    );
    attitude::put(pending.effect.as_array(), 6, car.turret_z);
    let response: Buf<u16, 88> = Buf::new([0; 88]);
    let s = response.as_array();
    copy(s, car.suspension, 88);
    let environment: Buf<u16, 36> = Buf::new([0; 36]);
    let e = environment.as_array();
    copy(e, c.environment, 36);
    let sample_env: Buf<u16, 20> = Buf::new([0; 20]);
    let se = sample_env.as_array();
    attitude::put(e, 0, attitude::get(f, 0));
    attitude::put(e, 2, attitude::get(f, 2));
    attitude::put(e, 5, attitude::get(car.motion, 9));
    attitude::put(e, 6, attitude::get(car.motion, 11));
    attitude::put(se, 0, attitude::get(f, 0));
    attitude::put(se, 1, attitude::get(f, 26));
    attitude::put(se, 2, attitude::get(f, 2));
    attitude::put(se, 3, attitude::get(f, 29));
    let local = unsafe { se.as_ptr().add(8).as_u16_array() };
    copy(local, c.auxiliary, 12);
    let mut i = 0u16;
    while i < 4 {
        pending.points[i].travel = attitude::get(s, i * 6 + 3);
        i += 1;
    }
    let before = world.seed;
    let mut status = ground::sample(&mut pending, world, car.tracks, c.sampling, se, surface);
    if status != 0 {
        world.seed = before;
        return status;
    }
    i = 0;
    while i < 4 {
        attitude::put(s, i * 6, pending.points[i].kind);
        attitude::put(s, i * 6 + 1, pending.points[i].gap);
        i += 1;
    }
    i = 0;
    while i < 2 {
        attitude::put(s, 34 + i, pending.auxiliary[i].kind);
        attitude::put(s, 36 + i, pending.auxiliary[i].cached);
        i += 1;
    }
    attitude::put(s, 24, attitude::get(car.attitude, 21));
    attitude::put(s, 25, attitude::get(car.attitude, 23));
    attitude::put(s, 31, attitude::get(car.motion, 1));
    attitude::put(s, 32, attitude::get(car.motion, 4));
    suspension::prepare(s, c.spring, events);
    let flags = suspension::conditions(e);
    i = 0;
    while i < 4 {
        let force = suspension::force(s, c.spring, e, i, flags, events);
        let side = i / 2;
        if force <= n(0) && attitude::get(s, 36 + side) == n(0) {
            status = ground::auxiliary(&mut pending, world, car.tracks, se, side);
            if status >= 2 {
                world.seed = before;
                return status;
            }
            attitude::put(s, 34 + side, pending.auxiliary[side].kind);
            attitude::put(s, 36 + side, n(1));
            events[0u16] |= 1u16 << side;
        }
        suspension::contact(
            s,
            i * 6,
            c.spring,
            force,
            attitude::get(f, 0),
            attitude::get(s, 34 + side),
        );
        attitude::put(s, 29, attitude::get(s, 29) + attitude::get(s, i * 6 + 3));
        pending.points[i].travel = attitude::get(s, i * 6 + 3);
        i += 1;
    }
    suspension::complete(s, e, events);
    i = 0;
    while i < 2 {
        pending.auxiliary[i].kind = attitude::get(s, 34 + i);
        pending.auxiliary[i].cached = attitude::get(s, 36 + i);
        i += 1;
    }
    copy(car.suspension, s, 88);
    car.ground = pending;
    attitude::put(
        car.articulation.as_array(),
        1,
        attitude::get(car.ground.effect.as_array(), 4),
    );
    attitude::put(
        car.articulation.as_array(),
        3,
        attitude::get(car.ground.effect.as_array(), 5),
    );
    car.turret_z = attitude::get(car.ground.effect.as_array(), 6);
    attitude::put(car.attitude, 21, attitude::get(s, 24));
    attitude::put(car.attitude, 23, attitude::get(s, 25));
    attitude::put(car.motion, 1, attitude::get(s, 31));
    attitude::put(car.motion, 4, attitude::get(s, 32));
    i = 0;
    while i < 3 {
        attitude::put(car.attitude, 26 + i, attitude::get(s, 41 + i));
        i += 1;
    }
    attitude::put(car.motion, 12, attitude::get(s, 29));
    attitude::put(car.motion, 13, attitude::get(s, 28));
    attitude::put(car.drive, 15, car.ground.load);
    attitude::put(f, 5, car.ground.material0);
    attitude::put(f, 6, car.ground.material1);
    attitude::put(f, 15, car.ground.airborne);
    i = 0;
    while i < 4 {
        attitude::put(f, 7 + i, car.ground.points[i].kind);
        attitude::put(f, 11 + i, car.ground.points[i].y);
        i += 1;
    }
    status = vehicle::power_stage(
        car.attitude,
        car.drive,
        car.motion,
        c.drive,
        f,
        attitude::get(car.suspension, 30),
    );
    if status != 0 {
        return 16 + status;
    }
    0
}
pub fn integrate_and_pose(car: &mut Car, c: &Config, f: Array<u16>) -> u16 {
    vehicle::integrate_position(car.motion, f);
    if !vehicle::attitude_stage(car.attitude, car.drive, c.attitude, f, c.sine, c.angle) {
        return 7;
    }
    let tuning: Buf<u16, 14> = Buf::new([0; 14]);
    let cfg = tuning.as_array();
    copy(cfg, c.body, 12);
    attitude::put(cfg, 6, attitude::get(f, 0));
    let status = body::articulate(car.articulation.as_array(), cfg, car.gun, car.gun_binding);
    if status != 0 {
        return 32 + status;
    }
    if attitude::get(f, 2) != n(0) && attitude::get(f, 1) != n(2) && attitude::get(f, 1) != n(3) {
        body::lean(
            car.lean.as_array(),
            unsafe { car.drive.as_ptr().add(26).as_u16_array() },
            cfg,
        );
    }
    let status = tracks(car, c);
    if status != 0 {
        return status;
    }
    vehicle::publish_pose(car.suspension, car.attitude, car.motion, c.pose);
    0
}
pub fn finish(car: &mut Car, f: Array<u16>) {
    let current = unsafe { car.motion.as_ptr().add(6).as_u16_array() };
    let previous = unsafe { car.motion.as_ptr().add(12).as_u16_array() };
    let contacts = unsafe { car.ground.points.as_array().as_ptr().add(6).as_u16_array() };
    body::hull_points(car.hull.as_array(), current, previous, contacts, 17, 4);
    car.ready = true;
    vehicle::finish_rates(car.motion, attitude::get(f, 0));
}
