//! Live waypoint queries, player lap/progress state and the raw-time race clock.
use crate::dsl_rt::*;
use crate::{attitude, collision, entities, integer, scene_world};

pub struct Routes {
    // Caller binds this view's bank for the route-query phase.
    // origin XZ, shifts XZ, radius XZ, backwards index, fallback presence/entity.
    pub header: Array<u16>,
    // Per object: presence, progress, next binding kind (0 none/1 direct/2 named),
    // next entity, lap byte, hairpin byte, width.
    pub points: Array<u16>,
    pub count: u16,
    pub ranges: Array<u16>,
    pub items: Array<u16>,
}
pub struct Car {
    // checkpoint presence/entity, passed, completed, records3, score,
    // previous score, wrong-way byte, warning timer (signed i16).
    pub route: Array<u16>,
    // elapsed, signed i16 laps, driving flag; shared with race_clock.
    pub race: Array<u16>,
}
pub struct Query {
    pub position: Array<u16>,
    pub current: fix32,
    pub lap: fix32,
    pub limit: fix32,
    pub nearest: bool,
    // Presence/value, committed only on success. Missing progress is value -1.
    pub result: Array<u16>,
}
pub struct Frame {
    pub position: Array<u16>,
    pub entity: fix32,
    pub number: fix32,
    pub dt: fix32,
    pub limit: fix32,
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
fn above(a: fix32, b: fix32) -> bool {
    a.hi_bits() > b.hi_bits() || (a.hi_bits() == b.hi_bits() && a.lo_bits() > b.lo_bits())
}
fn exhausted(used: fix32, limit: fix32) -> bool {
    !above(limit, used)
}
fn point(r: &Routes, ids: &entities::State, entity: fix32) -> fix32 {
    if entities::handle_kind(ids, entity) != 1 {
        return n(-4);
    }
    let id = entities::handle(ids, entity);
    if id.hi_bits() != 0 || id.lo_bits() >= r.count || g(at(r.points, id.lo_bits() * 14), 0) == n(0)
    {
        return n(-4);
    }
    id
}
pub fn search(r: &Routes, world: &scene_world::World, ids: &entities::State, q: &Query) -> fix32 {
    let mut current = at(r.points, 0);
    if !q.nearest {
        let id = point(r, ids, q.current);
        if id < n(0) {
            return id;
        }
        current = at(r.points, id.lo_bits() * 14);
    }
    if q.limit == n(0) {
        return n(-1);
    }
    let gx = integer::sar(g(q.position, 0) - g(r.header, 0), g(r.header, 2).lo_bits());
    let gz = integer::sar(g(q.position, 2) - g(r.header, 1), g(r.header, 3).lo_bits());
    let rx = g(r.header, 4);
    let rz = g(r.header, 5);
    let end_z = gz + rz;
    let end_x = gx + rx;
    let mut z = gz - rz;
    let mut used = n(0);
    let mut best = 65535u16;
    let mut distance = fix32::from_words(40960, 15);
    let collider: Buf<u16, 60> = Buf::new([0; 60]);
    let raw = collider.as_array();
    let delta: Buf<u16, 6> = Buf::new([0; 6]);
    let d = delta.as_array();
    while z <= end_z {
        if exhausted(used, q.limit) {
            return n(-3);
        }
        used = used + n(1);
        if z >= n(0) && z < n(8) {
            let mut x = gx - rx;
            while x <= end_x {
                if exhausted(used, q.limit) {
                    return n(-3);
                }
                used = used + n(1);
                if x >= n(0) && x < n(8) {
                    let cell = x.lo_bits() + z.lo_bits() * 8;
                    let start = r.ranges[cell * 2];
                    let count = r.ranges[cell * 2 + 1];
                    let mut index = 0u16;
                    while index < count {
                        if exhausted(used, q.limit) {
                            return n(-3);
                        }
                        used = used + n(1);
                        let id = g(r.items, start + index);
                        index += 1;
                        if id.hi_bits() != 0 || id.lo_bits() >= r.count {
                            return n(-4);
                        }
                        let row = at(r.points, id.lo_bits() * 14);
                        if g(row, 0) == n(0) {
                            return n(-4);
                        }
                        if g(row, 1) == n(-1) {
                            continue;
                        }
                        if !scene_world::load_collider(world, id.lo_bits(), raw) {
                            return n(-4);
                        }
                        let mut axis = 0u16;
                        let mut far = false;
                        while axis < 3 {
                            p(d, axis, g(q.position, axis) - g(raw, 1 + axis));
                            if (q.nearest || axis != 1)
                                && abs(g(d, axis)) >= fix32::from_words(22528, 2)
                            {
                                far = true;
                            }
                            axis += 1;
                        }
                        if far {
                            continue;
                        }
                        let mut value = n(0);
                        if q.nearest {
                            value = collision::distance(d);
                        } else {
                            if g(raw, 29) == n(0) {
                                return n(-4);
                            }
                            let basis = at(raw, 26);
                            let dx = g(d, 0);
                            let dz = g(d, 2);
                            if integer::mul(g(basis, 8), dz) + integer::mul(g(basis, 6), dx) >= n(0)
                            {
                                continue;
                            }
                            let mut width = n(0);
                            if g(row, 5) != n(0) && g(row, 1) - g(current, 1) != n(1) {
                                width =
                                    integer::mul(dx, g(basis, 0)) + integer::mul(g(basis, 2), dz);
                            }
                            if width != n(0) && abs(width) >= integer::sar(g(row, 6), 1) {
                                continue;
                            }
                            let ax = abs(dx);
                            let az = abs(dz);
                            let mut minimum = ax;
                            if az < minimum {
                                minimum = az;
                            }
                            value = az - integer::sar(minimum, 1) + ax;
                        }
                        if (q.nearest && above(distance, value)) || (!q.nearest && value < distance)
                        {
                            distance = value;
                            best = id.lo_bits();
                        }
                    }
                }
                x = x + n(1);
            }
        }
        z = z + n(1);
    }
    if best == 65535 {
        p(q.result, 0, n(0));
        let mut value = n(-1);
        if q.nearest {
            value = n(0);
        }
        p(q.result, 1, value);
        return n(0);
    }
    let row = at(r.points, best * 14);
    if !scene_world::load_collider(world, best, raw) {
        return n(-4);
    }
    let mut value = g(raw, 0);
    if !q.nearest {
        if g(row, 4) != n(0) {
            value = integer::mul(q.lap + n(1), fix32::from_words(16384, 156)) - distance;
            if g(current, 2) == n(2) {
                return n(-5);
            }
            let target = g(current, 3);
            if g(current, 2) == n(0) || target != g(raw, 0) {
                value = value - fix32::from_words(16384, 156);
            }
        } else {
            value = integer::mul(
                g(row, 1) + integer::mul(q.lap, n(50)),
                fix32::from_words(8192, 3),
            ) - distance;
            if g(current, 1) < g(row, 1) - n(2) {
                value = value - fix32::from_words(16384, 156);
            }
        }
    }
    p(q.result, 0, n(1));
    p(q.result, 1, value);
    n(0)
}
pub fn cross(
    r: &Routes,
    world: &scene_world::World,
    ids: &entities::State,
    car: &Car,
    position: Array<u16>,
    result: Array<u16>,
) -> fix32 {
    if g(car.route, 0) == n(0) {
        return n(-4);
    }
    let id = point(r, ids, g(car.route, 1));
    if id < n(0) {
        return id;
    }
    let current = at(r.points, id.lo_bits() * 14);
    let target = g(current, 3);
    if g(current, 2) == n(2) {
        return n(-5);
    }
    if g(current, 2) == n(0) {
        p(result, 0, n(0));
        return n(0);
    }
    let next_id = point(r, ids, target);
    if next_id < n(0) {
        return next_id;
    }
    let row = at(r.points, next_id.lo_bits() * 14);
    let collider: Buf<u16, 60> = Buf::new([0; 60]);
    let raw = collider.as_array();
    if !scene_world::load_collider(world, next_id.lo_bits(), raw) {
        return n(-4);
    }
    let delta: Buf<u16, 6> = Buf::new([0; 6]);
    let d = delta.as_array();
    let mut axis = 0u16;
    while axis < 3 {
        let value = g(position, axis) - g(raw, 1 + axis);
        if abs(value) >= fix32::from_words(51200, 0) {
            p(result, 0, n(0));
            return n(0);
        }
        p(d, axis, value);
        axis += 1;
    }
    if g(raw, 29) != n(0) {
        collision::transform(d, at(raw, 26), d, true);
    }
    if g(d, 2) <= n(0) || abs(g(d, 0)) >= integer::sar(g(row, 6), 1) {
        p(result, 0, n(0));
        return n(0);
    }
    p(car.route, 1, target);
    p(car.route, 2, g(car.route, 2) + n(1));
    if g(row, 4) == n(0) {
        p(result, 0, n(0));
        return n(0);
    }
    let completed = g(car.route, 3);
    let events = g(car.race, 1) == completed;
    if events && completed < n(3) {
        if completed < n(0) {
            return n(-4);
        }
        p(car.route, 4 + completed.lo_bits(), g(car.race, 0));
        p(car.race, 0, n(0));
        p(car.route, 3, completed + n(1));
    }
    p(car.race, 1, n((g(car.race, 1) + n(1)).lo_bits() as i16));
    let mut value = n(0);
    if events {
        value = n(1);
    }
    p(result, 0, value);
    n(0)
}
pub fn warning(car: &Car, entity: fix32, number: fix32, dt: fix32) {
    if g(car.race, 2) != n(0) && (entity.lo_bits() ^ number.lo_bits()) & 7 == 0 {
        let backwards = g(car.route, 7) < g(car.route, 8);
        let state = g(car.route, 9);
        let timer = g(car.route, 10);
        if state == n(0) && backwards {
            p(car.route, 9, n(1));
            p(car.route, 10, n(175));
        } else if state == n(1) {
            if !backwards {
                p(car.route, 9, n(0));
            } else if timer <= dt {
                p(car.route, 9, n(2));
                p(car.route, 10, n(50));
            } else {
                p(car.route, 10, n((timer - dt).lo_bits() as i16));
            }
        } else if state == n(2) {
            if timer > dt {
                p(car.route, 10, n((timer - dt).lo_bits() as i16));
            } else if backwards {
                p(car.route, 10, n(50));
            } else {
                p(car.route, 9, n(0));
            }
        }
    }
    p(car.route, 8, g(car.route, 7));
}
pub fn progress(
    r: &Routes,
    world: &scene_world::World,
    ids: &entities::State,
    car: &Car,
    f: &Frame,
) -> fix32 {
    if g(car.route, 0) == n(0) {
        return n(-4);
    }
    let current = g(car.route, 1);
    let result: Buf<u16, 4> = Buf::new([0; 4]);
    let out = result.as_array();
    let status = search(
        r,
        world,
        ids,
        &Query {
            position: f.position,
            current,
            lap: g(car.race, 1),
            limit: f.limit,
            nearest: false,
            result: out,
        },
    );
    if status != n(0) {
        return status;
    }
    let score = g(out, 1);
    p(car.route, 7, score);
    let id = point(r, ids, current);
    if id < n(0) {
        return id;
    }
    let relative = score - integer::mul(g(car.race, 1), fix32::from_words(16384, 156));
    let maximum = g(r.header, 6);
    if g(at(r.points, id.lo_bits() * 14), 1) > maximum
        && relative < integer::mul(maximum, fix32::from_words(8192, 3)) - fix32::from_words(8192, 3)
        && relative > n(0)
    {
        p(car.route, 0, g(r.header, 7));
        p(car.route, 1, g(r.header, 8));
    }
    warning(car, f.entity, f.number, f.dt);
    n(0)
}
// Clock: countdown, release/GO timer and display stage, before any racer update.
pub fn clock_time(clock: Array<u16>, elapsed: fix32) -> bool {
    let running = g(clock, 0) == n(0);
    let mut launch = false;
    if running {
        if g(clock, 1) != n(0) {
            let mut timer = g(clock, 1) - elapsed;
            if timer < n(0) {
                timer = n(0);
            }
            p(clock, 1, timer);
        }
    } else {
        let timer = g(clock, 0) - elapsed;
        if timer < n(1) {
            p(clock, 0, n(0));
            p(clock, 1, n(1000));
            p(clock, 2, n(4));
            launch = true;
        } else {
            p(clock, 0, timer);
            let mut display = n(1);
            if timer <= n(1500) {
                display = n(2);
                if timer < n(1001) {
                    display = n(3);
                }
            }
            p(clock, 2, display);
        }
    }
    launch
}
// Dense standalone racers have three i32 each. The session uses clock_time
// once and visits its existing racer-slot table over authoritative vehicle rows.
pub fn clock(clock: Array<u16>, racers: Array<u16>, count: u16, elapsed: fix32) {
    let running = g(clock, 0) == n(0);
    let launch = clock_time(clock, elapsed);
    let mut i = 0u16;
    while i < count {
        let racer = at(racers, i * 6);
        if running {
            p(racer, 0, g(racer, 0) + elapsed);
        } else if launch {
            p(racer, 2, n(1));
        }
        i += 1;
    }
}
pub fn physics_dt(raw: fix32) -> fix32 {
    if raw < n(10) {
        return n(10);
    }
    if raw > n(100) {
        return n(100);
    }
    raw
}
