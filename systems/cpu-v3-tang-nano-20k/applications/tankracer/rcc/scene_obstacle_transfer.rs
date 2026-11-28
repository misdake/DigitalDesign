//! Obstacle target methods on the live collider, grid and ordered memberships.
use crate::dsl_rt::*;
use crate::{
    attitude, entities, integer, lifecycle, ordered, scene_creation, scene_lifecycle, scene_query,
    scene_world, spatial,
};

pub struct Call {
    pub entity: fix32,
    pub slot: u16,
    // 1 is Value, 2 is Position. Other payloads retain the source's missing value.
    pub payload: u16,
    pub value: fix32,
    pub position: Array<u16>,
    // Current object mesh resolves in the resource owner, not just a mesh handle.
    pub shape_present: bool,
    pub result: Array<u16>,
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
fn at(v: Array<u16>, i: u16) -> Array<u16> {
    unsafe { v.as_ptr().add(i as i16).as_u16_array() }
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
fn cell(store: &scene_lifecycle::Store, id: u16) -> spatial::Cell {
    let row = at(store.records, id * 8);
    spatial::Cell {
        x: row[4u16] as i16,
        z: row[5u16],
        present: row[6u16] != 0,
    }
}
fn insert(
    store: &scene_lifecycle::Store,
    world: &mut scene_world::World,
    id: u16,
    shape: bool,
) -> fix32 {
    if id >= store.object_count || at(store.records, id * 8)[7u16] & 1 == 0 {
        return n(-1);
    }
    let collider: Buf<u16, 60> = Buf::new([0; 60]);
    let row = collider.as_array();
    if !scene_world::load_collider(world, id, row) {
        return n(-1);
    }
    let inserted = spatial::insert(
        &mut world.grid,
        fix32::from_words(id, 0),
        at(row, 2),
        shape,
        g(row, 28).lo_bits(),
    );
    if inserted.status != 0 {
        return error(inserted.status);
    }
    let mut meta = at(store.records, id * 8);
    meta[4u16] = inserted.cell.x as u16;
    meta[5u16] = inserted.cell.z;
    meta[6u16] = 1;
    n(0)
}
fn remove(store: &scene_lifecycle::Store, world: &mut scene_world::World, id: u16) -> fix32 {
    if id >= store.object_count || at(store.records, id * 8)[7u16] & 1 == 0 {
        return n(-1);
    }
    let status = spatial::remove(
        &mut world.grid,
        fix32::from_words(id, 0),
        &cell(store, id),
        store.limit,
    );
    if status != 0 {
        return error(status);
    }
    n(0)
}
pub fn dispatch(
    scene: &scene_query::Scene,
    store: &mut scene_lifecycle::Store,
    runtime: &mut scene_world::Runtime,
    world: &mut scene_world::World,
    weapons: &mut scene_creation::Weapons,
    call: &Call,
) -> fix32 {
    let class = entities::class(&store.identities, call.entity);
    if class < n(0) || class >= fix32::from_words(scene.header[4u16], 0) {
        return n(-1);
    }
    let saved = g(runtime.slot, 0);
    p(runtime.slot, 0, fix32::from_words(call.slot, 0));
    let method = scene.method(class.lo_bits(), call.slot);
    if method.status == 4 {
        return n(-1);
    }
    if method.status == 2 {
        p(call.result, 0, n(0));
        return n(0);
    }
    let mut result = n(0);
    if method.status == 1 {
        let mut status = 0u16;
        if method.id == 2 {
            if store.limit == n(0) {
                return n(-1);
            }
            status = lifecycle::enable(&mut runtime.ticks, call.entity);
            if status < 2 {
                result = fix32::from_words(status, 0);
                status = 0;
            }
        } else if method.id == 35 {
            status = lifecycle::disable(&mut runtime.ticks, call.entity, store.limit);
            if status < 2 {
                result = fix32::from_words(status, 0);
                status = 0;
            }
        } else if method.id == 36 {
            status = lifecycle::queue(
                &mut runtime.queues,
                &mut runtime.ticks,
                call.entity,
                true,
                store.limit,
            );
            result = call.entity;
        } else if method.id == 32 || method.id == 33 || method.id == 34 {
            if method.id == 33 {
                result = n(1);
            } else if method.id == 34 {
                result = call.entity;
            }
        } else if method.id == 37 || method.id == 38 {
            // Shared retirement owns component and identity publication order.
            let code = scene_creation::dispatch(
                scene,
                store,
                runtime,
                world,
                weapons,
                &scene_creation::Call {
                    entity: call.entity,
                    slot: call.slot,
                },
            );
            if code != n(0) {
                return code;
            }
            result = n(1);
        } else {
            if entities::handle_kind(&store.identities, call.entity) != 1 {
                return n(-1);
            }
            let object = entities::handle(&store.identities, call.entity);
            if object.hi_bits() != 0 {
                return n(-1);
            }
            let id = object.lo_bits();
            if method.id == 46 {
                if store.limit == n(0) || call.payload != 1 {
                    return n(-1);
                }
                if call.value == n(1) {
                    status = ordered::remove_first(&mut runtime.ordinary, object, store.limit);
                    if status == 0 {
                        status = ordered::append(&mut runtime.special, object);
                    }
                } else {
                    status = ordered::remove_first(&mut runtime.special, object, store.limit);
                    if status == 0 {
                        status = ordered::append(&mut runtime.ordinary, object);
                    }
                }
            } else if method.id == 47 {
                if call.payload != 2 || store.limit == n(0) {
                    return n(-1);
                }
                let collider: Buf<u16, 60> = Buf::new([0; 60]);
                let row = collider.as_array();
                if !scene_world::load_collider(world, id, row) {
                    return n(-1);
                }
                let mut i = 0u16;
                while i < 3 {
                    p(at(row, 2), i, g(call.position, i));
                    i += 1;
                }
                if !scene_world::store_collider(world, id, row) {
                    return n(-1);
                }
                let code = remove(store, world, id);
                if code != n(0) {
                    return code;
                }
                let code = insert(store, world, id, call.shape_present);
                if code != n(0) {
                    return code;
                }
                result = g(call.position, 1);
            } else if method.id == 48 {
                if store.limit == n(0) || call.payload != 1 {
                    return n(-1);
                }
                let code = if call.value == n(1) {
                    insert(store, world, id, call.shape_present)
                } else {
                    remove(store, world, id)
                };
                if code != n(0) {
                    return code;
                }
            } else {
                return n(-5);
            }
        }
        if status != 0 {
            return error(status);
        }
    }
    p(call.result, 0, result);
    p(runtime.slot, 0, saved);
    n(0)
}
