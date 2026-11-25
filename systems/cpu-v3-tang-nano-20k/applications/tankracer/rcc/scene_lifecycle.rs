//! Shared identity, memberships and ordinary component retirement.
use crate::dsl_rt::*;
use crate::{attitude, entities, integer, lifecycle, ordered, scene_query, scene_world, spatial};
pub struct Store {
    pub identities: entities::State,
    // Eight-word scenery rows; word 7 owns the lifetime/resource flags.
    // present=1, transient=2, auxiliary=4, debris=8, private mesh=16,
    // fragment motion=32, attached bindings=64, active fragments=128.
    pub records: Array<u16>,
    pub object_count: u16,
    pub colliders: Array<u16>,
    pub collider_stride: u16,
    pub generic_class: u16,
    pub obstacle_class: u16,
    pub limit: fix32,
    pub free_objects: ordered::List,
    pub lap_objects: ordered::List,
    pub signs: ordered::List,
    pub enter: ordered::List,
    pub leave: ordered::List,
}
fn n(value: i16) -> fix32 {
    integer::from_i16(value)
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
fn at(words: Array<u16>, offset: u16) -> Array<u16> {
    unsafe { words.as_ptr().add(offset as i16).as_u16_array() }
}
fn object(store: &Store, entity: fix32) -> fix32 {
    let kind = entities::handle_kind(&store.identities, entity);
    if kind == 0 || kind == 3 || kind == 4 {
        return n(-1);
    }
    if kind != 1 {
        return n(-5);
    }
    let id = entities::handle(&store.identities, entity);
    if id.hi_bits() != 0 || id.lo_bits() >= store.object_count {
        return n(-1);
    }
    id
}
fn retain(list: &mut ordered::List, id: fix32) {
    let mut from = 0u16;
    let mut to = 0u16;
    while from < list.count {
        let value = attitude::get(list.items, from);
        if value != id {
            attitude::put(list.items, to, value);
            to += 1;
        }
        from += 1;
    }
    list.count = to;
}
fn base(
    method: u16,
    store: &mut Store,
    runtime: &mut scene_world::Runtime,
    entity: fix32,
) -> fix32 {
    if method == 32 || method == 33 || method == 34 {
        return n(0);
    }
    if method == 2 {
        if store.limit == n(0) {
            return n(-1);
        }
        let status = lifecycle::enable(&mut runtime.ticks, entity);
        if status >= 2 {
            return error(status);
        }
        return n(0);
    }
    if method == 35 {
        let status = lifecycle::disable(&mut runtime.ticks, entity, store.limit);
        if status >= 2 {
            return error(status);
        }
        return n(0);
    }
    if method == 36 {
        let status = lifecycle::queue(
            &mut runtime.queues,
            &mut runtime.ticks,
            entity,
            true,
            store.limit,
        );
        if status != 0 {
            return error(status);
        }
        return n(0);
    }
    if method == 39 {
        if entities::handle_kind(&store.identities, entity) == 2 {
            return n(-5);
        }
        if entities::handle_kind(&store.identities, entity) == 1
            && (entity.hi_bits() != 0 || entity.lo_bits() >= runtime.ticks.flag_count)
        {
            return n(-1);
        }
        return n(0);
    }
    if method != 37 && method != 38 {
        return n(-5);
    }
    let kind = entities::handle_kind(&store.identities, entity);
    let mut id = n(-1);
    let mut flags = 0u16;
    if kind == 1 {
        id = object(store, entity);
        if id < n(0) {
            return id;
        }
        let row = at(store.records, id.lo_bits() * 8);
        flags = row[7u16];
        if row[0u16] > 4 || flags & 65532 != 0 {
            return n(-5);
        }
    }
    let mut status = 0u16;
    if method == 37 {
        status = entities::detach(&store.identities, entity);
    } else {
        status = entities::release_id(&store.identities, entity);
    }
    if status != 0 {
        return error(status);
    }
    if kind == 1 {
        let mut row = at(store.records, id.lo_bits() * 8);
        // An absent object has no remaining per-class components in this
        // ordinary owner. The original slot.take precedes free-list writes.
        row[7u16] = 0;
        if flags & 1 != 0 && flags & 2 != 0 {
            let collider_entity =
                attitude::get(at(store.colliders, id.lo_bits() * store.collider_stride), 0);
            retain(&mut runtime.ordinary, id);
            retain(&mut runtime.special, id);
            retain(&mut store.lap_objects, id);
            retain(&mut store.signs, id);
            retain(&mut store.enter, collider_entity);
            retain(&mut store.leave, collider_entity);
            let s = ordered::append(&mut store.free_objects, id);
            if s != 0 {
                return error(s);
            }
        }
        if flags & 1 != 0 {
            row[0u16] = 0;
        }
    }
    n(0)
}
pub fn parent_base(
    scene: &scene_query::Scene,
    store: &mut Store,
    runtime: &mut scene_world::Runtime,
    entity: fix32,
    class: u16,
) -> fix32 {
    let slot = attitude::get(runtime.slot, 0);
    if slot.hi_bits() != 0 {
        return n(0);
    }
    let method = scene.method(class, slot.lo_bits());
    if method.status == 4 {
        return n(-1);
    }
    if method.status != 1 {
        return n(0);
    }
    base(method.id, store, runtime, entity)
}
fn obstacle_destroy(
    scene: &scene_query::Scene,
    store: &mut Store,
    runtime: &mut scene_world::Runtime,
    grid: &mut spatial::Grid,
    entity: fix32,
) -> fix32 {
    if store.limit == n(0) {
        return n(-1);
    }
    let id = object(store, entity);
    if id < n(0) {
        return id;
    }
    let row = at(store.records, id.lo_bits() * 8);
    if row[7u16] & 1 == 0 {
        return n(-1);
    }
    if row[7u16] & 65532 != 0 {
        return n(-5);
    }
    let cell = spatial::Cell {
        x: row[4u16] as i16,
        z: row[5u16],
        present: row[6u16] != 0,
    };
    let status = spatial::remove(grid, id, &cell, store.limit);
    if status != 0 {
        return error(status);
    }
    parent_base(scene, store, runtime, entity, store.generic_class)
}
fn deactivate(
    scene: &scene_query::Scene,
    store: &mut Store,
    runtime: &mut scene_world::Runtime,
    grid: &mut spatial::Grid,
    entity: fix32,
) -> fix32 {
    let id = object(store, entity);
    if id < n(0) {
        return id;
    }
    let row = at(store.records, id.lo_bits() * 8);
    if row[7u16] & 1 == 0 || store.limit == n(0) {
        return n(-1);
    }
    let slot = attitude::get(runtime.slot, 0);
    if slot.hi_bits() == 0 {
        let method = scene.method(store.generic_class, slot.lo_bits());
        if method.status == 4 {
            return n(-1);
        }
        if method.status == 1 {
            let mut status = n(0);
            if method.id == 45 {
                status = deactivate(scene, store, runtime, grid, entity);
            } else if method.id == 2
                || method.id == 36
                || method.id == 32
                || method.id == 33
                || method.id == 34
            {
                status = base(method.id, store, runtime, entity);
            } else {
                return n(-5);
            }
            if status != n(0) {
                return status;
            }
        }
    }
    let cell = spatial::Cell {
        x: row[4u16] as i16,
        z: row[5u16],
        present: row[6u16] != 0,
    };
    let status = spatial::remove(grid, id, &cell, store.limit);
    if status != 0 {
        return error(status);
    }
    let status = ordered::remove_first(&mut runtime.ordinary, id, store.limit);
    if status != 0 {
        return error(status);
    }
    let status = ordered::remove_first(&mut runtime.special, id, store.limit);
    if status != 0 {
        return error(status);
    }
    n(0)
}
fn invoke(
    scene: &scene_query::Scene,
    store: &mut Store,
    runtime: &mut scene_world::Runtime,
    grid: &mut spatial::Grid,
    entity: fix32,
    method: u16,
) -> fix32 {
    if method == 45 {
        return deactivate(scene, store, runtime, grid, entity);
    }
    if method == 57 {
        let id = object(store, entity);
        if id < n(0) {
            return id;
        }
        return dispatch(scene, store, runtime, grid, entity, 8);
    }
    if method == 49 {
        return obstacle_destroy(scene, store, runtime, grid, entity);
    }
    if method != 66 {
        return base(method, store, runtime, entity);
    }
    if store.limit == n(0) {
        return n(-1);
    }
    let id = object(store, entity);
    if id < n(0) {
        return id;
    }
    let row = at(store.records, id.lo_bits() * 8);
    if row[0u16] == 3 || row[7u16] & 64 != 0 {
        return n(-5);
    }
    if row[0u16] != 1 {
        return n(-1);
    }
    let slot = attitude::get(runtime.slot, 0);
    if slot.hi_bits() != 0 {
        return n(0);
    }
    let parent = scene.method(store.obstacle_class, slot.lo_bits());
    if parent.status == 4 {
        return n(-1);
    }
    if parent.status != 1 {
        return n(0);
    }
    if parent.id == 49 {
        return obstacle_destroy(scene, store, runtime, grid, entity);
    }
    base(parent.id, store, runtime, entity)
}
pub fn dispatch(
    scene: &scene_query::Scene,
    store: &mut Store,
    runtime: &mut scene_world::Runtime,
    grid: &mut spatial::Grid,
    entity: fix32,
    slot: u16,
) -> fix32 {
    let class = entities::class(&store.identities, entity);
    if class < n(0) || class >= fix32::from_words(scene.header[4u16], 0) {
        return n(-1);
    }
    let saved = attitude::get(runtime.slot, 0);
    attitude::put(runtime.slot, 0, fix32::from_words(slot, 0));
    let method = scene.method(class.lo_bits(), slot);
    if method.status == 4 {
        return n(-1);
    }
    if method.status == 2 {
        return n(0);
    }
    if method.status == 1 {
        let status = invoke(scene, store, runtime, grid, entity, method.id);
        if status != n(0) {
            return status;
        }
    }
    attitude::put(runtime.slot, 0, saved);
    n(0)
}
pub fn stop(
    scene: &scene_query::Scene,
    store: &mut Store,
    runtime: &mut scene_world::Runtime,
    grid: &mut spatial::Grid,
    entity: fix32,
) -> fix32 {
    dispatch(scene, store, runtime, grid, entity, 5)
}
pub fn destroy(
    scene: &scene_query::Scene,
    store: &mut Store,
    runtime: &mut scene_world::Runtime,
    grid: &mut spatial::Grid,
    entity: fix32,
) -> fix32 {
    if !entities::present(&store.identities, entity) {
        return n(0);
    }
    let status = dispatch(scene, store, runtime, grid, entity, 2);
    if status != n(0) {
        return status;
    }
    let status = dispatch(scene, store, runtime, grid, entity, 3);
    if status != n(0) {
        return status;
    }
    entities::finish_destroy(&store.identities);
    n(0)
}
pub fn drain(
    scene: &scene_query::Scene,
    store: &mut Store,
    runtime: &mut scene_world::Runtime,
    grid: &mut spatial::Grid,
) -> fix32 {
    let mut pass = lifecycle::begin_drain(store.limit);
    loop {
        let event = lifecycle::pop(&mut runtime.queues, &mut pass, store.limit);
        if event.status == 0 {
            break;
        }
        if event.status != 1 {
            // Factory traversal errors are distinct from a class method's
            // -3 work failure and are normalized to -1 at the scene boundary.
            return n(-1);
        }
        let status = stop(scene, store, runtime, grid, event.id);
        if status != n(0) {
            return status;
        }
        if event.destroy {
            let status = destroy(scene, store, runtime, grid, event.id);
            if status != n(0) {
                return status;
            }
        }
    }
    n(0)
}
