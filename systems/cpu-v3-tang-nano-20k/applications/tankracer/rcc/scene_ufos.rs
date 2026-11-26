//! Shared UFO creation uses live target positions, identities and scene memberships.
use crate::dsl_rt::*;
use crate::{
    attitude, entities, integer, ordered, scene_creation, scene_lifecycle, scene_query,
    scene_world, spatial,
};

pub struct Car {
    pub present: bool,
    pub player: bool,
    pub slot: u16,
    pub disabled: bool,
    pub position: Array<u16>,
    pub basis: Array<u16>,
}
pub struct Cars {
    pub records: Array<Car>,
    pub count: u16,
    pub present: bool,
}
pub struct Spawn {
    pub class: fix32,
    pub resources_present: bool,
    pub mesh: fix32,
    pub material: fix32,
    pub target: fix32,
    pub emitter_present: bool,
    pub emitter: fix32,
    pub destination: Array<u16>,
    pub cars: Cars,
}
pub struct Context {
    pub class: fix32,
    pub resources_present: bool,
    pub mesh: fix32,
    pub material: fix32,
    pub cars: Cars,
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
fn contains(list: &ordered::List, id: fix32) -> bool {
    let mut i = 0u16;
    while i < list.count {
        if g(list.items, i) == id {
            return true;
        }
        i += 1;
    }
    false
}
// Resolve before terrain/resources, as the class Tick does. Only frame views change.
pub fn carrier(
    store: &scene_lifecycle::Store,
    weapons: &scene_creation::Weapons,
    id: u16,
    cars: &Cars,
    frame: &mut crate::projectile_flight::Frame,
) -> fix32 {
    frame.carrier_present = false;
    if id >= weapons.object_capacity || scene_creation::component_kind(weapons, id) != 1 {
        return n(-4);
    }
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(
            3,
            scene_creation::page(
                weapons,
                1,
                weapons.directory[id * weapons.directory_stride + 1],
            ),
        );
    }
    let state = at(scene_creation::record(weapons, id, 3), 30);
    let flags = g(state, 4).lo_bits();
    let attached = g(state, 25) != n(0);
    let entity = g(state, 26);
    unsafe {
        write_dseg_page(3, saved);
    }
    if flags & 4 == 0 || flags & 3 != 0 {
        return n(0);
    }
    if !attached || entities::handle_kind(&store.identities, entity) != 1 {
        return n(-4);
    }
    let ufo = entities::handle(&store.identities, entity);
    if ufo.hi_bits() != 0
        || ufo.lo_bits() >= weapons.object_capacity
        || scene_creation::component_kind(weapons, ufo.lo_bits()) != 4
    {
        return n(-4);
    }
    let object = ufo.lo_bits();
    unsafe {
        write_dseg_page(
            3,
            scene_creation::page(
                weapons,
                4,
                weapons.directory[object * weapons.directory_stride + 1],
            ),
        );
    }
    let state = at(scene_creation::record(weapons, object, 3), 30);
    let target = g(state, 0);
    let released = g(state, 1).lo_bits() & 2 != 0;
    unsafe {
        write_dseg_page(3, saved);
    }
    if entities::handle_kind(&store.identities, target) != 2 {
        return n(-4);
    }
    let vehicle = entities::handle(&store.identities, target);
    if !cars.present
        || vehicle.hi_bits() != 0
        || vehicle.lo_bits() >= cars.count
        || !cars.records[vehicle.lo_bits()].present
    {
        return n(-4);
    }
    let car = &cars.records[vehicle.lo_bits()];
    frame.carrier_present = true;
    frame.carrier_position = car.position;
    frame.carrier_basis = car.basis;
    frame.carrier_released = released;
    n(0)
}
pub fn spawn(
    scene: &scene_query::Scene,
    store: &mut scene_lifecycle::Store,
    runtime: &mut scene_world::Runtime,
    world: &mut scene_world::World,
    weapons: &mut scene_creation::Weapons,
    request: &Spawn,
) -> fix32 {
    let kind = entities::handle_kind(&store.identities, request.target);
    let target = entities::handle(&store.identities, request.target);
    if kind == 0 {
        return n(-4);
    }
    let position: Buf<u16, 6> = Buf::new([0; 6]);
    let point = position.as_array();
    let mut player = false;
    let mut slot = 0u16;
    let mut collidable = false;
    if kind == 1 {
        if target.hi_bits() != 0 || target.lo_bits() >= world.count {
            return n(-4);
        }
        let collider: Buf<u16, 60> = Buf::new([0; 60]);
        if !scene_world::load_collider(world, target.lo_bits(), collider.as_array()) {
            return n(-4);
        }
        let mut i = 0u16;
        while i < 3 {
            p(point, i, g(at(collider.as_array(), 2), i));
            i += 1;
        }
    } else if kind == 2 {
        if !request.cars.present
            || target.hi_bits() != 0
            || target.lo_bits() >= request.cars.count
            || !request.cars.records[target.lo_bits()].present
        {
            return n(-4);
        }
        let car = &request.cars.records[target.lo_bits()];
        let mut i = 0u16;
        while i < 3 {
            p(point, i, g(car.position, i));
            i += 1;
        }
        player = car.player;
        slot = car.slot;
        collidable = !car.disabled;
    } else {
        return n(-5);
    }
    if !request.resources_present || request.class < n(0) {
        return n(-5);
    }
    let entity = scene_creation::create(
        scene,
        store,
        runtime,
        world,
        weapons,
        request.class.lo_bits(),
    );
    if entity < n(0) {
        return entity;
    }
    let object = scene_lifecycle::object(store, entity);
    if object < n(0) {
        return n(-4);
    }
    let id = object.lo_bits();
    if id >= weapons.object_capacity
        || scene_creation::component_kind(weapons, id) != 4
        || id >= store.object_count
        || at(store.records, id * 8)[7u16] & 1 == 0
    {
        return n(-1);
    }
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(
            3,
            scene_creation::page(
                weapons,
                4,
                weapons.directory[id * weapons.directory_stride + 1],
            ),
        );
    }
    let mut row = scene_creation::record(weapons, id, 3);
    row[7u16] = 0;
    if request.emitter_present {
        row[7u16] = 1;
        p(at(row, 8), 0, request.emitter);
    }
    row[2u16] = 1;
    let mut metadata = at(store.records, id * 8);
    metadata[3u16] = metadata[3u16] | 2;
    p(at(row, 3), 0, request.mesh);
    p(at(row, 5), 0, n(22528));
    unsafe {
        write_dseg_page(3, saved);
    }
    let mut special = false;
    if kind == 1 {
        special = contains(&runtime.special, target);
        let incoming = spatial::select(&world.grid);
        let mut cell = 0u16;
        while cell < 64 {
            let list = spatial::list(&world.grid, cell);
            if contains(&list, target) {
                collidable = true;
                break;
            }
            cell += 1;
        }
        unsafe {
            write_dseg_page(3, incoming);
        }
    }
    p(point, 1, g(point, 1) + fix32::from_words(36864, 1));
    unsafe {
        write_dseg_page(
            3,
            scene_creation::page(
                weapons,
                4,
                weapons.directory[id * weapons.directory_stride + 1],
            ),
        );
    }
    let state = at(scene_creation::record(weapons, id, 3), 30);
    let mut raw = state;
    let mut i = 0u16;
    while i < 56 {
        raw[i] = 0;
        i += 1;
    }
    p(state, 0, request.target);
    p(state, 1, n(1));
    i = 0;
    while i < 3 {
        p(state, 2 + i, g(request.destination, i));
        p(state, 5 + i, g(point, i));
        i += 1;
    }
    if special {
        p(state, 15, n(1));
    }
    if collidable {
        p(state, 16, n(1));
    }
    let mut sprite = at(state, 34);
    p(sprite, 0, request.material);
    i = 0;
    while i < 3 {
        p(sprite, 1 + i, g(point, i));
        i += 1;
    }
    p(sprite, 4, n(-1));
    p(sprite, 5, n(-20));
    p(sprite, 6, n(7000));
    p(sprite, 7, n(7000));
    sprite[16u16] = 0;
    sprite[17u16] = 16191;
    sprite[18u16] = 32896;
    sprite[19u16] = 128;
    unsafe {
        write_dseg_page(3, saved);
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
    if ordered::append(&mut runtime.special, object) != 0 {
        return n(-4);
    }
    if player {
        unsafe {
            write_dseg_page(
                3,
                scene_creation::page(
                    weapons,
                    4,
                    weapons.directory[id * weapons.directory_stride + 1],
                ),
            );
        }
        let state = at(scene_creation::record(weapons, id, 3), 30);
        let flags = g(state, 1);
        p(
            state,
            1,
            fix32::from_words(flags.lo_bits() | 32, flags.hi_bits()),
        );
        p(state, 12, fix32::from_words(slot, 0));
        unsafe {
            write_dseg_page(3, saved);
        }
    }
    entity
}
