//! Persistent authored pickups in the property page and one shared effects page.
#![link_cseg = "vehicle"]
use super::*;
pub const FLIGHT_WORD: u16 = 8200;
pub const FLIGHT_WORDS: u16 = 1620;
static PICKUP_MATERIALS: Buf<u16, 4> = Buf::new([5, 0, 6, 0]);
pub struct Pickups {
    pub enabled: bool,
    pub bindings: Array<u16>,
    pub flights: Array<u16>,
    pub meta: Array<u16>,
    pub page: u16,
    pub motion_page: u16,
    pub effects_page: u16,
    pub quads: quad_pool::Pool,
    pub lines: Array<effect_pool::Pool>,
    pub sine: Array<i16>,
}
pub struct PickupContact {
    pub vehicle: scenery_contact::Vehicle,
    pub target: u16,
    pub own: u16,
    pub mode: fix32,
    pub number: fix32,
    pub camera: Array<u16>,
}
pub fn pickup_index(store: &Pickups, object: u16) -> u16 {
    if !store.enabled {
        return 65535;
    }
    let mut i = 0u16;
    let mut selected = 65535u16;
    while i < 30 {
        if store.bindings[i * 26] == object {
            selected = i;
            break;
        }
        i += 1;
    }
    selected
}
fn pickup_actor(
    store: &Pickups,
    index: u16,
    components: &scenery_contact::Components,
    world: &scene_world::World,
    state: &scene_parent::State,
) -> scene_parent::Actor {
    let row = at(store.bindings, (index * 26) as i16);
    let mut actor =
        scene_auxiliaries::actor(row, components, world, &state.resources, store.motion_page);
    actor.kind = 3;
    actor.has_state = true;
    actor.state = at(store.flights, (index * 54) as i16);
    actor
}
fn save_pickup(actor: &scene_parent::Actor) {
    attitude::put(at(actor.definition, 8), 0, actor.fragments.private_mesh);
    let mut row = actor.definition;
    row[11u16] = actor.fragments.motion.count;
}
pub fn pickup_contact(
    scene: &scene_query::Scene,
    state: &mut scene_parent::State,
    world: &mut scene_world::World,
    components: &scenery_contact::Components,
    store: &mut Pickups,
    frame: &PickupContact,
) -> fix32 {
    spatial::select(&world.grid);
    let selected = pickup_index(store, frame.target);
    if selected == 65535 {
        return integer::from_i16(-1);
    }
    let mut actor = pickup_actor(store, selected, components, world, state);
    let class = attitude::get(actor.properties, 0);
    if class < fix32::zero() || class >= fix32::from_words(scene.header[4u16], 0) {
        return integer::from_i16(-1);
    }
    let saved = attitude::get(state.runtime.slot, 0);
    attitude::put(state.runtime.slot, 0, integer::from_i16(24));
    let method = scene.method(class.lo_bits(), 24);
    let mut status = fix32::zero();
    if method.status == 4 {
        return integer::from_i16(-1);
    }
    if method.status == 2 {
        return status;
    }
    if method.status == 1 {
        if method.id != 115 {
            return integer::from_i16(-5);
        }
        let descriptor: Buf<u16, 8> = Buf::new([0; 8]);
        let row = actor.definition;
        let model = mesh_catalog::model(&state.models, row[2u16], descriptor.as_array());
        let result: Buf<u16, 2> = Buf::new([0; 2]);
        let nested: Buf<u16, 2> = Buf::new([0; 2]);
        let v = &frame.vehicle;
        let hit = scene_parent::Hit {
            method: 115,
            auxiliary: scene_fragments::Hit {
                row: at(components.records, (actor.fragments.id * 8) as i16),
                limit: components.work_limit,
                powerup: true,
                name: row[3u16],
                model_status: fix32::zero(),
                model: model,
                has_terrain: true,
                vehicle: v.entity,
                velocity: v.velocity,
                paired_tail: 0,
                has_paired_tail: false,
                result: nested.as_array(),
            },
            result: result.as_array(),
            forward: v.forward,
            minimum_turn: components.minimum_turn,
            maximum_turn: components.maximum_turn,
            current: frame.own,
            mode: frame.mode,
            dt: v.dt,
            sine: store.sine,
        };
        let mut collection = scene_pickups::Collection {
            inventory: at(store.meta, (51 + frame.own * 4) as i16),
            amounts: at(store.meta, 40),
            cooldown: at(state.cars[frame.own].suspension, 66),
            lights: pickup_light::State {
                banks: unsafe { Ptr::from_addr(0xc000).as_u16_array() },
            },
            context: pickup_light::Context {
                camera: frame.camera,
                materials: PICKUP_MATERIALS.as_array(),
                tuning: at(store.meta, 4),
                sine: store.sine,
            },
            quads: view_of(&mut store.quads),
            lines: store.lines,
            effects_page: store.effects_page,
            frame: frame.number,
            follow: state.cars[frame.own].player,
        };
        status = scene_pickups::collect(
            scene,
            state,
            &mut world.grid,
            &mut actor,
            &hit,
            &mut collection,
        );
        save_pickup(&actor);
    }
    if status == fix32::zero() {
        attitude::put(state.runtime.slot, 0, saved);
    }
    status
}
pub fn pickup_tick(
    scene: &scene_query::Scene,
    state: &mut scene_parent::State,
    world: &mut scene_world::World,
    components: &scenery_contact::Components,
    store: &Pickups,
    frame: &scenery_contact::Frame,
) -> fix32 {
    spatial::select(&world.grid);
    let selected = pickup_index(store, frame.object);
    if selected == 65535 {
        return integer::from_i16(-1);
    }
    let mut actor = pickup_actor(store, selected, components, world, state);
    let class = attitude::get(actor.properties, 0);
    if class < fix32::zero() || class >= fix32::from_words(scene.header[4u16], 0) {
        return integer::from_i16(-1);
    }
    let saved = attitude::get(state.runtime.slot, 0);
    attitude::put(state.runtime.slot, 0, integer::from_i16(12));
    let method = scene.method(class.lo_bits(), 12);
    let mut status = fix32::zero();
    if method.status == 4 {
        return integer::from_i16(-1);
    }
    if method.status == 2 {
        return status;
    }
    if method.status == 1 {
        if method.id != 114 {
            return integer::from_i16(-5);
        }
        let result: Buf<u16, 2> = Buf::new([0; 2]);
        let row = at(components.records, (frame.object * 8) as i16);
        let entity = attitude::get(actor.fragments.collider, 0);
        let step = scene_pickups::Frame {
            motion: powerup::Frame {
                dt: frame.dt,
                number: frame.number,
                entity: entity,
                player: at(state.cars[state.slots[0u16]].motion, 18),
                camera: frame.camera,
                visibility: actor.definition[24u16],
                auxiliary_active: actor.fragments.active[0u16] != 0,
                separation: attitude::get(store.meta, 0),
                diameter: attitude::get(store.meta, 1),
                sine: store.sine,
                angle: components.tables.angle,
            },
            has_vehicles: true,
            has_player: true,
            result: result.as_array(),
            auxiliary: scene_parent::Frame {
                auxiliary: scene_fragments::Frame {
                    entity: entity,
                    dt: frame.dt,
                    mode: fix32::zero(),
                    sine: store.sine,
                    result: result.as_array(),
                },
                result: result.as_array(),
                group: frame.number,
                player: at(state.cars[state.slots[0u16]].motion, 18),
                has_player: true,
                peer: frame.peer_basis,
                has_peer: frame.peer_valid,
                tuning: at(components.config, row[2u16] as i16),
                angle: components.tables.angle,
                inverse_cosine: unsafe { addr_of(&DRIVING_INVERSE_COSINE).as_i16_array() },
            },
        };
        status = scene_pickups::tick(scene, state, &mut world.grid, &mut actor, &step);
        save_pickup(&actor);
    }
    if status == fix32::zero() {
        attitude::put(state.runtime.slot, 0, saved);
    }
    status
}
pub fn fade_pickups(store: &mut Pickups, car: &driving::Car, number: fix32, dt: fix32) -> fix32 {
    if !store.enabled {
        return fix32::zero();
    }
    let frame = pickup_light::Frame {
        target: car.entity,
        position: at(car.motion, 18),
        number: number,
        dt: dt,
    };
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(3, store.effects_page);
    }
    let state = pickup_light::State {
        banks: unsafe { Ptr::from_addr(0xc000).as_u16_array() },
    };
    let status = pickup_light::fade(
        &state,
        &frame,
        at(store.meta, 4),
        &mut store.quads,
        &store.lines[0u16],
    );
    unsafe {
        write_dseg_page(3, saved);
    }
    status
}
