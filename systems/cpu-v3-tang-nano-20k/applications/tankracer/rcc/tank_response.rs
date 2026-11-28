//! Ordered tank queries, mandatory separation, impulses and synchronous effects.
use crate::dsl_rt::*;
use crate::{attitude, collision, damage, impact, integer, object_query};
pub struct Vehicle {
    pub velocity: Array<u16>,
    pub local_velocity: Array<u16>,
    pub previous_position: Array<u16>,
    pub timer: Array<u16>,
    pub delta: Array<u16>,
    pub corners: Array<u16>,
    pub corner_stride: u16,
    pub impact: impact::Vehicle,
    pub damage: damage::Target,
}
pub struct World {
    pub geometry: Array<object_query::Tank>,
    pub vehicles: Array<Vehicle>,
    pub slots: Array<u16>,
    pub present: Array<u16>,
    pub geometry_count: u16,
    pub vehicle_count: u16,
    pub hit: fix32,
}
pub struct Environment {
    pub mode: fix32,
    pub dt: fix32,
    pub separation_limit: fix32,
}
pub struct Peer {
    pub tank: fix32,
    pub vx: fix32,
    pub vz: fix32,
}
pub struct Pair {
    pub geometry: Array<object_query::Tank>,
    pub vehicles: Array<Vehicle>,
    pub a: u16,
    pub b: u16,
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
fn length2(v: Array<u16>) -> fix32 {
    let a = abs(g(v, 0));
    let b = abs(g(v, 2));
    if b <= a {
        return a + b - integer::sar(b, 1);
    }
    a + b - integer::sar(a, 1)
}
fn sum(pair: &Pair) -> fix32 {
    length2(pair.vehicles[pair.a].velocity) + length2(pair.vehicles[pair.b].velocity)
}
pub fn facing(a: &object_query::Tank, b: &object_query::Tank, out: Array<u16>) -> u16 {
    let relative: Buf<u16, 6> = Buf::new([0; 6]);
    let local = relative.as_array();
    let mut i = 0u16;
    while i < 3 {
        p(
            local,
            i,
            g(b.collider.position.as_array(), i) - g(a.collider.position.as_array(), i),
        );
        i += 1;
    }
    collision::transform(local, a.collider.basis.as_array(), local, true);
    collision::normalize(local);
    let code = collision::direction(g(local, 0), g(local, 2));
    p(local, 0, n(0));
    p(local, 1, n(0));
    p(local, 2, n(0));
    if code == 0 {
        p(local, 0, n(-1024));
    } else if code == 1 {
        p(local, 0, n(1024));
    } else if code == 2 {
        p(local, 2, n(1024));
    } else {
        p(local, 2, n(-1024));
    }
    collision::transform(out, a.collider.basis.as_array(), local, false);
    code
}
fn approaching(pair: &Pair, dt: fix32) -> bool {
    let trajectory: Buf<u16, 24> = Buf::new([0; 24]);
    let data = trajectory.as_array();
    let mut i = 0u16;
    while i < 3 {
        p(
            data,
            i,
            g(pair.geometry[pair.a].collider.position.as_array(), i),
        );
        p(data, 3 + i, g(pair.vehicles[pair.a].velocity, i));
        p(
            data,
            6 + i,
            g(pair.geometry[pair.b].collider.position.as_array(), i),
        );
        p(data, 9 + i, g(pair.vehicles[pair.b].velocity, i));
        i += 1;
    }
    collision::approaching(data, dt)
}
fn separate(velocity: Array<u16>, normal: Array<u16>, factor: fix32) {
    p(
        velocity,
        0,
        g(velocity, 0) - fmul_lo32(g(normal, 0), factor),
    );
    p(
        velocity,
        2,
        g(velocity, 2) - fmul_lo32(g(normal, 2), factor),
    );
}
fn exchange(pair: &Pair, normal: Array<u16>) {
    let scratch: Buf<u16, 48> = Buf::new([0; 48]);
    let input = scratch.as_array();
    let output: Buf<u16, 14> = Buf::new([0; 14]);
    let out = output.as_array();
    let a = pair.vehicles[pair.a].velocity;
    let b = pair.vehicles[pair.b].velocity;
    let mut i = 0u16;
    while i < 9 {
        p(
            input,
            i,
            g(pair.geometry[pair.a].collider.basis.as_array(), i),
        );
        i += 1;
    }
    i = 0;
    while i < 3 {
        p(input, 9 + i, g(pair.vehicles[pair.a].local_velocity, i));
        p(input, 12 + i, g(a, i));
        p(input, 15 + i, g(b, i));
        p(input, 18 + i, g(normal, i));
        p(input, 21 + i, g(b, i));
        i += 1;
    }
    collision::exchange(input, out);
    i = 0;
    while i < 3 {
        p(a, i, g(out, i));
        p(b, i, g(out, 3 + i));
        i += 1;
    }
}
pub fn resolve_overlap(
    pair: &Pair,
    speed: fix32,
    env: &Environment,
    impacts: &mut impact::State,
    c: &mut impact::Context,
) -> u16 {
    if pair.a == pair.b {
        return 4;
    }
    let mut overlap = false;
    let mut i = 0u16;
    while i < 4 {
        let point = unsafe {
            pair.geometry[pair.b]
                .contacts
                .as_array()
                .as_ptr()
                .add((i * 6) as i16)
                .as_u16_array()
        };
        if object_query::contains(&pair.geometry[pair.a], point) {
            overlap = true;
            break;
        }
        i += 1;
    }
    i = 0;
    while i < 4 {
        let point = unsafe {
            pair.geometry[pair.a]
                .contacts
                .as_array()
                .as_ptr()
                .add((i * 6) as i16)
                .as_u16_array()
        };
        if object_query::contains(&pair.geometry[pair.b], point) {
            let mut vehicles = pair.vehicles;
            let a = &mut vehicles[pair.a];
            if a.damage.player && env.mode != n(4) && env.mode != n(5) {
                let corner = g(a.corners, i * a.corner_stride);
                let mut kind = n(3);
                if i >= 2 {
                    kind = n(4);
                }
                let status = impact::emit(impacts, &mut a.impact, corner, kind, c);
                if status >= 2 {
                    return status;
                }
            }
            overlap = true;
        }
        i += 1;
    }
    if !overlap {
        return 0;
    }
    let normals: Buf<u16, 12> = Buf::new([0; 12]);
    let a = normals.as_array();
    let b = unsafe { a.as_ptr().add(6).as_u16_array() };
    let code_a = facing(&pair.geometry[pair.a], &pair.geometry[pair.b], a);
    let code_b = facing(&pair.geometry[pair.b], &pair.geometry[pair.a], b);
    if speed < fix32::from_words(56784, 6) {
        let threshold = sum(pair) + n(20000);
        let mut limit = env.separation_limit;
        if limit == n(0) {
            limit = fix32::from_words(0, 1);
        }
        let mut iterations = n(0);
        loop {
            if iterations == limit {
                return 6;
            }
            iterations = iterations + n(1);
            separate(pair.vehicles[pair.a].velocity, a, n(10));
            separate(pair.vehicles[pair.b].velocity, b, n(10));
            if !(sum(pair) < threshold && approaching(pair, env.dt)) {
                break;
            }
        }
        if speed > fix32::from_words(53392, 3) {
            let mut vehicles = pair.vehicles;
            if code_a >= 2
                && damage::track(
                    &mut vehicles[pair.a].damage,
                    fix32::from_words(code_a, 0),
                    pair.geometry[pair.b].collider.position.as_array(),
                ) >= 2
            {
                return 5;
            }
            if code_b >= 2
                && damage::track(
                    &mut vehicles[pair.b].damage,
                    fix32::from_words(code_b, 0),
                    pair.geometry[pair.a].collider.position.as_array(),
                ) >= 2
            {
                return 5;
            }
        }
    } else if approaching(pair, env.dt) {
        if code_a < 2 && code_b < 2 {
            separate(pair.vehicles[pair.a].velocity, a, n(50));
            separate(pair.vehicles[pair.b].velocity, b, n(50));
        } else {
            exchange(pair, b);
        }
    }
    1
}
// Status: 0 miss, 1 resolved, 2 arithmetic, 3 pool, 4 resource/alias,
// 5 damage, 6 separation limit. Prior effects/state remain committed on error.
pub fn step(
    own: u16,
    world: &mut World,
    env: &Environment,
    peer: &mut Peer,
    impacts: &mut impact::State,
    c: &mut impact::Context,
) -> u16 {
    world.hit = n(-1);
    if own >= world.geometry_count || own >= world.vehicle_count {
        return 4;
    }
    let tank = &world.geometry[own];
    let start = tank.collider.position.as_array();
    let end_storage: Buf<u16, 6> = Buf::new([0; 6]);
    let end = end_storage.as_array();
    let mut i = 0u16;
    while i < 3 {
        p(
            end,
            i,
            fmul_lo32(g(start, i), n(2)) - g(world.vehicles[own].previous_position, i),
        );
        i += 1;
    }
    let query = object_query::Query {
        start: start,
        end: end,
        self_entity: tank.collider.entity,
        object_count: world.geometry_count,
        deformable: false,
    };
    let mut hit = object_query::hit_zero();
    let status = object_query::tanks(tank, world.geometry, world.slots, &query, &mut hit);
    if status == 3 {
        return 2;
    }
    if status >= 3 {
        return 4;
    }
    if hit.object == n(-1) {
        return 0;
    }
    let other = hit.object.lo_bits();
    if other >= world.vehicle_count {
        return 4;
    }
    let delta_storage: Buf<u16, 6> = Buf::new([0; 6]);
    let delta = delta_storage.as_array();
    p(
        delta,
        0,
        g(world.vehicles[other].velocity, 0) - g(world.vehicles[own].velocity, 0),
    );
    p(
        delta,
        2,
        g(world.vehicles[other].velocity, 2) - g(world.vehicles[own].velocity, 2),
    );
    if env.mode == n(4) || env.mode == n(5) {
        peer.tank = hit.object;
        peer.vx = g(world.vehicles[other].velocity, 0);
        peer.vz = g(world.vehicles[other].velocity, 2);
    }
    if own == other {
        return 4;
    }
    let pair = Pair {
        geometry: world.geometry,
        vehicles: world.vehicles,
        a: own,
        b: other,
    };
    let status = resolve_overlap(&pair, collision::distance(delta), env, impacts, c);
    if status >= 2 {
        return status;
    }
    if status == 0 && approaching(&pair, env.dt) {
        let normal: Buf<u16, 6> = Buf::new([0; 6]);
        facing(
            &pair.geometry[other],
            &pair.geometry[own],
            normal.as_array(),
        );
        exchange(&pair, normal.as_array());
    }
    let timer = world.vehicles[own].timer;
    p(timer, 0, g(timer, 0) + fmul_lo32(env.dt, n(4)));
    world.hit = hit.object;
    1
}

// The live car runtime: slot alias validation precedes candidate geometry;
// disabled/hidden self skips the query, and delta commits only after success.
pub fn cars(
    own: u16,
    world: &mut World,
    env: &Environment,
    impacts: &mut impact::State,
    c: &mut impact::Context,
) -> fix32 {
    if own >= world.geometry_count || own >= world.vehicle_count || world.present[own] == 0 {
        return n(-1);
    }
    let initial: Buf<u16, 6> = Buf::new([0; 6]);
    let before = initial.as_array();
    let velocity = world.vehicles[own].velocity;
    let mut i = 0u16;
    while i < 3 {
        p(before, i, g(velocity, i));
        i += 1;
    }
    let tank = &world.geometry[own];
    if tank.disabled == n(0) && tank.active != n(0) {
        let start = tank.collider.position.as_array();
        let end_storage: Buf<u16, 6> = Buf::new([0; 6]);
        let end = end_storage.as_array();
        i = 0;
        while i < 3 {
            p(
                end,
                i,
                fmul_lo32(g(start, i), n(2)) - g(world.vehicles[own].previous_position, i),
            );
            i += 1;
        }
        let query = object_query::Query {
            start: start,
            end: end,
            self_entity: tank.collider.entity,
            object_count: world.geometry_count,
            deformable: false,
        };
        let car_env = Environment {
            mode: env.mode,
            dt: env.dt,
            separation_limit: n(0),
        };
        let mut slot = 0u16;
        while slot < 9 {
            let other = world.slots[slot];
            if other != 65535 && fix32::from_words(slot, 0) != tank.slot {
                if other == own || other >= world.geometry_count || other >= world.vehicle_count {
                    return n(-1);
                }
                if world.present[other] == 0 {
                    return n(-1);
                }
                let mut hit = object_query::SegmentHit {
                    point: Buf::new([0; 6]),
                    normal: Buf::new([0; 6]),
                };
                let code =
                    object_query::tank_candidate(tank, &world.geometry[other], &query, &mut hit);
                if code == 3 {
                    return n(-2);
                }
                if code != 0 {
                    let relative: Buf<u16, 6> = Buf::new([0; 6]);
                    let delta = relative.as_array();
                    p(
                        delta,
                        0,
                        g(world.vehicles[other].velocity, 0) - g(before, 0),
                    );
                    p(
                        delta,
                        2,
                        g(world.vehicles[other].velocity, 2) - g(before, 2),
                    );
                    let pair = Pair {
                        geometry: world.geometry,
                        vehicles: world.vehicles,
                        a: own,
                        b: other,
                    };
                    let status =
                        resolve_overlap(&pair, collision::distance(delta), &car_env, impacts, c);
                    if status == 2 {
                        return n(-2);
                    }
                    if status == 4 {
                        return n(-4);
                    }
                    if status == 6 {
                        return n(-3);
                    }
                    if status >= 2 {
                        return n(-1);
                    }
                    if status == 0 && approaching(&pair, env.dt) {
                        let normal: Buf<u16, 6> = Buf::new([0; 6]);
                        facing(
                            &pair.geometry[other],
                            &pair.geometry[own],
                            normal.as_array(),
                        );
                        exchange(&pair, normal.as_array());
                    }
                    let timer = world.vehicles[own].timer;
                    p(timer, 0, g(timer, 0) + fmul_lo32(env.dt, n(4)));
                    break;
                }
            }
            slot += 1;
        }
    }
    i = 0;
    while i < 3 {
        p(world.vehicles[own].delta, i, g(velocity, i) - g(before, i));
        i += 1;
    }
    n(0)
}
