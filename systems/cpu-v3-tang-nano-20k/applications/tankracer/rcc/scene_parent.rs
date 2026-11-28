//! Fall/Closer class calls on persistent resource, lifetime and map owners.
use crate::dsl_rt::*;
use crate::{
    attitude, closer, driving, entities, falling, ground, integer, lifecycle, mesh_catalog,
    scene_debris, scene_fragments, scene_lifecycle, scene_meshes, scene_query, scene_world,
    shatter, spatial, symbols,
};

pub struct State {
    pub lifetime: scene_lifecycle::Store,
    pub runtime: scene_world::Runtime,
    pub resources: scene_meshes::Store,
    pub map: ground::World,
    pub actors: Array<Actor>,
    pub actor_count: u16,
    pub cars: Array<driving::Car>,
    pub car_count: u16,
    pub slots: Array<u16>,
    pub symbols: Array<u16>,
    pub models: mesh_catalog::Sources,
}
pub struct Actor {
    pub fragments: scene_fragments::Object,
    pub properties: Array<u16>,
    pub state: Array<u16>,
    // 1 Fall, 2 Closer, 3 Pickup. Component families, separate from method IDs.
    pub kind: u16,
    pub has_state: bool,
    pub reference_alias: bool,
    pub debris: Array<shatter::State>,
    pub attached: Array<u16>,
    pub attached_count: u16,
    pub definition: Array<u16>,
}
pub struct Hit {
    pub method: u16,
    // Nested result belongs to the auxiliary call, separate from the method's
    // own output: Fall clears its output even when a nested call fails.
    pub auxiliary: scene_fragments::Hit,
    pub result: Array<u16>,
    pub forward: fix32,
    pub minimum_turn: fix32,
    pub maximum_turn: fix32,
    pub current: u16,
    pub mode: fix32,
    pub dt: fix32,
    pub sine: Array<i16>,
}
pub struct Frame {
    pub auxiliary: scene_fragments::Frame,
    pub result: Array<u16>,
    pub group: fix32,
    pub player: Array<u16>,
    pub has_player: bool,
    pub peer: Array<u16>,
    pub has_peer: bool,
    pub tuning: Array<u16>,
    pub angle: Array<i16>,
    pub inverse_cosine: Array<i16>,
}
fn n(v: i16) -> fix32 {
    integer::from_i16(v)
}
fn at(data: Array<u16>, offset: u16) -> Array<u16> {
    unsafe { data.as_ptr().add(offset as i16).as_u16_array() }
}
fn error(status: u16) -> fix32 {
    if status == 3 {
        return n(-3);
    }
    if status == 4 {
        return n(-4);
    }
    n(-1)
}
fn activate(scene: &scene_query::Scene, state: &mut State, actor: &Actor, class: fix32) -> fix32 {
    let entity = attitude::get(actor.fragments.collider, 0);
    if class < n(0) || class >= fix32::from_words(scene.header[4u16], 0) {
        return n(-1);
    }
    let saved = attitude::get(state.runtime.slot, 0);
    attitude::put(state.runtime.slot, 0, n(4));
    let method = scene.method(class.lo_bits(), 4);
    if method.status == 4 {
        return n(-1);
    }
    if method.status == 2 {
        return n(0);
    }
    if method.status == 1 {
        if method.id == 2 {
            if state.lifetime.limit == n(0) {
                return n(-1);
            }
            let status = lifecycle::enable(&mut state.runtime.ticks, entity);
            if status >= 2 {
                return error(status);
            }
        } else if method.id != 32 && method.id != 33 && method.id != 34 {
            return n(-5);
        }
    }
    attitude::put(state.runtime.slot, 0, saved);
    n(0)
}
pub fn auxiliary_hit(
    scene: &scene_query::Scene,
    state: &mut State,
    grid: &mut spatial::Grid,
    actor: &mut Actor,
    call: &Hit,
) -> fix32 {
    let class = attitude::get(actor.properties, 0);
    if class < n(0) || class >= fix32::from_words(scene.header[4u16], 0) {
        return n(-1);
    }
    let saved = attitude::get(state.runtime.slot, 0);
    attitude::put(state.runtime.slot, 0, n(25));
    let method = scene.method(class.lo_bits(), 25);
    if method.status == 4 {
        return n(-1);
    }
    if method.status == 2 {
        return n(0);
    }
    if method.status == 1 {
        if method.id == 70 {
            let status = scene_fragments::activate(
                &mut state.resources,
                &mut actor.fragments,
                &mut state.runtime,
                grid,
                &mut state.map,
                &call.auxiliary,
            );
            if status != n(0) {
                return status;
            }
        } else if method.id != 32 && method.id != 33 && method.id != 34 {
            return n(-5);
        }
    }
    attitude::put(state.runtime.slot, 0, saved);
    n(0)
}
// Direct BREAKABLE_HIT / CLOSER_HIT entry after collision-slot selection.
pub fn hit(
    scene: &scene_query::Scene,
    state: &mut State,
    grid: &mut spatial::Grid,
    actor: &mut Actor,
    call: &Hit,
) -> fix32 {
    let status = hit_inner(scene, state, grid, actor, call);
    if call.method == 1 || status == n(0) {
        attitude::put(call.result, 0, n(0));
    }
    status
}
fn hit_inner(
    scene: &scene_query::Scene,
    state: &mut State,
    grid: &mut spatial::Grid,
    actor: &mut Actor,
    call: &Hit,
) -> fix32 {
    let row = call.auxiliary.row;
    if call.method == 1 {
        if actor.kind != 1 {
            return n(-1);
        }
        let flags = attitude::get(actor.properties, 4);
        if attitude::get(actor.properties, 3) != n(0) || flags.lo_bits() & 1 != 0 {
            return n(0);
        }
        if !actor.has_state {
            attitude::put(
                actor.properties,
                4,
                fix32::from_words((flags.lo_bits() & 65523) | 1, flags.hi_bits()),
            );
            return n(-1);
        }
        let incoming = falling::Hit {
            velocity: call.auxiliary.velocity,
            forward: call.forward,
            vehicle: call.auxiliary.vehicle,
            entity: attitude::get(actor.fragments.collider, 0),
            minimum_turn_rate: call.minimum_turn,
            maximum_turn_rate: call.maximum_turn,
        };
        falling::begin_strike(actor.state, actor.properties, &incoming);
        // Optional impact audio is an external successful device operation.
        if row[7u16] & 1 == 0 {
            return n(-1);
        }
        attitude::put(actor.properties, 3, n(1));
        let class = attitude::get(actor.properties, 0);
        let status = activate(scene, state, actor, class);
        if status != n(0) {
            return status;
        }
        falling::finish_strike(actor.state, actor.properties, &incoming);
        if row[7u16] & 4 != 0 {
            let status = auxiliary_hit(scene, state, grid, actor, call);
            if status != n(0) {
                return status;
            }
            attitude::put(actor.properties, 3, n(0));
        }
        if row[7u16] & 8 != 0 {
            let class = attitude::get(actor.properties, 0);
            if class < n(0) || class >= fix32::from_words(scene.header[4u16], 0) {
                return n(-1);
            }
            let saved = attitude::get(state.runtime.slot, 0);
            attitude::put(state.runtime.slot, 0, n(26));
            let method = scene.method(class.lo_bits(), 26);
            if method.status == 4 {
                return n(-1);
            }
            if method.status == 1 {
                if method.id == 71 {
                    let status = scene_debris::hit(state, actor, call);
                    if status != n(0) {
                        return status;
                    }
                } else if method.id != 32 && method.id != 33 && method.id != 34 {
                    return n(-5);
                }
            }
            if method.status != 2 {
                attitude::put(state.runtime.slot, 0, saved);
            }
            if row[7u16] & 8 == 0 || !scene_debris::retained(actor) {
                attitude::put(actor.properties, 3, n(0));
            }
            let cell = spatial::Cell {
                x: row[4u16] as i16,
                z: row[5u16],
                present: row[6u16] != 0,
            };
            let status = spatial::remove(
                grid,
                fix32::from_words(actor.fragments.id, 0),
                &cell,
                state.lifetime.limit,
            );
            if status != 0 {
                return error(status);
            }
        }
        let status = attached_hit(scene, state, grid, actor, call);
        if status != n(0) {
            return status;
        }
    } else if call.method == 82 {
        if actor.kind != 2 {
            return n(-1);
        }
        if !actor.has_state {
            return n(-1);
        }
        if attitude::get(actor.state, 1).lo_bits() & 1 != 0 {
            let class = entities::class(
                &state.lifetime.identities,
                attitude::get(actor.fragments.collider, 0),
            );
            let status = activate(scene, state, actor, class);
            if status != n(0) {
                return status;
            }
        }
        if row[7u16] & 1 == 0 {
            return n(-1);
        }
        if row[7u16] & 4 != 0 {
            let status = auxiliary_hit(scene, state, grid, actor, call);
            if status != n(0) {
                return status;
            }
            attitude::put(actor.state, 2, n(3));
        }
    } else {
        return n(-5);
    }
    n(0)
}
fn attached_hit(
    scene: &scene_query::Scene,
    state: &mut State,
    grid: &mut spatial::Grid,
    actor: &Actor,
    call: &Hit,
) -> fix32 {
    let bindings = actor.attached;
    let count = actor.attached_count;
    let mut i = 0u16;
    while i < count {
        let entity = symbols::resolve(state.symbols, at(bindings, i * 3));
        i += 1;
        if !entities::present(&state.lifetime.identities, entity)
            || entities::handle_kind(&state.lifetime.identities, entity) != 1
        {
            continue;
        }
        let id = entities::handle(&state.lifetime.identities, entity);
        if id.hi_bits() != 0 || id.lo_bits() >= state.lifetime.object_count {
            return n(-1);
        }
        let mut j = 0u16;
        while j < state.actor_count && state.actors[j].fragments.id != id.lo_bits() {
            j += 1;
        }
        if j == state.actor_count {
            return n(-5);
        }
        let mut actors = state.actors;
        let target = &mut actors[j];
        let class = attitude::get(target.properties, 0);
        if class < n(0) || class >= fix32::from_words(scene.header[4u16], 0) {
            return n(-1);
        }
        let saved = attitude::get(state.runtime.slot, 0);
        attitude::put(state.runtime.slot, 0, n(24));
        let method = scene.method(class.lo_bits(), 24);
        if method.status == 4 {
            return n(-1);
        }
        if method.status == 2 {
            continue;
        }
        if method.status == 1 {
            let definition = target.definition;
            let scratch: Buf<u16, 8> = Buf::new([0; 8]);
            let mut model = scene_meshes::empty_model();
            if definition[2u16] != 65535 {
                model = mesh_catalog::model(&state.models, definition[2u16], scratch.as_array());
            }
            let nested = Hit {
                method: method.id,
                auxiliary: scene_fragments::Hit {
                    row: at(state.lifetime.records, id.lo_bits() * 8),
                    limit: call.auxiliary.limit,
                    powerup: call.auxiliary.powerup,
                    name: definition[3u16],
                    model_status: n(0),
                    model: model,
                    has_terrain: call.auxiliary.has_terrain,
                    vehicle: call.auxiliary.vehicle,
                    velocity: call.auxiliary.velocity,
                    paired_tail: call.auxiliary.paired_tail,
                    has_paired_tail: call.auxiliary.has_paired_tail,
                    result: call.auxiliary.result,
                },
                result: call.auxiliary.result,
                forward: call.forward,
                minimum_turn: call.minimum_turn,
                maximum_turn: call.maximum_turn,
                current: call.current,
                mode: call.mode,
                dt: call.dt,
                sine: call.sine,
            };
            let status = hit(scene, state, grid, target, &nested);
            if status != n(0) {
                return status;
            }
        }
        attitude::put(state.runtime.slot, 0, saved);
    }
    n(0)
}
fn debris_tick(
    scene: &scene_query::Scene,
    state: &mut State,
    grid: &mut spatial::Grid,
    actor: &Actor,
    frame: &Frame,
) -> fix32 {
    let class = entities::class(&state.lifetime.identities, frame.auxiliary.entity);
    if class < n(0) || class >= fix32::from_words(scene.header[4u16], 0) {
        return n(-1);
    }
    let saved = attitude::get(state.runtime.slot, 0);
    attitude::put(state.runtime.slot, 0, n(23));
    let method = scene.method(class.lo_bits(), 23);
    if method.status == 4 {
        return n(-1);
    }
    if method.status == 2 {
        return n(0);
    }
    if method.status == 1 {
        if method.id == 69 {
            let status = scene_debris::tick(state, actor, frame);
            if status < n(0) {
                return status;
            }
            if status == n(1) {
                let status = scene_lifecycle::dispatch(
                    scene,
                    &mut state.lifetime,
                    &mut state.runtime,
                    grid,
                    frame.auxiliary.entity,
                    8,
                );
                if status != n(0) {
                    return status;
                }
            }
            attitude::put(frame.auxiliary.result, 0, frame.auxiliary.entity);
        } else if method.id != 32 && method.id != 33 && method.id != 34 {
            return n(-5);
        }
    }
    attitude::put(state.runtime.slot, 0, saved);
    n(0)
}
pub fn auxiliary_tick(
    scene: &scene_query::Scene,
    state: &mut State,
    grid: &mut spatial::Grid,
    actor: &mut Actor,
    frame: &Frame,
) -> fix32 {
    // Slot lifetime and geometry lifetime differ. Invalid indices never touch
    // a revision; released valid slots retain the original failed-access touch.
    actor.fragments.resource_present = actor.fragments.private_mesh.hi_bits() == 0
        && actor.fragments.private_mesh.lo_bits() < state.resources.count;
    if actor.fragments.resource_present {
        let view = scene_meshes::view(
            &state.resources,
            actor.fragments.private_mesh.lo_bits(),
            false,
        );
        if view.page != 65535 && view.page != actor.fragments.motion_page {
            halt(0x7f82);
        }
        actor.fragments.mesh = view.mesh;
        actor.fragments.index_page = view.index_page;
        actor.fragments.version = view.revision;
        actor.fragments.geometry_present = view.valid;
    }
    scene_fragments::tick(
        scene,
        &mut state.lifetime,
        &mut state.runtime,
        grid,
        &actor.fragments,
        &frame.auxiliary,
    )
}
pub fn tick(
    scene: &scene_query::Scene,
    state: &mut State,
    grid: &mut spatial::Grid,
    actor: &mut Actor,
    frame: &Frame,
) -> fix32 {
    let entity = frame.auxiliary.entity;
    let class = entities::class(&state.lifetime.identities, entity);
    if class < n(0) || class >= fix32::from_words(scene.header[4u16], 0) {
        return n(-1);
    }
    let saved = attitude::get(state.runtime.slot, 0);
    attitude::put(state.runtime.slot, 0, n(12));
    let method = scene.method(class.lo_bits(), 12);
    if method.status == 4 {
        return n(-1);
    }
    if method.status == 2 {
        attitude::put(frame.result, 0, n(0));
        return n(0);
    }
    if method.status == 1 {
        let kind = entities::handle_kind(&state.lifetime.identities, entity);
        if kind == 2 {
            return n(-5);
        }
        let id = entities::handle(&state.lifetime.identities, entity);
        if kind != 1
            || id.hi_bits() != 0
            || id.lo_bits() >= state.lifetime.object_count
            || id.lo_bits() != actor.fragments.id
        {
            return n(-1);
        }
        let row = at(state.lifetime.records, actor.fragments.id * 8);
        if state.lifetime.limit == n(0) || !actor.has_state {
            return n(-1);
        }
        if method.id == 65 && actor.kind == 1 {
            let body = falling::Body {
                state: actor.state,
                properties: actor.properties,
                collider: actor.fragments.collider,
            };
            if row[7u16] & 1 == 0 {
                return n(-1);
            }
            let step = falling::Step {
                id: id,
                dt: frame.auxiliary.dt,
                limit: state.lifetime.limit,
                cell: spatial::Cell {
                    x: row[4u16] as i16,
                    z: row[5u16],
                    present: row[6u16] != 0,
                },
                vehicle_basis: frame.peer,
                peer_valid: frame.has_peer,
                effect_active: row[7u16] & 8 != 0 && scene_debris::active(actor),
                tuning: frame.tuning,
                sine: frame.auxiliary.sine,
                angle: frame.angle,
            };
            let status = falling::tick_body(&body, &step, &mut state.runtime, &mut state.map, grid);
            if status == 5 {
                return n(-2);
            }
            if status >= 2 {
                return error(status);
            }
            if row[7u16] & 4 != 0 && actor.fragments.active[0u16] != 0 {
                let status = auxiliary_tick(scene, state, grid, actor, frame);
                if status != n(0) {
                    return status;
                }
            }
            if row[7u16] & 8 != 0 && scene_debris::active(actor) {
                let status = debris_tick(scene, state, grid, actor, frame);
                if status != n(0) {
                    return status;
                }
            }
        } else if method.id == 83 && actor.kind == 2 {
            let phase = attitude::get(actor.state, 2);
            if phase == n(0) {
                return n(-5);
            }
            if phase == n(1) {
                closer::poll(
                    actor.state,
                    entity,
                    frame.group,
                    at(actor.fragments.collider, 2),
                    frame.player,
                    frame.has_player,
                );
            } else if phase == n(2) {
                let collider = actor.fragments.collider;
                if attitude::get(collider, 29) == n(0) {
                    return n(-4);
                }
                let step = closer::Step {
                    entity: entity,
                    dt: frame.auxiliary.dt,
                    limit: state.lifetime.limit,
                };
                let tables = closer::Tables {
                    sine: frame.auxiliary.sine,
                    inverse_cosine: frame.inverse_cosine,
                };
                let status = closer::advance(
                    actor.state,
                    at(collider, 26),
                    &mut state.runtime,
                    &step,
                    &tables,
                );
                if actor.reference_alias {
                    let mut words = collider;
                    let mut i = 0u16;
                    while i < 18 {
                        words[8 + i] = collider[26 + i];
                        i += 1;
                    }
                }
                if status != n(0) {
                    return status;
                }
            } else if phase == n(3) {
                let status = auxiliary_tick(scene, state, grid, actor, frame);
                if status != n(0) {
                    return status;
                }
            } else {
                return n(-5);
            }
        } else {
            return n(-5);
        }
    }
    attitude::put(frame.result, 0, n(0));
    attitude::put(state.runtime.slot, 0, saved);
    n(0)
}
// Resource-aware destructor method, before the factory's final release slot.
pub fn destroy_method(
    scene: &scene_query::Scene,
    state: &mut State,
    grid: &mut spatial::Grid,
    actor: &mut Actor,
    entity: fix32,
    method: u16,
) -> fix32 {
    if state.lifetime.limit == n(0) {
        return n(-1);
    }
    let mut row = at(state.lifetime.records, actor.fragments.id * 8);
    if method == 66 {
        if actor.kind != 1 {
            return n(-1);
        }
        if !actor.has_state {
            return n(-1);
        }
        if row[7u16] & 64 != 0 {
            let limit = state.lifetime.limit;
            if limit.hi_bits() == 0 && actor.attached_count > limit.lo_bits() {
                actor.attached = at(actor.attached, limit.lo_bits() * 3);
                actor.attached_count -= limit.lo_bits();
                return n(-3);
            }
            actor.attached_count = 0;
            row[7u16] &= 65471;
        }
        let slot = attitude::get(state.runtime.slot, 0);
        if slot.hi_bits() != 0 {
            return n(0);
        }
        let parent = scene.method(state.lifetime.obstacle_class, slot.lo_bits());
        if parent.status == 4 {
            return n(-1);
        }
        if parent.status != 1 {
            return n(0);
        }
        if parent.id != 49 {
            let class = state.lifetime.obstacle_class;
            return scene_lifecycle::parent_base(
                scene,
                &mut state.lifetime,
                &mut state.runtime,
                entity,
                class,
            );
        }
    }
    if row[7u16] & 1 == 0 {
        return n(-1);
    }
    if row[7u16] & 16 != 0 {
        let private = actor.fragments.private_mesh;
        if private.hi_bits() != 0
            || !scene_meshes::release_private(&mut state.resources, private.lo_bits())
        {
            return n(-1);
        }
    }
    actor.fragments.private_mesh = n(-1);
    actor.fragments.motion.count = 0;
    let mut active = actor.fragments.active;
    active[0u16] = 0;
    row[7u16] &= 65347;
    let cell = spatial::Cell {
        x: row[4u16] as i16,
        z: row[5u16],
        present: row[6u16] != 0,
    };
    let status = spatial::remove(
        grid,
        fix32::from_words(actor.fragments.id, 0),
        &cell,
        state.lifetime.limit,
    );
    if status != 0 {
        return error(status);
    }
    let class = state.lifetime.generic_class;
    let status = scene_lifecycle::parent_base(
        scene,
        &mut state.lifetime,
        &mut state.runtime,
        entity,
        class,
    );
    if row[7u16] & 1 == 0 {
        actor.has_state = false;
    }
    status
}
pub fn destroy(
    scene: &scene_query::Scene,
    state: &mut State,
    grid: &mut spatial::Grid,
    actor: &mut Actor,
    entity: fix32,
) -> fix32 {
    if !entities::present(&state.lifetime.identities, entity) {
        return n(0);
    }
    let class = entities::class(&state.lifetime.identities, entity);
    if class < n(0) || class >= fix32::from_words(scene.header[4u16], 0) {
        return n(-1);
    }
    let saved = attitude::get(state.runtime.slot, 0);
    attitude::put(state.runtime.slot, 0, n(2));
    let method = scene.method(class.lo_bits(), 2);
    if method.status == 4 {
        return n(-1);
    }
    if method.status == 1 {
        let kind = entities::handle_kind(&state.lifetime.identities, entity);
        let id = entities::handle(&state.lifetime.identities, entity);
        if kind == 2 {
            return n(-5);
        }
        if kind != 1 || id.hi_bits() != 0 || id.lo_bits() != actor.fragments.id {
            return n(-1);
        }
        if method.id == 66 || method.id == 49 {
            let status = destroy_method(scene, state, grid, actor, entity, method.id);
            if status != n(0) {
                return status;
            }
        } else {
            let status = scene_lifecycle::dispatch(
                scene,
                &mut state.lifetime,
                &mut state.runtime,
                grid,
                entity,
                2,
            );
            if status != n(0) {
                return status;
            }
        }
    }
    if method.status != 2 {
        attitude::put(state.runtime.slot, 0, saved);
    }
    let status = scene_lifecycle::dispatch(
        scene,
        &mut state.lifetime,
        &mut state.runtime,
        grid,
        entity,
        3,
    );
    if at(state.lifetime.records, actor.fragments.id * 8)[7u16] & 1 == 0 {
        actor.has_state = false;
    }
    if status != n(0) {
        return status;
    }
    entities::finish_destroy(&state.lifetime.identities);
    n(0)
}
