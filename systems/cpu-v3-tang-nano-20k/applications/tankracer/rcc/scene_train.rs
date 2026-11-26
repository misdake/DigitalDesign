//! Actual Train class Tick, retaining live motion state on a failed pose update.
use crate::dsl_rt::*;
use crate::{
    attitude, entities, integer, scene_creation, scene_lifecycle, scene_query, scene_world, train,
};

pub struct Car {
    pub vehicles: bool,
    pub present: bool,
    pub position: Array<u16>,
}
pub struct Record {
    pub object: u16,
    pub present: bool,
    pub travel: Array<u16>,
    // speed/range then start/end/velocity, 11 i32 columns.
    pub motion: Array<u16>,
    pub sound: bool,
}
pub struct Tick {
    pub call: scene_creation::Call,
    pub dt: fix32,
    pub number: fix32,
    pub simulation: bool,
    pub player: Car,
    pub records: Array<Record>,
    pub count: u16,
    pub sine: Array<i16>,
}
fn n(value: i16) -> fix32 {
    integer::from_i16(value)
}
fn g(values: Array<u16>, index: u16) -> fix32 {
    attitude::get(values, index)
}
fn at(values: Array<u16>, word: u16) -> Array<u16> {
    unsafe { values.as_ptr().add(word as i16).as_u16_array() }
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
    attitude::put(runtime.slot, 0, fix32::from_words(tick.call.slot, 0));
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
            if !tick.player.vehicles {
                return n(-5);
            }
            if !tick.player.present {
                return n(-4);
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
            let record = &tick.records[index];
            let collider: Buf<u16, 60> = Buf::new([0; 60]);
            let raw = collider.as_array();
            if !scene_world::load_collider(world, object.lo_bits(), raw) {
                return n(-1);
            }
            let mut basis = at(raw, 8);
            if g(raw, 29) != n(0) {
                let external = at(raw, 26);
                let mut i = 0u16;
                while i < 18 {
                    basis[i] = external[i];
                    i += 1;
                }
            }
            let status = train::tick(
                &train::State {
                    travel: record.travel,
                    path: at(record.motion, 4),
                    position: at(raw, 2),
                    basis,
                    speed: g(record.motion, 0),
                    range: g(record.motion, 1),
                    sound: record.sound,
                    seed: runtime.seed,
                    sine: tick.sine,
                },
                &train::Frame {
                    player: tick.player.position,
                    entity,
                    number: tick.number,
                    dt: tick.dt,
                },
            );
            if status != n(0) {
                return status;
            }
            let mut external = at(raw, 26);
            let mut i = 0u16;
            while i < 18 {
                external[i] = basis[i];
                i += 1;
            }
            attitude::put(raw, 29, n(1));
            if !scene_world::store_collider(world, object.lo_bits(), raw) {
                return n(-1);
            }
        }
    }
    attitude::put(runtime.slot, 0, saved);
    n(0)
}
