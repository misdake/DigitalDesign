//! Route restoration uses the live ordered checkpoints and complete part poses.
use crate::dsl_rt::*;
use crate::{attitude, collision, entities, integer, ordered, scene_world, vehicle_pose};
pub struct Car {
    pub pose: vehicle_pose::State,
    // Presence/entity, unsigned passed count, extended signed-i16 laps.
    pub checkpoint: Array<u16>,
    pub passed: Array<u16>,
    pub laps: Array<u16>,
    pub controls: Array<u16>,
    pub velocity: Array<u16>,
    pub acceleration: Array<u16>,
    pub angular: Array<u16>,
    pub track_slip: Array<u16>,
    pub temporary_timer: Array<u16>,
    pub config: Array<u16>,
    pub travel: Array<u16>,
    pub environment: Array<u16>,
}
pub struct Routes {
    pub order: ordered::List,
    // Per scene object: i32 point presence, i32 progress.
    pub points: Array<u16>,
    pub count: u16,
    pub occupied: ordered::List,
}
pub struct Request {
    pub limit: fix32,
    pub sine: Array<i16>,
    pub angle: Array<i16>,
    // Written only on success. A valid ground height may be negative.
    pub height: Array<u16>,
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
fn at(v: Array<u16>, word: u16) -> Array<u16> {
    unsafe { v.as_ptr().add(word as i16).as_u16_array() }
}
fn abs(v: fix32) -> fix32 {
    if v < n(0) {
        return -v;
    }
    v
}
fn exhausted(used: fix32, limit: fix32) -> bool {
    used.hi_bits() > limit.hi_bits()
        || (used.hi_bits() == limit.hi_bits() && used.lo_bits() >= limit.lo_bits())
}
fn point(routes: &Routes, ids: &entities::State, entity: fix32) -> fix32 {
    if entities::handle_kind(ids, entity) != 1 {
        return n(-4);
    }
    let id = entities::handle(ids, entity);
    if id.hi_bits() != 0
        || id.lo_bits() >= routes.count
        || g(at(routes.points, id.lo_bits() * 4), 0) == n(0)
    {
        return n(-4);
    }
    id
}
fn load(world: &scene_world::World, id: fix32, row: Array<u16>) -> bool {
    id.hi_bits() == 0 && scene_world::load_collider(world, id.lo_bits(), row)
}
pub fn restore(
    car: &Car,
    routes: &Routes,
    world: &scene_world::World,
    ids: &entities::State,
    request: &Request,
) -> fix32 {
    let limit = request.limit;
    if limit == n(0) {
        return n(-1);
    }
    if g(car.checkpoint, 0) == n(0) {
        return n(-4);
    }
    let initial = g(car.checkpoint, 1);
    let mut saved = initial;
    let mut found = false;
    let mut index = 0u16;
    let mut used = n(0);
    let collider: Buf<u16, 60> = Buf::new([0; 60]);
    let row = collider.as_array();
    while index < routes.order.count {
        if !load(world, g(routes.order.items, index), row) {
            return n(-4);
        }
        let current = g(row, 0);
        if current == initial {
            found = true;
            break;
        }
        if exhausted(used, limit) {
            return n(-3);
        }
        used = used + n(1);
        index += 1;
    }
    if found {
        let mut blocked = true;
        while blocked {
            if exhausted(used, limit) {
                return n(-3);
            }
            used = used + n(1);
            if !load(world, g(routes.order.items, index), row) {
                return n(-4);
            }
            let current = g(row, 0);
            if index == 0 {
                index = routes.order.count - 1;
            } else {
                index -= 1;
            }
            if !load(world, g(routes.order.items, index), row) {
                return n(-4);
            }
            let candidate = g(row, 0);
            p(car.checkpoint, 0, n(1));
            p(car.checkpoint, 1, candidate);
            let current_id = point(routes, ids, current);
            if current_id < n(0) {
                return current_id;
            }
            if g(at(routes.points, current_id.lo_bits() * 4), 1) != n(-1) {
                saved = current;
            }
            p(car.passed, 0, g(car.passed, 0) - n(1));
            let candidate_id = point(routes, ids, candidate);
            if candidate_id < n(0) {
                return candidate_id;
            }
            if g(at(routes.points, candidate_id.lo_bits() * 4), 1) != n(-1) {
                let occupied = ordered::find(&routes.occupied, candidate, limit);
                if occupied.status != 0 {
                    return n(-3);
                }
                blocked = occupied.found;
            }
        }
        let start = point(routes, ids, initial);
        if start < n(0) {
            return start;
        }
        let end = point(routes, ids, g(car.checkpoint, 1));
        if end < n(0) {
            return end;
        }
        if g(at(routes.points, start.lo_bits() * 4), 1) < g(at(routes.points, end.lo_bits() * 4), 1)
        {
            p(
                car.laps,
                0,
                integer::from_i16((g(car.laps, 0).lo_bits() - 1) as i16),
            );
        }
    }
    let source = point(routes, ids, g(car.checkpoint, 1));
    if source < n(0) {
        return source;
    }
    if !load(world, source, row) {
        return n(-4);
    }
    let source_position: Buf<u16, 6> = Buf::new([0; 6]);
    let position = source_position.as_array();
    let mut i = 0u16;
    while i < 3 {
        p(position, i, g(at(row, 2), i));
        i += 1;
    }
    let target = point(routes, ids, saved);
    if target < n(0) {
        return target;
    }
    if !load(world, target, row) {
        return n(-4);
    }
    i = 0;
    while i < 3 {
        p(car.pose.physical_position, i, g(position, i));
        i += 1;
    }
    p(car.pose.physical_position, 1, g(position, 1) + n(6144));
    let direction: Buf<u16, 6> = Buf::new([0; 6]);
    let local = direction.as_array();
    i = 0;
    while i < 3 {
        p(local, i, g(at(row, 2), i) - g(position, i));
        i += 1;
    }
    collision::transform(local, car.pose.body_basis, local, true);
    while abs(g(local, 0)) > fix32::from_words(32768, 0)
        || abs(g(local, 1)) > fix32::from_words(32768, 0)
        || abs(g(local, 2)) > fix32::from_words(32768, 0)
    {
        if exhausted(used, limit) {
            return n(-3);
        }
        used = used + n(1);
        i = 0;
        while i < 3 {
            p(local, i, integer::sar(g(local, i), 1));
            i += 1;
        }
    }
    let yaw = attitude::angle(request.angle, g(local, 0), g(local, 2));
    if yaw == n(-1) {
        return n(-2);
    }
    attitude::rotate(car.pose.body_basis, 0, 1, -yaw, request.sine);
    let env = vehicle_pose::Environment {
        timer: g(car.temporary_timer, 0).lo_bits(),
        values: car.environment,
    };
    let status = vehicle_pose::update(
        &car.pose,
        car.config,
        car.travel,
        &env,
        request.sine,
        request.angle,
    );
    if status != n(0) {
        return status;
    }
    i = 0;
    while i < 3 {
        p(car.controls, i, n(0));
        p(car.velocity, i, n(0));
        p(car.angular, i, n(0));
        i += 1;
    }
    p(car.acceleration, 0, n(0));
    p(car.acceleration, 1, n(0));
    p(car.track_slip, 0, n(0));
    p(request.height, 0, g(position, 1));
    n(0)
}
