//! Ambient class updates: chimney state and source sound-region selection.
use crate::dsl_rt::*;
use crate::{
    attitude, entities, integer, scene_creation, scene_lifecycle, scene_query, scene_world, smoker,
};

pub struct Smoke {
    pub object: u16,
    pub present: bool,
    pub state: smoker::State,
}
pub struct Noisy {
    pub object: u16,
    pub present: bool,
    pub selected: Array<u16>,
    // Position XYZ, range and external sample identity, five i32 per row.
    pub sounds: Array<u16>,
    pub count: u16,
}
pub struct Tick {
    pub call: scene_creation::Call,
    pub dt: fix32,
    pub number: fix32,
    pub camera: Array<u16>,
    pub vehicles: bool,
    pub player_present: bool,
    pub simulation: bool,
    pub smoke: Array<Smoke>,
    pub smoke_count: u16,
    pub noisy: Array<Noisy>,
    pub noisy_count: u16,
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
fn valid(selected: fix32, count: u16) -> bool {
    selected.hi_bits() == 0 && selected.lo_bits() < count
}
fn noisy_tick(record: &Noisy, tick: &Tick) -> fix32 {
    let mut selected = g(record.selected, 0);
    if (tick.call.entity.lo_bits() ^ tick.number.lo_bits()) & 4 == 0 {
        if selected != n(-1) {
            if !valid(selected, record.count) {
                return n(-4);
            }
            let sound = at(record.sounds, selected.lo_bits() * 10);
            if !smoker::near(sound, tick.camera, g(sound, 3)) {
                selected = n(-1);
                p(record.selected, 0, selected);
            }
        } else {
            let mut index = 0u16;
            while index < record.count {
                let sound = at(record.sounds, index * 10);
                if smoker::near(sound, tick.camera, g(sound, 3)) {
                    selected = fix32::from_words(index, 0);
                    p(record.selected, 0, selected);
                    break;
                }
                index += 1;
            }
        }
    }
    if selected >= n(0) && !valid(selected, record.count) {
        return n(-4);
    }
    // Playback is the external audio operation after the committed selection.
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
            let mut index = 0u16;
            while index < tick.noisy_count {
                let record = &tick.noisy[index];
                if record.object == object.lo_bits() && record.present {
                    let status = noisy_tick(record, tick);
                    if status != n(0) {
                        return status;
                    }
                    p(runtime.slot, 0, saved);
                    return n(0);
                }
                index += 1;
            }
            if !tick.vehicles {
                return n(-5);
            }
            if !tick.player_present {
                return n(-4);
            }
            if !tick.simulation {
                return n(-5);
            }
            index = 0;
            while index < tick.smoke_count {
                if tick.smoke[index].object == object.lo_bits() && tick.smoke[index].present {
                    break;
                }
                index += 1;
            }
            if index == tick.smoke_count {
                return n(-1);
            }
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
            smoker::tick(&tick.smoke[index].state, at(raw, 2), tick.camera, tick.dt);
            let mut external = at(raw, 26);
            let mut i = 0u16;
            while i < 18 {
                external[i] = basis[i];
                i += 1;
            }
            p(raw, 29, n(1));
            if !scene_world::store_collider(world, object.lo_bits(), raw) {
                return n(-1);
            }
        }
    }
    p(runtime.slot, 0, saved);
    n(0)
}
