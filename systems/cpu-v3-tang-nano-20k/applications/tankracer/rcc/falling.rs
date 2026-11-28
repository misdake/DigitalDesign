//! Falling scenery: orient, query the world when needed, then publish rotation.
use crate::dsl_rt::*;
use crate::{attitude, collision, ground, integer, lifecycle, scene_world, spatial};

pub struct Hit {
    pub velocity: Array<u16>,
    pub forward: fix32,
    pub vehicle: fix32,
    pub entity: fix32,
    pub minimum_turn_rate: fix32,
    pub maximum_turn_rate: fix32,
}
// State is angular velocity, rotation, turn rate, slope limit, last_vehicle.
// Properties is the live 16-word scene row. Sound is a successful device stub.
// The owner then dispatches any auxiliary/debris/attached methods in source order.
// 0 already active/hit, 1 started; other statuses match lifecycle mutations.
pub fn strike(
    state: Array<u16>,
    properties: Array<u16>,
    hit: &Hit,
    ticks: &mut lifecycle::Ticks,
    slot: Array<u16>,
    limit: fix32,
) -> u16 {
    if begin_strike(state, properties, hit) == 0 {
        return 0;
    }
    attitude::put(properties, 3, n(1));
    let saved = attitude::get(slot, 0);
    attitude::put(slot, 0, n(4));
    if limit == n(0) {
        return 2;
    }
    let status = lifecycle::enable(ticks, hit.entity);
    if status >= 2 {
        return status;
    }
    attitude::put(slot, 0, saved);
    finish_strike(state, properties, hit);
    1
}
// Class entry inserts its actual object/audio and activation calls between
// these two phases. Ordinary Fall uses the same phases above.
pub fn begin_strike(state: Array<u16>, properties: Array<u16>, hit: &Hit) -> u16 {
    let flags = attitude::get(properties, 4);
    if attitude::get(properties, 3) != n(0) || flags.lo_bits() & 1 != 0 {
        return 0;
    }
    let mut low = (flags.lo_bits() & 65523) | 1;
    attitude::put(properties, 4, fix32::from_words(low, flags.hi_bits()));
    let mut rate = integer::trunc_shift(-collision::distance(hit.velocity), 3);
    attitude::put(state, 0, rate);
    if hit.forward < n(0) {
        rate = -rate;
        attitude::put(state, 0, rate);
        low |= 8;
        attitude::put(properties, 4, fix32::from_words(low, flags.hi_bits()));
    }
    1
}
pub fn finish_strike(state: Array<u16>, properties: Array<u16>, hit: &Hit) {
    let flags = attitude::get(properties, 4);
    attitude::put(
        properties,
        4,
        fix32::from_words(flags.lo_bits() | 2, flags.hi_bits()),
    );
    attitude::put(state, 1, n(0));
    attitude::put(state, 4, hit.vehicle);
    let mut speed = hit.forward;
    if speed < n(0) {
        speed = -speed;
    }
    let mut rate = integer::trunc_shift(speed, 3);
    if rate < hit.minimum_turn_rate {
        rate = hit.minimum_turn_rate;
    } else if rate > hit.maximum_turn_rate {
        rate = hit.maximum_turn_rate;
    }
    attitude::put(state, 2, rate);
    attitude::put(
        properties,
        4,
        fix32::from_words((flags.lo_bits() | 2) & 65519, flags.hi_bits()),
    );
}

pub struct Motion {
    // Angular velocity, rotation, turn rate, slope limit; strike adds last_vehicle.
    pub state: Array<u16>,
    pub properties: Array<u16>,
    pub matrix: Array<u16>,
    pub vehicle_basis: Array<u16>,
    pub position: Array<u16>,
    pub width: fix32,
    pub effect_active: bool,
}
pub struct Tables {
    pub sine: Array<i16>,
    pub angle: Array<i16>,
}
pub struct Action {
    pub status: u16,
    pub terrain: bool,
    pub retire: bool,
    pub x: fix32,
    pub z: fix32,
    pub delta: fix32,
}
pub struct Step {
    pub id: fix32,
    pub dt: fix32,
    pub limit: fix32,
    pub cell: spatial::Cell,
    pub vehicle_basis: Array<u16>,
    pub peer_valid: bool,
    pub effect_active: bool,
    pub tuning: Array<u16>,
    pub sine: Array<i16>,
    pub angle: Array<i16>,
}
pub struct Body {
    pub state: Array<u16>,
    pub properties: Array<u16>,
    pub collider: Array<u16>,
}
// Plain Fall tick on the live world. The owner resolves last_vehicle at the
// original use point, then dispatches any authored auxiliary/debris methods.
// 0 success; 2 invalid limit, 3 work, 4 object/capacity, 5 arithmetic,
// 6 peer/activation binding, 7 missing external basis, 8 terrain residency.
pub fn tick(
    state: Array<u16>,
    world: &mut scene_world::World,
    step: &Step,
    runtime: &mut scene_world::Runtime,
    map: &mut ground::World,
) -> u16 {
    if step.limit == n(0) {
        return 2;
    }
    if step.id.hi_bits() != 0 || step.id.lo_bits() >= world.count {
        return 4;
    }
    let id = step.id.lo_bits();
    let properties = unsafe {
        world
            .properties
            .as_ptr()
            .add((id * 16) as i16)
            .as_u16_array()
    };
    let collider = unsafe {
        world
            .colliders
            .as_ptr()
            .add((id * 60) as i16)
            .as_u16_array()
    };
    let body = Body {
        state: state,
        properties: properties,
        collider: collider,
    };
    tick_body(&body, step, runtime, map, &mut world.grid)
}
pub fn tick_body(
    body: &Body,
    step: &Step,
    runtime: &mut scene_world::Runtime,
    map: &mut ground::World,
    grid: &mut spatial::Grid,
) -> u16 {
    if step.limit == n(0) {
        return 2;
    }
    let state = body.state;
    let properties = body.properties;
    let collider = body.collider;
    let active = attitude::get(properties, 3) != n(0);
    let aligning = attitude::get(properties, 4).lo_bits() & 2 != 0;
    if active && aligning && !step.peer_valid {
        return 6;
    }
    let present = attitude::get(collider, 29) != n(0);
    if active && !present {
        return 7;
    }
    if !active {
        return 0;
    }
    let mut alias = present;
    let mut i = 0u16;
    while i < 9 {
        if attitude::get(collider, 4 + i) != attitude::get(collider, 13 + i) {
            alias = false;
        }
        i += 1;
    }
    let mut motion = Motion {
        state: state,
        properties: properties,
        matrix: unsafe { collider.as_ptr().add(26).as_u16_array() },
        vehicle_basis: step.vehicle_basis,
        position: unsafe { collider.as_ptr().add(2).as_u16_array() },
        width: fmul_lo32(
            integer::from_i16(properties[14u16] as i16)
                - integer::from_i16(properties[11u16] as i16),
            n(8),
        ),
        effect_active: step.effect_active,
    };
    let tables = Tables {
        sine: step.sine,
        angle: step.angle,
    };
    let action = advance(&mut motion, step.tuning, step.dt, &tables);
    let mut status = 0u16;
    if action.status != 0 {
        status = 5;
    }
    if status == 0 && action.terrain {
        let height = map.height(action.x, action.z);
        if height.status >= 2 {
            status = 8;
            if height.status == 2 {
                status = 5;
            }
        } else if slope(&mut motion, height.height, step.angle) != 0 {
            status = 5;
        }
    }
    if status == 0 && action.retire {
        status = spatial::remove(grid, step.id, &step.cell, step.limit);
        if status == 0 {
            status = lifecycle::queue(
                &mut runtime.queues,
                &mut runtime.ticks,
                attitude::get(collider, 0),
                false,
                step.limit,
            );
        }
    }
    if status == 0 && !aligning {
        finish(&mut motion, &action, step.sine);
    }
    // The source publishes the external matrix on all exits; pose follows only
    // when its value matched that external matrix before this tick.
    if alias {
        let mut words = collider;
        i = 0;
        while i < 18 {
            words[8 + i] = collider[26 + i];
            i += 1;
        }
    }
    status
}
fn n(v: i16) -> fix32 {
    integer::from_i16(v)
}
fn abs(v: fix32) -> fix32 {
    if v < n(0) {
        return -v;
    }
    v
}
// Tuning: squash angle, gravity, rebound multiplier, rebound shift.
// A terrain request follows committed orientation changes. A retirement request
// precedes the final X rotation; the caller removes memberships and queues Stop.
pub fn advance(m: &mut Motion, tuning: Array<u16>, dt: fix32, tables: &Tables) -> Action {
    let s = m.state;
    let mut action = Action {
        status: 0,
        terrain: false,
        retire: false,
        x: n(0),
        z: n(0),
        delta: n(0),
    };
    if attitude::get(m.properties, 3) == n(0) {
        return action;
    }
    let flags = attitude::get(m.properties, 4);
    if flags.lo_bits() & 2 != 0 {
        let scratch: Buf<u16, 6> = Buf::new([0; 6]);
        collision::transform(scratch.as_array(), m.matrix, m.vehicle_basis, true);
        let angle = attitude::angle(
            tables.angle,
            attitude::get(scratch.as_array(), 2),
            attitude::get(scratch.as_array(), 0),
        );
        if angle == n(-1) {
            action.status = 3;
            return action;
        }
        let squash = attitude::get(tuning, 0);
        let mut turn = 0i16;
        let mut flip = 0u16;
        let half = fix32::from_words(32768, 0);
        if angle >= n(0) && angle < n(16384) {
            if angle < squash {
                flip = 1;
            } else {
                turn = 1;
            }
        } else if angle >= -half && angle <= n(-16384) && angle != -half {
            if abs(angle + half) < squash {
                flip = 2;
            } else {
                turn = 1;
            }
        } else if angle >= n(16384) && angle <= half {
            if abs(angle - half) < squash {
                flip = 2;
            } else {
                turn = -1;
            }
        } else if angle >= n(-16384) && angle < n(0) {
            if -angle < squash {
                flip = 1;
            } else {
                turn = -1;
            }
        }
        if flip != 0 {
            let mut low = flags.lo_bits() & 65533;
            if flip == 2 {
                low |= 4;
            }
            attitude::put(m.properties, 4, fix32::from_words(low, flags.hi_bits()));
        }
        if turn != 0 {
            let mut delta = integer::trunc_shift(fmul_lo32(attitude::get(s, 2), dt), 8);
            if turn < 0 {
                delta = -delta;
            }
            attitude::rotate(m.matrix, 0, 1, delta, tables.sine);
        }
        let low = attitude::get(m.properties, 4).lo_bits();
        if low & 2 == 0 {
            let mut x = integer::trunc_shift(fmul_lo32(attitude::get(m.matrix, 6), m.width), 12);
            let mut z = integer::trunc_shift(fmul_lo32(attitude::get(m.matrix, 8), m.width), 12);
            if (low & 8 != 0 && low & 4 == 0) || (low & 8 == 0 && low & 4 != 0) {
                x = -x;
                z = -z;
            }
            action.x = attitude::get(m.position, 0) + x;
            action.z = attitude::get(m.position, 2) + z;
            action.terrain = true;
        }
        return action;
    }
    let speed = attitude::get(s, 0);
    let mut delta = integer::trunc_shift(fmul_lo32(dt, speed), 8);
    let old = attitude::get(s, 1);
    let gravity = fmul_lo32(attitude::get(tuning, 1), dt);
    let mut current = old + delta;
    if flags.lo_bits() & 8 != 0 {
        attitude::put(s, 0, speed + gravity);
        current = old - delta;
    } else {
        attitude::put(s, 0, speed - gravity);
    }
    let limit = attitude::get(s, 3);
    if abs(current) > limit + n(16380) {
        if delta > n(0) {
            delta = limit - abs(old) + n(16384);
        } else {
            delta = abs(old) - limit - n(16384);
        }
        attitude::put(s, 1, n(-16380) - limit);
        let bounce = fmul_lo32(attitude::get(tuning, 2), attitude::get(s, 0));
        let shift = attitude::get(tuning, 3).lo_bits() & 31;
        let mut rate = -integer::sar(bounce, shift);
        if -bounce >= n(0) {
            rate = integer::sar(-bounce, shift);
        }
        attitude::put(s, 0, rate);
        if abs(rate) < n(200) {
            attitude::put(m.properties, 3, n(0));
            action.retire = !m.effect_active;
        }
    } else {
        attitude::put(s, 1, current);
    }
    if flags.lo_bits() & 4 != 0 {
        delta = -delta;
    }
    action.delta = delta;
    action
}
pub fn slope(m: &mut Motion, height: fix32, angle: Array<i16>) -> u16 {
    let mut limit = n(0);
    if height != fix32::from_words(1, 32768) {
        limit = attitude::angle(angle, attitude::get(m.position, 1) - height, m.width);
        if limit == n(-1) {
            return 3;
        }
    }
    attitude::put(m.state, 3, limit);
    0
}
pub fn finish(m: &mut Motion, action: &Action, sine: Array<i16>) {
    attitude::rotate(m.matrix, 0, 0, action.delta, sine);
}
