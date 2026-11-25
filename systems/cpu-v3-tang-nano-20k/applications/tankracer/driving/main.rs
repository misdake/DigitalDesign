//! Source-loaded trajectories: only independent controls enter each persistent tick.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
mod boot_resources;
use boot_resources::*;
mod gpu_window;
use gpu_window::*;
mod static_assets;
use static_assets::AssetBytes;
mod attitude;
mod body;
mod collision;
mod damage;
mod deform_contact;
mod deformation;
mod drive;
mod driving;
mod effect_pool;
mod ground;
mod ground_response;
mod impact;
mod impact_capture;
mod integer;
mod lifecycle;
mod object_query;
mod ordered;
mod random;
mod sampling;
mod scene_contact;
mod scene_query;
mod scene_world;
mod spatial;
mod suspension;
mod tank_response;
mod terrain;
mod trig_lookup;
mod vehicle;
static DRIVING_PHASE: u16 = 0;
static DRIVING_FRAME: u16 = 0;
static DRIVING_CHECKS: u16 = 0;
static DRIVING_FIELD: u16 = 0;
fn mark(value: u16) {
    unsafe {
        addr_of(&DRIVING_PHASE).write(0, value);
    }
}
fn read(dst: Array<u16>, asset: AssetBytes, offset: fix32, count: u16) {
    if !gpu_window_asset_read(
        dst,
        asset.first_sector + (offset.hi_bits() << 10) + (offset.lo_bits() >> 6),
        offset.lo_bits() & 63,
        count,
    ) {
        halt(0x7f01);
    }
}
fn copy(mut to: Array<u16>, from: Array<u16>, count: u16) {
    let mut i = 0u16;
    while i < count {
        to[i] = from[i];
        i += 1;
    }
}
fn at(data: Array<u16>, offset: i16) -> Array<u16> {
    unsafe { data.as_ptr().add(offset).as_u16_array() }
}
fn check(actual: Array<u16>, expected: Array<u16>, count: u16, base: u16) {
    let mut i = 0u16;
    while i < count {
        if actual[i] != expected[i] {
            unsafe {
                addr_of(&DRIVING_FIELD).write(0, base + i);
            }
            halt(0x8000 + base + i);
        }
        i += 1;
    }
}
fn check_value(actual: fix32, expected: Array<u16>, index: u16) {
    if actual != attitude::get(expected, index) {
        unsafe {
            addr_of(&DRIVING_FIELD).write(0, index * 2);
        }
        halt(0x8000 + index * 2);
    }
}

fn car(state: Array<u16>, gun: Array<i16>, binding: Array<u16>) -> driving::Car {
    driving::Car {
        attitude: state,
        suspension: at(state, 58),
        drive: at(state, 146),
        motion: at(state, 178),
        boost: at(state, 210),
        tracks: at(state, 218),
        ground: ground::zero(),
        articulation: Buf::new([0; 8]),
        lean: Buf::new([0; 8]),
        hull: Buf::new([0; 37]),
        ready: false,
        turret_z: fix32::zero(),
        gun: gun,
        gun_binding: binding,
    }
}
fn load_car(car: &mut driving::Car, init: Array<u16>) {
    copy(car.attitude, init, 58);
    copy(car.suspension, at(init, 58), 88);
    copy(car.drive, at(init, 146), 32);
    copy(car.motion, at(init, 178), 32);
    copy(car.boost, at(init, 210), 8);
    copy(car.tracks, at(init, 218), 48);
    copy(
        unsafe { view_of(&mut car.ground).as_ptr().as_u16_array() },
        at(init, 266),
        206,
    );
    copy(car.articulation.as_array(), at(init, 472), 8);
    copy(car.lean.as_array(), at(init, 480), 8);
    copy(car.hull.as_array(), at(init, 488), 24);
    let mut hull = car.hull.as_array();
    let mut i = 0u16;
    while i < 12 {
        hull[24 + i] = attitude::get(init, 256 + i).lo_bits();
        i += 1;
    }
    hull[36u16] = attitude::get(init, 268).lo_bits();
    car.ready = attitude::get(init, 269) != fix32::zero();
    car.turret_z = attitude::get(init, 270);
    let mut gun = car.gun;
    i = 0;
    while i < DRIVING_GUN_WORDS {
        gun[i] = attitude::get(init, 271 + i).lo_bits() as i16;
        i += 1;
    }
}
fn check_car(car: &mut driving::Car, expected: Array<u16>) {
    check(car.attitude, expected, 58, 0);
    check(car.suspension, at(expected, 58), 88, 58);
    check(car.drive, at(expected, 146), 32, 146);
    check(car.motion, at(expected, 178), 32, 178);
    check(car.boost, at(expected, 210), 8, 210);
    check(car.tracks, at(expected, 218), 48, 218);
    check(
        unsafe { view_of(&mut car.ground).as_ptr().as_u16_array() },
        at(expected, 266),
        206,
        266,
    );
    check(car.articulation.as_array(), at(expected, 472), 8, 472);
    check(car.lean.as_array(), at(expected, 480), 8, 480);
    check(car.hull.as_array(), at(expected, 488), 24, 488);
    let mut i = 0u16;
    while i < 12 {
        check_value(
            integer::from_i16(car.hull[24 + i] as i16),
            expected,
            256 + i,
        );
        i += 1;
    }
    check_value(fix32::from_words(car.hull[36u16], 0), expected, 268);
    let mut ready = fix32::zero();
    if car.ready {
        ready = integer::from_i16(1);
    }
    check_value(ready, expected, 269);
    check_value(car.turret_z, expected, 270);
    i = 0;
    while i < DRIVING_GUN_WORDS {
        check_value(integer::from_i16(car.gun[i]), expected, 271 + i);
        i += 1;
    }
}
fn query_car(
    car: &mut driving::Car,
    tank: &mut object_query::Tank,
    binding: Array<u16>,
    meta: Array<u16>,
    id: u16,
) {
    tank.collider.entity = fix32::from_words(meta[1 + id * 2], meta[2 + id * 2]);
    tank.collider.radius = attitude::get(binding, 0);
    tank.collider.has_external = integer::from_i16(1);
    copy(tank.collider.position.as_array(), at(car.motion, 18), 6);
    copy(tank.collider.basis.as_array(), car.attitude, 18);
    copy(tank.collider.external.as_array(), car.attitude, 18);
    copy(tank.collider.bounds.as_array(), at(binding, 2), 12);
    copy(tank.overlap_bounds.as_array(), at(binding, 14), 12);
    tank.slot = fix32::from_words(id, 0);
    tank.active = fix32::from_words(meta[5 + id], 0);
    tank.disabled = fix32::zero();
    tank.ready = fix32::zero();
    if car.ready {
        tank.ready = integer::from_i16(1);
    }
    let points = unsafe { car.ground.points.as_array().as_ptr().as_u16_array() };
    let mut i = 0u16;
    while i < 4 {
        copy(
            at(tank.contacts.as_array(), (i * 6) as i16),
            at(points, (i * 34 + 6) as i16),
            6,
        );
        i += 1;
    }
}
fn collision_car(
    car: &mut driving::Car,
    meta: Array<u16>,
    body: Array<u16>,
    id: u16,
    aux: Array<u16>,
) -> tank_response::Vehicle {
    let points = unsafe { car.ground.points.as_array().as_ptr().as_u16_array() };
    tank_response::Vehicle {
        velocity: car.motion,
        local_velocity: car.drive,
        previous_position: at(car.motion, 12),
        timer: at(aux, 2),
        delta: at(aux, 4),
        corners: at(car.suspension, 4),
        corner_stride: 6,
        impact: impact::Vehicle {
            entity: fix32::from_words(meta[1 + id * 2], meta[2 + id * 2]),
            cooldown: aux,
            contacts: at(points, 6),
            contact_stride: 17,
            forward: attitude::get(car.drive, 2),
            basis: car.attitude,
            has_basis: true,
        },
        damage: damage::Target {
            state: at(aux, 16),
            player: id == 0,
            basis: at(car.attitude, 18),
            position: at(car.attitude, 36),
            selected: body,
            mesh: damage::Mesh {
                present: true,
                vertices: unsafe { body.as_ptr().add(4).as_i16_array() },
                vertex_count: DRIVING_BODY_VERTICES,
                indices: at(body, (4 + DRIVING_BODY_VERTICES * 3) as i16),
                normals: unsafe {
                    body.as_ptr()
                        .add((4 + DRIVING_BODY_VERTICES * 3 + DRIVING_BODY_FACES * 3) as i16)
                        .as_i16_array()
                },
                face_count: DRIVING_BODY_FACES,
            },
        },
    }
}
fn check_mesh(target: &damage::Target, expected: Array<u16>, flags: u16, vertices: u16) {
    let mut i = 0u16;
    while i < 10 {
        check_value(fix32::from_words(target.state[i], 0), expected, flags + i);
        i += 1;
    }
    i = 0;
    while i < DRIVING_BODY_VERTICES * 3 {
        check_value(
            integer::from_i16(target.mesh.vertices[i]),
            expected,
            vertices + i,
        );
        i += 1;
    }
    i = 0;
    while i < DRIVING_BODY_FACES * 3 {
        check_value(
            integer::from_i16(target.mesh.normals[i]),
            expected,
            vertices + DRIVING_BODY_VERTICES * 3 + i,
        );
        i += 1;
    }
}
fn check_surfaces(components: Array<deform_contact::Component>, expected: Array<u16>) {
    let mut i = 0u16;
    let mut offset = 0u16;
    while i < 7 {
        let c = &components[i];
        check_value(c.state.dirty, expected, offset);
        check_value(c.state.last_vehicle, expected, offset + 1);
        check_value(c.state.sound_timer, expected, offset + 2);
        check_value(fix32::from_words(c.sampling.dirty, 0), expected, offset + 3);
        let mut attached = fix32::zero();
        if c.sampling.attached {
            attached = integer::from_i16(1);
        }
        check_value(attached, expected, offset + 4);
        offset += 5;
        let mut j = 0u16;
        while j < c.mesh.vertex_count * 3 {
            check_value(integer::from_i16(c.mesh.vertices[j]), expected, offset);
            offset += 1;
            j += 1;
        }
        i += 1;
    }
    if offset != DRIVING_SURFACE_VALUES {
        halt(0x7f10);
    }
}
fn main() {
    mark(1);
    let config: Buf<u16, 695> = Buf::new([0; 695]);
    let cfg = config.as_array();
    read(cfg, DRIVING_CONFIG, fix32::zero(), DRIVING_CONFIG_WORDS);
    let c = driving::Config {
        spring: cfg,
        drive: at(cfg, 52),
        attitude: at(cfg, 572),
        pose: at(cfg, 577),
        sampling: at(cfg, 595),
        environment: at(cfg, 625),
        auxiliary: at(cfg, 661),
        body: at(cfg, 673),
        sine: unsafe { addr_of(&DRIVING_SINE).as_i16_array() },
        angle: unsafe { addr_of(&DRIVING_ANGLE).as_i16_array() },
    };
    let scene = scene_query::Scene::begin(DRIVING_SCENE);
    let colliders: Buf<u16, 14400> = Buf::new([0; 14400]);
    let properties: Buf<u16, 3840> = Buf::new([0; 3840]);
    let memberships: Buf<u16, 4096> = Buf::new([0; 4096]);
    let range_storage: Buf<u16, 192> = Buf::new([0; 192]);
    let origin: Buf<u16, 4> = Buf::new([0; 4]);
    let deformables: Buf<u16, 16> = Buf::new([0; 16]);
    let mut ranges = range_storage.as_array();
    let mut i = 0u16;
    while i < 64 {
        ranges[i * 3] = i * 64;
        ranges[i * 3 + 2] = 32;
        i += 1;
    }
    let mut scenery = scene_world::World {
        colliders: colliders.as_array(),
        properties: properties.as_array(),
        count: 240,
        grid: spatial::Grid {
            origin: origin.as_array(),
            shift_x: 0,
            shift_z: 0,
            items: memberships.as_array(),
            ranges: ranges,
        },
        deformables: ordered::List {
            items: deformables.as_array(),
            count: 0,
            capacity: 8,
        },
    };
    if !scene.valid || scene.load_world(&mut scenery) != 0 || scene.header[9u16] != 7 {
        halt(0x7f0c);
    }
    run_cases(&c, &scene, &scenery);
    mark(4);
    halt(0x600d);
}

fn run_cases(c: &driving::Config, scene: &scene_query::Scene, scenery: &scene_world::World) {
    let storage: DrivingStorage = driving_storage();
    let cfg = c.spring;
    let directory: Buf<u16, 64> = Buf::new([65535; 64]);
    let dir = directory.as_array();
    read(dir, DRIVING_DIRECTORY, fix32::zero(), 64);
    let binding: Buf<u16, 26> = Buf::new([0; 26]);
    read(binding.as_array(), DRIVING_BINDING, fix32::zero(), 26);
    let metadata: Buf<u16, 20> = Buf::new([0; 20]);
    let meta = metadata.as_array();
    let initial = storage.initial.as_array();
    let record = storage.record.as_array();
    let gun = storage.gun.as_array();
    let body = storage.body.as_array();
    let states: Buf<u16, 532> = Buf::new([0; 532]);
    let state = states.as_array();
    let events: Buf<u16, 3> = Buf::new([0; 3]);
    let incoming = read_dseg_page(3);
    let empty_records: Buf<deformation::Record, 1> = Buf::new([deformation::record_zero()]);
    let mut surface = deformation::disabled(cfg, empty_records.as_array());
    let mut case = 0u16;
    let mut serial = 0u16;
    while case < DRIVING_CASES {
        read(
            initial,
            DRIVING_INITIAL,
            fmul_lo32(
                fix32::from_words(case, 0),
                fix32::from_words(DRIVING_INITIAL_WORDS, 0),
            ),
            DRIVING_INITIAL_WORDS,
        );
        read(meta, DRIVING_CASE_META, fix32::from_words(case * 20, 0), 20);
        read(
            storage.deform.as_array(),
            DRIVING_DEFORM,
            fix32::zero(),
            DRIVING_DEFORM_WORDS,
        );
        let surfaces: Buf<deform_contact::Component, 7> = Buf::new([
            deform_contact::component(
                storage.deform.as_array(),
                scenery,
                0,
                empty_records.as_array(),
            ),
            deform_contact::component(
                storage.deform.as_array(),
                scenery,
                1,
                empty_records.as_array(),
            ),
            deform_contact::component(
                storage.deform.as_array(),
                scenery,
                2,
                empty_records.as_array(),
            ),
            deform_contact::component(
                storage.deform.as_array(),
                scenery,
                3,
                empty_records.as_array(),
            ),
            deform_contact::component(
                storage.deform.as_array(),
                scenery,
                4,
                empty_records.as_array(),
            ),
            deform_contact::component(
                storage.deform.as_array(),
                scenery,
                5,
                empty_records.as_array(),
            ),
            deform_contact::component(
                storage.deform.as_array(),
                scenery,
                6,
                empty_records.as_array(),
            ),
        ]);
        let mut deform = surfaces.as_array();
        read(body, DRIVING_MESH, fix32::zero(), DRIVING_MESH_WORDS);
        copy(
            at(body, DRIVING_MESH_WORDS as i16),
            body,
            DRIVING_MESH_WORDS,
        );
        let cars: Buf<driving::Car, 2> = Buf::new([
            car(state, gun, at(cfg, 685)),
            car(
                at(state, 266),
                unsafe { gun.as_ptr().add(DRIVING_GUN_WORDS as i16).as_i16_array() },
                at(cfg, 685),
            ),
        ]);
        let mut actors = cars.as_array();
        load_car(&mut actors[0u16], initial);
        load_car(&mut actors[1u16], at(initial, 3566));
        let mut world = ground::World::begin(DRIVING_CELLS, fix32::from_words(0xabcd, 0x1234), dir);
        if world.seed != attitude::get(initial, DRIVING_STATE_VALUES) {
            halt(0x7f03);
        }
        check(
            world.noise.as_array(),
            at(initial, (DRIVING_STATE_VALUES * 2 + 2) as i16),
            16,
            DRIVING_STATE_VALUES * 2 + 2,
        );
        let auxiliary: Buf<u16, 64> = Buf::new([0; 64]);
        let aux = auxiliary.as_array();
        copy(aux, at(initial, 602), 2);
        copy(at(aux, 32), at(initial, 4156), 2);
        copy(at(aux, 26), at(initial, 4558), 6);
        copy(at(aux, 58), at(initial, 4564), 6);
        let current_slot: Buf<u16, 2> = Buf::new([0; 2]);
        copy(current_slot.as_array(), at(meta, 16), 2);
        let geometry: Buf<object_query::Tank, 2> =
            Buf::new([object_query::tank_zero(), object_query::tank_zero()]);
        let mut geo = geometry.as_array();
        let lines: Buf<u16, 1600> = Buf::new([0; 1600]);
        let tail: Buf<u16, 16> = Buf::new([0; 16]);
        let batches: Buf<impact::Batch, 4> = Buf::new([
            impact::batch_zero(),
            impact::batch_zero(),
            impact::batch_zero(),
            impact::batch_zero(),
        ]);
        let mut impacts = impact::State {
            batches: batches.as_array(),
            next: fix32::zero(),
        };
        let offsets: Buf<u16, 12> = Buf::new([0, 0, 0, 0, 2000, 0, 0, 0, 0, 0, 63536, 65535]);
        let mut effects = impact::Context {
            frame: fix32::zero(),
            seed: unsafe { view_of(&mut world.seed).as_ptr().as_u16_array() },
            pool: effect_pool::Pool {
                lines: lines.as_array(),
                count: fix32::zero(),
                overflow: effect_pool::line_zero(),
                exhausted: tail.as_array(),
            },
            sine: c.sine,
            angle: c.angle,
            forward_offset: offsets.as_array(),
            reverse_offset: at(offsets.as_array(), 6),
        };
        effect_pool::reset(&mut effects.pool);
        let mut frame = 0u16;
        let frame_asset = driving_frames(case);
        let surface_asset = driving_surfaces(case);
        while frame < DRIVING_STEPS * meta[0u16] {
            unsafe {
                addr_of(&DRIVING_FRAME).write(0, serial);
            }
            read(
                record,
                frame_asset,
                fmul_lo32(
                    fix32::from_words(frame, 0),
                    fix32::from_words(DRIVING_FRAME_WORDS, 0),
                ),
                DRIVING_FRAME_WORDS,
            );
            let data = record;
            let own = attitude::get(data, 28).lo_bits();
            if own >= meta[0u16] || own >= 2 {
                halt(0x7f0b);
            }
            mark(2);
            let actor_aux = at(aux, (own * 32) as i16);
            let selected =
                deform_contact::attached(deform, 7, scenery, attitude::get(actor_aux, 14));
            let mut status = 0u16;
            if selected == 65535 {
                if attitude::get(actor_aux, 14) != integer::from_i16(-1) {
                    halt(0x7f0d);
                }
                status = driving::contact_and_power(
                    &mut actors[own],
                    c,
                    &mut world,
                    data,
                    events.as_array(),
                    &mut surface,
                );
            } else {
                status = driving::contact_and_power(
                    &mut actors[own],
                    c,
                    &mut world,
                    data,
                    events.as_array(),
                    &mut deform[selected].sampling,
                );
            }
            if status != 0 {
                unsafe {
                    addr_of(&DRIVING_FIELD).write(0, status);
                }
                halt(0x7f04);
            }
            effects.frame = attitude::get(data, 26);
            query_car(
                &mut actors[0u16],
                &mut geo[0u16],
                binding.as_array(),
                meta,
                0,
            );
            query_car(
                &mut actors[1u16],
                &mut geo[1u16],
                binding.as_array(),
                meta,
                1,
            );
            let vehicles: Buf<tank_response::Vehicle, 2> = Buf::new([
                collision_car(&mut actors[0u16], meta, body, 0, aux),
                collision_car(
                    &mut actors[1u16],
                    meta,
                    at(body, DRIVING_MESH_WORDS as i16),
                    1,
                    at(aux, 32),
                ),
            ]);
            let mut peers = tank_response::World {
                geometry: geo,
                vehicles: vehicles.as_array(),
                slots: at(meta, 7),
                present: at(meta, 5),
                geometry_count: 2,
                vehicle_count: 2,
                hit: integer::from_i16(-1),
            };
            let env = tank_response::Environment {
                mode: attitude::get(data, 1),
                dt: attitude::get(data, 0),
                separation_limit: fix32::zero(),
            };
            let status = tank_response::cars(own, &mut peers, &env, &mut impacts, &mut effects);
            if status != fix32::zero() {
                unsafe {
                    addr_of(&DRIVING_FIELD).write(0, status.lo_bits());
                }
                halt(0x7f0a);
            }
            let current = collision_car(
                &mut actors[own],
                meta,
                at(body, (own * DRIVING_MESH_WORDS) as i16),
                own,
                actor_aux,
            );
            let car = &mut actors[own];
            let mut contact = deform_contact::Vehicle {
                position: at(car.motion, 6),
                base_position: at(car.motion, 18),
                previous: at(car.motion, 12),
                velocity: car.motion,
                material: at(actor_aux, 26),
                attached: at(actor_aux, 28),
                accumulator: at(actor_aux, 30),
                corners: at(car.suspension, 4),
                corner_stride: 6,
                player: own == 0,
                mode: attitude::get(data, 1),
                dt: attitude::get(data, 0),
                current_slot: current_slot.as_array(),
                impact: current.impact,
            };
            if meta[18u16] != 0 {
                let status = deform_contact::respond(
                    &mut contact,
                    scene,
                    scenery,
                    deform,
                    &mut impacts,
                    &mut effects,
                );
                if status != fix32::zero() {
                    unsafe {
                        addr_of(&DRIVING_FIELD).write(0, status.lo_bits());
                    }
                    halt(0x7f0e);
                }
            }
            let mut response = ground_response::State {
                velocity: car.motion,
                angular: at(car.attitude, 42),
                points: unsafe { car.ground.points.as_array().as_ptr().as_u16_array() },
                springs: car.suspension,
                position: at(car.motion, 18),
                delta: at(actor_aux, 10),
                impact: contact.impact,
                damage: current.damage,
                mode: attitude::get(data, 1),
            };
            let status = ground_response::step(&mut response, &mut impacts, &mut effects);
            if status != 0 {
                unsafe {
                    addr_of(&DRIVING_FIELD).write(0, status);
                }
                halt(0x7f07);
            }
            let status = driving::integrate_and_pose(car, c, data);
            if status != 0 {
                unsafe {
                    addr_of(&DRIVING_FIELD).write(0, status);
                }
                halt(0x7f08);
            }
            let status = impact::tick(
                &mut impacts,
                &mut response.impact,
                attitude::get(data, 0),
                &mut effects,
            );
            if status != 0 {
                unsafe {
                    addr_of(&DRIVING_FIELD).write(0, status);
                }
                halt(0x7f09);
            }
            driving::finish(car, data);
            mark(3);
            let expected = at(data, 60);
            check_car(&mut actors[0u16], expected);
            check_car(&mut actors[1u16], at(expected, 3566));
            let extra = at(initial, 590);
            impact_capture::write(
                &mut impacts,
                &peers.vehicles[0u16].impact,
                &mut effects,
                extra,
            );
            check(extra, at(expected, 590), 2576, 590);
            check_mesh(&peers.vehicles[0u16].damage, expected, 1583, 1596);
            check_mesh(&peers.vehicles[1u16].damage, expected, 2086, 2096);
            check_value(attitude::get(aux, 1), expected, 1779);
            check_value(attitude::get(aux, 16), expected, 2078);
            check_value(attitude::get(aux, 17), expected, 2079);
            let mut i = 0u16;
            while i < 3 {
                check_value(attitude::get(aux, 5 + i), expected, 1593 + i);
                check_value(attitude::get(aux, 2 + i), expected, 1780 + i);
                check_value(attitude::get(aux, 18 + i), expected, 2080 + i);
                check_value(attitude::get(aux, 21 + i), expected, 2083 + i);
                check_value(attitude::get(aux, 13 + i), expected, 2279 + i);
                check_value(attitude::get(aux, 29 + i), expected, 2282 + i);
                i += 1;
            }
            check_value(attitude::get(current_slot.as_array(), 0), expected, 2285);
            if meta[19u16] != 0 {
                read(
                    storage.surface_check.as_array(),
                    surface_asset,
                    fmul_lo32(
                        fix32::from_words(frame, 0),
                        fix32::from_words(DRIVING_SURFACE_VALUES * 2, 0),
                    ),
                    DRIVING_SURFACE_VALUES * 2,
                );
                check_surfaces(deform, storage.surface_check.as_array());
            }
            if world.seed != attitude::get(expected, DRIVING_STATE_VALUES) {
                unsafe {
                    addr_of(&DRIVING_FIELD).write(0, DRIVING_STATE_VALUES * 2);
                }
                halt(0x7f05);
            }
            unsafe {
                addr_of(&DRIVING_CHECKS).write(0, DRIVING_CHECKS + 1);
            }
            frame += 1;
            serial += 1;
        }
        world.terrain.finish();
        if read_dseg_page(3) != incoming {
            halt(0x7f06);
        }
        case += 1;
    }
}
