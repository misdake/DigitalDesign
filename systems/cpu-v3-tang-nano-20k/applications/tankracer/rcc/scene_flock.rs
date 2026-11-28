//! Flock class Tick on authoritative birds, model vertices and resource revisions.
use crate::dsl_rt::*;
use crate::{
    attitude, entities, flock, integer, ordered, projectile, scene_creation, scene_lifecycle,
    scene_query, scene_world,
};

pub struct Geometry {
    pub exists: bool,
    // Bit zero is the live geometry-presence flag in its resource owner.
    pub flags: Array<u16>,
    pub revision: Array<u16>,
    pub vertices: Array<i16>,
    pub page: u16,
}
pub struct Record {
    pub object: u16,
    pub present: bool,
    pub state: flock::State,
    pub meshes: Array<Geometry>,
    pub mesh_count: u16,
}
pub struct Tick {
    pub call: scene_creation::Call,
    pub dt: fix32,
    pub number: fix32,
    pub simulation: bool,
    pub vehicles: bool,
    pub player_present: bool,
    pub player: Array<u16>,
    pub records: Array<Record>,
    pub count: u16,
    pub tables: projectile::Tables,
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
fn touch(mut version: Array<u16>) {
    let mut i = 0u16;
    while i < 4 {
        version[i] += 1;
        if version[i] != 0 {
            return;
        }
        i += 1;
    }
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
            if !tick.vehicles {
                return n(-5);
            }
            if !tick.player_present {
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
            let status = flock::tick(
                &record.state,
                &flock::Frame {
                    origin: at(raw, 2),
                    player: tick.player,
                    entity,
                    number: tick.number,
                    dt: tick.dt,
                    seed: runtime.seed,
                    tables: tick.tables,
                },
            );
            if status < n(0) {
                return status;
            }
            if g(record.state.flight, 0) == n(1) {
                let mut i = 0u16;
                while i < record.mesh_count && i < record.state.model_count {
                    let mesh = &record.meshes[i];
                    if !mesh.exists {
                        return n(-1);
                    }
                    touch(mesh.revision);
                    if mesh.flags[0u16] & 1 == 0 {
                        return n(-1);
                    }
                    let model = &record.state.models[i];
                    let previous = flock::select(model);
                    let low = model.vertices[model.low * 3 + 1];
                    let high = model.vertices[model.high * 3 + 1];
                    if mesh.page != 65535 && mesh.page != read_dseg_page(3) {
                        unsafe {
                            write_dseg_page(3, mesh.page);
                        }
                    } else if mesh.page == 65535 {
                        unsafe {
                            write_dseg_page(3, previous);
                        }
                    }
                    let mut vertices = mesh.vertices;
                    vertices[model.low * 3 + 1] = low;
                    vertices[model.high * 3 + 1] = high;
                    unsafe {
                        write_dseg_page(3, previous);
                    }
                    i += 1;
                }
            }
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
            if status == n(1) {
                let removed = ordered::remove_first(&mut runtime.special, object, store.limit);
                if removed != 0 {
                    if removed == 3 {
                        return n(-3);
                    }
                    return n(-1);
                }
                let stopped = scene_creation::dispatch(
                    scene,
                    store,
                    runtime,
                    world,
                    weapons,
                    &scene_creation::Call { entity, slot: 5 },
                );
                if stopped != n(0) {
                    return stopped;
                }
            }
        }
    }
    p(runtime.slot, 0, saved);
    n(0)
}
