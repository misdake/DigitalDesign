//! Mine placement on the shared scene, terrain, component and lifetime owners.
use crate::dsl_rt::*;
use crate::{
    attitude, entities, explosion, ground, integer, mine, ordered, projectile, scene_creation,
    scene_explosions, scene_lifecycle, scene_query, scene_world,
};
pub struct Placement {
    pub owner: fix32,
    pub position: Array<u16>,
    pub repeat: bool,
    pub kind: fix32,
    pub frame: fix32,
    pub class: fix32,
    pub resources_present: bool,
    pub terrain_present: bool,
    // Two mesh i32 handles, two u16 radii, then two six-i16 bounds.
    pub resources: Array<u16>,
    // A live view of the caller's existing terrain owner, never a second copy.
    pub map: Array<ground::World>,
    pub tables: projectile::Tables,
}
pub struct Vehicle {
    pub present: bool,
    // Live physical XYZ, corresponding to driving::Car.motion i32 slots 3..6.
    pub position: Array<u16>,
}
pub struct Tick {
    pub call: scene_creation::Call,
    pub dt: fix32,
    pub placement: Array<Placement>,
    pub vehicles: Array<Vehicle>,
    pub vehicle_count: u16,
    pub has_vehicles: bool,
}
pub struct HitVehicle {
    pub entity: fix32,
    pub player: bool,
    pub protection: Array<u16>,
    pub physics: mine::Vehicle,
    // The same eight-i32 surface effect used by ground sampling.
    pub effect: Array<u16>,
}
pub struct HitPeer {
    pub present: bool,
    pub entity: fix32,
    pub effect: Array<u16>,
}
pub struct Contact {
    pub target: u16,
    pub frame: fix32,
    pub vehicle: HitVehicle,
    // Live tails of the existing shield banks, in bank order.
    pub shield_tails: Array<u16>,
    pub shield_stride: u16,
    pub peers: Array<HitPeer>,
    pub peer_count: u16,
    // Optional camera impulse: presence, unsigned duration, signed magnitude.
    pub camera_impulse: Array<u16>,
    pub class: fix32,
    pub stamp: fix32,
    pub resources: Array<u16>,
    pub sine: Array<i16>,
    pub camera: Array<u16>,
    pub materials: Array<u16>,
}
fn n(v: i16) -> fix32 {
    integer::from_i16(v)
}
fn at(data: Array<u16>, words: u16) -> Array<u16> {
    unsafe { data.as_ptr().add(words as i16).as_u16_array() }
}
fn g(data: Array<u16>, i: u16) -> fix32 {
    attitude::get(data, i)
}
fn p(data: Array<u16>, i: u16, v: fix32) {
    attitude::put(data, i, v)
}
// Return a pose view while the selected collider page remains mapped.
fn collider(world: &scene_world::World, index: u16) -> Array<u16> {
    if index < world.fixed_count {
        return at(world.colliders, index * 60);
    }
    unsafe {
        write_dseg_page(3, world.dynamic_page);
    }
    at(
        unsafe { Ptr::from_addr(0xc000).as_u16_array() },
        (index - world.fixed_count) * 76,
    )
}
pub fn spawn(
    scene: &scene_query::Scene,
    store: &mut scene_lifecycle::Store,
    runtime: &mut scene_world::Runtime,
    world: &mut scene_world::World,
    weapons: &mut scene_creation::Weapons,
    placement: &Placement,
) -> fix32 {
    if !placement.resources_present {
        return n(-5);
    }
    if placement.kind.hi_bits() != 0 || placement.kind.lo_bits() >= 2 {
        return n(-4);
    }
    if placement.class < n(0) {
        return n(-5);
    }
    let entity = scene_creation::create(
        scene,
        store,
        runtime,
        world,
        weapons,
        placement.class.lo_bits(),
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
    let kind = placement.kind.lo_bits();
    if !placement.terrain_present {
        return n(-1);
    }
    let mut maps = placement.map;
    let height = maps[0u16].height(g(placement.position, 0), g(placement.position, 2));
    if height.status > 1 {
        return n(-2);
    }
    let saved = read_dseg_page(3);
    let raw = collider(world, index);
    p(raw, 1, g(placement.position, 0));
    p(raw, 2, height.height);
    p(raw, 3, g(placement.position, 2));
    let mut i = 0u16;
    while i < 9 {
        let mut value = n(0);
        if i == 0 || i == 4 || i == 8 {
            value = n(4096);
        }
        p(at(raw, 8), i, value);
        p(at(raw, 26), i, value);
        i += 1;
    }
    p(raw, 29, n(1));
    unsafe {
        write_dseg_page(3, saved);
    }
    if index >= weapons.object_capacity || scene_creation::component_kind(weapons, index) != 2 {
        return n(-4);
    }
    unsafe {
        write_dseg_page(
            3,
            scene_creation::page(
                weapons,
                2,
                weapons.directory[index * weapons.directory_stride + 1],
            ),
        );
    }
    let row = scene_creation::record(weapons, index, 3);
    p(at(row, 5), 0, fix32::from_words(0x9000, 0));
    let state = at(row, 30);
    i = 0;
    let mut state_words = state;
    while i < 18 {
        state_words[i] = 0;
        i += 1;
    }
    p(state, 0, n(1));
    p(state, 1, placement.owner);
    p(state, 2, placement.kind);
    let mut repeat = n(0);
    if placement.repeat {
        repeat = n(1);
    }
    p(state, 3, repeat);
    p(state, 4, repeat);
    p(state, 6, placement.frame);
    p(state, 7, placement.frame);
    unsafe {
        write_dseg_page(3, saved);
    }
    let status = scene_creation::dispatch(
        scene,
        store,
        runtime,
        world,
        weapons,
        &scene_creation::Call {
            entity: entity,
            slot: 6,
        },
    );
    if status != n(0) {
        return status;
    }
    unsafe {
        write_dseg_page(
            3,
            scene_creation::page(
                weapons,
                2,
                weapons.directory[index * weapons.directory_stride + 1],
            ),
        );
    }
    let mut record = scene_creation::record(weapons, index, 3);
    record[2u16] = 1;
    record[3u16] = g(placement.resources, kind).lo_bits();
    record[4u16] = g(placement.resources, kind).hi_bits();
    unsafe {
        write_dseg_page(3, saved);
    }
    let mut metadata = at(store.records, index * 8);
    metadata[3u16] = metadata[3u16] | 2;
    let raw = collider(world, index);
    p(raw, 28, fix32::from_words(placement.resources[4 + kind], 0));
    i = 0;
    while i < 6 {
        let value = integer::from_i16(placement.resources[6 + kind * 6 + i] as i16);
        let mut margin = n(-1536);
        if i >= 3 {
            margin = n(1536);
        }
        p(at(raw, 44), i, fmul_lo32(value, n(8)) + margin);
        i += 1;
    }
    let pose = projectile::Pose {
        basis: at(raw, 8),
        position: at(raw, 2),
    };
    let status = mine::align(&pose, &mut maps[0u16], &placement.tables);
    if status == n(0) {
        let mut data = raw;
        i = 0;
        while i < 18 {
            data[26 + i] = pose.basis[i];
            i += 1;
        }
    }
    unsafe {
        write_dseg_page(3, saved);
    }
    if status != n(0) {
        return status;
    }
    let status = scene_creation::dispatch(
        scene,
        store,
        runtime,
        world,
        weapons,
        &scene_creation::Call {
            entity: entity,
            slot: 4,
        },
    );
    if status != n(0) {
        return status;
    }
    if ordered::append(&mut weapons.mine_order, id) != 0 {
        return n(-4);
    }
    let count = g(weapons.mine_count, 0) + n(1);
    p(weapons.mine_count, 0, count);
    // Count and stamp fields preserve unsigned source words.
    if count.hi_bits() != 0 || count.lo_bits() > 30 {
        if weapons.mine_order.count == 0 {
            return n(-4);
        }
        let oldest = g(weapons.mine_order.items, 0);
        if oldest.hi_bits() != 0 || oldest.lo_bits() >= world.count {
            return n(-4);
        }
        let saved = read_dseg_page(3);
        let raw = collider(world, oldest.lo_bits());
        let oldest_entity = g(raw, 0);
        unsafe {
            write_dseg_page(3, saved);
        }
        let status = scene_creation::dispatch(
            scene,
            store,
            runtime,
            world,
            weapons,
            &scene_creation::Call {
                entity: oldest_entity,
                slot: 8,
            },
        );
        if status != n(0) {
            return status;
        }
    }
    entity
}
fn update(
    scene: &scene_query::Scene,
    store: &mut scene_lifecycle::Store,
    runtime: &mut scene_world::Runtime,
    world: &mut scene_world::World,
    weapons: &mut scene_creation::Weapons,
    frame: &Tick,
) -> fix32 {
    let id = scene_lifecycle::object(store, frame.call.entity);
    if id < n(0) {
        return id;
    }
    let index = id.lo_bits();
    if index >= weapons.object_capacity || scene_creation::component_kind(weapons, index) != 2 {
        return n(-4);
    }
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(
            3,
            scene_creation::page(
                weapons,
                2,
                weapons.directory[index * weapons.directory_stride + 1],
            ),
        );
    }
    let state = mine::State {
        core: at(scene_creation::record(weapons, index, 3), 30),
    };
    let action = mine::tick(&state, frame.dt);
    let owner_present = g(state.core, 0) != n(0);
    let owner = g(state.core, 1);
    let kind = g(state.core, 2);
    unsafe {
        write_dseg_page(3, saved);
    }
    // The source requires an owner even when the lifetime action is None.
    if !owner_present {
        return n(-4);
    }
    if action == 2 {
        return scene_creation::dispatch(
            scene,
            store,
            runtime,
            world,
            weapons,
            &scene_creation::Call {
                entity: frame.call.entity,
                slot: 8,
            },
        );
    }
    if action == 1 {
        if entities::handle_kind(&store.identities, owner) != 2 {
            return n(-4);
        }
        let vehicle = entities::handle(&store.identities, owner);
        if !frame.has_vehicles || vehicle.hi_bits() != 0 || vehicle.lo_bits() >= frame.vehicle_count
        {
            return n(-4);
        }
        let car = &frame.vehicles[vehicle.lo_bits()];
        if !car.present {
            return n(-4);
        }
        let configured = &frame.placement[0u16];
        let placement = Placement {
            owner: owner,
            position: car.position,
            repeat: false,
            kind: kind,
            frame: configured.frame,
            class: configured.class,
            resources_present: configured.resources_present,
            terrain_present: configured.terrain_present,
            resources: configured.resources,
            map: configured.map,
            tables: projectile::Tables {
                sine: configured.tables.sine,
                angle: configured.tables.angle,
            },
        };
        let child = spawn(scene, store, runtime, world, weapons, &placement);
        if child < n(0) {
            return child;
        }
        // The source fire notification is an external audio command. It does
        // not read/write physics or RNG and belongs to the host I/O boundary.
    }
    n(0)
}
pub fn dispatch(
    scene: &scene_query::Scene,
    store: &mut scene_lifecycle::Store,
    runtime: &mut scene_world::Runtime,
    world: &mut scene_world::World,
    weapons: &mut scene_creation::Weapons,
    frame: &Tick,
) -> fix32 {
    let class = entities::class(&store.identities, frame.call.entity);
    if class < n(0) || class >= fix32::from_words(scene.header[4u16], 0) {
        return n(-1);
    }
    let saved = g(runtime.slot, 0);
    p(runtime.slot, 0, fix32::from_words(frame.call.slot, 0));
    let method = scene.method(class.lo_bits(), frame.call.slot);
    if method.status == 4 {
        return n(-1);
    }
    if method.status == 2 {
        return n(0);
    }
    let mut status = n(0);
    if method.status == 1 {
        if method.id == 120 {
            status = update(scene, store, runtime, world, weapons, frame);
        } else {
            status = scene_creation::dispatch(scene, store, runtime, world, weapons, &frame.call);
        }
    }
    if status != n(0) {
        return status;
    }
    p(runtime.slot, 0, saved);
    n(0)
}
// Method 122 receives an already selected obstacle and current live vehicle.
pub fn contact(
    scene: &scene_query::Scene,
    store: &mut scene_lifecycle::Store,
    runtime: &mut scene_world::Runtime,
    world: &mut scene_world::World,
    weapons: &mut scene_creation::Weapons,
    frame: &Contact,
) -> fix32 {
    let index = frame.target;
    if index >= weapons.object_capacity || scene_creation::component_kind(weapons, index) != 2 {
        return n(-4);
    }
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(
            3,
            scene_creation::page(
                weapons,
                2,
                weapons.directory[index * weapons.directory_stride + 1],
            ),
        );
    }
    let state = mine::State {
        core: at(scene_creation::record(weapons, index, 3), 30),
    };
    let armed = mine::contact(&state, frame.frame);
    let kind = g(state.core, 2);
    unsafe {
        write_dseg_page(3, saved);
    }
    if !armed {
        return n(0);
    }
    if index >= world.count {
        return n(-1);
    }
    let position: Buf<u16, 6> = Buf::new([0; 6]);
    let raw = collider(world, index);
    let entity = g(raw, 0);
    let mut point = position.as_array();
    let mut i = 0u16;
    while i < 6 {
        point[i] = raw[2 + i];
        i += 1;
    }
    unsafe {
        write_dseg_page(3, saved);
    }
    let request = scene_explosions::MineSpawn {
        start: scene_explosions::Spawn {
            class: frame.class,
            frame: frame.frame,
            position: position.as_array(),
            context: explosion::Context {
                resources: frame.resources,
                seed: runtime.seed,
                sine: frame.sine,
            },
        },
        kind,
        owner: frame.vehicle.entity,
        velocity: frame.vehicle.physics.velocity,
        stamp: frame.stamp,
        camera: frame.camera,
        materials: frame.materials,
    };
    let status = scene_explosions::spawn_mine(scene, store, runtime, world, weapons, &request);
    if status < n(0) {
        return status;
    }
    let car = &frame.vehicle;
    if g(car.protection, 0) < n(1) {
        let status = mine::point_hit(&car.physics, position.as_array());
        if status != n(0) {
            return status;
        }
        if car.player {
            p(frame.camera_impulse, 0, n(1));
            p(frame.camera_impulse, 1, n(600));
            p(frame.camera_impulse, 2, n(1500));
        }
        if kind == n(1) {
            p(car.effect, 0, n(6000));
            p(
                car.effect,
                1,
                integer::from_i16(g(car.effect, 6).lo_bits() as i16),
            );
            p(car.effect, 2, n(1));
            let mut bank = 0u16;
            if car.player {
                bank = 1;
            }
            let previous = g(frame.shield_tails, bank * frame.shield_stride);
            if bank == 0 && previous != n(0) {
                if previous == car.entity {
                    p(car.effect, 2, n(0));
                } else {
                    i = 0;
                    while i < frame.peer_count {
                        let peer = &frame.peers[i];
                        if peer.present && peer.entity == previous {
                            p(peer.effect, 2, n(0));
                            break;
                        }
                        i += 1;
                    }
                }
            }
            p(frame.shield_tails, bank * frame.shield_stride, car.entity);
        }
    }
    scene_creation::dispatch(
        scene,
        store,
        runtime,
        world,
        weapons,
        &scene_creation::Call { entity, slot: 8 },
    )
}
