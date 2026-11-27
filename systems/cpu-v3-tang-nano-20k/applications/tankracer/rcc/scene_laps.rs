//! Lap gates, delayed camera queues and off-camera Fall restoration.
use crate::dsl_rt::*;
use crate::{
    attitude, entities, integer, lifecycle, ordered, scene_creation, scene_lifecycle, scene_query,
    scene_world, spatial,
};
pub struct Attachment {
    pub object: u16,
    pub present: Array<u16>,
    pub attached: Array<u16>,
}
pub struct Context {
    // Per object: signed add/remove lap, entered, resolved shape, pending Crush,
    // actor kind (1 pickup, 2 train, 3 lander, 4 flock, 5 background, 6 other).
    pub objects: Array<u16>,
    pub count: u16,
    // Sorted object ID followed by nine i32 backup basis columns, 19 words/row.
    pub backups: Array<u16>,
    pub backup_count: u16,
    pub attachments: Array<Attachment>,
    pub attachment_count: u16,
    pub camera: Array<u16>,
    pub lap: i16,
    pub flush: bool,
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
fn error(code: u16) -> fix32 {
    if code == 3 {
        return n(-3);
    }
    if code == 4 {
        return n(-4);
    }
    n(-1)
}
fn near(world: &scene_world::World, id: fix32, camera: Array<u16>) -> fix32 {
    let collider: Buf<u16, 60> = Buf::new([0; 60]);
    let row = collider.as_array();
    if id.hi_bits() != 0 || !scene_world::load_collider(world, id.lo_bits(), row) {
        return n(-1);
    }
    let mut axis = 0u16;
    while axis < 3 {
        let mut delta = integer::sar(g(at(row, 2), axis), 3) - g(camera, axis);
        if delta < n(0) {
            delta = -delta;
        }
        if delta >= n(19200) {
            return n(0);
        }
        axis += 2;
    }
    n(1)
}
fn component(store: &scene_lifecycle::Store, id: u16) -> bool {
    id < store.object_count && at(store.records, id * 8)[7u16] & 1 != 0
}
fn insert(
    store: &scene_lifecycle::Store,
    world: &mut scene_world::World,
    ctx: &Context,
    id: u16,
) -> fix32 {
    if !component(store, id) || id >= ctx.count {
        return n(-1);
    }
    let collider: Buf<u16, 60> = Buf::new([0; 60]);
    let raw = collider.as_array();
    if !scene_world::load_collider(world, id, raw) {
        return n(-1);
    }
    let added = spatial::insert(
        &mut world.grid,
        fix32::from_words(id, 0),
        at(raw, 2),
        at(ctx.objects, id * 6)[3u16] != 0,
        raw[56u16],
    );
    if added.status != 0 {
        return error(added.status);
    }
    let mut row = at(store.records, id * 8);
    row[4u16] = added.cell.x as u16;
    row[5u16] = added.cell.z;
    row[6u16] = 1;
    n(0)
}
fn remove(store: &scene_lifecycle::Store, world: &mut scene_world::World, id: u16) -> fix32 {
    if !component(store, id) {
        return n(-1);
    }
    let row = at(store.records, id * 8);
    let cell = spatial::Cell {
        x: row[4u16] as i16,
        z: row[5u16],
        present: row[6u16] != 0,
    };
    let status = spatial::remove(
        &mut world.grid,
        fix32::from_words(id, 0),
        &cell,
        store.limit,
    );
    if status != 0 {
        return error(status);
    }
    n(0)
}
// 0/1 are live detached/attached state; -1 is a missing surface/initial record.
fn records(ctx: &Context, id: u16) -> fix32 {
    let mut i = 0u16;
    while i < ctx.attachment_count {
        let entry = &ctx.attachments[i];
        if entry.object == id && entry.present[0u16] != 0 {
            if entry.attached[0u16] != 0 {
                return n(1);
            }
            return n(0);
        }
        i += 1;
    }
    if id < ctx.count && at(ctx.objects, id * 6)[4u16] != 0 {
        return n(0);
    }
    n(-1)
}
pub struct Action {
    pub context: Array<Context>,
    pub entity: fix32,
    pub leave: bool,
}
fn action(
    scene: &scene_query::Scene,
    store: &mut scene_lifecycle::Store,
    runtime: &mut scene_world::Runtime,
    world: &mut scene_world::World,
    weapons: &mut scene_creation::Weapons,
    call: &Action,
) -> fix32 {
    let ctx = &call.context[0u16];
    let entity = call.entity;
    if entities::handle_kind(&store.identities, entity) != 1 {
        return n(-5);
    }
    let object = entities::handle(&store.identities, entity);
    if object.hi_bits() != 0 || object.lo_bits() >= ctx.count {
        return n(-1);
    }
    let id = object.lo_bits();
    let class = entities::class(&store.identities, entity);
    if class < n(0) || class >= fix32::from_words(scene.header[4u16], 0) {
        return n(-1);
    }
    let mut slot = 6u16;
    if call.leave {
        slot = 7;
    }
    let saved = g(runtime.slot, 0);
    p(runtime.slot, 0, fix32::from_words(slot, 0));
    let method = scene.method(class.lo_bits(), slot);
    if method.status == 4 {
        return n(-1);
    }
    if method.status == 1 {
        let mut status = 0u16;
        let mut code = n(0);
        if method.id == 43 {
            if store.limit == n(0) {
                return n(-1);
            }
            status = ordered::append(&mut runtime.ordinary, object);
            if status == 0 {
                code = insert(store, world, ctx, id);
            }
        } else if method.id == 44 {
            if store.limit == n(0) {
                return n(-1);
            }
            status = ordered::remove_first(&mut runtime.ordinary, object, store.limit);
            if status == 0 {
                code = remove(store, world, id);
            }
        } else if method.id == 52 || method.id == 53 {
            let attached = records(ctx, id);
            if attached < n(0) {
                return attached;
            }
            if store.limit == n(0) {
                return n(-1);
            }
            if method.id == 52 {
                status = ordered::append(&mut world.deformables, object);
                if status == 0 {
                    status = ordered::append(&mut runtime.special, object);
                }
                if status == 0 && attached == n(1) {
                    code = scene_creation::dispatch(
                        scene,
                        store,
                        runtime,
                        world,
                        weapons,
                        &scene_creation::Call { entity, slot: 4 },
                    );
                }
            } else {
                status = ordered::remove_first(&mut world.deformables, object, store.limit);
                if status == 0 {
                    status = ordered::remove_first(&mut runtime.special, object, store.limit);
                }
                if status == 0 && attached == n(1) {
                    status = lifecycle::queue(
                        &mut runtime.queues,
                        &mut runtime.ticks,
                        entity,
                        false,
                        store.limit,
                    );
                }
            }
        } else if method.id == 112 || method.id == 113 {
            let kind = at(ctx.objects, id * 6)[5u16];
            if kind == 0 {
                return n(-1);
            }
            if kind > 5 {
                return n(-5);
            }
            if method.id == 112 {
                if kind == 2 || kind == 4 {
                    status = ordered::append(&mut runtime.special, object);
                }
                if status == 0 {
                    code = scene_creation::dispatch(
                        scene,
                        store,
                        runtime,
                        world,
                        weapons,
                        &scene_creation::Call { entity, slot: 4 },
                    );
                }
            } else {
                status = ordered::remove_first(&mut runtime.special, object, store.limit);
                if status == 0 && kind == 1 {
                    code = remove(store, world, id);
                }
                if status == 0 && code == n(0) {
                    status = lifecycle::queue(
                        &mut runtime.queues,
                        &mut runtime.ticks,
                        entity,
                        false,
                        store.limit,
                    );
                    if status != 0 {
                        return n(-3);
                    }
                }
            }
        } else {
            code = scene_creation::dispatch(
                scene,
                store,
                runtime,
                world,
                weapons,
                &scene_creation::Call { entity, slot },
            );
        }
        if status != 0 {
            return error(status);
        }
        if code != n(0) {
            return code;
        }
    }
    // Above-maximum calls keep their requested slot; all other successes restore.
    if method.status != 2 {
        p(runtime.slot, 0, saved);
    }
    if !component(store, id) {
        return n(-1);
    }
    let mut data = at(ctx.objects, id * 6);
    data[2u16] = 1;
    if call.leave {
        data[2u16] = 0;
    }
    n(0)
}
fn restore(store: &scene_lifecycle::Store, world: &mut scene_world::World, ctx: &Context) -> fix32 {
    let mut i = 0u16;
    while i < ctx.backup_count {
        let saved = at(ctx.backups, i * 19);
        let id = saved[0u16];
        if id < store.object_count && at(store.records, id * 8)[0u16] == 1 {
            let nearby = near(world, fix32::from_words(id, 0), ctx.camera);
            if nearby < n(0) {
                return nearby;
            }
            if nearby == n(0) {
                let props: Buf<u16, 16> = Buf::new([0; 16]);
                let row = props.as_array();
                let previous = spatial::select(&world.grid);
                let loaded = scene_world::load_properties(world, id, row);
                unsafe {
                    write_dseg_page(3, previous);
                }
                if !loaded {
                    return n(-1);
                }
                if g(row, 4).lo_bits() & 1 != 0 && g(row, 3) == n(0) {
                    let collider: Buf<u16, 60> = Buf::new([0; 60]);
                    let raw = collider.as_array();
                    if !scene_world::load_collider(world, id, raw) {
                        return n(-1);
                    }
                    let mut alias = g(raw, 29) != n(0);
                    let mut j = 0u16;
                    while alias && j < 18 {
                        alias = at(raw, 8)[j] == at(raw, 26)[j];
                        j += 1;
                    }
                    let mut basis = at(raw, 8);
                    let mut external = at(raw, 26);
                    j = 0;
                    while j < 18 {
                        basis[j] = saved[1 + j];
                        if alias {
                            external[j] = saved[1 + j];
                        }
                        j += 1;
                    }
                    if !scene_world::store_collider(world, id, raw) {
                        return n(-1);
                    }
                    p(row, 4, n(32));
                    let previous = spatial::select(&world.grid);
                    let written = scene_world::store_properties(world, id, row);
                    unsafe {
                        write_dseg_page(3, previous);
                    }
                    if !written {
                        return n(-1);
                    }
                    let code = insert(store, world, ctx, id);
                    if code != n(0) {
                        return code;
                    }
                }
            }
        }
        i += 1;
    }
    n(0)
}
pub fn step(
    scene: &scene_query::Scene,
    store: &mut scene_lifecycle::Store,
    runtime: &mut scene_world::Runtime,
    world: &mut scene_world::World,
    weapons: &mut scene_creation::Weapons,
    contexts: Array<Context>,
) -> fix32 {
    let ctx = &contexts[0u16];
    let mut call = Action {
        context: contexts,
        entity: n(0),
        leave: false,
    };
    if ctx.flush {
        let mut work = n(0);
        let mut pass = 0u16;
        while pass < 2 {
            let mut index = 0u16;
            loop {
                let mut count = store.enter.count;
                let mut items = store.enter.items;
                if pass == 1 {
                    count = store.leave.count;
                    items = store.leave.items;
                }
                if index >= count {
                    break;
                }
                let saved = read_dseg_page(3);
                if store.queue_page != 65535 && store.queue_page != saved {
                    unsafe {
                        write_dseg_page(3, store.queue_page);
                    }
                }
                let entity = g(items, index);
                if read_dseg_page(3) != saved {
                    unsafe {
                        write_dseg_page(3, saved);
                    }
                }
                work = work + n(1);
                if work.hi_bits() > store.limit.hi_bits()
                    || (work.hi_bits() == store.limit.hi_bits()
                        && work.lo_bits() > store.limit.lo_bits())
                {
                    return n(-3);
                }
                let kind = entities::handle_kind(&store.identities, entity);
                let mut far = false;
                if kind == 1 {
                    let result = near(
                        world,
                        entities::handle(&store.identities, entity),
                        ctx.camera,
                    );
                    if result < n(0) {
                        return result;
                    }
                    far = result == n(0);
                } else if kind != 0 {
                    return n(-5);
                }
                if far {
                    call.entity = entity;
                    call.leave = pass == 1;
                    let code = action(scene, store, runtime, world, weapons, &call);
                    if code != n(0) {
                        return code;
                    }
                    let mut count = 0u16;
                    let saved = read_dseg_page(3);
                    if store.queue_page != 65535 && store.queue_page != saved {
                        unsafe {
                            write_dseg_page(3, store.queue_page);
                        }
                    }
                    if pass == 0 {
                        ordered::remove_at(&mut store.enter, index);
                        count = store.enter.count;
                    } else {
                        ordered::remove_at(&mut store.leave, index);
                        count = store.leave.count;
                    }
                    if read_dseg_page(3) != saved {
                        unsafe {
                            write_dseg_page(3, saved);
                        }
                    }
                    if index < count {
                        index += 1;
                    } else {
                        index = 0;
                    }
                } else {
                    index += 1;
                }
            }
            pass += 1;
        }
        return n(0);
    }
    if store.limit.hi_bits() == 0 && store.lap_objects.count > store.limit.lo_bits() {
        return n(-3);
    }
    let count = store.lap_objects.count;
    let mut i = 0u16;
    while i < count {
        let original = g(store.lap_objects.items, i);
        let collider: Buf<u16, 60> = Buf::new([0; 60]);
        let raw = collider.as_array();
        if original.hi_bits() != 0 || !scene_world::load_collider(world, original.lo_bits(), raw) {
            return n(-1);
        }
        let entity = g(raw, 0);
        if entities::present(&store.identities, entity) {
            if entities::handle_kind(&store.identities, entity) != 1 {
                return n(-5);
            }
            let object = entities::handle(&store.identities, entity);
            if object.hi_bits() == 0
                && object.lo_bits() < ctx.count
                && component(store, object.lo_bits())
            {
                let data = at(ctx.objects, object.lo_bits() * 6);
                let mut matched = false;
                let mut leave = false;
                if data[0u16] as i16 == ctx.lap {
                    matched = true;
                } else if data[1u16] as i16 == ctx.lap {
                    matched = true;
                    leave = true;
                }
                if matched {
                    let nearby = near(world, object, ctx.camera);
                    if nearby < n(0) {
                        return nearby;
                    }
                    if nearby == n(1) {
                        let mut status = 0u16;
                        let saved = read_dseg_page(3);
                        if store.queue_page != 65535 && store.queue_page != saved {
                            unsafe {
                                write_dseg_page(3, store.queue_page);
                            }
                        }
                        if leave {
                            status = ordered::append(&mut store.leave, entity);
                        } else {
                            status = ordered::append(&mut store.enter, entity);
                        }
                        if read_dseg_page(3) != saved {
                            unsafe {
                                write_dseg_page(3, saved);
                            }
                        }
                        if status != 0 {
                            return error(status);
                        }
                    } else {
                        call.entity = entity;
                        call.leave = leave;
                        let code = action(scene, store, runtime, world, weapons, &call);
                        if code != n(0) {
                            return code;
                        }
                    }
                }
            }
        }
        i += 1;
    }
    restore(store, world, ctx)
}
