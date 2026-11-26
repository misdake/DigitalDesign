//! First-map emitter Tick and ownerless Debris publication through real factories.
use crate::dsl_rt::*;
use crate::{
    attitude, emitter, entities, integer, ordered, particle_sprite, projectile, scene_creation,
    scene_lifecycle, scene_query, scene_world,
};

pub struct Record {
    pub object: u16,
    pub present: bool,
    pub data: Array<u16>,
}
pub struct Tick {
    pub call: scene_creation::Call,
    pub dt: fix32,
    pub simulation: bool,
    pub camera: Array<u16>,
    pub player: Array<u16>,
    pub player_present: bool,
    pub records: Array<Record>,
    pub count: u16,
    pub resources_present: bool,
    pub material: fix32,
    pub class: fix32,
    pub sine: Array<i16>,
}
pub struct Emission {
    pub emitter: fix32,
    pub mesh_present: bool,
    pub mesh: fix32,
    pub size: fix32,
    pub flags: fix32,
    pub glow: Buf<u16, 6>,
    pub color: Buf<u16, 3>,
    pub launch: Array<u16>,
    pub material: fix32,
    pub class: fix32,
}
fn n(value: i16) -> fix32 {
    integer::from_i16(value)
}
fn g(values: Array<u16>, index: u16) -> fix32 {
    attitude::get(values, index)
}
fn p(values: Array<u16>, index: u16, value: fix32) {
    attitude::put(values, index, value);
}
fn at(values: Array<u16>, word: u16) -> Array<u16> {
    unsafe { values.as_ptr().add(word as i16).as_u16_array() }
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

fn spawn(
    scene: &scene_query::Scene,
    store: &mut scene_lifecycle::Store,
    runtime: &mut scene_world::Runtime,
    world: &mut scene_world::World,
    weapons: &mut scene_creation::Weapons,
    emission: &Emission,
) -> fix32 {
    if emission.class < n(0) {
        return n(-5);
    }
    let entity = scene_creation::create(
        scene,
        store,
        runtime,
        world,
        weapons,
        emission.class.lo_bits(),
    );
    if entity < n(0) {
        return entity;
    }
    if entities::handle_kind(&store.identities, entity) != 1 {
        return n(-4);
    }
    let object = entities::handle(&store.identities, entity);
    if object.hi_bits() != 0 || object.lo_bits() >= store.object_count {
        return n(-4);
    }
    let id = object.lo_bits();
    let mut metadata = at(store.records, id * 8);
    if metadata[7u16] & 1 == 0 {
        return n(-1);
    }
    // The content loader binds the Projectile class and its banked component.
    if id >= weapons.object_capacity || scene_creation::component_kind(weapons, id) != 1 {
        return n(-4);
    }
    let bank = scene_creation::page(
        weapons,
        1,
        weapons.directory[id * weapons.directory_stride + 1],
    );
    let previous = read_dseg_page(3);
    unsafe {
        write_dseg_page(3, bank);
    }
    let mut row = scene_creation::record(weapons, id, 3);
    row[7u16] = 1;
    row[8u16] = emission.emitter.lo_bits();
    row[9u16] = emission.emitter.hi_bits();
    row[2u16] = 0;
    if emission.mesh_present {
        row[2u16] = 1;
    }
    row[3u16] = emission.mesh.lo_bits();
    row[4u16] = emission.mesh.hi_bits();
    row[1u16] = 65535;
    metadata[3u16] = metadata[3u16] & 65533;
    if row[2u16] != 0 {
        metadata[3u16] = metadata[3u16] | 2;
    }
    unsafe {
        write_dseg_page(3, previous);
    }
    let collider: Buf<u16, 60> = Buf::new([0; 60]);
    let raw = collider.as_array();
    if !scene_world::load_collider(world, id, raw) {
        return n(-1);
    }
    let mut i = 0u16;
    let mut position = at(raw, 2);
    let mut basis = at(raw, 8);
    let mut external = at(raw, 26);
    while i < 6 {
        position[i] = emission.launch[i];
        i += 1;
    }
    i = 0;
    let source = at(emission.launch, 6);
    while i < 18 {
        basis[i] = source[i];
        external[i] = source[i];
        i += 1;
    }
    p(raw, 29, n(1));
    if !scene_world::store_collider(world, id, raw) {
        return n(-1);
    }
    let previous = read_dseg_page(3);
    unsafe {
        write_dseg_page(3, bank);
    }
    let state = projectile::view(at(scene_creation::record(weapons, id, 3), 30));
    let mut all = state.core;
    i = 0;
    while i < 610 {
        all[i] = 0;
        i += 1;
    }
    p(state.core, 2, n(4));
    p(state.core, 4, g(emission.launch, 15));
    i = 0;
    while i < 3 {
        p(state.core, 5 + i, g(emission.launch, 12 + i));
        p(state.core, 27 + i, g(emission.glow.as_array(), i));
        i += 1;
    }
    p(state.sprite, 0, emission.material);
    p(state.sprite, 4, emission.flags);
    p(state.sprite, 6, emission.size);
    p(state.sprite, 7, emission.size);
    particle_sprite::rectangle(state.sprite, 0, 0, 63, 63);
    particle_sprite::rgb(
        state.sprite,
        emission.color[0u16],
        emission.color[1u16],
        emission.color[2u16],
    );
    unsafe {
        write_dseg_page(3, previous);
    }
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
    let removed = ordered::remove_first(&mut runtime.ordinary, object, store.limit);
    if removed != 0 {
        return error(removed);
    }
    if ordered::append(&mut runtime.special, object) != 0 {
        return n(-4);
    }
    entity
}
pub fn dispatch(
    scene: &scene_query::Scene,
    store: &mut scene_lifecycle::Store,
    runtime: &mut scene_world::Runtime,
    world: &mut scene_world::World,
    weapons: &mut scene_creation::Weapons,
    tick: &Tick,
) -> fix32 {
    let entity = tick.call.entity;
    let class = entities::class(&store.identities, entity);
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
    if method.status == 1 {
        if method.id != 109 {
            let status =
                scene_creation::dispatch(scene, store, runtime, world, weapons, &tick.call);
            if status != n(0) {
                return status;
            }
        } else {
            let object = scene_lifecycle::object(store, entity);
            if object < n(0) {
                return object;
            }
            if !tick.simulation {
                return n(-5);
            }
            let mut index = 0u16;
            while index < tick.count {
                if tick.records[index].object == object.lo_bits() {
                    break;
                }
                index += 1;
            }
            if index == tick.count || !tick.records[index].present {
                return n(-1);
            }
            let data = tick.records[index].data;
            let collider: Buf<u16, 60> = Buf::new([0; 60]);
            let raw = collider.as_array();
            if !scene_world::load_collider(world, object.lo_bits(), raw) {
                return n(-1);
            }
            let mut basis = at(raw, 8);
            if g(raw, 29) != n(0) {
                basis = at(raw, 26);
            }
            let launch: Buf<u16, 32> = Buf::new([0; 32]);
            if emitter::tick(
                data,
                &emitter::Frame {
                    camera: tick.camera,
                    player: tick.player,
                    player_present: tick.player_present,
                    position: at(raw, 2),
                    basis,
                    dt: tick.dt,
                    seed: runtime.seed,
                    sine: tick.sine,
                },
                launch.as_array(),
            ) {
                if !tick.resources_present {
                    return n(-5);
                }
                let mut material = tick.material;
                if g(data, 23) != n(0) {
                    material = g(data, 24);
                }
                let alpha = g(data, 16).lo_bits() & 255;
                let mut flags = fix32::from_words(alpha, 0);
                if alpha >= 128 {
                    flags = flags - n(256);
                }
                // Publish a compact immutable prototype before factories borrow DSEG3.
                let mut emission = Emission {
                    emitter: entity,
                    mesh_present: g(data, 21) != n(0),
                    mesh: g(data, 22),
                    size: g(data, 2),
                    flags,
                    glow: Buf::new([0; 6]),
                    color: Buf::new([0; 3]),
                    launch: launch.as_array(),
                    material,
                    class: tick.class,
                };
                let mut i = 0u16;
                while i < 3 {
                    p(emission.glow.as_array(), i, g(data, 17 + i));
                    emission.color[i] = g(data, 13 + i).lo_bits();
                    i += 1;
                }
                let emitted = spawn(scene, store, runtime, world, weapons, &emission);
                if emitted < n(0) {
                    return emitted;
                }
            }
        }
    }
    p(runtime.slot, 0, saved);
    n(0)
}
