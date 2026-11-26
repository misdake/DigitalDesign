//! Explosion factory on shared identities, collision rows, ticks and component banks.
use crate::dsl_rt::*;
use crate::{
    attitude, entities, explosion, explosion_blasts, integer, ordered, projectile, scene_creation,
    scene_lifecycle, scene_query, scene_world,
};
pub struct Spawn {
    pub class: fix32,
    pub frame: fix32,
    pub position: Array<u16>,
    pub context: explosion::Context,
}
pub struct Vehicle {
    pub present: bool,
    pub entity: fix32,
    // Live base position and reference basis, independent of physical_position.
    pub position: Array<u16>,
    pub basis: Array<u16>,
}
pub struct Tick {
    pub call: scene_creation::Call,
    pub dt: fix32,
    pub resources: Array<u16>,
    pub sine: Array<i16>,
    pub simulation: bool,
    pub vehicles: Array<Vehicle>,
    pub vehicle_count: u16,
}
pub struct HitVehicle {
    pub present: bool,
    pub entity: fix32,
    pub velocity: Array<u16>,
    pub spin: Array<u16>,
    pub has_body: bool,
    pub radius: i16,
}
pub struct WeaponSpawn {
    pub start: Spawn,
    pub kind: u16,
    pub stamp: fix32,
    pub camera: Array<u16>,
    pub materials: Array<u16>,
    pub has_contact: bool,
    pub vehicle: u16,
    pub local_point: Array<u16>,
    pub world_point: Array<u16>,
    pub velocity: Array<u16>,
    pub vehicles: Array<HitVehicle>,
    pub vehicle_count: u16,
}
pub struct MineSpawn {
    pub start: Spawn,
    pub kind: fix32,
    pub owner: fix32,
    pub velocity: Array<u16>,
    pub stamp: fix32,
    pub camera: Array<u16>,
    pub materials: Array<u16>,
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
fn before(a: fix32, b: fix32) -> bool {
    a.hi_bits() < b.hi_bits() || (a.hi_bits() == b.hi_bits() && a.lo_bits() < b.lo_bits())
}
pub fn clear_blast_owner(
    store: &scene_lifecycle::Store,
    weapons: &scene_creation::Weapons,
    owner: fix32,
    ring: bool,
) -> fix32 {
    if entities::handle_kind(&store.identities, owner) != 1 {
        return n(-4);
    }
    let id = entities::handle(&store.identities, owner);
    if id.hi_bits() != 0
        || id.lo_bits() >= weapons.object_capacity
        || weapons.directory[id.lo_bits() * 2] != 3
    {
        return n(-4);
    }
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(
            3,
            scene_creation::page(weapons, 3, weapons.directory[id.lo_bits() * 2 + 1]),
        );
    }
    let state = at(scene_creation::record(weapons, id.lo_bits(), 3), 30);
    let mut field = 6u16;
    if ring {
        field = 7;
    }
    p(state, field, n(-1));
    unsafe {
        write_dseg_page(3, saved);
    }
    n(0)
}
fn update(
    scene: &scene_query::Scene,
    store: &mut scene_lifecycle::Store,
    runtime: &mut scene_world::Runtime,
    world: &mut scene_world::World,
    weapons: &mut scene_creation::Weapons,
    tick: &Tick,
) -> fix32 {
    let id = scene_lifecycle::object(store, tick.call.entity);
    if id < n(0) {
        return id;
    }
    if store.limit == n(0) || !tick.simulation {
        return n(-1);
    }
    let index = id.lo_bits();
    if index >= weapons.object_capacity || weapons.directory[index * 2] != 3 {
        return n(-1);
    }
    if index >= world.count {
        return n(-1);
    }
    let saved2 = read_dseg_page(2);
    let saved3 = read_dseg_page(3);
    let mut position = at(world.colliders, index * 60 + 2);
    if index >= world.fixed_count {
        let local = index - world.fixed_count;
        if local >= world.dynamic_capacity || local >= scene_world::DYNAMIC_ROWS {
            return n(-1);
        }
        unsafe {
            write_dseg_page(3, world.dynamic_page);
        }
        position = at(
            unsafe { Ptr::from_addr(0xc000).as_u16_array() },
            local * 76 + 2,
        );
    }
    let origin: Buf<u16, 6> = Buf::new([0; 6]);
    let mut words = origin.as_array();
    let mut axis = 0u16;
    while axis < 6 {
        words[axis] = position[axis];
        axis += 1;
    }
    unsafe {
        write_dseg_page(
            3,
            scene_creation::page(weapons, 3, weapons.directory[index * 2 + 1]),
        );
    }
    let state = explosion::view(at(scene_creation::record(weapons, index, 3), 30));
    let mut frame = explosion::Frame {
        dt: tick.dt,
        origin: origin.as_array(),
        has_owner_pose: false,
        owner_basis: origin.as_array(),
        owner_position: origin.as_array(),
    };
    if g(state.core, 8) != n(0) {
        let owner = g(state.core, 9);
        let mut i = 0u16;
        while i < tick.vehicle_count {
            let car = &tick.vehicles[i];
            if car.present && car.entity == owner {
                frame.has_owner_pose = true;
                frame.owner_basis = car.basis;
                frame.owner_position = car.position;
                break;
            }
            i += 1;
        }
        if !frame.has_owner_pose {
            unsafe {
                write_dseg_page(3, saved3);
            }
            return n(-4);
        }
    }
    let ctx = explosion::Context {
        resources: tick.resources,
        seed: runtime.seed,
        sine: tick.sine,
    };
    let retire = explosion::begin_tick(&state, &frame, &ctx);
    let released: Buf<u16, 8> = Buf::new([0; 8]);
    let mut status = n(0);
    if !retire {
        let mut kind = 0u16;
        while kind < 2 && status == n(0) {
            let slot = g(state.core, 6 + kind);
            if slot != n(-1) {
                if !weapons.has_effect_pool {
                    status = n(-5);
                } else {
                    unsafe {
                        write_dseg_page(2, weapons.effect_page);
                    }
                    let step = explosion_blasts::Step { slot, dt: tick.dt };
                    let mut quads = weapons.quads;
                    let lines = weapons.lines;
                    let mut out = explosion_blasts::Outcome {
                        status: n(0),
                        slot: n(-1),
                        has_released: false,
                        owner: n(0),
                        ring: false,
                    };
                    if kind == 0 {
                        out = explosion_blasts::tick_band(
                            &weapons.blasts,
                            &step,
                            &mut quads[0u16],
                            &lines[0u16],
                        );
                    } else {
                        out = explosion_blasts::tick_ring(
                            &weapons.blasts,
                            &step,
                            runtime.seed,
                            &mut quads[0u16],
                            &lines[0u16],
                        );
                    }
                    status = out.status;
                    if status == n(0) && out.has_released {
                        p(state.core, 6 + kind, n(-1));
                        let mut row = at(released.as_array(), kind * 4);
                        row[0u16] = 1;
                        p(at(row, 1), 0, out.owner);
                        row[3u16] = kind;
                    }
                }
            }
            kind += 1;
        }
        if status == n(0) {
            explosion::finish_tick(&state, &frame, &ctx);
        }
    }
    unsafe {
        write_dseg_page(2, saved2);
        write_dseg_page(3, saved3);
    }
    if status != n(0) {
        return status;
    }
    if retire {
        let removed = ordered::remove_first(&mut runtime.special, id, store.limit);
        if removed != 0 {
            if removed == 3 {
                return n(-3);
            }
            if removed == 4 {
                return n(-4);
            }
            return n(-1);
        }
        let status = scene_creation::dispatch(
            scene,
            store,
            runtime,
            world,
            weapons,
            &scene_creation::Call {
                entity: tick.call.entity,
                slot: 8,
            },
        );
        if status != n(0) {
            return status;
        }
        let status = scene_creation::release_explosion(weapons, world, index);
        if status != n(0) {
            return status;
        }
    }
    let mut kind = 0u16;
    while kind < 2 {
        let row = at(released.as_array(), kind * 4);
        if row[0u16] != 0 {
            let status = clear_blast_owner(store, weapons, g(at(row, 1), 0), row[3u16] != 0);
            if status != n(0) {
                return status;
            }
        }
        kind += 1;
    }
    n(0)
}
pub fn dispatch(
    scene: &scene_query::Scene,
    store: &mut scene_lifecycle::Store,
    runtime: &mut scene_world::Runtime,
    world: &mut scene_world::World,
    weapons: &mut scene_creation::Weapons,
    tick: &Tick,
) -> fix32 {
    let class = entities::class(&store.identities, tick.call.entity);
    if class < n(0) || class >= fix32::from_words(scene.header[4u16], 0) {
        return n(-1);
    }
    let saved = g(runtime.slot, 0);
    p(runtime.slot, 0, fix32::from_words(tick.call.slot, 0));
    let method = scene.method(class.lo_bits(), tick.call.slot);
    if method.status == 4 {
        return n(-1);
    }
    if method.status == 2 {
        return n(0);
    }
    let mut status = n(0);
    if method.status == 1 {
        if method.id == 93 {
            status = update(scene, store, runtime, world, weapons, tick);
        } else {
            status = scene_creation::dispatch(scene, store, runtime, world, weapons, &tick.call);
        }
    }
    if status != n(0) {
        return status;
    }
    p(runtime.slot, 0, saved);
    n(0)
}
pub fn spawn(
    scene: &scene_query::Scene,
    store: &mut scene_lifecycle::Store,
    runtime: &mut scene_world::Runtime,
    world: &mut scene_world::World,
    weapons: &mut scene_creation::Weapons,
    start: &Spawn,
) -> fix32 {
    if store.limit == n(0) {
        return n(-1);
    }
    if start.class < n(0) {
        return n(-5);
    }
    let entity =
        scene_creation::create(scene, store, runtime, world, weapons, start.class.lo_bits());
    if entity < n(0) {
        return entity;
    }
    // Source next(seed)>>4 retains 28 bits; random_word exposes only its low 16.
    projectile::random_word(start.context.seed);
    let seed = g(start.context.seed, 0);
    let first = fix32::from_words(
        (seed.lo_bits() >> 4) | (seed.hi_bits() << 12),
        seed.hi_bits() >> 4,
    );
    if entities::handle_kind(&store.identities, entity) != 1 {
        return n(-4);
    }
    let id = entities::handle(&store.identities, entity);
    if id.hi_bits() != 0 || id.lo_bits() >= world.count {
        return n(-4);
    }
    let index = id.lo_bits();
    let status = scene_creation::dispatch(
        scene,
        store,
        runtime,
        world,
        weapons,
        &scene_creation::Call { entity, slot: 4 },
    );
    if status != n(0) {
        return status;
    }
    if ordered::append(&mut runtime.special, id) != 0 {
        return n(-4);
    }
    let collider: Buf<u16, 60> = Buf::new([0; 60]);
    if !scene_world::load_collider(world, index, collider.as_array()) {
        return n(-1);
    }
    let mut i = 0u16;
    while i < 3 {
        p(collider.as_array(), 1 + i, g(start.position, i));
        i += 1;
    }
    if !scene_world::store_collider(world, index, collider.as_array()) {
        return n(-1);
    }
    if index >= weapons.object_capacity || weapons.directory[index * 2] != 3 {
        return n(-4);
    }
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(
            3,
            scene_creation::page(weapons, 3, weapons.directory[index * 2 + 1]),
        );
    }
    let state = explosion::view(at(scene_creation::record(weapons, index, 3), 30));
    explosion::initialize(&state, start.position, first, &start.context);
    unsafe {
        write_dseg_page(3, saved);
    }
    let data = weapons.explosion_world;
    let mut selected = 15u16;
    let replacing = g(data, 0) >= n(15);
    if g(data, 0) < n(15) {
        p(data, 0, g(data, 0) + n(1));
        i = 0;
        while i < 15 {
            if g(at(data, 10 + i * 6), 0) == n(0) {
                selected = i;
                break;
            }
            i += 1;
        }
    } else {
        selected = 0;
        i = 1;
        while i < 15 {
            if before(
                g(at(data, 10 + i * 6), 2),
                g(at(data, 10 + selected * 6), 2),
            ) {
                selected = i;
            }
            i += 1;
        }
        let old = at(data, 10 + selected * 6);
        if g(old, 0) == n(0) {
            return n(-1);
        }
        let old_entity = g(old, 1);
        if entities::handle_kind(&store.identities, old_entity) != 1 {
            return n(-1);
        }
        let old_id = entities::handle(&store.identities, old_entity);
        let removed = ordered::remove_first(&mut runtime.special, old_id, store.limit);
        if removed != 0 {
            if removed == 3 {
                return n(-3);
            }
            if removed == 4 {
                return n(-4);
            }
            return n(-1);
        }
        let status = scene_creation::dispatch(
            scene,
            store,
            runtime,
            world,
            weapons,
            &scene_creation::Call {
                entity: old_entity,
                slot: 8,
            },
        );
        if status != n(0) {
            return status;
        }
    }
    if selected < 15 {
        let slot = at(data, 10 + selected * 6);
        p(slot, 0, n(1));
        p(slot, 1, entity);
        p(slot, 2, start.frame);
    }
    unsafe {
        write_dseg_page(
            3,
            scene_creation::page(weapons, 3, weapons.directory[index * 2 + 1]),
        );
    }
    let state = explosion::view(at(scene_creation::record(weapons, index, 3), 30));
    if selected < 15 {
        p(state.core, 0, fix32::from_words(selected, 0));
    }
    if replacing {
        p(data, 1, n(1));
        p(data, 2, g(start.context.resources, 4));
    }
    explosion::finish_spawn(&state);
    unsafe {
        write_dseg_page(3, saved);
    }
    entity
}
pub fn spawn_weapon(
    scene: &scene_query::Scene,
    store: &mut scene_lifecycle::Store,
    runtime: &mut scene_world::Runtime,
    world: &mut scene_world::World,
    weapons: &mut scene_creation::Weapons,
    request: &WeaponSpawn,
) -> fix32 {
    let entity = spawn(scene, store, runtime, world, weapons, &request.start);
    if entity < n(0) {
        return entity;
    }
    if entities::handle_kind(&store.identities, entity) != 1 {
        return n(-4);
    }
    let id = entities::handle(&store.identities, entity);
    if id.hi_bits() != 0
        || id.lo_bits() >= weapons.object_capacity
        || weapons.directory[id.lo_bits() * 2] != 3
    {
        return n(-4);
    }
    let index = id.lo_bits();
    if request.kind == 1 || request.kind == 4 || request.kind == 5 {
        if !weapons.has_effect_pool {
            return n(-5);
        }
        let ctx = explosion_blasts::Context {
            resources: request.materials,
            camera: request.camera,
            sine: request.start.context.sine,
            seed: request.start.context.seed,
        };
        let mut outer = n(15360);
        if request.kind == 1 {
            outer = n(20480);
        }
        let start = explosion_blasts::Start {
            origin: request.start.position,
            owner: entity,
            time: request.stamp,
            outer,
            flipped: request.kind == 1,
            kind: 2,
        };
        let saved = read_dseg_page(2);
        unsafe {
            write_dseg_page(2, weapons.effect_page);
        }
        let mut quads = weapons.quads;
        let lines = weapons.lines;
        let mut out = explosion_blasts::Outcome {
            status: n(0),
            slot: n(-1),
            has_released: false,
            owner: n(0),
            ring: false,
        };
        if request.kind == 5 {
            out = explosion_blasts::ring(
                &weapons.blasts,
                &start,
                &ctx,
                &mut quads[0u16],
                &lines[0u16],
            );
        } else {
            out = explosion_blasts::band(
                &weapons.blasts,
                &start,
                &ctx,
                &mut quads[0u16],
                &lines[0u16],
            );
        }
        unsafe {
            write_dseg_page(2, saved);
        }
        if out.status != n(0) {
            return out.status;
        }
        if out.has_released {
            let status = clear_blast_owner(store, weapons, out.owner, out.ring);
            if status != n(0) {
                return status;
            }
        }
        let saved = read_dseg_page(3);
        unsafe {
            write_dseg_page(
                3,
                scene_creation::page(weapons, 3, weapons.directory[index * 2 + 1]),
            );
        }
        let state = explosion::view(at(scene_creation::record(weapons, index, 3), 30));
        if request.kind == 5 {
            p(state.core, 7, out.slot);
            p(state.core, 3, n(0));
        } else {
            p(state.core, 6, out.slot);
        }
        unsafe {
            write_dseg_page(3, saved);
        }
    }
    if request.has_contact {
        if request.vehicle >= request.vehicle_count {
            return n(-4);
        }
        let car = &request.vehicles[request.vehicle];
        if !car.present {
            return n(-4);
        }
        let contact = explosion::Contact {
            owner: car.entity,
            vehicle_velocity: car.velocity,
            local_point: request.local_point,
            world_point: request.world_point,
            velocity: request.velocity,
            kind: request.kind,
            spin: g(car.spin, 0),
            has_body: car.has_body,
            radius: car.radius,
        };
        let saved = read_dseg_page(3);
        unsafe {
            write_dseg_page(
                3,
                scene_creation::page(weapons, 3, weapons.directory[index * 2 + 1]),
            );
        }
        let state = explosion::view(at(scene_creation::record(weapons, index, 3), 30));
        let status = explosion::contact(&state, &contact, &request.start.context);
        unsafe {
            write_dseg_page(3, saved);
        }
        if status != n(0) {
            return status;
        }
    }
    if request.kind == 1 {
        p(weapons.explosion_world, 3, n(50));
        p(weapons.explosion_world, 4, n(1));
        p(weapons.explosion_world, 1, n(0));
    }
    entity
}
pub fn spawn_mine(
    scene: &scene_query::Scene,
    store: &mut scene_lifecycle::Store,
    runtime: &mut scene_world::Runtime,
    world: &mut scene_world::World,
    weapons: &mut scene_creation::Weapons,
    request: &MineSpawn,
) -> fix32 {
    let entity = spawn(scene, store, runtime, world, weapons, &request.start);
    if entity < n(0) {
        return entity;
    }
    if entities::handle_kind(&store.identities, entity) != 1 {
        return n(-4);
    }
    let id = entities::handle(&store.identities, entity);
    if id.hi_bits() != 0
        || id.lo_bits() >= weapons.object_capacity
        || weapons.directory[id.lo_bits() * 2] != 3
    {
        return n(-4);
    }
    let index = id.lo_bits();
    let saved3 = read_dseg_page(3);
    unsafe {
        write_dseg_page(
            3,
            scene_creation::page(weapons, 3, weapons.directory[index * 2 + 1]),
        );
    }
    let state = explosion::view(at(scene_creation::record(weapons, index, 3), 30));
    p(state.core, 8, n(1));
    p(state.core, 9, request.owner);
    // Source repeats the X store; the zero mine normal leaves attachment at zero.
    p(state.core, 10, integer::sar(g(request.velocity, 2), 6));
    unsafe {
        write_dseg_page(3, saved3);
    }
    if !weapons.has_effect_pool {
        return n(-5);
    }
    let mut kind = 1u16;
    if request.kind == n(1) {
        kind = 3;
    }
    let start = explosion_blasts::Start {
        origin: request.start.position,
        owner: entity,
        time: request.stamp,
        outer: n(0),
        flipped: false,
        kind,
    };
    let ctx = explosion_blasts::Context {
        resources: request.materials,
        camera: request.camera,
        sine: request.start.context.sine,
        seed: request.start.context.seed,
    };
    let saved2 = read_dseg_page(2);
    unsafe {
        write_dseg_page(2, weapons.effect_page);
    }
    let mut quads = weapons.quads;
    let lines = weapons.lines;
    let out = explosion_blasts::ring(
        &weapons.blasts,
        &start,
        &ctx,
        &mut quads[0u16],
        &lines[0u16],
    );
    unsafe {
        write_dseg_page(2, saved2);
    }
    if out.status != n(0) {
        return out.status;
    }
    if out.has_released {
        let status = clear_blast_owner(store, weapons, out.owner, out.ring);
        if status != n(0) {
            return status;
        }
    }
    unsafe {
        write_dseg_page(
            3,
            scene_creation::page(weapons, 3, weapons.directory[index * 2 + 1]),
        );
    }
    let state = explosion::view(at(scene_creation::record(weapons, index, 3), 30));
    p(state.core, 7, out.slot);
    p(state.core, 3, n(0));
    unsafe {
        write_dseg_page(3, saved3);
    }
    entity
}
