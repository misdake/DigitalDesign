//! Complete projectile ticks with shared explosions and live non-AI vehicle effects.
use crate::dsl_rt::*;
use crate::{
    attitude, blast, entities, explosion, integer, projectile_flight, projectile_marks,
    scene_creation, scene_explosions, scene_lifecycle, scene_projectiles, scene_query, scene_ufos,
    scene_world, tank_hit, temporary,
};
pub struct Car {
    pub present: bool,
    pub entity: fix32,
    pub player: bool,
    pub visible: bool,
    pub base_position: Array<u16>,
    pub physical_position: Array<u16>,
    pub velocity: Array<u16>,
    pub angular: Array<u16>,
    pub blast: Array<u16>,
    pub marker: Array<u16>,
}
pub struct Impact {
    pub object: u16,
    pub action: Array<projectile_flight::Action>,
    pub resources_present: bool,
    pub terrain_present: bool,
    pub simulation: bool,
    pub crater: fix32,
    pub map: Array<crate::ground::World>,
    pub marks: projectile_marks::State,
    pub class: fix32,
    pub resources: Array<u16>,
    pub camera: Array<u16>,
    pub materials: Array<u16>,
    pub frame: fix32,
    pub stamp: fix32,
    pub sine: Array<i16>,
    pub hits: Array<scene_explosions::HitVehicle>,
    pub cars: Array<Car>,
    pub vehicle_count: u16,
    pub has_vehicles: bool,
    pub slots: Array<u16>,
    pub camera_impulse: Array<u16>,
}
pub struct Tick {
    pub call: scene_creation::Call,
    pub flight: scene_projectiles::Frame,
    pub impact: Impact,
    pub damage: Damage,
    pub ufos: scene_ufos::Context,
}
pub struct HitVehicle {
    pub present: bool,
    pub car: tank_hit::Vehicle,
    pub geometry_page: u16,
}
pub struct Damage {
    pub cars: Array<HitVehicle>,
    pub count: u16,
    pub shell: Array<u16>,
    pub homer: Array<u16>,
    pub cup: u16,
    pub temporary: temporary::State,
    pub present: bool,
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
fn abs(v: fix32) -> fix32 {
    if v < n(0) {
        return -v;
    }
    v
}
fn component(weapons: &scene_creation::Weapons, id: u16) -> bool {
    id < weapons.object_capacity && weapons.directory[id * 2] == 1
}
// The caller has resolved UFO attachment and shield reflection before this phase.
pub fn begin(
    scene: &scene_query::Scene,
    store: &mut scene_lifecycle::Store,
    runtime: &mut scene_world::Runtime,
    world: &mut scene_world::World,
    weapons: &mut scene_creation::Weapons,
    frame: &Impact,
) -> fix32 {
    if !frame.resources_present {
        return n(-5);
    }
    let action = &frame.action[0u16];
    if action.has_mark {
        if !frame.terrain_present {
            return n(-1);
        }
        if !weapons.has_effect_pool {
            return n(-5);
        }
        let saved = read_dseg_page(2);
        unsafe {
            write_dseg_page(2, weapons.effect_page);
        }
        let mut quads = weapons.quads;
        let lines = weapons.lines;
        let mut maps = frame.map;
        let mark = projectile_marks::Mark {
            origin: action.mark.as_array(),
            material: frame.crater,
        };
        let status = projectile_marks::add(
            &frame.marks,
            &mark,
            &mut maps[0u16],
            &mut quads[0u16],
            &lines[0u16],
        );
        unsafe {
            write_dseg_page(2, saved);
        }
        if status != n(0) {
            return status;
        }
    }
    if !frame.simulation {
        return n(-5);
    }
    let id = frame.object;
    if !component(weapons, id) {
        return n(-4);
    }
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(
            3,
            scene_creation::page(weapons, 1, weapons.directory[id * 2 + 1]),
        );
    }
    let row = at(scene_creation::record(weapons, id, 3), 30);
    let kind = g(row, 2).lo_bits();
    let velocity: Buf<u16, 6> = Buf::new([0; 6]);
    let mut v = velocity.as_array();
    let mut i = 0u16;
    while i < 6 {
        v[i] = row[10 + i];
        i += 1;
    }
    unsafe {
        write_dseg_page(3, saved);
    }
    let request = scene_explosions::WeaponSpawn {
        start: scene_explosions::Spawn {
            class: frame.class,
            frame: frame.frame,
            position: action.origin.as_array(),
            context: explosion::Context {
                resources: frame.resources,
                seed: runtime.seed,
                sine: frame.sine,
            },
        },
        kind,
        stamp: frame.stamp,
        camera: frame.camera,
        materials: frame.materials,
        has_contact: action.has_contact,
        vehicle: action.contact.vehicle.lo_bits(),
        local_point: action.contact.local.point.as_array(),
        world_point: action.contact.world.point.as_array(),
        velocity: velocity.as_array(),
        vehicles: frame.hits,
        vehicle_count: frame.vehicle_count,
    };
    let entity = scene_explosions::spawn_weapon(scene, store, runtime, world, weapons, &request);
    if entity < n(0) {
        return entity;
    }
    if id >= store.object_count || at(store.records, id * 8)[7u16] & 1 == 0 {
        return n(-1);
    }
    unsafe {
        write_dseg_page(
            3,
            scene_creation::page(weapons, 1, weapons.directory[id * 2 + 1]),
        );
    }
    let row = scene_creation::record(weapons, id, 3);
    let has_emitter = row[7u16] != 0;
    let emitter = g(at(row, 8), 0);
    unsafe {
        write_dseg_page(3, saved);
    }
    if has_emitter {
        if entities::handle_kind(&store.identities, entity) != 1 {
            return n(-4);
        }
        let child = entities::handle(&store.identities, entity);
        if child.hi_bits() != 0 || child.lo_bits() >= store.object_count {
            return n(-4);
        }
        let index = child.lo_bits();
        if at(store.records, index * 8)[7u16] & 1 == 0 {
            return n(-1);
        }
        unsafe {
            write_dseg_page(
                3,
                scene_creation::page(weapons, 3, weapons.directory[index * 2 + 1]),
            );
        }
        let mut target = scene_creation::record(weapons, index, 3);
        target[7u16] = 1;
        p(at(target, 8), 0, emitter);
        unsafe {
            write_dseg_page(3, saved);
        }
    }
    if !component(weapons, id) {
        return n(-4);
    }
    unsafe {
        write_dseg_page(
            3,
            scene_creation::page(weapons, 1, weapons.directory[id * 2 + 1]),
        );
    }
    let state = at(scene_creation::record(weapons, id, 3), 30);
    let flags = g(state, 4);
    p(
        state,
        4,
        fix32::from_words(flags.lo_bits() | 1, flags.hi_bits()),
    );
    unsafe {
        write_dseg_page(3, saved);
    }
    // The non-AI notification still validates every scheduled peer before flash.
    if frame.has_vehicles {
        i = 2;
        while i < 9 {
            let car = frame.slots[i];
            if car != 65535 && (car >= frame.vehicle_count || !frame.cars[car].present) {
                return n(-1);
            }
            i += 1;
        }
    }
    if kind == 1 || kind == 4 {
        let primary = frame.slots[0u16];
        if !frame.has_vehicles
            || primary == 65535
            || primary >= frame.vehicle_count
            || !frame.cars[primary].present
        {
            return n(-4);
        }
        let base = frame.cars[primary].base_position;
        let distance = attitude::length2(
            g(base, 0) - g(action.origin.as_array(), 0),
            g(base, 2) - g(action.origin.as_array(), 2),
        );
        let mut radius = fix32::from_words(32768, 2);
        let mut intensity = n(255);
        if kind == 4 {
            radius = fix32::from_words(16384, 1);
            intensity = n(128);
        }
        if distance < radius {
            let alpha = integer::div(fmul_lo32(radius - distance, intensity), radius);
            unsafe {
                write_dseg_page(
                    3,
                    scene_creation::page(weapons, 1, weapons.directory[id * 2 + 1]),
                );
            }
            let state = at(scene_creation::record(weapons, id, 3), 30);
            let flags = g(state, 4);
            p(state, 11, n(1));
            p(
                state,
                4,
                fix32::from_words(flags.lo_bits() | 2, flags.hi_bits()),
            );
            p(state, 13, integer::from_i16(alpha.lo_bits() as i16));
            unsafe {
                write_dseg_page(3, saved);
            }
        }
    }
    entity
}
pub fn area(
    runtime: &scene_world::Runtime,
    weapons: &scene_creation::Weapons,
    frame: &Impact,
) -> fix32 {
    if !frame.has_vehicles {
        return n(-4);
    }
    let id = frame.object;
    if !component(weapons, id) {
        return n(-4);
    }
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(
            3,
            scene_creation::page(weapons, 1, weapons.directory[id * 2 + 1]),
        );
    }
    let state = at(scene_creation::record(weapons, id, 3), 30);
    let kind = g(state, 2);
    let has_owner = g(state, 0) != n(0);
    let owner = g(state, 1);
    unsafe {
        write_dseg_page(3, saved);
    }
    let origin = frame.action[0u16].origin.as_array();
    let mut radius = n(30720);
    let mut mode = n(1);
    let mut duration = n(350);
    if kind == n(4) {
        radius = n(20480);
        mode = n(4);
        duration = n(300);
    }
    let mut slot = 0u16;
    while slot < 9 {
        let index = frame.slots[slot];
        slot += 1;
        if index == 65535 {
            continue;
        }
        if index >= frame.vehicle_count || !frame.cars[index].present {
            return n(-4);
        }
        let car = &frame.cars[index];
        if has_owner && car.entity == owner {
            continue;
        }
        if abs(g(car.base_position, 0) - g(origin, 0)) >= radius
            || abs(g(car.base_position, 2) - g(origin, 2)) >= radius
            || g(car.blast, 3) > n(0)
        {
            continue;
        }
        let mut visible = n(0);
        if car.visible {
            visible = n(1);
        }
        p(car.blast, 4, visible);
        let mut target = blast::Target {
            state: car.blast,
            velocity: car.velocity,
            angular: car.angular,
            marker: g(car.marker, 0),
        };
        let input = blast::Input {
            position: car.physical_position,
            source: origin,
            mode,
            damage: false,
            max_health: fix32::from_words(40000, 0),
            record_axes: false,
            mark: car.player,
        };
        let result = blast::apply(&mut target, &input, runtime.seed);
        p(car.marker, 0, target.marker);
        if result == 2 {
            return n(-2);
        }
        if car.player {
            p(frame.camera_impulse, 0, n(1));
            p(frame.camera_impulse, 1, duration);
            p(frame.camera_impulse, 2, n(450));
        }
    }
    n(0)
}
pub fn direct(
    runtime: &scene_world::Runtime,
    weapons: &scene_creation::Weapons,
    tick: &Tick,
) -> fix32 {
    let action = &tick.impact.action[0u16];
    if !action.has_contact {
        return n(0);
    }
    let id = tick.impact.object;
    if !component(weapons, id) {
        return n(-4);
    }
    let saved3 = read_dseg_page(3);
    unsafe {
        write_dseg_page(
            3,
            scene_creation::page(weapons, 1, weapons.directory[id * 2 + 1]),
        );
    }
    let state = at(scene_creation::record(weapons, id, 3), 30);
    let kind = g(state, 2).lo_bits();
    let age = g(state, 3);
    let velocity: Buf<u16, 6> = Buf::new([0; 6]);
    let mut words = velocity.as_array();
    let mut i = 0u16;
    while i < 6 {
        words[i] = state[10 + i];
        i += 1;
    }
    unsafe {
        write_dseg_page(3, saved3);
    }
    if kind != 0 && kind != 2 && kind != 5 {
        return n(0);
    }
    let index = action.contact.vehicle;
    if !tick.impact.has_vehicles
        || index.hi_bits() != 0
        || index.lo_bits() >= tick.damage.count
        || !tick.damage.cars[index.lo_bits()].present
    {
        return n(-4);
    }
    let slot = index.lo_bits();
    if g(tick.impact.hits[slot].spin, 0) > n(0) {
        return n(0);
    }
    let vehicle = &tick.damage.cars[slot];
    let coefficients: Buf<u16, 9> = Buf::new([0; 9]);
    tank_hit::coefficients(
        coefficients.as_array(),
        kind,
        vehicle.car.player,
        tick.damage.cup,
        tick.damage.shell,
        tick.damage.homer,
    );
    if !tick.damage.present {
        return n(-5);
    }
    if !weapons.has_effect_pool {
        return n(-5);
    }
    let saved2 = read_dseg_page(2);
    unsafe {
        write_dseg_page(2, weapons.effect_page);
        write_dseg_page(3, vehicle.geometry_page);
    }
    let mut actions = tick.impact.action;
    let shot = tank_hit::Shot {
        hit: view_of(&mut actions[0u16].contact),
        velocity: velocity.as_array(),
        kind,
        age,
        time: tick.impact.stamp,
    };
    let context = temporary::Context {
        pool: weapons.lines,
        seed: runtime.seed,
        sine: tick.impact.sine,
    };
    let status = tank_hit::apply(
        &vehicle.car,
        &shot,
        coefficients.as_array(),
        &tick.damage.temporary,
        &context,
    );
    unsafe {
        write_dseg_page(3, saved3);
        write_dseg_page(2, saved2);
    }
    if status != n(0) {
        return status;
    }
    if vehicle.car.player {
        let mut duration = n(100);
        let mut magnitude = n(400);
        if kind == 2 {
            duration = n(500);
        } else if kind == 5 {
            duration = n(50);
            magnitude = n(300);
        }
        p(tick.impact.camera_impulse, 0, n(1));
        p(tick.impact.camera_impulse, 1, duration);
        p(tick.impact.camera_impulse, 2, magnitude);
    }
    n(0)
}
pub fn dispatch(
    scene: &scene_query::Scene,
    store: &mut scene_lifecycle::Store,
    runtime: &mut scene_world::Runtime,
    world: &mut scene_world::World,
    weapons: &mut scene_creation::Weapons,
    tick: &mut Tick,
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
        if method.id == 117 {
            let id = tick.impact.object;
            if !component(weapons, id) {
                return n(-4);
            }
            let stage = read_dseg_page(3);
            unsafe {
                write_dseg_page(
                    3,
                    scene_creation::page(weapons, 1, weapons.directory[id * 2 + 1]),
                );
            }
            let kind = g(at(scene_creation::record(weapons, id, 3), 30), 2);
            unsafe {
                write_dseg_page(3, stage);
            }
            status =
                scene_ufos::carrier(store, weapons, id, &tick.ufos.cars, &mut tick.flight.flight);
            if status != n(0) {
                return status;
            }
            let mut actions = tick.impact.action;
            status = scene_projectiles::advance(world, weapons, &tick.flight, &mut actions[0u16]);
            if status != n(0) {
                return status;
            }
            let action = &actions[0u16];
            if action.kind != 4 {
                status = scene_projectiles::finish(
                    scene,
                    store,
                    runtime,
                    world,
                    weapons,
                    &scene_projectiles::Finish {
                        call: tick.call,
                        object: id,
                        dt: tick.flight.flight.dt,
                        action: action.kind,
                    },
                );
            } else {
                if action.has_contact {
                    let car = action.contact.vehicle;
                    if car.hi_bits() != 0 || car.lo_bits() >= tick.impact.vehicle_count {
                        return n(-4);
                    }
                    if kind == n(3) {
                        unsafe {
                            write_dseg_page(
                                3,
                                scene_creation::page(weapons, 1, weapons.directory[id * 2 + 1]),
                            );
                        }
                        let state = at(scene_creation::record(weapons, id, 3), 30);
                        let flags = g(state, 4);
                        let attached = g(state, 25) != n(0);
                        unsafe {
                            write_dseg_page(3, stage);
                        }
                        if flags.lo_bits() & 4 != 0 {
                            p(runtime.slot, 0, saved);
                            return n(0);
                        }
                        let index = car.lo_bits();
                        if !tick.impact.has_vehicles || !tick.impact.cars[index].present {
                            return n(-4);
                        }
                        if g(tick.impact.cars[index].blast, 3) < n(1) && !attached {
                            let destination: Buf<u16, 6> = Buf::new([0; 6]);
                            let request = scene_ufos::Spawn {
                                class: tick.ufos.class,
                                resources_present: tick.ufos.resources_present,
                                mesh: tick.ufos.mesh,
                                material: tick.ufos.material,
                                target: tick.impact.cars[index].entity,
                                emitter_present: false,
                                emitter: n(0),
                                destination: destination.as_array(),
                                cars: tick.ufos.cars,
                            };
                            let ufo =
                                scene_ufos::spawn(scene, store, runtime, world, weapons, &request);
                            if ufo < n(0) {
                                return ufo;
                            }
                            if !component(weapons, id) {
                                return n(-4);
                            }
                            unsafe {
                                write_dseg_page(
                                    3,
                                    scene_creation::page(weapons, 1, weapons.directory[id * 2 + 1]),
                                );
                            }
                            let state = at(scene_creation::record(weapons, id, 3), 30);
                            p(state, 25, n(1));
                            p(state, 26, ufo);
                            let mut words = at(state, 54);
                            let local = action.contact.local.point.as_array();
                            let mut i = 0u16;
                            while i < 6 {
                                words[i] = local[i];
                                i += 1;
                            }
                            let flags = g(state, 4);
                            p(
                                state,
                                4,
                                fix32::from_words(flags.lo_bits() | 4, flags.hi_bits()),
                            );
                            unsafe {
                                write_dseg_page(3, stage);
                            }
                            p(runtime.slot, 0, saved);
                            return n(0);
                        }
                    }
                    let reflection = scene_projectiles::Reflection {
                        object: id,
                        vehicle: car.lo_bits(),
                        cars: tick.flight.flight.cars,
                        spin: tick.impact.hits[car.lo_bits()].spin,
                        context: tick.flight.context,
                        simulation: tick.impact.simulation,
                    };
                    let reflected = scene_projectiles::reflect(world, weapons, &reflection);
                    if reflected < n(0) {
                        return reflected;
                    }
                    if reflected == n(1) {
                        p(runtime.slot, 0, saved);
                        return n(0);
                    }
                }
                status = begin(scene, store, runtime, world, weapons, &tick.impact);
                if status >= n(0) {
                    if kind == n(1) || kind == n(4) {
                        status = area(runtime, weapons, &tick.impact);
                    } else {
                        status = direct(runtime, weapons, tick);
                    }
                }
            }
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
