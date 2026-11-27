//! Class construction into shared identities, scene rows and banked weapon state.
use crate::dsl_rt::*;
use crate::{
    attitude, effect_pool, entities, explosion_blasts, integer, ordered, projectile, quad_pool,
    scene_lifecycle, scene_query, scene_world, spatial,
};

pub struct Weapons {
    // Kind (0 absent, 1 projectile, 2 mine, 3 explosion, 4 UFO), then pool slot.
    // Stride 2 owns a dense directory. Stride 8 borrows component columns 1/2;
    // column 0 must be zero for these weapon rows and keeps scenery separate.
    pub directory: Array<u16>,
    pub directory_stride: u16,
    pub object_capacity: u16,
    pub projectile_page: u16,
    pub projectile_pages: u16,
    pub mine_page: u16,
    pub mine_pages: u16,
    pub explosion_page: u16,
    pub explosion_pages: u16,
    pub ufo_page: u16,
    pub ufo_pages: u16,
    // Five i32 world fields, then fifteen presence/entity/frame slot triples.
    pub explosion_world: Array<u16>,
    pub blasts: explosion_blasts::State,
    // Common effect records occupy one stage bank in DSEG2; descriptors live in CPU memory.
    pub effect_page: u16,
    pub quads: Array<quad_pool::Pool>,
    pub lines: Array<effect_pool::Pool>,
    pub has_effect_pool: bool,
    pub world_class: u16,
    pub mine_order: ordered::List,
    pub mine_count: Array<u16>,
}
pub struct Call {
    pub entity: fix32,
    pub slot: u16,
}
pub struct Launch {
    pub owner: projectile::Owner,
    pub context: projectile::Context,
    pub kind: u16,
    pub class: fix32,
    pub resources_present: bool,
    pub homing_mesh: fix32,
}
fn n(value: i16) -> fix32 {
    integer::from_i16(value)
}
fn at(words: Array<u16>, offset: u16) -> Array<u16> {
    unsafe { words.as_ptr().add(offset as i16).as_u16_array() }
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
fn leave(
    store: &scene_lifecycle::Store,
    runtime: &mut scene_world::Runtime,
    world: &mut scene_world::World,
    entity: fix32,
) -> fix32 {
    if store.limit == n(0) {
        return n(-1);
    }
    let id = scene_lifecycle::object(store, entity);
    if id < n(0) {
        return id;
    }
    let status = ordered::remove_first(&mut runtime.ordinary, id, store.limit);
    if status != 0 {
        return error(status);
    }
    let row = at(store.records, id.lo_bits() * 8);
    let cell = spatial::Cell {
        x: row[4u16] as i16,
        z: row[5u16],
        present: row[6u16] != 0,
    };
    let status = spatial::remove(&mut world.grid, id, &cell, store.limit);
    if status != 0 {
        return error(status);
    }
    n(0)
}
fn enter(
    store: &scene_lifecycle::Store,
    runtime: &mut scene_world::Runtime,
    world: &mut scene_world::World,
    entity: fix32,
) -> fix32 {
    if store.limit == n(0) {
        return n(-1);
    }
    let id = scene_lifecycle::object(store, entity);
    if id < n(0) {
        return id;
    }
    let status = ordered::append(&mut runtime.ordinary, id);
    if status != 0 {
        return error(status);
    }
    let collider: Buf<u16, 60> = Buf::new([0; 60]);
    if !scene_world::load_collider(world, id.lo_bits(), collider.as_array()) {
        return n(-1);
    }
    let mut row = at(store.records, id.lo_bits() * 8);
    // Weapon factories supply handles validated by the resource catalog.
    let inserted = spatial::insert(
        &mut world.grid,
        id,
        at(collider.as_array(), 2),
        row[3u16] & 2 != 0,
        collider[56u16],
    );
    if inserted.status != 0 {
        return error(inserted.status);
    }
    row[4u16] = inserted.cell.x as u16;
    row[5u16] = inserted.cell.z;
    row[6u16] = 1;
    n(0)
}
// Each record starts with 30 words of scene data. Word 0 is object ID + 1,
// word 1 is signed sort position; words 2..4 are optional mesh presence/ID.
// Remaining words retain visibility/emitter data for the factory stages.
// Payloads are 610 projectile words, eighteen mine, 676 explosion or 56 UFO words.
pub fn stride(kind: u16) -> u16 {
    if kind == 1 {
        return 640;
    }
    if kind == 3 {
        return 706;
    }
    if kind == 4 {
        return 86;
    }
    48
}
pub fn slots(kind: u16) -> u16 {
    if kind == 1 {
        return 25;
    }
    if kind == 3 {
        return 23;
    }
    if kind == 4 {
        return 190;
    }
    341
}
pub fn page(weapons: &Weapons, kind: u16, slot: u16) -> u16 {
    let mut first = weapons.mine_page;
    if kind == 1 {
        first = weapons.projectile_page;
    }
    if kind == 3 {
        first = weapons.explosion_page;
    }
    if kind == 4 {
        first = weapons.ufo_page;
    }
    first + slot / slots(kind)
}
pub fn component_kind(weapons: &Weapons, id: u16) -> u16 {
    if weapons.directory_stride == 8 {
        let components = unsafe { weapons.directory.as_ptr().add(-1).as_u16_array() };
        if components[id * 8] != 0 {
            return 0;
        }
    }
    weapons.directory[id * weapons.directory_stride]
}
pub fn record(weapons: &Weapons, id: u16, segment: u16) -> Array<u16> {
    let kind = component_kind(weapons, id);
    let slot = weapons.directory[id * weapons.directory_stride + 1];
    let mut base = 0xc000u16;
    if segment == 2 {
        base = 0x8000;
    }
    at(
        unsafe { Ptr::from_addr(base).as_u16_array() },
        (slot % slots(kind)) * stride(kind),
    )
}
fn insert(weapons: &Weapons, id: u16, kind: u16) -> fix32 {
    if id >= weapons.object_capacity || component_kind(weapons, id) != 0 {
        return n(-1);
    }
    let mut pages = weapons.mine_pages;
    if kind == 1 {
        pages = weapons.projectile_pages;
    }
    if kind == 3 {
        pages = weapons.explosion_pages;
    }
    if kind == 4 {
        pages = weapons.ufo_pages;
    }
    let saved = read_dseg_page(3);
    let count = slots(kind);
    let size = stride(kind);
    let mut bank = 0u16;
    while bank < pages {
        unsafe {
            write_dseg_page(3, page(weapons, kind, bank * count));
        }
        let raw = unsafe { Ptr::from_addr(0xc000).as_u16_array() };
        let mut slot = 0u16;
        while slot < count {
            let mut row = at(raw, slot * size);
            if row[0u16] == 0 {
                let mut i = 0u16;
                while i < size {
                    row[i] = 0;
                    i += 1;
                }
                row[0u16] = id + 1;
                if kind == 3 {
                    attitude::put(at(row, 30), 6, n(-1));
                    attitude::put(at(row, 30), 7, n(-1));
                }
                let mut directory = weapons.directory;
                directory[id * weapons.directory_stride] = kind;
                directory[id * weapons.directory_stride + 1] = bank * count + slot;
                unsafe {
                    write_dseg_page(3, saved);
                }
                return n(0);
            }
            slot += 1;
        }
        bank += 1;
    }
    unsafe {
        write_dseg_page(3, saved);
    }
    n(-4)
}
// The caller selects the bank in DSEG2 or DSEG3 for the entire weapon stage.
// Fixed colliders stay in DSEG2; appended colliders occupy DSEG3 instead.
pub fn retire(weapons: &Weapons, id: u16) {
    if id >= weapons.object_capacity || component_kind(weapons, id) == 0 {
        return;
    }
    let kind = component_kind(weapons, id);
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(
            3,
            page(
                weapons,
                kind,
                weapons.directory[id * weapons.directory_stride + 1],
            ),
        );
    }
    let mut row = record(weapons, id, 3);
    row[0u16] = 0;
    let mut directory = weapons.directory;
    directory[id * weapons.directory_stride] = 0;
    unsafe {
        write_dseg_page(3, saved);
    }
}
// Resources retire before identity detach/release and before component reuse.
pub fn release_explosion(weapons: &mut Weapons, world: &scene_world::World, id: u16) -> fix32 {
    if id >= weapons.object_capacity || component_kind(weapons, id) != 3 {
        return n(0);
    }
    let collider: Buf<u16, 60> = Buf::new([0; 60]);
    if !scene_world::load_collider(world, id, collider.as_array()) {
        return n(-1);
    }
    let entity = attitude::get(collider.as_array(), 0);
    let saved2 = read_dseg_page(2);
    let saved3 = read_dseg_page(3);
    unsafe {
        write_dseg_page(
            3,
            page(
                weapons,
                3,
                weapons.directory[id * weapons.directory_stride + 1],
            ),
        );
    }
    let state = at(record(weapons, id, 3), 30);
    let mut kind = 0u16;
    let mut status = n(0);
    while kind < 2 && status == n(0) {
        let slot = attitude::get(state, 6 + kind);
        if slot != n(-1) {
            if !weapons.has_effect_pool {
                status = n(-5);
            } else {
                let component = read_dseg_page(3);
                unsafe {
                    write_dseg_page(2, weapons.effect_page);
                    write_dseg_page(3, weapons.effect_page);
                }
                let mut quads = weapons.quads;
                let lines = weapons.lines;
                let released = explosion_blasts::release(
                    &weapons.blasts,
                    kind == 1,
                    slot,
                    &mut quads[0u16],
                    &lines[0u16],
                );
                unsafe {
                    write_dseg_page(3, component);
                }
                status = released.status;
                if status == n(0) {
                    attitude::put(state, 6 + kind, n(-1));
                }
            }
        }
        kind += 1;
    }
    unsafe {
        write_dseg_page(2, saved2);
        write_dseg_page(3, saved3);
    }
    if status != n(0) {
        return status;
    }
    let mut slot = 0u16;
    while slot < 15 {
        let row = at(weapons.explosion_world, 10 + slot * 6);
        if attitude::get(row, 0) != n(0) && attitude::get(row, 1) == entity {
            attitude::put(row, 0, n(0));
            attitude::put(row, 1, n(0));
            attitude::put(
                weapons.explosion_world,
                0,
                attitude::get(weapons.explosion_world, 0) - n(1),
            );
            break;
        }
        slot += 1;
    }
    n(0)
}
fn initialize(
    scene: &scene_query::Scene,
    store: &mut scene_lifecycle::Store,
    world: &mut scene_world::World,
    weapons: &Weapons,
    call: &Call,
    class: u16,
) -> fix32 {
    let method = scene.method(class, call.slot);
    if method.status == 4 {
        return n(-1);
    }
    if method.status != 1 {
        return n(0);
    }
    let id = scene_lifecycle::object(store, call.entity);
    if id < n(0) {
        return id;
    }
    if id.lo_bits() >= world.count {
        return n(-1);
    }
    let index = id.lo_bits();
    if method.id == 40 {
        let collider: Buf<u16, 60> = Buf::new([0; 60]);
        if !scene_world::load_collider(world, index, collider.as_array()) {
            return n(-1);
        }
        let mut raw = collider.as_array();
        let mut i = 0u16;
        while i < 9 {
            let mut value = n(0);
            if i == 0 || i == 4 || i == 8 {
                value = n(4096);
            }
            attitude::put(at(raw, 8), i, value);
            attitude::put(at(raw, 26), i, value);
            i += 1;
        }
        raw[58u16] = 1;
        raw[59u16] = 0;
        if !scene_world::store_collider(world, index, raw) {
            return n(-1);
        }
        return n(0);
    }
    if method.id == 41 || method.id == 42 {
        if method.id == 42 && store.limit == n(0) {
            return n(-1);
        }
        let mut parent = weapons.world_class;
        if method.id == 42 {
            parent = store.generic_class;
        }
        let status = initialize(scene, store, world, weapons, call, parent);
        if status != n(0) {
            return status;
        }
        if method.id == 42 {
            let mut row = at(store.records, index * 8);
            row[6u16] = 0;
        } else {
            if index >= weapons.object_capacity || component_kind(weapons, index) == 0 {
                return n(-1);
            }
            let kind = component_kind(weapons, index);
            let saved = read_dseg_page(3);
            unsafe {
                write_dseg_page(
                    3,
                    page(
                        weapons,
                        kind,
                        weapons.directory[index * weapons.directory_stride + 1],
                    ),
                );
            }
            let mut row = record(weapons, index, 3);
            row[1u16] = 65528;
            unsafe {
                write_dseg_page(3, saved);
            }
        }
        return n(0);
    }
    if method.id == 32 || method.id == 33 || method.id == 34 {
        return n(0);
    }
    n(-5)
}
pub fn dispatch(
    scene: &scene_query::Scene,
    store: &mut scene_lifecycle::Store,
    runtime: &mut scene_world::Runtime,
    world: &mut scene_world::World,
    weapons: &mut Weapons,
    call: &Call,
) -> fix32 {
    let entity = call.entity;
    let slot = call.slot;
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
    let mut status = n(0);
    if method.status == 1 {
        if method.id == 116 || method.id == 119 || method.id == 92 || method.id == 124 {
            let allocated = scene_lifecycle::allocate_transient(store, world, runtime, entity);
            if allocated.status != 0 {
                status = n(-1);
                if allocated.status == 4 {
                    status = n(-4);
                }
            } else {
                let mut kind = 1u16;
                if method.id == 119 {
                    kind = 2;
                }
                if method.id == 92 {
                    kind = 3;
                }
                if method.id == 124 {
                    kind = 4;
                }
                status = insert(weapons, allocated.id.lo_bits(), kind);
            }
        } else if method.id == 40 || method.id == 41 || method.id == 42 {
            status = initialize(scene, store, world, weapons, call, class.lo_bits());
        } else if method.id == 43 {
            status = enter(store, runtime, world, entity);
        } else if method.id == 44 {
            status = leave(store, runtime, world, entity);
        } else if method.id == 121 || method.id == 123 {
            let id = scene_lifecycle::object(store, entity);
            if id < n(0) {
                return id;
            }
            let parent = scene.method(store.obstacle_class, slot);
            if parent.status == 4 {
                return n(-1);
            }
            if parent.status == 1 {
                if parent.id == 45 {
                    status =
                        scene_lifecycle::deactivate(scene, store, runtime, &mut world.grid, entity);
                } else if parent.id == 44 {
                    status = leave(store, runtime, world, entity);
                } else {
                    status = scene_lifecycle::parent_base(
                        scene,
                        store,
                        runtime,
                        entity,
                        store.obstacle_class,
                    );
                }
            }
            if status == n(0) && method.id == 121 {
                let id = entities::handle(&store.identities, entity);
                let removed = ordered::remove_first(&mut weapons.mine_order, id, store.limit);
                if removed != 0 {
                    return error(removed);
                }
                attitude::put(
                    weapons.mine_count,
                    0,
                    attitude::get(weapons.mine_count, 0) - n(1),
                );
            }
        } else if method.id == 118 || method.id == 126 {
            let id = scene_lifecycle::object(store, entity);
            if id < n(0) {
                return id;
            }
            status =
                scene_lifecycle::parent_base(scene, store, runtime, entity, store.generic_class);
            if status == n(0) && method.id == 126 {
                let removed = ordered::remove_first(&mut runtime.special, id, store.limit);
                if removed != 0 {
                    return error(removed);
                }
            }
        } else {
            let id = entities::handle(&store.identities, entity);
            if (method.id == 37 || method.id == 38)
                && entities::handle_kind(&store.identities, entity) == 1
                && id.hi_bits() == 0
            {
                let status = release_explosion(weapons, world, id.lo_bits());
                if status != n(0) {
                    return status;
                }
            }
            status =
                scene_lifecycle::dispatch(scene, store, runtime, &mut world.grid, entity, slot);
            if status == n(0) && (method.id == 37 || method.id == 38) && id.hi_bits() == 0 {
                retire(weapons, id.lo_bits());
                if id.lo_bits() < store.object_count {
                    let mut metadata = at(store.records, id.lo_bits() * 8);
                    metadata[3u16] = 0;
                }
            }
        }
    }
    if status != n(0) {
        return status;
    }
    attitude::put(runtime.slot, 0, saved);
    n(0)
}
pub fn create(
    scene: &scene_query::Scene,
    store: &mut scene_lifecycle::Store,
    runtime: &mut scene_world::Runtime,
    world: &mut scene_world::World,
    weapons: &mut Weapons,
    class: u16,
) -> fix32 {
    let created = entities::create_record(&store.identities, store.limit);
    if created.status != 0 {
        return n(-1);
    }
    if entities::bind_class(&store.identities, created.id, class) != 0 {
        return n(-1);
    }
    let mut status = dispatch(
        scene,
        store,
        runtime,
        world,
        weapons,
        &Call {
            entity: created.id,
            slot: 0,
        },
    );
    if status == n(0) {
        status = dispatch(
            scene,
            store,
            runtime,
            world,
            weapons,
            &Call {
                entity: created.id,
                slot: 1,
            },
        );
    }
    if status != n(0) {
        return status;
    }
    created.id
}
pub fn spawn_projectile(
    scene: &scene_query::Scene,
    store: &mut scene_lifecycle::Store,
    runtime: &mut scene_world::Runtime,
    world: &mut scene_world::World,
    weapons: &mut Weapons,
    launch: &Launch,
) -> fix32 {
    if launch.class < n(0) || !launch.resources_present {
        return n(-5);
    }
    // The collider and component occupy both mapped windows during launch.
    // Capture only the owner's read-only launch inputs before borrowing them.
    let owner_words: Buf<u16, 30> = Buf::new([0; 30]);
    let mut owner_data = owner_words.as_array();
    let mut i = 0u16;
    while i < 18 {
        owner_data[i] = launch.owner.turret[i];
        i += 1;
    }
    i = 0;
    while i < 6 {
        owner_data[18 + i] = launch.owner.gun[i];
        owner_data[24 + i] = launch.owner.velocity[i];
        i += 1;
    }
    let owner = projectile::Owner {
        entity: launch.owner.entity,
        turret: owner_data,
        gun: at(owner_data, 18),
        velocity: at(owner_data, 24),
    };
    // Homing launch produces one shared scalar. Publish it after restoring the
    // caller's shared-effect mapping, before activation can invoke other methods.
    let drift: Buf<u16, 4> = Buf::new([0; 4]);
    let context = projectile::Context {
        shared: projectile::Shared {
            core: drift.as_array(),
            homing: launch.context.shared.homing,
            concussion: launch.context.shared.concussion,
        },
        resources: projectile::Resources {
            tuning: launch.context.resources.tuning,
            main: launch.context.resources.main,
            trail: launch.context.resources.trail,
            homing_tail: launch.context.resources.homing_tail,
            cup: launch.context.resources.cup,
        },
        tables: projectile::Tables {
            sine: launch.context.tables.sine,
            angle: launch.context.tables.angle,
        },
        seed: launch.context.seed,
    };
    let entity = create(
        scene,
        store,
        runtime,
        world,
        weapons,
        launch.class.lo_bits(),
    );
    if entity < n(0) {
        return entity;
    }
    if entities::handle_kind(&store.identities, entity) != 1 {
        return n(-4);
    }
    let id = entities::handle(&store.identities, entity);
    if id.hi_bits() != 0 || id.lo_bits() >= world.count {
        return n(-4);
    }
    let index = id.lo_bits();
    if index >= weapons.object_capacity || component_kind(weapons, index) != 1 {
        return n(-1);
    }
    let saved2 = read_dseg_page(2);
    let saved3 = read_dseg_page(3);
    let mut segment = 3u16;
    let mut collider = at(world.colliders, index * 60);
    if index >= world.fixed_count {
        segment = 2;
        unsafe {
            write_dseg_page(3, world.dynamic_page);
        }
        collider = at(
            unsafe { Ptr::from_addr(0xc000).as_u16_array() },
            (index - world.fixed_count) * 76,
        );
    }
    let component_page = page(
        weapons,
        1,
        weapons.directory[index * weapons.directory_stride + 1],
    );
    unsafe {
        if segment == 2 {
            write_dseg_page(2, component_page);
        } else {
            write_dseg_page(3, component_page);
        }
    }
    let row = record(weapons, index, segment);
    let state = projectile::view(at(row, 30));
    let pose = projectile::Pose {
        basis: at(collider, 8),
        position: at(collider, 2),
    };
    let status = projectile::launch(&state, &pose, &owner, launch.kind, &context);
    if status == n(0) {
        let mut i = 0u16;
        let mut raw = collider;
        while i < 18 {
            raw[26 + i] = pose.basis[i];
            i += 1;
        }
        raw[58u16] = 1;
        raw[59u16] = 0;
    }
    unsafe {
        write_dseg_page(2, saved2);
        write_dseg_page(3, saved3);
    }
    if status != n(0) {
        return status;
    }
    if launch.kind == 2 {
        attitude::put(
            launch.context.shared.core,
            1,
            attitude::get(drift.as_array(), 1),
        );
    }
    let status = dispatch(
        scene,
        store,
        runtime,
        world,
        weapons,
        &Call {
            entity: entity,
            slot: 4,
        },
    );
    if status != n(0) {
        return status;
    }
    let status = ordered::remove_first(&mut runtime.ordinary, id, store.limit);
    if status != 0 {
        if status == 2 {
            return n(-1);
        }
        if status == 3 {
            return n(-3);
        }
        return n(-4);
    }
    if ordered::append(&mut runtime.special, id) != 0 {
        return n(-4);
    }
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(
            3,
            page(
                weapons,
                1,
                weapons.directory[index * weapons.directory_stride + 1],
            ),
        );
    }
    let mut row = record(weapons, index, 3);
    projectile::sprite(&projectile::view(at(row, 30)), &launch.context);
    if launch.kind == 2 {
        row[2u16] = 1;
        row[3u16] = launch.homing_mesh.lo_bits();
        row[4u16] = launch.homing_mesh.hi_bits();
        let mut metadata = at(store.records, index * 8);
        metadata[3u16] = metadata[3u16] | 2;
        unsafe {
            write_dseg_page(3, saved);
        }
        attitude::put(launch.context.shared.core, 3, n(1));
        attitude::put(launch.context.shared.homing, 6, n(150));
        attitude::put(launch.context.shared.homing, 7, n(150));
    }
    unsafe {
        write_dseg_page(3, saved);
    }
    entity
}
