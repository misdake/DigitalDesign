//! Ordered look, race and drive checkpoint updates for the finish vehicle.
#![link_cseg = "parents"]
use crate::dsl_rt::*;
use crate::{attitude, entities, finish_drive, integer, player_route, scene_world};

pub struct Gate {
    pub checkpoint: fix32,
    pub side: fix32,
    pub position: Array<u16>,
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
// On success publish presence, entity and finishes-lap. Errors leave the
// caller's result untouched. A direct None link is a successful miss.
pub fn checkpoint(
    r: &finish_drive::Routes,
    ids: &entities::State,
    world: &scene_world::World,
    gate: &Gate,
    out: Array<u16>,
) -> fix32 {
    let id = finish_drive::point(r, ids, gate.checkpoint);
    if id < n(0) {
        return id;
    }
    if gate.side.hi_bits() != 0 || gate.side.lo_bits() > 1 {
        return n(-4);
    }
    let target = finish_drive::next(r, id.lo_bits(), gate.side.lo_bits());
    if target < n(-1) {
        return target;
    }
    if target == n(-1) {
        p(out, 0, n(0));
        p(out, 1, n(0));
        p(out, 2, n(0));
        return n(0);
    }
    let next_id = finish_drive::point(r, ids, target);
    if next_id < n(0) {
        return next_id;
    }
    let raw: Buf<u16, 60> = Buf::new([0; 60]);
    let collider = raw.as_array();
    if !finish_drive::load_collider(r, world, next_id.lo_bits(), collider) {
        return n(-4);
    }
    let point = finish_drive::row(r, next_id.lo_bits());
    let hit = player_route::inside_gate(collider, gate.position, g(point, 9));
    let mut present = n(0);
    let mut entity = n(0);
    let mut lap = n(0);
    if hit {
        present = n(1);
        entity = target;
        if g(point, 10) != n(0) {
            lap = n(1);
        }
    }
    p(out, 0, present);
    p(out, 1, entity);
    p(out, 2, lap);
    n(0)
}
// Success is 0/1 (far-car refresh needed); negative values are source errors.
pub fn waypoints(
    car: &finish_drive::Car,
    race: &player_route::Car,
    r: &finish_drive::Routes,
    ids: &entities::State,
    world: &scene_world::World,
    frame: &finish_drive::Frame,
) -> fix32 {
    if !car.present {
        return n(-1);
    }
    let result: Buf<u16, 6> = Buf::new([0; 6]);
    let out = result.as_array();
    if g(car.state, 2).lo_bits() & 1 == 0 {
        if g(car.state, 7) == n(0) {
            return n(-4);
        }
        let status = checkpoint(
            r,
            ids,
            world,
            &Gate {
                checkpoint: g(car.state, 8),
                side: g(car.state, 11),
                position: at(car.state, 8),
            },
            out,
        );
        if status != n(0) {
            return status;
        }
        if g(out, 0) != n(0) {
            let next = g(out, 1);
            p(car.state, 7, n(1));
            p(car.state, 8, next);
            let side =
                finish_drive::branch(r, ids, next, g(race.race, 1).lo_bits() as i16, frame.seed);
            if side < n(0) {
                return side;
            }
            p(car.state, 11, side);
        }
    }
    if g(race.route, 0) == n(0) {
        return n(-4);
    }
    let status = checkpoint(
        r,
        ids,
        world,
        &Gate {
            checkpoint: g(race.route, 1),
            side: n(0),
            position: car.position,
        },
        out,
    );
    if status != n(0) {
        return status;
    }
    if g(out, 0) != n(0) {
        p(race.route, 1, g(out, 1));
        p(race.route, 2, g(race.route, 2) + n(1));
        if g(out, 2) != n(0) {
            let lap = n(g(race.race, 1).lo_bits() as i16);
            let completed = g(race.route, 3);
            if lap == completed {
                if lap < n(3) {
                    p(race.race, 1, n((lap + n(1)).lo_bits() as i16));
                }
                if completed < n(3) {
                    if completed < n(0) {
                        return n(-4);
                    }
                    p(race.route, 4 + completed.lo_bits(), g(race.race, 0));
                    p(race.race, 0, n(0));
                    p(race.route, 3, completed + n(1));
                }
            } else if lap < n(3) {
                p(race.race, 1, n((lap + n(1)).lo_bits() as i16));
            }
        }
    }
    if g(car.state, 9) == n(0) {
        return n(-4);
    }
    let status = checkpoint(
        r,
        ids,
        world,
        &Gate {
            checkpoint: g(car.state, 10),
            side: g(car.state, 12),
            position: car.position,
        },
        out,
    );
    if status != n(0) {
        return status;
    }
    let mut refresh = n(0);
    if g(out, 0) != n(0) {
        let next = g(out, 1);
        p(car.state, 9, n(1));
        p(car.state, 10, next);
        let side = finish_drive::branch(r, ids, next, g(race.race, 1).lo_bits() as i16, frame.seed);
        if side < n(0) {
            return side;
        }
        p(car.state, 12, side);
        if g(car.state, 2).lo_bits() & 1 != 0 {
            refresh = n(1);
        }
    }
    refresh
}
