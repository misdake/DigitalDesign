//! Ordered car/scenery query and immediate ordinary class methods.
use crate::dsl_rt::*;
use crate::{
    attitude, bounce, falling, ground, integer, lifecycle, object_query, scene_contact,
    scene_query, scene_world, spatial,
};
pub struct Components {
    // Eight u16 words per object: kind, state/config word offsets, flags,
    // cell X/Z/present, reserved. Kinds: 1 ordinary Fall, 2 Bounce.
    pub records: Array<u16>,
    pub state: Array<u16>,
    pub config: Array<u16>,
    pub fall_class: fix32,
    pub minimum_turn: fix32,
    pub maximum_turn: fix32,
    pub work_limit: fix32,
    pub tables: Tables,
}
pub struct Vehicle {
    pub entity: fix32,
    pub position: Array<u16>,
    pub previous: Array<u16>,
    pub base_position: Array<u16>,
    pub velocity: Array<u16>,
    pub forward: fix32,
    pub reference: Array<u16>,
    pub external: Array<u16>,
    pub has_external: bool,
    // Adjacent live impact_accumulator and unsigned collision_size.
    pub counters: Array<u16>,
    pub dt: fix32,
    pub mode: fix32,
}
pub struct Tables {
    pub sine: Array<i16>,
    pub angle: Array<i16>,
}
pub struct Frame {
    pub object: u16,
    pub dt: fix32,
    pub peer_basis: Array<u16>,
    pub peer_valid: bool,
}
fn n(v: i16) -> fix32 {
    integer::from_i16(v)
}
fn mutation_error(status: u16) -> fix32 {
    if status == 3 {
        return n(-3);
    }
    n(-1)
}
fn tick_error(status: u16) -> fix32 {
    if status == 5 {
        return n(-2);
    }
    mutation_error(status)
}
// The immutable ENGLAND Fall/Bounce activation binding is EnableTick. Its
// nested writes operate on the same slot, flags and live order as the car call.
pub fn respond(
    v: &Vehicle,
    scene: &scene_query::Scene,
    world: &scene_world::World,
    components: &Components,
    runtime: &mut scene_world::Runtime,
    hit: &mut object_query::Hit,
) -> fix32 {
    let status = query(v, world, components, hit);
    if status != n(1) {
        return status;
    }
    method(v, scene, world, components, runtime, hit)
}
// Query and reflection are shared; the caller dispatches the selected class.
pub fn query(
    v: &Vehicle,
    world: &scene_world::World,
    components: &Components,
    hit: &mut object_query::Hit,
) -> fix32 {
    let endpoint: Buf<u16, 6> = Buf::new([0; 6]);
    let mut i = 0u16;
    while i < 3 {
        attitude::put(
            endpoint.as_array(),
            i,
            fmul_lo32(attitude::get(v.position, i), n(2)) - attitude::get(v.previous, i),
        );
        i += 1;
    }
    let query = object_query::Query {
        start: v.position,
        end: endpoint.as_array(),
        self_entity: v.entity,
        object_count: world.count,
        deformable: false,
    };
    let env = scene_contact::Environment {
        mode: v.mode,
        breakable_class: components.fall_class,
        sine: components.tables.sine,
        angle: components.tables.angle,
    };
    let status = scene_contact::obstacles(world, &query, v.velocity, v.counters, &env, hit);
    if status == 0 || status == 2 {
        return n(0);
    }
    if status == 3 {
        return n(-2);
    }
    if status != 1 {
        return n(-1);
    }
    n(1)
}

pub fn method(
    v: &Vehicle,
    scene: &scene_query::Scene,
    world: &scene_world::World,
    components: &Components,
    runtime: &mut scene_world::Runtime,
    hit: &object_query::Hit,
) -> fix32 {
    let index = hit.object.lo_bits();
    let properties = unsafe {
        world
            .properties
            .as_ptr()
            .add((index * 16) as i16)
            .as_u16_array()
    };
    let class = attitude::get(properties, 0);
    if class < n(0) || class >= fix32::from_words(scene.header[4u16], 0) {
        return n(-1);
    }
    let saved = attitude::get(runtime.slot, 0);
    attitude::put(runtime.slot, 0, n(24));
    let method = scene.method(class.lo_bits(), 24);
    if method.status == 4 {
        return n(-1);
    }
    if method.status == 2 {
        return n(0);
    }
    if method.status == 1 {
        let row = unsafe {
            components
                .records
                .as_ptr()
                .add((index * 8) as i16)
                .as_u16_array()
        };
        let state = unsafe {
            components
                .state
                .as_ptr()
                .add(row[1u16] as i16)
                .as_u16_array()
        };
        let config = unsafe {
            components
                .config
                .as_ptr()
                .add(row[2u16] as i16)
                .as_u16_array()
        };
        let collider = unsafe {
            world
                .colliders
                .as_ptr()
                .add((index * 60) as i16)
                .as_u16_array()
        };
        let activation = scene.method(class.lo_bits(), 4);
        if activation.status != 1 || activation.id != 2 {
            return n(-5);
        }
        if method.id == 1 && row[0u16] == 1 {
            let hit = falling::Hit {
                velocity: v.velocity,
                forward: v.forward,
                vehicle: v.entity,
                entity: attitude::get(collider, 0),
                minimum_turn_rate: components.minimum_turn,
                maximum_turn_rate: components.maximum_turn,
            };
            let status = falling::strike(
                state,
                properties,
                &hit,
                &mut runtime.ticks,
                runtime.slot,
                components.work_limit,
            );
            if status >= 2 {
                return mutation_error(status);
            }
        } else if method.id == 79 && row[0u16] == 2 {
            let mut motion = bounce::Motion {
                state: state,
                properties: properties,
                collider: collider,
                reference_alias: row[3u16] & 1 != 0,
                cell: spatial::Cell {
                    x: row[4u16] as i16,
                    z: row[5u16],
                    present: row[6u16] != 0,
                },
                impact_threshold: n(0),
            };
            let config = bounce::Config {
                data: config,
                sine: components.tables.sine,
                angle: components.tables.angle,
            };
            let hit = bounce::Hit {
                position: v.base_position,
                velocity: v.velocity,
                forward: v.forward,
                reference: v.reference,
                external: v.external,
                has_external: v.has_external,
            };
            let frame = bounce::Frame {
                entity: attitude::get(collider, 0),
                limit: components.work_limit,
                dt: v.dt,
                id: fix32::from_words(index, 0),
                has_mesh: row[3u16] & 2 != 0,
            };
            let status = bounce::hit(&mut motion, &config, &hit, runtime, &frame);
            if status != 0 {
                return mutation_error(status);
            }
        } else {
            return n(-5);
        }
    }
    attitude::put(runtime.slot, 0, saved);
    n(0)
}
// One actual class tick. The world scheduler selects entities in live order;
// this method reads current geometry, cells, properties and the shared seed.
pub fn tick(
    scene: &scene_query::Scene,
    world: &mut scene_world::World,
    components: &Components,
    runtime: &mut scene_world::Runtime,
    map: &mut ground::World,
    frame: &Frame,
) -> fix32 {
    spatial::select(&world.grid);
    let index = frame.object;
    if index >= world.count {
        return n(-1);
    }
    let properties = unsafe {
        world
            .properties
            .as_ptr()
            .add((index * 16) as i16)
            .as_u16_array()
    };
    let class = attitude::get(properties, 0);
    if class < n(0) || class >= fix32::from_words(scene.header[4u16], 0) {
        return n(-1);
    }
    let saved = attitude::get(runtime.slot, 0);
    attitude::put(runtime.slot, 0, n(12));
    let method = scene.method(class.lo_bits(), 12);
    if method.status == 4 {
        return n(-1);
    }
    if method.status == 2 {
        return n(0);
    }
    if method.status == 1 {
        let mut row = unsafe {
            components
                .records
                .as_ptr()
                .add((index * 8) as i16)
                .as_u16_array()
        };
        let state = unsafe {
            components
                .state
                .as_ptr()
                .add(row[1u16] as i16)
                .as_u16_array()
        };
        let config = unsafe {
            components
                .config
                .as_ptr()
                .add(row[2u16] as i16)
                .as_u16_array()
        };
        let collider = unsafe {
            world
                .colliders
                .as_ptr()
                .add((index * 60) as i16)
                .as_u16_array()
        };
        let cell = spatial::Cell {
            x: row[4u16] as i16,
            z: row[5u16],
            present: row[6u16] != 0,
        };
        if method.id == 65 && row[0u16] == 1 {
            let step = falling::Step {
                id: fix32::from_words(index, 0),
                dt: frame.dt,
                limit: components.work_limit,
                cell: cell,
                vehicle_basis: frame.peer_basis,
                peer_valid: frame.peer_valid,
                effect_active: false,
                tuning: config,
                sine: components.tables.sine,
                angle: components.tables.angle,
            };
            let status = falling::tick(state, world, &step, runtime, map);
            if status != 0 {
                return tick_error(status);
            }
        } else if method.id == 80 && row[0u16] == 2 {
            let mut motion = bounce::Motion {
                state: state,
                properties: properties,
                collider: collider,
                reference_alias: row[3u16] & 1 != 0,
                cell: cell,
                impact_threshold: attitude::get(config, 10),
            };
            let config = bounce::Config {
                data: config,
                sine: components.tables.sine,
                angle: components.tables.angle,
            };
            let frame = bounce::Frame {
                entity: attitude::get(collider, 0),
                limit: components.work_limit,
                dt: frame.dt,
                id: fix32::from_words(index, 0),
                has_mesh: row[3u16] & 2 != 0,
            };
            let status = bounce::tick(&mut motion, &config, runtime, &frame, &mut world.grid, map);
            row[4u16] = motion.cell.x as u16;
            row[5u16] = motion.cell.z;
            row[6u16] = 0;
            if motion.cell.present {
                row[6u16] = 1;
            }
            if status != 0 {
                return tick_error(status);
            }
        } else {
            return n(-5);
        }
    }
    attitude::put(runtime.slot, 0, saved);
    n(0)
}
// Stop dispatch is shared by cars, scenery and session owners. Entity-class
// rows use 65535 for absent identities. Destruction is a separate owner path.
pub fn drain(
    scene: &scene_query::Scene,
    runtime: &mut scene_world::Runtime,
    classes: Array<u16>,
    count: u16,
    limit: fix32,
) -> fix32 {
    let mut pass = lifecycle::begin_drain(limit);
    loop {
        let event = lifecycle::pop(&mut runtime.queues, &mut pass, limit);
        if event.status == 0 {
            break;
        }
        if event.status != 1 {
            return n(-1);
        }
        if event.id.hi_bits() != 0 || event.id.lo_bits() >= count {
            return n(-1);
        }
        let class = classes[event.id.lo_bits()];
        if class >= scene.header[4u16] {
            return n(-1);
        }
        let saved = attitude::get(runtime.slot, 0);
        attitude::put(runtime.slot, 0, n(5));
        let method = scene.method(class, 5);
        if method.status == 4 {
            return n(-1);
        }
        if method.status == 1 {
            if method.id == 35 {
                let status = lifecycle::disable(&mut runtime.ticks, event.id, limit);
                if status >= 2 {
                    return mutation_error(status);
                }
            } else if method.id != 32 && method.id != 33 && method.id != 34 {
                return n(-5);
            }
        }
        if method.status != 2 {
            attitude::put(runtime.slot, 0, saved);
        }
        if event.destroy {
            return n(-5);
        }
    }
    n(0)
}
