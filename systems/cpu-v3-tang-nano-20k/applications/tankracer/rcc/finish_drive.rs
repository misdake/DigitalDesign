//! Route driving used by the finish owner, over live vehicle and scene views.
#![link_cseg = "parents"]
use crate::dsl_rt::*;
use crate::{attitude, entities, integer, random, scene_world};

pub struct Routes {
    // Global obstacle ID -> compact row; 65535 is not a route point.
    pub indices: Array<u16>,
    // Per object: present, next0 kind/entity, next1 kind/entity, lap byte,
    // braking range (u16), gate size, maximum speed, width, lap flag.
    // Binding kinds: 0/1/2.
    pub points: Array<u16>,
    pub count: u16,
    pub point_count: u16,
    // Fixed colliders use DSEG2. 65535 retains the caller's mapping.
    pub collider_page: u16,
}
pub struct Car {
    pub present: bool,
    // Original AI state, in i32 indices: vehicle/colour/flags/spacing 0..4,
    // look position 4..7, look checkpoint presence/entity 7..9, drive checkpoint
    // presence/entity 9..11, look/drive branch 11..13, distance 13, turn/reverse
    // timers 14..16, stuck time/distance 16..18, confusion/anger 18..20,
    // target presence/id 20..22, awareness/aim/warn/mine 22..26,
    // view timer/span/behind span/shell error 26..30. Borrowed, never copied.
    pub state: Array<u16>,
    // Throttle, brake and steering, each a raw signed i32.
    pub controls: Array<u16>,
    pub horn: Array<u16>,
    pub position: Array<u16>,
    pub velocity: Array<u16>,
    pub reference: Array<u16>,
    pub local_speed: fix32,
    pub laps: i16,
    pub visible: bool,
}
pub struct Config {
    // Acceleration, braking power, look ahead, hidden gain multiplier/shift,
    // reverse period; then 4 steering, 4 inner and 9 speed rules (3 i32 each).
    // Columns 57..62: selected max speed, debunch accel and slow mul/shift.
    pub values: Array<u16>,
    pub angle: Array<i16>,
}
pub struct Frame {
    pub dt: fix32,
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
fn at(v: Array<u16>, w: u16) -> Array<u16> {
    unsafe { v.as_ptr().add(w as i16).as_u16_array() }
}
fn abs(v: fix32) -> fix32 {
    if v < n(0) {
        return -v;
    }
    v
}
// ID has already been validated by point/next; retain global IDs in identities.
pub fn row(r: &Routes, id: u16) -> Array<u16> {
    at(r.points, r.indices[id] * 22)
}
pub fn load_collider(r: &Routes, world: &scene_world::World, id: u16, out: Array<u16>) -> bool {
    let saved = read_dseg_page(2);
    if r.collider_page != 65535 && r.collider_page != saved {
        unsafe {
            write_dseg_page(2, r.collider_page);
        }
    }
    let found = scene_world::load_collider(world, id, out);
    if read_dseg_page(2) != saved {
        unsafe {
            write_dseg_page(2, saved);
        }
    }
    found
}
pub fn point(r: &Routes, ids: &entities::State, entity: fix32) -> fix32 {
    if entities::handle_kind(ids, entity) != 1 {
        return n(-4);
    }
    let id = entities::handle(ids, entity);
    if id.hi_bits() != 0 || id.lo_bits() >= r.count {
        return n(-4);
    }
    if r.indices[id.lo_bits()] >= r.point_count {
        return n(-4);
    }
    if g(row(r, id.lo_bits()), 0) == n(0) {
        return n(-4);
    }
    id
}
// -1 is a direct None binding; named bindings remain unresolved errors.
pub fn next(r: &Routes, id: u16, side: u16) -> fix32 {
    if side > 1 || id >= r.count {
        return n(-4);
    }
    if r.indices[id] >= r.point_count {
        return n(-4);
    }
    let row = row(r, id);
    let kind = g(row, 1 + side * 2);
    if kind == n(0) {
        return n(-1);
    }
    if kind == n(1) {
        return g(row, 2 + side * 2);
    }
    n(-5)
}
fn successor(r: &Routes, ids: &entities::State, s: Array<u16>, look: bool) -> fix32 {
    let mut index = 9u16;
    let mut side = 12u16;
    if look {
        index = 7;
        side = 11;
    }
    if g(s, index) == n(0) {
        return n(-4);
    }
    let id = point(r, ids, g(s, index + 1));
    if id < n(0) {
        return id;
    }
    let selected = g(s, side);
    if selected.hi_bits() != 0 || selected.lo_bits() > 1 {
        return n(-4);
    }
    let entity = next(r, id.lo_bits(), selected.lo_bits());
    if entity == n(-1) {
        return n(-4);
    }
    entity
}
pub fn branch(
    r: &Routes,
    ids: &entities::State,
    entity: fix32,
    lap: i16,
    seed: Array<u16>,
) -> fix32 {
    let id = point(r, ids, entity);
    if id < n(0) {
        return id;
    }
    let second = next(r, id.lo_bits(), 1);
    if second == n(-1) {
        return n(0);
    }
    if second < n(-1) {
        return second;
    }
    let mut side = 0u16;
    while side < 2 {
        let target = next(r, id.lo_bits(), side);
        if target == n(-1) {
            return n(-4);
        }
        if target < n(-1) {
            return target;
        }
        let target_id = point(r, ids, target);
        if target_id < n(0) {
            return target_id;
        }
        let row = row(r, target_id.lo_bits());
        let byte = g(row, 5).lo_bits() & 255;
        let tag = n(((byte << 8) as i16) >> 8);
        if tag != n(-1) {
            if n(lap) != tag && (tag != n(2) || lap < 3) {
                return fix32::from_words(1 - side, 0);
            }
            return fix32::from_words(side, 0);
        }
        side += 1;
    }
    let value = random::next(g(seed, 0));
    p(seed, 0, value);
    fix32::from_words((value.lo_bits() >> 4) & 1, 0)
}
pub fn throttle(
    car: &Car,
    r: &Routes,
    ids: &entities::State,
    config: &Config,
    frame: &Frame,
) -> fix32 {
    if !car.present {
        return n(-1);
    }
    let entity = successor(r, ids, car.state, false);
    if entity < n(0) {
        return entity;
    }
    let id = point(r, ids, entity);
    if id < n(0) {
        return id;
    }
    let row = row(r, id.lo_bits());
    if g(car.state, 13) < n(g(row, 6).lo_bits() as i16) && g(row, 8) < car.local_speed {
        p(car.controls, 0, n(0));
        p(car.controls, 1, g(config.values, 1));
    } else {
        // The finish owner always accelerates in this branch. Opponent speed
        // caps are pure computations whose result is unused for this owner.
        let mut value = g(config.values, 0);
        if !car.visible {
            value = integer::sar(
                integer::mul(value, g(config.values, 3)),
                g(config.values, 4).lo_bits(),
            );
        }
        p(car.controls, 0, value);
        if g(car.controls, 1) != n(0) {
            p(car.controls, 1, n(0));
        }
    }
    p(
        car.state,
        17,
        g(car.state, 17) + integer::sar(integer::mul(car.local_speed, frame.dt), 14),
    );
    p(car.state, 16, g(car.state, 16) + frame.dt);
    if g(car.state, 16) > n(1500) {
        if abs(g(car.state, 17)) < n(3072) {
            let flags = g(car.state, 2);
            p(
                car.state,
                2,
                fix32::from_words(flags.lo_bits() | 4, flags.hi_bits()),
            );
        }
        p(car.state, 17, n(0));
        p(car.state, 16, n(0));
    }
    n(0)
}
fn xz(out: Array<u16>, matrix: Array<u16>, x: fix32, z: fix32, inverse: bool) {
    let mut a = 6u16;
    let mut b = 2u16;
    if inverse {
        a = 2;
        b = 6;
    }
    p(
        out,
        0,
        integer::sar(
            integer::mul(x, g(matrix, 0)) + integer::mul(z, g(matrix, a)),
            12,
        ),
    );
    p(
        out,
        1,
        integer::sar(
            integer::mul(x, g(matrix, b)) + integer::mul(z, g(matrix, 8)),
            12,
        ),
    );
}
fn angle(out: Array<u16>, table: Array<i16>, x: fix32, z: fix32) -> fix32 {
    let mut a = x;
    let mut b = z;
    let mut steps = 0u16;
    while abs(a) >= fix32::from_words(32768, 0) || abs(b) >= fix32::from_words(32768, 0) {
        if steps >= 32 {
            return n(-3);
        }
        a = integer::sar(a, 1);
        b = integer::sar(b, 1);
        steps += 1;
    }
    let yaw = attitude::angle(table, a, b);
    if yaw == n(-1) {
        return n(-2);
    }
    p(out, 0, yaw);
    n(0)
}
fn rule(value: fix32, distance: fix32, rules: Array<u16>, count: u16, controls: Array<u16>) {
    let mut index = count;
    while index != 0 {
        index -= 1;
        let row = at(rules, index * 6);
        if g(row, 0) < abs(distance) {
            p(
                controls,
                2,
                integer::sar(integer::mul(value, g(row, 1)), g(row, 2).lo_bits()),
            );
            return;
        }
    }
}
pub fn steer(
    car: &Car,
    r: &Routes,
    ids: &entities::State,
    world: &scene_world::World,
    config: &Config,
    frame: &Frame,
) -> fix32 {
    if !car.present {
        return n(-1);
    }
    let entity = successor(r, ids, car.state, true);
    if entity < n(0) {
        return entity;
    }
    let id = point(r, ids, entity);
    if id < n(0) {
        return id;
    }
    let raw: Buf<u16, 60> = Buf::new([0; 60]);
    let collider = raw.as_array();
    if !load_collider(r, world, id.lo_bits(), collider) {
        return n(-4);
    }
    if g(collider, 29) == n(0) {
        return n(-4);
    }
    let position = at(collider, 2);
    let basis = at(collider, 26);
    let mut i = 0u16;
    while i < 3 {
        p(
            car.state,
            4 + i,
            g(car.position, i)
                + integer::sar(integer::mul(g(config.values, 2), g(car.velocity, i)), 14),
        );
        i += 1;
    }
    if g(car.state, 2).lo_bits() & 4 != 0 {
        return n(0);
    }
    let temp: Buf<u16, 4> = Buf::new([0; 4]);
    let v = temp.as_array();
    xz(
        v,
        basis,
        g(car.state, 4) - g(position, 0),
        g(car.state, 6) - g(position, 2),
        true,
    );
    let x = g(v, 0);
    let z = g(v, 1);
    let a = abs(x);
    let b = abs(z);
    let mut distance = a + b - integer::sar(a, 1);
    if b <= a {
        distance = a + b - integer::sar(b, 1);
    }
    p(car.state, 13, distance);
    xz(
        v,
        basis,
        integer::sar(g(car.velocity, 0), 6),
        integer::sar(g(car.velocity, 2), 6),
        true,
    );
    let vx = g(v, 0);
    let vz = g(v, 1);
    let mut cross = n(0);
    if vz != n(0) {
        let numerator = integer::mul(vx, z);
        if numerator == fix32::from_words(0, 32768) && vz == n(-1) {
            return n(-2);
        }
        cross = x - integer::div(numerator, vz);
    }
    let row = row(r, id.lo_bits());
    let inner = car.visible && abs(cross) <= g(row, 7);
    let mut aim_x = n(0);
    let mut aim_z = n(0);
    if inner {
        let side = branch(r, ids, entity, car.laps, frame.seed);
        if side < n(0) {
            return side;
        }
        let first = next(r, id.lo_bits(), side.lo_bits());
        if first < n(-1) {
            return first;
        }
        if first != n(-1) {
            let again = branch(r, ids, entity, car.laps, frame.seed);
            if again < n(0) {
                return again;
            }
            let target = next(r, id.lo_bits(), again.lo_bits());
            if target == n(-1) {
                return n(-4);
            }
            if target < n(-1) {
                return target;
            }
            let target_id = point(r, ids, target);
            if target_id < n(0) {
                return target_id;
            }
            let next_raw: Buf<u16, 60> = Buf::new([0; 60]);
            let next_collider = next_raw.as_array();
            if !load_collider(r, world, target_id.lo_bits(), next_collider) {
                return n(-4);
            }
            aim_x = g(next_collider, 1);
            aim_z = g(next_collider, 3);
        }
        aim_x = aim_x - g(car.state, 4);
        aim_z = aim_z - g(car.state, 6);
    } else {
        let mut width = g(row, 7);
        if cross < n(1) {
            width = -width;
        }
        if car.visible {
            width = integer::sar(width, 1);
        }
        xz(v, basis, width, n(0), false);
        aim_x = g(v, 0) + g(position, 0) - g(car.state, 4);
        aim_z = g(v, 1) + g(position, 2) - g(car.state, 6);
    }
    xz(v, car.reference, aim_x, aim_z, true);
    let status = angle(v, config.angle, g(v, 0), g(v, 1));
    if status != n(0) {
        return status;
    }
    let mut rules = at(config.values, 12);
    if inner {
        rules = at(config.values, 36);
    }
    rule(g(v, 0), distance, rules, 4, car.controls);
    if !inner {
        rule(
            g(car.controls, 2),
            car.local_speed,
            at(config.values, 60),
            9,
            car.controls,
        );
    }
    n(0)
}
pub fn reverse(
    car: &Car,
    r: &Routes,
    ids: &entities::State,
    world: &scene_world::World,
    config: &Config,
    frame: &Frame,
) -> fix32 {
    if !car.present {
        return n(-1);
    }
    if g(car.state, 9) == n(0) {
        return n(-4);
    }
    let previous = point(r, ids, g(car.state, 10));
    if previous < n(0) {
        return previous;
    }
    let entity = successor(r, ids, car.state, false);
    if entity < n(0) {
        return entity;
    }
    let next_id = point(r, ids, entity);
    if next_id < n(0) {
        return next_id;
    }
    let timer = g(car.state, 15) + frame.dt;
    p(car.state, 15, timer);
    let period = g(config.values, 5);
    let limit = integer::sar(integer::mul(period, n(5)), 3);
    let mut id = next_id;
    let backwards = timer < limit;
    if backwards {
        p(car.controls, 0, -fix32::from_words(65535, 0));
        id = previous;
    } else if timer < period {
        p(car.controls, 0, fix32::from_words(0, 65535));
    } else {
        p(car.controls, 0, fix32::from_words(0, 1));
    }
    let raw: Buf<u16, 60> = Buf::new([0; 60]);
    let collider = raw.as_array();
    if !load_collider(r, world, id.lo_bits(), collider) {
        return n(-4);
    }
    let mut x = g(collider, 1) - g(car.position, 0);
    let mut z = g(collider, 3) - g(car.position, 2);
    if backwards {
        x = -x;
        z = -z;
    }
    let temp: Buf<u16, 4> = Buf::new([0; 4]);
    let v = temp.as_array();
    xz(v, car.reference, x, z, true);
    let status = angle(v, config.angle, g(v, 0), g(v, 1));
    if status != n(0) {
        return status;
    }
    p(car.controls, 2, integer::mul(g(v, 0), n(16)));
    if timer >= limit && integer::mul(period, n(2)) < timer {
        p(car.state, 15, n(0));
        p(car.state, 17, n(0));
        let flags = g(car.state, 2);
        p(
            car.state,
            2,
            fix32::from_words(flags.lo_bits() & 65531, flags.hi_bits()),
        );
        p(car.state, 16, n(-3500));
    }
    n(0)
}
// The near finish-driver stage follows the parent Tank update. Combat is not
// part of this owner. Caller supplies already validated cup/difficulty tuning.
pub fn controls(
    car: &Car,
    r: &Routes,
    ids: &entities::State,
    world: &scene_world::World,
    config: &Config,
    frame: &Frame,
) -> fix32 {
    if !car.present {
        return n(-1);
    }
    p(car.horn, 0, n(0));
    if g(car.state, 2).lo_bits() & 4 == 0 {
        if g(car.state, 18) == n(0) {
            let status = throttle(car, r, ids, config, frame);
            if status != n(0) {
                return status;
            }
            let status = steer(car, r, ids, world, config, frame);
            if status != n(0) {
                return status;
            }
            if abs(g(car.controls, 2)) < fix32::from_words(48929, 2)
                || g(car.state, 2).lo_bits() & 4 != 0
            {
                p(car.state, 14, n(0));
            } else {
                let time = g(car.state, 14) + frame.dt;
                p(car.state, 14, time);
                if time > n(50) {
                    p(car.controls, 0, n(0));
                    p(car.controls, 1, n(10000));
                }
            }
        } else {
            p(car.controls, 0, n(0));
            let mut value = g(car.state, 18) - frame.dt;
            if value < n(0) {
                value = n(0);
            }
            p(car.state, 18, value);
        }
    } else {
        let status = reverse(car, r, ids, world, config, frame);
        if status != n(0) {
            return status;
        }
    }
    let cap = fix32::from_words(65535, 0);
    let mut i = 0u16;
    while i < 3 {
        let mut low = -cap;
        if i == 1 {
            low = n(0);
        }
        let mut value = g(car.controls, i);
        if value < low {
            value = low;
        } else if value > cap {
            value = cap;
        }
        p(car.controls, i, value);
        i += 1;
    }
    let mut anger = g(car.state, 19);
    if anger < n(5001) {
        anger = anger - integer::sar(frame.dt, 2);
    } else {
        p(car.horn, 0, n(1));
        let value = random::next(g(frame.seed, 0));
        p(frame.seed, 0, value);
        anger = anger - fix32::from_words((value.lo_bits() >> 4) & 4095, 0);
    }
    if anger < n(0) {
        anger = n(0);
    }
    p(car.state, 19, anger);
    let spacing = g(car.state, 3);
    if spacing == n(1) || spacing == n(2) {
        let mut offset = 58u16;
        if spacing == n(2) {
            offset = 60;
        }
        let threshold = integer::sar(
            integer::mul(g(config.values, 57), g(config.values, offset)),
            g(config.values, offset + 1).lo_bits(),
        );
        if spacing == n(1) {
            let mut throttle = n(0);
            if car.local_speed < threshold {
                throttle = cap;
            }
            p(car.controls, 0, throttle);
        } else if car.local_speed > threshold {
            p(car.controls, 0, n(0));
        }
    }
    n(0)
}
