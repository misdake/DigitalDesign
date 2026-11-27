//! Source-loaded trajectories: only independent controls enter each persistent tick.
#![link_csegs(
    default = "app",
    app = 7,
    checks = 6,
    world = 2,
    parents = 5,
    prefix = 4,
    pickups = 3
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
static DRIVING_ACTORS: StaticBuffer = StaticBuffer::new(14208, 32768);
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
mod ground;
mod ground_response;
use frame_check::*;
mod blast;
mod car_effects;
mod car_motion;
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
mod lifecycle;
mod mesh_break;
mod mesh_catalog;
mod mesh_storage;
mod mine;
mod object_query;
mod ordered;
mod parent_check;
mod particle_sprite;
mod pickup_check;
mod pickup_light;
mod player_check;
mod player_control;
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
mod startup_check;
mod tank_hit;
mod temporary;
mod track_effects;
mod train;
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
            Ptr::from_addr(
                0xc000 + complete_check::CAR_WORD + car.owner * complete_check::CAR_WORDS,
            )
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
    let previous2 = read_dseg_page(2);
    let previous3 = read_dseg_page(3);
    let page = DRIVING_WORLD.first_sector() >> 8;
    if page >= 255 || page & 1 != 0 {
        halt(0x7f37);
    }
    unsafe {
        write_dseg_page(2, page);
        write_dseg_page(3, page + 1);
    }
    let colliders = unsafe { Ptr::from_addr(0x8000).as_u16_array() };
    let properties = unsafe { Ptr::from_addr(0xc000).as_u16_array() };
    unsafe {
        write_dseg_page(3, DRIVING_DYNAMIC_WORLD.first_sector() >> 8);
    }
    let mut dynamic = properties;
    dynamic[16340u16] = 0x6a39;
    dynamic[16383u16] = 0x93a6;
    unsafe {
        write_dseg_page(3, page + 1);
    }
    let memberships = at(properties, 3840);
    let ranges = at(properties, 7936);
    let origin = at(properties, 8128);
    let deformables: Buf<u16, 20> = Buf::new([0; 20]);
    let mut words = colliders;
    words[14400u16] = 0xa55a;
    words[16383u16] = 0x5aa5;
    words = properties;
    words[8132u16] = 0x393c;
    words[16383u16] = 0xc3c3;
    let mut ranges = ranges;
    let mut i = 0u16;
    while i < 64 {
        ranges[i * 3] = i * 64;
        ranges[i * 3 + 2] = 32;
        i += 1;
    }
    let mut scenery = scene_world::World {
        colliders: colliders,
        properties: properties,
        count: 240,
        fixed_count: 240,
        dynamic_page: DRIVING_DYNAMIC_WORLD.first_sector() >> 8,
        dynamic_capacity: scene_world::DYNAMIC_ROWS,
        grid: spatial::Grid {
            page: page + 1,
            origin: origin,
            shift_x: 0,
            shift_z: 0,
            items: memberships,
            ranges: ranges,
        },
        deformables: ordered::List {
            items: deformables.as_array(),
            count: 0,
            capacity: DRIVING_DEFORM_COUNT,
        },
    };
    check_grid_residency(page + 1);
    if !scene.valid || scene.load_world(&mut scenery) != 0 || scene.header[9u16] != 7 {
        halt(0x7f0c);
    }
    run_cases(&c, &mut scenery);
    check_world_guards();
    unsafe {
        write_dseg_page(3, previous3);
        write_dseg_page(2, previous2);
    }
    mark(4);
    halt(0x600d);
}

// This table is needed only while reserving the level's motion layout.
#[link_cseg = "checks"]
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

#[link_cseg = "checks"]
fn run_cases(c: &driving::Config, scenery: &mut scene_world::World) {
    let finish_routes = finish_world::load(scenery);
    let storage: DrivingStorage = driving_storage();
    let cfg = c.spring;
    let directory: Buf<u16, 64> = Buf::new([65535; 64]);
    let dir = directory.as_array();
    read(dir, DRIVING_DIRECTORY, fix32::zero(), 64);
    let binding: Buf<u16, 26> = Buf::new([0; 26]);
    read(binding.as_array(), DRIVING_BINDING, fix32::zero(), 26);
    let resource_ids: Buf<u16, 10> = Buf::new([0; 10]);
    let catalog_scratch: Buf<u16, 8> = Buf::new([0; 8]);
    let metadata: Buf<u16, 24> = Buf::new([0; 24]);
    let meta = metadata.as_array();
    let controls: Buf<u16, 60> = Buf::new([0; 60]);
    let peer_controls: Buf<u16, 60> = Buf::new([0; 60]);
    let record = controls.as_array();
    let gun = storage.gun.as_array();
    let body = unsafe { Ptr::from_addr(0xc000 + complete_check::BODY_WORD).as_u16_array() };
    let states: Buf<u16, 798> = Buf::new([0; 798]);
    let state = states.as_array();
    let events: Buf<u16, 3> = Buf::new([0; 3]);
    let incoming = read_dseg_page(3);
    let empty_records: Buf<deformation::Record, 1> = Buf::new([deformation::record_zero()]);
    let mut surface = deformation::disabled(cfg, empty_records.as_array());
    let live = DRIVING_LIVE.as_array();
    if !load_live(live, storage.records.as_array(), DRIVING_SCENERY) {
        halt(0x7f11);
    }
    let components = scenery_contact::Components {
        records: storage.records.as_array(),
        state: live_section(live, 1),
        config: live_section(live, 2),
        fall_class: attitude::get(at(live, 32), 0),
        minimum_turn: attitude::get(at(live, 34), 0),
        maximum_turn: attitude::get(at(live, 36), 0),
        work_limit: fix32::from_words(34464, 1),
        tables: scenery_contact::Tables {
            sine: c.sine,
            angle: c.angle,
        },
    };
    let deform_data_ptr = try_malloc(DRIVING_DEFORM_WORDS);
    if deform_data_ptr.addr() == 0 {
        halt(0x7f82);
    }
    let deform_data = unsafe { deform_data_ptr.as_u16_array() };
    let mut deform = unsafe {
        storage
            .surface_views
            .as_array()
            .as_ptr()
            .as_array::<deform_contact::Component>()
    };
    let stride =
        view_of(&mut deform[1u16]).as_ptr().addr() - view_of(&mut deform[0u16]).as_ptr().addr();
    if stride == 0 || stride > 40 {
        halt(0x7f82);
    }
    let mut case = 0u16;
    let mut serial = 0u16;
    while case < DRIVING_CASES {
        let active_scene = scene_query::Scene::begin(driving_scene(case));
        if !active_scene.valid {
            halt(0x7f63);
        }
        let scene = &active_scene;
        read(meta, DRIVING_CASE_META, fix32::from_words(case * 24, 0), 24);
        let present: Buf<u16, 3> = Buf::new([meta[5u16], meta[6u16], 0]);
        let mut attitude_config = c.attitude;
        attitude_config[4u16] = driving_orthogonal(case) as u16;
        if meta[21u16] != 0 {
            if !load_live(
                live,
                storage.records.as_array(),
                driving_initial_scenery(case),
            ) {
                halt(0x7f11);
            }
            scenery.count = scenery.fixed_count;
            if scene.load_world(scenery) != 0 {
                halt(0x7f12);
            }
        }
        read(
            resource_ids.as_array(),
            driving_deform_ids(case),
            fix32::zero(),
            DRIVING_DEFORM_COUNT,
        );
        read(
            deform_data,
            DRIVING_DEFORM,
            fix32::zero(),
            DRIVING_DEFORM_WORDS,
        );
        let mut flag_cursor = DRIVING_DEFORM_COUNT * 40;
        let mut surface_index = 0u16;
        while surface_index < DRIVING_DEFORM_COUNT {
            let row = at(deform_data, (surface_index * 40) as i16);
            if row[0u16] >= scenery.fixed_count
                || row[3u16] != 0
                || row[4u16] != flag_cursor
                || row[1u16] != row[2u16]
                || flag_cursor > DRIVING_DEFORM_WORDS
                || row[2u16] > DRIVING_DEFORM_WORDS - flag_cursor
                || row[38u16] != 0
            {
                halt(0x7f82);
            }
            flag_cursor += row[2u16];
            deform[surface_index] = deform_contact::component(
                deform_data,
                scenery,
                surface_index,
                empty_records.as_array(),
            );
            surface_index += 1;
        }
        if flag_cursor != DRIVING_DEFORM_WORDS {
            halt(0x7f82);
        }
        let catalog = mesh_catalog::load(
            driving_catalog(case),
            storage.catalog.as_array(),
            DRIVING_CATALOG_WORDS,
            catalog_scratch.as_array(),
        );

        let cars: Buf<driving::Car, 3> = Buf::new([
            car(state, gun, at(cfg, 685), 0),
            car(
                at(state, 266),
                unsafe { gun.as_ptr().add(DRIVING_GUN_WORDS as i16).as_i16_array() },
                at(cfg, 685),
                1,
            ),
            car(
                at(state, 532),
                unsafe {
                    gun.as_ptr()
                        .add((DRIVING_GUN_WORDS * 2) as i16)
                        .as_i16_array()
                },
                at(cfg, 685),
                2,
            ),
        ]);
        let mut actors = cars.as_array();
        let auxiliary: Buf<u16, 108> = Buf::new([0; 108]);
        let aux = auxiliary.as_array();
        let current_slot: Buf<u16, 2> = Buf::new([0; 2]);
        copy(current_slot.as_array(), at(meta, 16), 2);
        let no_actors: Buf<scene_parent::Actor, 0> = Buf::new([]);
        let mut state = scene_parent::State {
            lifetime: load_life(
                scenery,
                storage.identity.as_array(),
                storage.lifetime_lists.as_array(),
                storage.records.as_array(),
            ),
            runtime: live_runtime(live, current_slot.as_array(), unsafe {
                Ptr::from_addr(0).as_u16_array()
            }),
            resources: catalog.store,
            map: start_case(actors, aux, case, dir),
            actors: no_actors.as_array(),
            actor_count: 0,
            cars: actors,
            car_count: 2,
            slots: at(meta, 7),
            symbols: unsafe { Ptr::from_addr(0).as_u16_array() },
            models: catalog.models,
        };
        state.runtime.seed = unsafe { view_of(&mut state.map.seed).as_ptr().as_u16_array() };
        let parent_data = at(scenery.colliders, 14402);
        read(
            parent_data,
            driving_parents(case),
            fix32::zero(),
            DRIVING_PARENT_WORDS,
        );
        if parent_data[0u16] != 0x4150
            || parent_data[1u16] != 1
            || parent_data[2u16] != 13
            || parent_data[3u16] != 26
        {
            halt(0x7f45);
        }
        let motion_page = DRIVING_MOTION.first_sector() >> 8;
        let mut bank = scene_fragments::MotionBank {
            page: motion_page,
            used: 0,
        };
        load_motion_layout(&mut bank);
        let debris: Buf<shatter::State, 6> = Buf::new([
            shatter::empty(),
            shatter::empty(),
            shatter::empty(),
            shatter::empty(),
            shatter::empty(),
            shatter::empty(),
        ]);
        bank.used = debris_catalog::load(
            DRIVING_DEBRIS,
            motion_page,
            bank.used,
            storage.debris_directory.as_array(),
            at(scenery.colliders, 14748),
            debris.as_array(),
        );
        if DRIVING_SYMBOL_WORDS > 1449 {
            halt(0x7f4c);
        }
        let symbols = at(scenery.colliders, 14934);
        read(
            symbols,
            DRIVING_SYMBOLS,
            fix32::zero(),
            DRIVING_SYMBOL_WORDS,
        );
        state.symbols = symbols;
        let car_bindings: Buf<u16, 57> = Buf::new([0; 57]);
        read(
            car_bindings.as_array(),
            DRIVING_CAR_BINDINGS,
            fmul_lo32(fix32::from_words(case, 0), fix32::from_words(38, 0)),
            38,
        );
        let mut car_index = 0u16;
        while car_index < 3 {
            let row = at(car_bindings.as_array(), (car_index * 19) as i16);
            let car = &mut actors[car_index];
            car.entity = attitude::get(row, 0);
            car.slot = row[2u16];
            car.player = row[3u16] != 0;
            car.visible = row[4u16] != 0;
            if car_index == 2 {
                car.entity = integer::from_i16(-1);
                car.profile = 0;
            }
            car.blast = at(row, 5);
            car.marker = at(aux, (car_index * 36 + 32) as i16);
            car_index += 1;
        }
        if !scene_meshes::use_arena(
            &mut state.resources,
            motion_page,
            bank.used,
            16384 - bank.used,
        ) {
            halt(0x7f47);
        }
        if case == 0 {
            check_debris_bank(&state.resources, bank.used);
        }
        let parents: Buf<scene_parent::Actor, 13> = Buf::new([
            scene_auxiliaries::actor(
                at(parent_data, 8),
                &components,
                scenery,
                &state.resources,
                motion_page,
            ),
            scene_auxiliaries::actor(
                at(parent_data, 34),
                &components,
                scenery,
                &state.resources,
                motion_page,
            ),
            scene_auxiliaries::actor(
                at(parent_data, 60),
                &components,
                scenery,
                &state.resources,
                motion_page,
            ),
            scene_auxiliaries::actor(
                at(parent_data, 86),
                &components,
                scenery,
                &state.resources,
                motion_page,
            ),
            scene_auxiliaries::actor(
                at(parent_data, 112),
                &components,
                scenery,
                &state.resources,
                motion_page,
            ),
            scene_auxiliaries::actor(
                at(parent_data, 138),
                &components,
                scenery,
                &state.resources,
                motion_page,
            ),
            scene_auxiliaries::actor(
                at(parent_data, 164),
                &components,
                scenery,
                &state.resources,
                motion_page,
            ),
            scene_auxiliaries::actor(
                at(parent_data, 190),
                &components,
                scenery,
                &state.resources,
                motion_page,
            ),
            scene_auxiliaries::actor(
                at(parent_data, 216),
                &components,
                scenery,
                &state.resources,
                motion_page,
            ),
            scene_auxiliaries::actor(
                at(parent_data, 242),
                &components,
                scenery,
                &state.resources,
                motion_page,
            ),
            scene_auxiliaries::actor(
                at(parent_data, 268),
                &components,
                scenery,
                &state.resources,
                motion_page,
            ),
            scene_auxiliaries::actor(
                at(parent_data, 294),
                &components,
                scenery,
                &state.resources,
                motion_page,
            ),
            scene_auxiliaries::actor(
                at(parent_data, 320),
                &components,
                scenery,
                &state.resources,
                motion_page,
            ),
        ]);
        let mut parent_actors = parents.as_array();
        state.actors = parent_actors;
        state.actor_count = 13;
        let mut attached_offset = 1 + symbols[0u16] * 4;
        let mut parent_index = 0u16;
        while parent_index < 13 {
            let actor = &mut parent_actors[parent_index];
            let row = actor.definition;
            actor.attached_count = row[25u16];
            actor.attached = at(symbols, attached_offset as i16);
            if actor.attached_count > (DRIVING_SYMBOL_WORDS - attached_offset) / 3 {
                halt(0x7f4c);
            }
            attached_offset += actor.attached_count * 3;
            let mut d = 0u16;
            while d < 6 {
                if storage.debris_directory.as_array()[d * 10] == actor.fragments.id {
                    actor.debris = view_of(&mut debris.as_array()[d]);
                    break;
                }
                d += 1;
            }
            parent_index += 1;
        }
        if attached_offset != DRIVING_SYMBOL_WORDS {
            halt(0x7f4c);
        }
        let owner = view_of(&mut state.resources);
        let mut surface_index = 0u16;
        while surface_index < DRIVING_DEFORM_COUNT {
            let c = &mut deform[surface_index];
            if resource_ids[surface_index] >= state.resources.count {
                halt(0x7f42);
            }
            c.resources = owner;
            c.resource = resource_ids[surface_index];
            let vertices = c.mesh.vertex_count;
            if !deform_contact::refresh(c, false) || c.mesh.vertex_count != vertices {
                halt(0x7f42);
            }
            surface_index += 1;
        }

        let geometry: Buf<object_query::Tank, 3> = Buf::new([
            object_query::tank_zero(),
            object_query::tank_zero(),
            object_query::tank_zero(),
        ]);
        let mut geo = geometry.as_array();
        let lines = unsafe { Ptr::from_addr(0x8000 + IMPACT_LINES_WORD).as_u16_array() };
        let tail: Buf<u16, 16> = Buf::new([0; 16]);
        let batches =
            unsafe { Ptr::from_addr(0x8000 + IMPACT_BATCHES_WORD).as_array::<impact::Batch>() };
        let mut impacts = impact::State {
            page: DRIVING_ACTORS.first_sector() >> 8,
            batches: batches,
            next: fix32::zero(),
        };
        let batch_page = impact::select(&impacts);
        let mut batch_rows = batches;
        let stride = view_of(&mut batch_rows[1u16]).as_ptr().addr() - batches.as_ptr().addr();
        if IMPACT_LINES_WORD <= 2937
            || IMPACT_LINES_WORD + IMPACT_LINE_WORDS >= IMPACT_BATCHES_WORD - 1
            || IMPACT_GUARD_WORD >= DRIVING_ACTORS.sector_count() * 64
            || stride == 0
            || stride > (IMPACT_GUARD_WORD - IMPACT_BATCHES_WORD) / 4
        {
            halt(0x7f5a);
        }
        let mut batch = 0u16;
        while batch < 4 {
            batch_rows[batch] = impact::batch_zero();
            batch += 1;
        }
        let mut guards = unsafe { Ptr::from_addr(0x8000).as_u16_array() };
        guards[IMPACT_LINES_WORD - 1] = 0x1a4b;
        guards[IMPACT_LINES_WORD + IMPACT_LINE_WORDS] = 0x4b1a;
        guards[IMPACT_BATCHES_WORD - 1] = 0x5b6c;
        guards[IMPACT_GUARD_WORD] = 0x6c5b;
        effect_pool::restore(batch_page);
        let offsets: Buf<u16, 12> = Buf::new([0, 0, 0, 0, 2000, 0, 0, 0, 0, 0, 63536, 65535]);
        let mut effects = impact::Context {
            frame: fix32::zero(),
            seed: state.runtime.seed,
            pool: effect_pool::Pool {
                page: DRIVING_ACTORS.first_sector() >> 8,
                lines: lines,
                count: fix32::zero(),
                overflow: effect_pool::line_zero(),
                exhausted: tail.as_array(),
            },
            sine: c.sine,
            angle: c.angle,
            forward_offset: offsets.as_array(),
            reverse_offset: at(offsets.as_array(), 6),
        };
        let line_page = effect_pool::select(&effects.pool);
        let mut line_data = lines;
        let mut line_word = 0u16;
        while line_word < IMPACT_LINE_WORDS {
            line_data[line_word] = 0;
            line_word += 1;
        }
        effect_pool::reset(&mut effects.pool);
        effect_pool::restore(line_page);
        // Prefix hot rows occupy the fixed collision page's remaining tail.
        // Boost scalars and auxiliary contacts stay in the existing car owner.
        let prefix_data = at(scenery.colliders, (14934 + DRIVING_SYMBOL_WORDS) as i16);
        if 14934 + DRIVING_SYMBOL_WORDS + 238 + 780 + 63 > 16383 {
            halt(0x7f4f);
        }
        let prefix_cores: Buf<u16, 150> = Buf::new([0; 150]);
        let cores = prefix_cores.as_array();
        let prefix_config: Buf<u16, 12> = Buf::new([0; 12]);
        if driving_has_prefix(case) {
            if driving_has_startup(case) {
                read(
                    at(prefix_data, 100),
                    driving_prefix_initial(case),
                    fix32::from_words(100, 0),
                    138,
                );
            } else {
                read(cores, driving_prefix_initial(case), fix32::zero(), 100);
                read(
                    at(prefix_data, 100),
                    driving_prefix_initial(case),
                    fix32::from_words(100, 0),
                    138,
                );
            }
            read(
                prefix_config.as_array(),
                DRIVING_PREFIX_CONFIG,
                fix32::zero(),
                12,
            );
        }
        let prefixes: Buf<prefix::State, 3> = Buf::new([
            prefix::State {
                core: cores,
                spin: at(cores, 20),
                boost: actors[0u16].boost,
                boost_control: at(cores, 38),
                auxiliary: actors[0u16].ground.auxiliary.as_array(),
            },
            prefix::State {
                core: at(cores, 50),
                spin: at(cores, 70),
                boost: actors[1u16].boost,
                boost_control: at(cores, 88),
                auxiliary: actors[1u16].ground.auxiliary.as_array(),
            },
            prefix::State {
                core: at(cores, 100),
                spin: at(cores, 120),
                boost: actors[2u16].boost,
                boost_control: at(cores, 138),
                auxiliary: actors[2u16].ground.auxiliary.as_array(),
            },
        ]);
        let mut prefix_shared = prefix::Shared {
            view: at(prefix_data, 100),
            camera: at(prefix_data, 130),
            spin: at(prefix_data, 136),
            boost: at(prefix_data, 138),
            temporary: at(prefix_data, 156),
            pool: view_of(&mut effects.pool),
            seed: state.runtime.seed,
            slot: state.runtime.slot,
        };
        if driving_starts_spin(case) {
            spin::start(
                prefixes.as_array()[0u16].spin,
                actors[0u16].player,
                fix32::zero(),
                prefix_shared.spin,
                state.runtime.seed,
            );
        }
        let prefix_camera: Buf<u16, 24> = Buf::new([
            0, 0, 0, 0, 0, 0, 4096, 0, 0, 0, 0, 0, 0, 0, 4096, 0, 0, 0, 0, 0, 0, 0, 4096, 0,
        ]);
        let mut prefix_offset = fix32::zero();
        let mut pickups = load_pickups(
            case,
            scenery,
            at(prefix_data, 238),
            motion_page,
            view_of(&mut effects.pool),
        );
        pickups.sine = c.sine;
        let mut complete = complete_check::load(
            case,
            &mut pickups,
            view_of(&mut impacts),
            aux,
            view_of(&mut prefix_shared),
        );
        if complete_check::BODY_WORD + complete_check::CAR_CAPACITY * DRIVING_MESH_WORDS
            >= complete_check::BODY_END_WORD
        {
            halt(0x7f5a);
        }
        let saved = read_dseg_page(3);
        unsafe {
            write_dseg_page(3, complete.page);
        }
        pickup_check::read_pickup_stage(body, DRIVING_MESH, 0, DRIVING_MESH_WORDS);
        copy(
            at(body, DRIVING_MESH_WORDS as i16),
            body,
            DRIVING_MESH_WORDS,
        );
        let mut reserved = at(body, (DRIVING_MESH_WORDS * 2) as i16);
        let mut word = 0u16;
        while word < DRIVING_MESH_WORDS {
            reserved[word] = 0;
            word += 1;
        }
        let mut guard =
            unsafe { Ptr::from_addr(0xc000 + complete_check::BODY_END_WORD - 1).as_u16_array() };
        guard[0u16] = 0x38b1;
        unsafe {
            write_dseg_page(3, saved);
        }
        complete.prefixes = cores;
        if complete.enabled {
            state.map.shield_page = complete.page;
            state.map.shield = complete.shared.shield;
        }
        if complete.enabled {
            prefix_shared.temporary = complete.shared.temporary.core;
        }
        if driving_has_startup(case) {
            startup_check::construct(
                actors,
                prefixes.as_array(),
                c,
                &complete,
                driving_startup_models(case),
            );
        }
        let route_fields: Buf<u16, 90> = Buf::new([0; 90]);
        let mine_fields: Buf<u16, 66> = Buf::new([0; 66]);
        let explosion_fields: Buf<u16, 100> = Buf::new([0; 100]);
        let mut player = player_world::load(
            case,
            scenery,
            route_fields.as_array(),
            mine_fields.as_array(),
            explosion_fields.as_array(),
        );
        player.present = present.as_array();
        player.sine = c.sine;
        player.angle = c.angle;
        player.drive = c.drive;
        player.body = body;
        player.auxiliary = aux;
        player.prefixes = cores;
        complete.hit_fields = player.hit_fields.as_array();
        player.surfaces = deform;
        player.pickup_bindings = pickups.bindings;
        player.inventory = at(prefix_data, 238 + 780 + 51);
        complete.racers = player.fields;
        if driving_has_player(case) {
            read(
                player.inventory,
                driving_player(case),
                fix32::from_words(62, 0),
                8,
            );
        }
        player.weapons.directory = at(storage.records.as_array(), 1);
        player.weapons.quads = view_of(&mut pickups.quads);
        player.weapons.lines = view_of(&mut effects.pool);
        let mut complete_offset = fix32::zero();
        let mut pickup_offset = fix32::zero();

        if driving_has_cold_scene(case) {
            scene_boot::construct(scene, &mut state, scenery, &mut player, &complete);
            let expected = ExpectedScene {
                asset: driving_scene_boot_expected(),
                offset: fix32::zero(),
            };
            check_live(
                live,
                &state.lifetime,
                &state.runtime,
                scenery,
                scene,
                &expected,
            );
            check_debris(&state, driving_scene_boot_debris(), fix32::zero());
            finish_world::check(&finish_routes, &state, scenery);
            let offset = player.expected_offset;
            let clock = player.clock_enabled;
            player.expected_offset = fix32::zero();
            player.clock_enabled = false;
            player_world::check(&mut player, case, scenery, &state.resources);
            player.expected_offset = offset;
            player.clock_enabled = clock;
        }

        let live_asset = driving_scenery(case);
        let mut live_offset = fix32::zero();
        let mut parent_offset = fix32::zero();
        let mut debris_offset = fix32::zero();
        let mut frame = 0u16;
        let frame_asset = driving_frames(case);
        let surface_asset = driving_surfaces(case);
        while frame < meta[20u16] + driving_finish_frames() {
            let finish_tick = frame >= meta[20u16];
            let first_finish = frame == meta[20u16];
            let direct_finish = first_finish && !driving_finish_world();
            unsafe {
                addr_of(&DRIVING_FRAME).write(0, serial);
            }
            if !finish_tick {
                read(
                    record,
                    frame_asset,
                    fmul_lo32(
                        fix32::from_words(frame, 0),
                        fix32::from_words(DRIVING_FRAME_WORDS, 0),
                    ),
                    60,
                );
            }
            let data = record;
            if !finish_tick {
                if driving_has_camera_input(case) {
                    let saved = read_dseg_page(3);
                    unsafe {
                        write_dseg_page(3, player.weapons.effect_page);
                    }
                    pickup_check::read_pickup_stage(
                        ptr_camera(),
                        driving_camera_input(case),
                        frame * 6,
                        6,
                    );
                    unsafe {
                        write_dseg_page(3, saved);
                    }
                }
                if player_world::clock(&state, &mut player, data) != fix32::zero() {
                    halt(0x7f76);
                }
                if complete.enabled {
                    let saved = read_dseg_page(3);
                    unsafe {
                        write_dseg_page(3, complete.page);
                    }
                    if driving_world_pass(case) {
                        read(
                            prefix_camera.as_array(),
                            driving_world_view(case),
                            fmul_lo32(fix32::from_words(frame, 0), fix32::from_words(24, 0)),
                            24,
                        );
                        copy(ptr_camera(), prefix_camera.as_array(), 6);
                    } else {
                        copy(prefix_camera.as_array(), ptr_camera(), 6);
                    }
                    unsafe {
                        write_dseg_page(3, saved);
                    }
                }
                if driving_has_world_events(case) {
                    apply_world_event(
                        scene,
                        &mut state,
                        scenery,
                        &mut player,
                        data,
                        driving_world_events(case),
                    );
                }
            } else if first_finish {
                finish_check::setup(&state, &player, &complete, &pickups, case);
                if driving_finish_world() {
                    attitude::put(record, 26, attitude::get(record, 26) + integer::from_i16(1));
                    attitude::put(record, 29, fix32::zero());
                }
            } else {
                let input: Buf<u16, 14> = Buf::new([0; 14]);
                read(
                    input.as_array(),
                    driving_finish_input(),
                    fix32::from_words((frame - meta[20u16] - 1) * 14, 0),
                    14,
                );
                copy(at(actors[0u16].motion, 18), at(input.as_array(), 8), 6);
                if driving_finish_ufo() && frame == meta[20u16] + 1 {
                    let event: Buf<u16, 4> = Buf::new([5, 1, 2, 0]);
                    if weapon_ufos::place(scene, &mut state, scenery, &mut player, event.as_array())
                        != fix32::zero()
                    {
                        halt(0x7f9c);
                    }
                }
                attitude::put(record, 26, attitude::get(input.as_array(), 3));
                attitude::put(record, 29, fix32::zero());
                if driving_finish_ufo() {
                    attitude::put(record, 0, integer::from_i16(112));
                }
                let saved = read_dseg_page(3);
                unsafe {
                    write_dseg_page(3, complete.page);
                }
                copy(ptr_camera(), input.as_array(), 6);
                copy(prefix_camera.as_array(), input.as_array(), 6);
                unsafe {
                    write_dseg_page(3, saved);
                }
            }
            if !direct_finish && finish_world::activate(&mut state, &mut player) != fix32::zero() {
                halt(0x7f9b);
            }
            let own = attitude::get(data, 27).hi_bits();
            if own >= state.car_count {
                halt(0x7f0b);
            }
            if meta[21u16] & 1 != 0 {
                if meta[22u16] != 0 && frame == meta[23u16] {
                    let index = fix32::from_words(meta[22u16] - 1, 0);
                    if index.hi_bits() != 0 || index.lo_bits() >= scenery.count {
                        halt(0x7f30);
                    }
                    let collider: Buf<u16, 60> = Buf::new([0; 60]);
                    if !scene_world::load_collider(scenery, index.lo_bits(), collider.as_array()) {
                        halt(0x7f30);
                    }
                    let entity = attitude::get(collider.as_array(), 0);
                    if lifecycle::queue(
                        &mut state.runtime.queues,
                        &mut state.runtime.ticks,
                        entity,
                        true,
                        state.lifetime.limit,
                    ) != 0
                    {
                        halt(0x7f31);
                    }
                }
            }
            let saved_slot = attitude::get(state.runtime.slot, 0);
            let vehicles: Buf<tank_response::Vehicle, 3> = Buf::new([
                collision_car(&mut actors[0u16], &player, 0, aux),
                collision_car(&mut actors[1u16], &player, 1, at(aux, 36)),
                collision_car(&mut actors[2u16], &player, 2, at(aux, 72)),
            ]);
            let mut peers = tank_response::World {
                geometry: geo,
                vehicles: vehicles.as_array(),
                slots: at(meta, 7),
                present: present.as_array(),
                geometry_count: state.car_count,
                vehicle_count: state.car_count,
                hit: integer::from_i16(-1),
            };
            let mut pass = lifecycle::begin(&state.runtime.ticks, components.work_limit);
            loop {
                let mut data = record;
                let mut own = attitude::get(data, 27).hi_bits();
                if meta[21u16] & 1 != 0 && !direct_finish {
                    let progress = lifecycle::prepare(
                        &mut state.runtime.ticks,
                        &mut pass,
                        components.work_limit,
                    );
                    if progress == 0 {
                        break;
                    }
                    if progress != 1 {
                        halt(0x7f1e);
                    }
                    let kind = entities::handle_kind(&state.lifetime.identities, pass.current);
                    if kind == 1 {
                        let entity = pass.current;
                        if entity.hi_bits() != 0 || entity.lo_bits() >= live[13u16] {
                            halt(0x7f15);
                        }
                        if entities::handle_kind(&state.lifetime.identities, entity) != 1 {
                            halt(0x7f32);
                        }
                        let index = entities::handle(&state.lifetime.identities, entity);
                        if index.hi_bits() != 0 {
                            halt(0x7f33);
                        }
                        let object = index.lo_bits();
                        if object >= scenery.count {
                            halt(0x7f16);
                        }
                        spatial::select(&scenery.grid);
                        let frame = scenery_contact::Frame {
                            object: object,
                            dt: attitude::get(data, 0),
                            number: if driving_has_player(case) {
                                attitude::get(data, 26)
                            } else {
                                fix32::zero()
                            },
                            camera: prefix_camera.as_array(),
                            peer_basis: live_peer(
                                &components,
                                object,
                                actors,
                                &state.lifetime.identities,
                                scenery,
                                state.slots[0u16],
                            ),
                            peer_valid: true,
                        };
                        let selected = scene_auxiliaries::find(parent_data, object);
                        let mut status = fix32::zero();
                        if scene_creation::component_kind(&player.weapons, object) != 0 {
                            status = weapon_world::tick(
                                scene,
                                &mut state,
                                scenery,
                                &mut player,
                                data,
                                entity,
                            );
                        } else if scene_ambient_world::contains(object) {
                            status = scene_ambient_world::tick(
                                scene,
                                &mut state,
                                scenery,
                                &mut player,
                                data,
                                entity,
                            );
                        } else if scene_flock_world::contains(object) {
                            status = scene_flock_world::tick(
                                scene,
                                &mut state,
                                scenery,
                                &mut player,
                                data,
                                entity,
                            );
                        } else if scene_trains::contains(object) {
                            status = scene_trains::tick(
                                scene,
                                &mut state,
                                scenery,
                                &mut player,
                                data,
                                entity,
                            );
                        } else if weapon_emitters::contains(object) {
                            status = weapon_emitters::tick(
                                scene,
                                &mut state,
                                scenery,
                                &mut player,
                                data,
                                entity,
                            );
                        } else if pickup_index(&pickups, object) != 65535 {
                            status = pickup_tick(
                                scene,
                                &mut state,
                                scenery,
                                &components,
                                &pickups,
                                &frame,
                            );
                        } else if selected == 65535 {
                            status = scenery_contact::tick(
                                scene,
                                scenery,
                                &components,
                                &mut state.runtime,
                                &mut state.map,
                                &frame,
                            );
                        } else {
                            let actor = &mut parent_actors[selected];
                            let row = at(components.records, (object * 8) as i16);
                            let result: Buf<u16, 2> = Buf::new([0; 2]);
                            let nested: Buf<u16, 2> = Buf::new([0; 2]);
                            let tick = scene_parent::Frame {
                                auxiliary: scene_fragments::Frame {
                                    entity: entity,
                                    dt: attitude::get(data, 0),
                                    mode: attitude::get(data, 1),
                                    sine: c.sine,
                                    result: nested.as_array(),
                                },
                                result: result.as_array(),
                                group: frame.number,
                                player: at(actors[state.slots[0u16]].motion, 18),
                                has_player: true,
                                peer: frame.peer_basis,
                                has_peer: frame.peer_valid,
                                tuning: at(components.config, row[2u16] as i16),
                                angle: c.angle,
                                inverse_cosine: unsafe {
                                    addr_of(&DRIVING_INVERSE_COSINE).as_i16_array()
                                },
                            };
                            status = scene_parent::tick(
                                scene,
                                &mut state,
                                &mut scenery.grid,
                                actor,
                                &tick,
                            );
                        }
                        if status != fix32::zero() {
                            unsafe {
                                addr_of(&DRIVING_FIELD).write(0, object);
                            }
                            mark(0x4000 | (-status).lo_bits());
                            halt(0x7f17);
                        }
                        lifecycle::resume(&mut pass, &state.runtime.ticks);
                        continue;
                    }
                    let id = entities::handle(&state.lifetime.identities, pass.current);
                    if kind != 2 || id.hi_bits() != 0 || id.lo_bits() >= state.car_count {
                        halt(0x7f13);
                    }
                    own = id.lo_bits();
                    if own != attitude::get(data, 27).hi_bits() || player.finish_stage >= 2 {
                        if !driving_world_pass(case) {
                            halt(0x7f74);
                        }
                        player_world::tank_input(
                            &actors[own],
                            &player,
                            record,
                            peer_controls.as_array(),
                        );
                        data = peer_controls.as_array();
                    }
                    attitude::put(state.runtime.slot, 0, integer::from_i16(12));
                }
                if own == state.slots[0u16]
                    && player.finish_stage == 0
                    && attitude::get(at(player.fields, (own * 30) as i16), 12)
                        == integer::from_i16(3)
                {
                    let entity = finish_world::entity(&state);
                    if entity >= fix32::zero() {
                        if finish_world::transfer(
                            &mut state,
                            &mut player,
                            &mut complete,
                            &mut pickups,
                            own,
                            entity,
                        ) != fix32::zero()
                        {
                            halt(0x7f98);
                        }
                        own = 2;
                        attitude::put(state.runtime.slot, 0, integer::from_i16(12));
                        player_world::tank_input(
                            &actors[own],
                            &player,
                            record,
                            peer_controls.as_array(),
                        );
                        data = peer_controls.as_array();
                        peers.geometry_count = state.car_count;
                        peers.vehicle_count = state.car_count;
                    }
                }
                let mut far_finish = false;
                if own == 2 && player.finish_stage != 0 {
                    let status = finish_driver::before(
                        scene,
                        &mut state,
                        scenery,
                        &mut player,
                        &complete,
                        data,
                    );
                    if status < fix32::zero() {
                        unsafe {
                            addr_of(&DRIVING_FIELD).write(0, (-status).lo_bits());
                        }
                        halt(0x7f99);
                    }
                    far_finish = status == integer::from_i16(1);
                    attitude::put(
                        data,
                        27,
                        fix32::from_words(256 | actors[own].visible as u16, own),
                    );
                }
                let keys = attitude::get(data, 29).lo_bits() & 511;
                let visibility = attitude::get(data, 27).lo_bits();
                if visibility & 256 != 0 {
                    actors[own].visible = visibility & 1 != 0;
                }
                if attitude::get(data, 29).hi_bits() & 2 != 0 {
                    let status = player_world::prepare(
                        scene,
                        &mut state,
                        scenery,
                        &complete,
                        &mut player,
                        data,
                    );
                    if status != fix32::zero() {
                        unsafe {
                            addr_of(&DRIVING_FIELD).write(0, status.lo_bits());
                        }
                        halt(0x7f62);
                    }
                } else if attitude::get(data, 29).hi_bits() & 1 != 0 {
                    if !complete.enabled {
                        halt(0x7f57);
                    }
                    let status = player_check::sample(&complete, &mut actors[own], c, data);
                    if status != fix32::zero() {
                        unsafe {
                            addr_of(&DRIVING_FIELD).write(0, status.lo_bits());
                        }
                        halt(0x7f58);
                    }
                }
                let mut peer_rows = vehicles.as_array();
                let mut peer = 0u16;
                while peer < state.car_count {
                    peer_rows[peer] = collision_car(
                        &mut actors[peer],
                        &player,
                        peer,
                        at(aux, (peer * 36) as i16),
                    );
                    peer += 1;
                }
                if !far_finish {
                    if complete.enabled && !actors[own].visible {
                        let status = complete_check::inactive(
                            &mut complete,
                            &mut actors[own],
                            c,
                            data,
                            &mut state.map,
                            &mut effects,
                        );
                        if status != fix32::zero() {
                            unsafe {
                                addr_of(&DRIVING_FIELD).write(0, status.lo_bits());
                            }
                            halt(0x7f55);
                        }
                    } else {
                        if driving_has_prefix(case) {
                            let prefix_page = read_dseg_page(3);
                            if complete.enabled {
                                unsafe {
                                    write_dseg_page(3, complete.page);
                                }
                                copy(
                                    prefix_camera.as_array(),
                                    unsafe { Ptr::from_addr(0xc000 + 10174).as_u16_array() },
                                    6,
                                );
                            }
                            if complete.enabled {
                                let fields = unsafe {
                                    Ptr::from_addr(0xc000 + complete_check::INPUT_WORD + own * 26)
                                        .as_u16_array()
                                };
                                attitude::put(c.drive, 2, attitude::get(fields, 3));
                            }
                            let step = prefix::Frame {
                                dt: attitude::get(data, 0),
                                number: attitude::get(data, 26),
                                active: attitude::get(data, 3) != fix32::zero(),
                                player: actors[own].player,
                                entity: actors[own].entity,
                                camera: prefix_camera.as_array(),
                                sine: c.sine,
                            };
                            let status = prefix::prepare(
                                &prefixes.as_array()[own],
                                &prefix_shared,
                                &step,
                                prefix_config.as_array(),
                                scene,
                                &state.lifetime.identities,
                            );
                            if complete.enabled {
                                complete_check::publish_prefix(
                                    &actors[own],
                                    c,
                                    &prefixes.as_array()[own],
                                    at(player.hit_fields.as_array(), (own * 24 + 20) as i16),
                                );
                            }
                            unsafe {
                                write_dseg_page(3, prefix_page);
                            }
                            if status != fix32::zero() {
                                halt(0x7f50);
                            }
                            attitude::put(data, 16, attitude::get(actors[own].boost, 3));
                            attitude::put(data, 17, attitude::get(actors[own].boost, 0));
                            attitude::put(data, 18, attitude::get(actors[own].boost, 1));
                        } else {
                            // The raw physics fixture uses the low command word;
                            // its transport owner is outside the boost input.
                            let command = attitude::get(data, 27);
                            attitude::put(data, 27, fix32::from_words(command.lo_bits(), 0));
                            vehicle::boost(actors[own].boost, c.drive, data);
                            attitude::put(data, 27, command);
                        }
                        let gun_before: Buf<u16, 5> = Buf::new([0; 5]);
                        if driving_has_player(case) {
                            let car = &actors[own];
                            let tail = player_tail::State {
                                gun: car.gun,
                                binding: car.gun_binding,
                                fields: unsafe { Ptr::from_addr(0).as_u16_array() },
                            };
                            player_tail::capture(&tail, gun_before.as_array());
                        }
                        mark(2);
                        let actor_aux = at(aux, (own * 36) as i16);
                        let selected = deform_contact::attached(
                            deform,
                            DRIVING_DEFORM_COUNT,
                            scenery,
                            attitude::get(actor_aux, 14),
                        );
                        let mut status = 0u16;
                        if selected == 65535 {
                            if attitude::get(actor_aux, 14) != integer::from_i16(-1) {
                                halt(0x7f0d);
                            }
                            status = driving::contact_and_power(
                                &mut actors[own],
                                c,
                                &mut state.map,
                                data,
                                events.as_array(),
                                &mut surface,
                            );
                        } else {
                            if !deform_contact::refresh(&mut deform[selected], false) {
                                halt(0x7f43);
                            }
                            status = driving::contact_and_power(
                                &mut actors[own],
                                c,
                                &mut state.map,
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
                        if meta[21u16] != 0 {
                            let car = &mut actors[own];
                            let before: Buf<u16, 6> = Buf::new([0; 6]);
                            copy(before.as_array(), car.motion, 6);
                            let v = scenery_contact::Vehicle {
                                entity: car.entity,
                                position: at(car.motion, 6),
                                previous: at(car.motion, 12),
                                base_position: at(car.motion, 18),
                                velocity: car.motion,
                                forward: attitude::get(car.drive, 2),
                                reference: car.attitude,
                                external: car.attitude,
                                has_external: true,
                                counters: at(actor_aux, 30),
                                dt: attitude::get(data, 0),
                                mode: attitude::get(data, 1),
                            };
                            let mut hit = object_query::hit_zero();
                            let mut status =
                                scenery_contact::query(&v, scenery, &components, &mut hit);
                            if status == integer::from_i16(1) {
                                let selected =
                                    scene_auxiliaries::find(parent_data, hit.object.lo_bits());
                                if scene_creation::component_kind(
                                    &player.weapons,
                                    hit.object.lo_bits(),
                                ) == 2
                                {
                                    status = weapon_mines::contact(
                                        scene,
                                        &mut state,
                                        scenery,
                                        &mut player,
                                        data,
                                        hit.object.lo_bits(),
                                    );
                                } else if pickup_index(&pickups, hit.object.lo_bits()) != 65535 {
                                    let call = PickupContact {
                                        vehicle: v,
                                        target: hit.object.lo_bits(),
                                        own: own,
                                        mode: attitude::get(data, 1),
                                        number: if driving_has_player(case) {
                                            attitude::get(data, 26)
                                        } else {
                                            fix32::zero()
                                        },
                                        camera: prefix_camera.as_array(),
                                    };
                                    status = pickup_contact(
                                        scene,
                                        &mut state,
                                        scenery,
                                        &components,
                                        &mut pickups,
                                        &call,
                                    );
                                } else if selected == 65535 {
                                    status = scenery_contact::method(
                                        &v,
                                        scene,
                                        scenery,
                                        &components,
                                        &mut state.runtime,
                                        &hit,
                                    );
                                } else {
                                    let actor = &mut parent_actors[selected];
                                    let class = attitude::get(actor.properties, 0);
                                    status = integer::from_i16(-1);
                                    if class >= fix32::zero()
                                        && class < fix32::from_words(scene.header[4u16], 0)
                                    {
                                        let saved = attitude::get(state.runtime.slot, 0);
                                        attitude::put(state.runtime.slot, 0, integer::from_i16(24));
                                        let method = scene.method(class.lo_bits(), 24);
                                        if method.status == 0 {
                                            status = fix32::zero();
                                            attitude::put(state.runtime.slot, 0, saved);
                                        } else if method.status == 2 {
                                            status = fix32::zero();
                                        } else if method.status == 1 {
                                            let row = at(parent_data, (8 + selected * 26) as i16);
                                            let mut model = scene_meshes::empty_model();
                                            if row[2u16] != 65535 {
                                                model = mesh_catalog::model(
                                                    &state.models,
                                                    row[2u16],
                                                    catalog_scratch.as_array(),
                                                );
                                            }
                                            let result: Buf<u16, 2> = Buf::new([0; 2]);
                                            let nested: Buf<u16, 2> = Buf::new([0; 2]);
                                            let call = scene_parent::Hit {
                                                method: method.id,
                                                auxiliary: scene_fragments::Hit {
                                                    row: at(
                                                        components.records,
                                                        (actor.fragments.id * 8) as i16,
                                                    ),
                                                    limit: components.work_limit,
                                                    powerup: false,
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
                                                current: own,
                                                mode: attitude::get(data, 1),
                                                dt: attitude::get(data, 0),
                                                sine: c.sine,
                                            };
                                            status = scene_parent::hit(
                                                scene,
                                                &mut state,
                                                &mut scenery.grid,
                                                actor,
                                                &call,
                                            );
                                            if status == fix32::zero() {
                                                attitude::put(state.runtime.slot, 0, saved);
                                            }
                                        }
                                    }
                                }
                            }
                            if status != fix32::zero() {
                                halt(0x7f14);
                            }
                            let mut i = 0u16;
                            while i < 3 {
                                attitude::put(
                                    before.as_array(),
                                    i,
                                    attitude::get(car.motion, i)
                                        - attitude::get(before.as_array(), i),
                                );
                                i += 1;
                            }
                            attitude::put(actor_aux, 17, collision::distance(before.as_array()));
                        }
                        effects.frame = attitude::get(data, 26);
                        let mut peer = 0u16;
                        let mut views = vehicles.as_array();
                        while peer < state.car_count {
                            query_car(
                                &mut actors[peer],
                                &mut geo[peer],
                                binding.as_array(),
                                present.as_array(),
                                complete.enabled,
                            );
                            views[peer].impact.forward = attitude::get(actors[peer].drive, 2);
                            peer += 1;
                        }
                        let env = tank_response::Environment {
                            mode: attitude::get(data, 1),
                            dt: attitude::get(data, 0),
                            separation_limit: fix32::zero(),
                        };
                        let status =
                            tank_response::cars(own, &mut peers, &env, &mut impacts, &mut effects);
                        if status != fix32::zero() {
                            unsafe {
                                addr_of(&DRIVING_FIELD).write(0, status.lo_bits());
                            }
                            halt(0x7f0a);
                        }
                        let current = collision_car(&mut actors[own], &player, own, actor_aux);
                        let car = &mut actors[own];
                        let mut contact = deform_contact::Vehicle {
                            component_count: DRIVING_DEFORM_COUNT,
                            position: at(car.motion, 6),
                            base_position: at(car.motion, 18),
                            previous: at(car.motion, 12),
                            velocity: car.motion,
                            material: at(actor_aux, 26),
                            attached: at(actor_aux, 28),
                            accumulator: at(actor_aux, 30),
                            corners: at(car.suspension, 4),
                            corner_stride: 6,
                            player: car.player,
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
                        let status =
                            ground_response::step(&mut response, &mut impacts, &mut effects);
                        if status != 0 {
                            unsafe {
                                addr_of(&DRIVING_FIELD).write(0, status);
                            }
                            halt(0x7f07);
                        }
                        if complete.enabled {
                            let status = complete_check::advance(
                                &mut complete,
                                car,
                                c,
                                data,
                                &mut response.impact,
                                &mut effects,
                            );
                            if driving_has_player(case) {
                                let post = player_world::finish(
                                    &state,
                                    &player,
                                    data,
                                    gun_before.as_array(),
                                    keys,
                                    status == fix32::zero(),
                                );
                                if post != fix32::zero() {
                                    unsafe {
                                        addr_of(&DRIVING_FIELD).write(0, post.lo_bits());
                                    }
                                    halt(0x7f68);
                                }
                            }
                            if status != fix32::zero() {
                                unsafe {
                                    addr_of(&DRIVING_FIELD).write(0, status.lo_bits());
                                }
                                halt(0x7f52);
                            }
                        } else {
                            let status = driving::integrate_and_pose(car, c, data);
                            if status != 0 {
                                unsafe {
                                    addr_of(&DRIVING_FIELD).write(0, status);
                                }
                                halt(0x7f08);
                            }
                            if fade_pickups(
                                &mut pickups,
                                car,
                                attitude::get(data, 26),
                                attitude::get(data, 0),
                            ) != fix32::zero()
                            {
                                halt(0x7f50);
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
                        }
                    }
                }
                if own == 2 && player.finish_stage != 0 {
                    let status = finish_driver::after(&state, scenery, &player, data);
                    if status != fix32::zero() {
                        unsafe {
                            addr_of(&DRIVING_FIELD).write(0, (-status).lo_bits());
                        }
                        halt(0x7f9a);
                    }
                    if player.finish_stage == 1 {
                        finish_world::started(&state, &mut player);
                    }
                }
                if meta[21u16] & 1 != 0 && !direct_finish {
                    attitude::put(state.runtime.slot, 0, saved_slot);
                    lifecycle::resume(&mut pass, &state.runtime.ticks);
                } else {
                    if finish_tick {
                        attitude::put(state.runtime.slot, 0, saved_slot);
                    }
                    break;
                }
            }
            if meta[21u16] & 1 != 0 && !direct_finish {
                if scene_auxiliaries::drain(
                    scene,
                    &mut state,
                    scenery,
                    parent_data,
                    &mut player.weapons,
                ) != fix32::zero()
                {
                    halt(0x7f18);
                }
            }

            mark(3);
            if finish_tick {
                if driving_finish_compare(frame - meta[20u16]) {
                    finish_check::result(
                        &mut state,
                        scenery,
                        &mut player,
                        &complete,
                        case,
                        frame - meta[20u16],
                    );
                }
            } else {
                let mut expected = reference(
                    frame_asset,
                    fmul_lo32(
                        fix32::from_words(frame, 0),
                        fix32::from_words(DRIVING_FRAME_WORDS, 0),
                    ) + fix32::from_words(60, 0),
                    (DRIVING_STATE_VALUES + 1) * 2,
                );
                check_snapshot(
                    actors,
                    aux,
                    &peers,
                    &mut impacts,
                    &mut effects,
                    &mut expected,
                );
                expect_value(
                    attitude::get(current_slot.as_array(), 0),
                    &mut expected,
                    2285,
                );
                if meta[19u16] != 0 {
                    let mut expected = reference(
                        surface_asset,
                        fmul_lo32(
                            fix32::from_words(frame, 0),
                            fix32::from_words(DRIVING_SURFACE_VALUES * 2, 0),
                        ),
                        DRIVING_SURFACE_VALUES * 2,
                    );
                    check_surfaces(deform, &mut expected);
                    check_resource_versions(deform, driving_mesh_versions(case), frame);
                }
                if driving_has_prefix(case) {
                    let mut expected = reference(driving_prefix_frames(case), prefix_offset, 238);
                    if complete.enabled {
                        expect_block(cores, &mut expected, 0, 100);
                        expect_block(at(prefix_data, 100), &mut expected, 100, 56);
                        let saved = read_dseg_page(3);
                        unsafe {
                            write_dseg_page(3, complete.page);
                        }
                        expect_block(complete.shared.temporary.core, &mut expected, 156, 82);
                        unsafe {
                            write_dseg_page(3, saved);
                        }
                    } else {
                        expect_block(cores, &mut expected, 0, 100);
                        expect_block(at(prefix_data, 100), &mut expected, 100, 138);
                    }
                    prefix_offset = prefix_offset + fix32::from_words(238, 0);
                }
                complete_offset = complete_check::check(&complete, case, complete_offset);
                if driving_has_pickup(case) {
                    check_pickups(&pickups, &state, driving_pickup_frames(case), pickup_offset);
                    pickup_offset = pickup_offset + fix32::from_words(10040, 0);
                }
                if driving_world_pass(case) {
                    check_pickup_flights(&pickups, driving_world_flights(case), frame);
                }
                if driving_has_debris(case) {
                    let count = check_debris(&state, driving_debris_frames(case), debris_offset);
                    debris_offset = debris_offset + fix32::from_words(count, 0);
                }
                if driving_has_parents(case) {
                    let count = check_parents(
                        parent_actors,
                        parent_data,
                        &state,
                        driving_parent_frames(case),
                        parent_offset,
                    );
                    parent_offset = parent_offset + fix32::from_words(count, 0);
                }
                if meta[21u16] != 0 {
                    let expected = ExpectedScene {
                        asset: live_asset,
                        offset: live_offset,
                    };
                    let words = check_live(
                        live,
                        &state.lifetime,
                        &state.runtime,
                        scenery,
                        scene,
                        &expected,
                    );
                    live_offset = live_offset + fix32::from_words(words, 0);
                }
                if driving_has_player(case) {
                    player_world::check(&mut player, case, scenery, &state.resources);
                }
            }
            unsafe {
                addr_of(&DRIVING_CHECKS).write(0, DRIVING_CHECKS + 1);
            }
            frame += 1;
            serial += 1;
        }
        if driving_finish_transfer(case) {
            finish_check::qualify(
                &mut state,
                scenery,
                &mut player,
                &mut complete,
                &mut pickups,
                case,
            );
        }
        scene_meshes::finish(&mut state.resources);
        state.map.terrain.finish();
        check_world_guards();
        if read_dseg_page(3) != incoming {
            halt(0x7f06);
        }
        case += 1;
    }
    free(deform_data_ptr);
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
