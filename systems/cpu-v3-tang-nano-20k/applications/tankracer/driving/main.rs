//! Source-loaded trajectories: only independent controls enter each persistent tick.
#![link_csegs(
    default = "app",
    app = 7,
    frame = 5,
    world = 2,
    parents = 4,
    vehicle = 3
)]
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
mod boot_resources;
use boot_resources::*;
mod gpu_window;
use gpu_window::*;
mod static_assets;
use static_assets::AssetBytes;
mod static_buffer;
use static_buffer::StaticBuffer;
#[static_buffer]
static DRIVING_WORLD: StaticBuffer = StaticBuffer::new(65536, 65536);
#[static_buffer]
static DRIVING_MOTION: StaticBuffer = StaticBuffer::new(32768, 32768);
#[static_buffer]
static DRIVING_PICKUP_EFFECTS: StaticBuffer = StaticBuffer::new(32768, 32768);
#[static_buffer]
static DRIVING_DYNAMIC_WORLD: StaticBuffer = StaticBuffer::new(32768, 32768);
#[static_buffer]
static DRIVING_WEAPONS: StaticBuffer = StaticBuffer::new(196608, 32768);
#[static_buffer]
static DRIVING_ACTORS: StaticBuffer = StaticBuffer::new(32768, 32768);
const IMPACT_LINES_WORD: u16 = 3000;
const IMPACT_LINE_WORDS: u16 = 1600;
const IMPACT_BATCHES_WORD: u16 = 4608;
const IMPACT_GUARD_WORD: u16 = 5887;
mod attitude;
mod body;
mod bounce;
mod collision;
mod damage;
mod deform_contact;
mod deformation;
mod drive;
mod driving;
mod effect_pool;
mod entities;
mod falling;
mod finish_check;
mod finish_drive;
mod finish_driver;
mod finish_routes;
mod finish_world;
mod frame_check;
mod frame_output;
mod game_update;
mod ground;
mod ground_response;
use frame_check::*;
mod blast;
mod car_effects;
mod car_motion;
mod car_runtime;
mod closer;
mod complete_check;
mod debris_catalog;
mod emitter;
mod explosion;
mod explosion_blasts;
mod flock;
mod identity_capture;
mod impact;
mod impact_capture;
mod integer;
mod inventory;
mod inverse_basis;
mod level_init;
mod level_io;
mod level_run;
mod lifecycle;
mod mesh_break;
mod mesh_catalog;
mod mesh_storage;
mod mine;
mod object_query;
mod ordered;
mod parent_check;
mod particle_sprite;
use level_io::*;
mod pickup_runtime;
use pickup_runtime::*;
mod pickup_check;
mod pickup_light;
mod player_control;
mod player_input;
mod player_route;
mod player_tail;
mod player_world;
mod powerup;
mod prefix;
mod projectile;
mod projectile_flight;
mod projectile_marks;
mod projectile_ricochet;
mod projectile_sweep;
mod projectile_trail;
mod quad_pool;
mod scene_actor_initial;
mod scene_ambient;
mod scene_ambient_world;
mod scene_boot;
mod scene_creation;
mod scene_emitter;
mod scene_explosions;
mod scene_fire;
mod scene_flock;
mod scene_flock_world;
mod scene_initial;
mod scene_laps;
mod scene_mines;
mod scene_obstacle_transfer;
mod scene_pickups;
mod scene_player;
mod scene_projectile_impact;
mod scene_projectiles;
mod scene_train;
mod scene_trains;
mod scene_ufo_transfer;
mod scene_ufos;
mod scene_vehicle_transfer;
mod smoker;
mod tank_hit;
mod temporary;
mod track_effects;
mod train;
mod vehicle_boot;
mod vehicle_debris;
mod vehicle_inactive;
mod vehicle_pose;
mod vehicle_shield;
mod vehicle_smoke;
mod vehicle_startup;
mod vehicle_transfer;
mod weapon_emitters;
mod weapon_hits;
mod weapon_mines;
mod weapon_ufos;
mod weapon_world;
use pickup_check::*;
mod random;
mod sampling;
mod scene_auxiliaries;
mod scene_contact;
mod scene_debris;
mod scene_fragments;
mod scene_lifecycle;
mod scene_meshes;
mod scene_parent;
mod scene_query;
mod scene_world;
mod scenery_check;
mod shatter;
mod spin;
mod symbols;
use parent_check::*;
mod scenery_contact;
mod spatial;
mod suspension;
mod tank_response;
mod terrain;
mod trig_lookup;
use scenery_check::*;
mod vehicle;
static DRIVING_PHASE: u16 = 0;
static DRIVING_FRAME: u16 = 0;
static DRIVING_CHECKS: u16 = 0;
static DRIVING_FIELD: u16 = 0;
fn ptr_camera() -> Array<u16> {
    unsafe { Ptr::from_addr(0xc000 + 10174).as_u16_array() }
}
fn mark(value: u16) {
    unsafe {
        addr_of(&DRIVING_PHASE).write(0, value);
    }
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

fn car(state: Array<u16>, gun: Array<i16>, binding: Array<u16>, owner: u16) -> driving::Car {
    driving::Car {
        entity: fix32::zero(),
        owner,
        profile: owner,
        slot: 0,
        player: false,
        visible: true,
        blast: unsafe { Ptr::from_addr(0).as_u16_array() },
        marker: unsafe { Ptr::from_addr(0).as_u16_array() },
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
fn query_car(
    car: &mut driving::Car,
    tank: &mut object_query::Tank,
    binding: Array<u16>,
    present: Array<u16>,
    complete: bool,
) {
    tank.collider.entity = car.entity;
    tank.collider.radius = attitude::get(binding, 0);
    tank.collider.has_external = integer::from_i16(1);
    copy(tank.collider.position.as_array(), at(car.motion, 18), 6);
    copy(tank.collider.basis.as_array(), car.attitude, 18);
    copy(tank.collider.external.as_array(), car.attitude, 18);
    copy(tank.collider.bounds.as_array(), at(binding, 2), 12);
    copy(tank.overlap_bounds.as_array(), at(binding, 14), 12);
    tank.slot = fix32::from_words(car.slot, 0);
    tank.active = fix32::zero();
    if present[car.owner] != 0 && car.visible {
        tank.active = integer::from_i16(1);
    }
    tank.disabled = fix32::zero();
    if complete {
        let saved = read_dseg_page(3);
        unsafe {
            write_dseg_page(3, DRIVING_PICKUP_EFFECTS.first_sector() >> 8);
        }
        let effects = unsafe {
            Ptr::from_addr(0xc000 + car_runtime::CAR_WORD + car.owner * car_runtime::CAR_WORDS)
                .as_u16_array()
        };
        tank.disabled = attitude::get(at(effects, 950), 0);
        let external = attitude::get(at(effects, 902), 8);
        tank.collider.has_external = fix32::zero();
        if external != fix32::zero() {
            tank.collider.has_external = integer::from_i16(1);
        }
        if external == integer::from_i16(2) {
            copy(tank.collider.external.as_array(), at(effects, 920), 18);
        }
        unsafe {
            write_dseg_page(3, saved);
        }
    }
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
    player: &player_world::Player,
    id: u16,
    aux: Array<u16>,
) -> tank_response::Vehicle {
    let body = at(player.body, (id * DRIVING_MESH_WORDS) as i16);
    let model = at(player.hit_model.as_array(), (id * 7) as i16);
    let revision = at(player.hit_fields.as_array(), (id * 24 + 12) as i16);
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
            entity: car.entity,
            cooldown: aux,
            contacts: at(points, 6),
            contact_stride: 17,
            forward: attitude::get(car.drive, 2),
            basis: car.attitude,
            has_basis: true,
        },
        damage: damage::Target {
            has_revision: model[0u16] != 0,
            revision,
            state: at(aux, 16),
            player: car.player,
            basis: at(car.attitude, 18),
            position: at(car.attitude, 36),
            selected: player.body_selection.as_array(),
            mesh: damage::Mesh {
                page: player.weapons.effect_page,
                present: model[1u16] != 0,
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
#[inline(never)]
fn start_case(
    actors: Array<driving::Car>,
    aux: Array<u16>,
    case: u16,
    dir: Array<u16>,
) -> ground::World {
    let incoming = read_dseg_page(3);
    // Each car consumes only its 295-value initialization section. The same
    // temporary buffer then holds the independent seed/noise section.
    let image: Buf<u16, 640> = Buf::new([0; 640]);
    let initial = image.as_array();
    let car_words = 542 + DRIVING_GUN_WORDS * 2;
    if car_words > 640
        || DRIVING_INITIAL_WORDS != DRIVING_STATE_VALUES * 2 + 18
        || DRIVING_START_WORDS != car_words * 2 + 42
    {
        halt(0x7f35);
    }
    read(
        initial,
        DRIVING_INITIAL_OFFSETS,
        fix32::from_words(case, 0),
        1,
    );
    let base = fix32::from_words(initial[0u16], 0);
    let mut actors = actors;
    if !driving_has_startup(case) {
        read(initial, DRIVING_INITIAL, base, car_words);
        load_car(&mut actors[0u16], initial);
        read(
            initial,
            DRIVING_INITIAL,
            base + fix32::from_words(car_words, 0),
            car_words,
        );
        load_car(&mut actors[1u16], initial);
    }
    read(initial, DRIVING_SEEDS, fix32::zero(), 4);
    if initial[0u16] != 0x5253
        || initial[1u16] != 1
        || initial[2u16] != DRIVING_CASES
        || initial[3u16] != 0
    {
        halt(0x7f7c);
    }
    read(
        initial,
        DRIVING_SEEDS,
        fix32::from_words(4 + case * 2, 0),
        2,
    );
    let mut world = ground::World::begin(DRIVING_CELLS, attitude::get(initial, 0), dir);
    if DRIVING_VERIFY != 0 {
        read(
            initial,
            DRIVING_INITIAL,
            base + fix32::from_words(car_words * 2, 0),
            18,
        );
        if world.seed != attitude::get(initial, 0) {
            halt(0x7f03);
        }
        check(
            world.noise.as_array(),
            at(initial, 2),
            16,
            DRIVING_STATE_VALUES * 2 + 2,
        );
        check_map_residency(&mut world);
    }
    unsafe {
        write_dseg_page(3, incoming);
    }
    if driving_has_startup(case) {
        let mut i = 0u16;
        let mut data = aux;
        while i < 72 {
            data[i] = 0;
            i += 1;
        }
        attitude::put(aux, 14, integer::from_i16(-1));
        attitude::put(at(aux, 36), 14, integer::from_i16(-1));
        return world;
    }
    let auxiliary = base + fix32::from_words(car_words * 2 + 18, 0);
    read(aux, DRIVING_INITIAL, auxiliary, 2);
    read(
        at(aux, 36),
        DRIVING_INITIAL,
        auxiliary + fix32::from_words(2, 0),
        2,
    );
    read(
        at(aux, 26),
        DRIVING_INITIAL,
        auxiliary + fix32::from_words(4, 0),
        6,
    );
    read(
        at(aux, 62),
        DRIVING_INITIAL,
        auxiliary + fix32::from_words(10, 0),
        6,
    );
    read(
        at(aux, 32),
        DRIVING_INITIAL,
        auxiliary + fix32::from_words(16, 0),
        4,
    );
    read(
        at(aux, 68),
        DRIVING_INITIAL,
        auxiliary + fix32::from_words(20, 0),
        4,
    );

    world
}

fn main() {
    mark(1);
    let config: Buf<u16, 695> = Buf::new([0; 695]);
    let c = level_init::config(
        config.as_array(),
        DRIVING_CONFIG,
        unsafe { addr_of(&DRIVING_SINE).as_i16_array() },
        unsafe { addr_of(&DRIVING_ANGLE).as_i16_array() },
    );
    let scene = scene_query::Scene::begin(DRIVING_SCENE);
    let previous2 = read_dseg_page(2);
    let previous3 = read_dseg_page(3);
    let deformables: Buf<u16, 20> = Buf::new([0; 20]);
    let mut scenery = level_init::world(
        deformables.as_array(),
        DRIVING_WORLD.first_sector() >> 8,
        DRIVING_DYNAMIC_WORLD.first_sector() >> 8,
        DRIVING_DEFORM_COUNT,
    );
    if DRIVING_VERIFY != 0 {
        install_world_guards(&scenery);
        check_grid_residency(scenery.grid.page);
    }
    if !scene.valid || scene.load_world(&mut scenery) != 0 || scene.header[9u16] != 7 {
        halt(0x7f0c);
    }
    let mut case = 0u16;
    let mut serial = 0u16;
    while case < DRIVING_CASES {
        serial = level_run::run(&c, &mut scenery, case, serial);
        case += 1;
    }
    if DRIVING_VERIFY != 0 {
        check_world_guards();
    }
    unsafe {
        write_dseg_page(3, previous3);
        write_dseg_page(2, previous2);
    }
    mark(4);
    halt(0x600d);
}

// Qualification sentinels live outside the authored collider and membership data.
fn install_world_guards(world: &scene_world::World) {
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(3, world.dynamic_page);
    }
    let mut dynamic = unsafe { Ptr::from_addr(0xc000).as_u16_array() };
    dynamic[16340u16] = 0x6a39;
    dynamic[16383u16] = 0x93a6;
    unsafe {
        write_dseg_page(3, saved);
    }
    let mut words = world.colliders;
    words[14400u16] = 0xa55a;
    words[16383u16] = 0x5aa5;
    words = world.properties;
    words[8132u16] = 0x393c;
    words[16383u16] = 0xc3c3;
}

// This table is needed only while reserving the level's motion layout.
#[link_cseg = "frame"]
fn load_motion_layout(bank: &mut scene_fragments::MotionBank) {
    let layout: Buf<u16, 111> = Buf::new([0; 111]);
    read(layout.as_array(), DRIVING_MOTION_LAYOUT, fix32::zero(), 111);
    let rows = layout.as_array();
    let mut part = 0u16;
    while part < 37 {
        if scene_fragments::reserve(bank, rows[part * 3 + 1]) != rows[part * 3 + 2] {
            halt(0x7f46);
        }
        part += 1;
    }
}

fn check_world_guards() {
    let page = DRIVING_WORLD.first_sector() >> 8;
    if read_dseg_page(2) != page {
        halt(0x7f38);
    }
    let colliders = unsafe { Ptr::from_addr(0x8000).as_u16_array() };
    if colliders[14400u16] != 0xa55a || colliders[16383u16] != 0x5aa5 {
        halt(0x7f39);
    }
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(3, page + 1);
    }
    let words = unsafe { Ptr::from_addr(0xc000).as_u16_array() };
    if words[8132u16] != 0x393c
        || words[16383u16] != 0xc3c3
        || words[DRIVING_LIFE_ENTER_OFFSET - 1] != 0x1d47
        || words[DRIVING_LIFE_QUEUE_END] != 0x74d1
    {
        halt(0x7f3a);
    }
    unsafe {
        write_dseg_page(3, DRIVING_DYNAMIC_WORLD.first_sector() >> 8);
    }
    if words[16340u16] != 0x6a39 || words[16383u16] != 0x93a6 {
        halt(0x7f60);
    }
    unsafe {
        write_dseg_page(3, saved);
    }
}
