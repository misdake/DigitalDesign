//! One complete level instance over caller-owned configuration and mapped storage.
#![link_cseg = "frame"]
use super::*;

pub fn run(
    c: &driving::Config,
    scenery: &mut scene_world::World,
    asset_set: u16,
    mut serial: u16,
) -> u16 {
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
    let body = unsafe { Ptr::from_addr(0xc000 + car_runtime::BODY_WORD).as_u16_array() };
    let states: Buf<u16, 798> = Buf::new([0; 798]);
    let state = states.as_array();
    let events: Buf<u16, 3> = Buf::new([0; 3]);
    let incoming = read_dseg_page(3);
    let empty_records: Buf<deformation::Record, 1> = Buf::new([deformation::record_zero()]);
    let mut surface = deformation::disabled(cfg, empty_records.as_array());
    let live_storage = driving_live_storage();
    let live = live_storage.as_array();
    if !load_live(live, storage.records.as_array(), DRIVING_SCENERY) {
        halt(0x7f11);
    }
    let mut components = scenery_contact::Components {
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

    let active_scene = scene_query::Scene::begin(driving_scene(asset_set));
    if !active_scene.valid {
        halt(0x7f63);
    }
    let scene = &active_scene;
    read(
        meta,
        DRIVING_CASE_META,
        fix32::from_words(asset_set * 24, 0),
        24,
    );
    let present: Buf<u16, 3> = Buf::new([meta[5u16], meta[6u16], 0]);
    let mut attitude_config = c.attitude;
    attitude_config[4u16] = driving_orthogonal(asset_set) as u16;
    if meta[21u16] != 0 {
        if !load_live(
            live,
            storage.records.as_array(),
            driving_initial_scenery(asset_set),
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
        driving_deform_ids(asset_set),
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
    if bank.used < finish_world::LIMIT {
        halt(0x7f47);
    }
    let catalog = mesh_catalog::load(
        driving_catalog(asset_set),
        storage.catalog.as_array(),
        DRIVING_CATALOG_WORDS,
        catalog_scratch.as_array(),
        fix32::from_words(DRIVING_ACTORS.first_sector() >> 8, bank.used),
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
        map: start_case(actors, aux, asset_set, dir),
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
        driving_parents(asset_set),
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
        fmul_lo32(fix32::from_words(asset_set, 0), fix32::from_words(38, 0)),
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
    if !scene_meshes::open_arena_page(&mut state.resources, motion_page) {
        halt(0x7f47);
    }
    if DRIVING_VERIFY != 0 && asset_set == 0 {
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
    if driving_has_prefix(asset_set) {
        if driving_has_startup(asset_set) {
            read(
                at(prefix_data, 100),
                driving_prefix_initial(asset_set),
                fix32::from_words(100, 0),
                138,
            );
        } else {
            read(cores, driving_prefix_initial(asset_set), fix32::zero(), 100);
            read(
                at(prefix_data, 100),
                driving_prefix_initial(asset_set),
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
    if driving_starts_spin(asset_set) {
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
        asset_set,
        scenery,
        at(prefix_data, 238),
        motion_page,
        view_of(&mut effects.pool),
    );
    pickups.sine = c.sine;
    let mut complete = complete_check::load(
        asset_set,
        &mut pickups,
        view_of(&mut impacts),
        aux,
        view_of(&mut prefix_shared),
    );
    if car_runtime::BODY_WORD + car_runtime::CAR_CAPACITY * DRIVING_MESH_WORDS
        >= car_runtime::BODY_END_WORD
    {
        halt(0x7f5a);
    }
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(3, complete.page);
    }
    read_mapped(body, DRIVING_MESH, 0, DRIVING_MESH_WORDS);
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
        unsafe { Ptr::from_addr(0xc000 + car_runtime::BODY_END_WORD - 1).as_u16_array() };
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
    if driving_has_startup(asset_set) {
        let models = vehicle_boot::Resources {
            models: driving_startup_models(asset_set),
            actors_page: DRIVING_ACTORS.first_sector() >> 8,
            actors_words: DRIVING_ACTORS.sector_count() * 64,
            gun_words: DRIVING_GUN_WORDS,
            count: 2,
        };
        vehicle_boot::construct(actors, prefixes.as_array(), c, &complete, &models);
    }
    let route_fields: Buf<u16, 90> = Buf::new([0; 90]);
    let mine_fields: Buf<u16, 66> = Buf::new([0; 66]);
    let explosion_fields: Buf<u16, 100> = Buf::new([0; 100]);
    let mut player = player_world::load(
        asset_set,
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
    if driving_has_player(asset_set) {
        read(
            player.inventory,
            driving_player(asset_set),
            fix32::from_words(62, 0),
            8,
        );
    }
    player.weapons.directory = at(storage.records.as_array(), 1);
    player.weapons.quads = view_of(&mut pickups.quads);
    player.weapons.lines = view_of(&mut effects.pool);
    let mut complete_offset = fix32::zero();
    let mut pickup_offset = fix32::zero();

    if driving_has_cold_scene(asset_set) {
        let resources = scene_boot::Resources {
            scene: driving_scene_boot(),
            tilts: driving_scene_boot_tilts(),
            actors_page: DRIVING_ACTORS.first_sector() >> 8,
            actors_words: DRIVING_ACTORS.sector_count() * 64,
            surface_count: DRIVING_DEFORM_COUNT,
            inverse_cosine: unsafe { addr_of(&DRIVING_INVERSE_COSINE).as_i16_array() },
        };
        scene_boot::construct(
            scene,
            &mut state,
            scenery,
            &mut player,
            &complete,
            &resources,
        );
        if DRIVING_VERIFY != 0 {
            let camera_page = read_dseg_page(3);
            unsafe {
                write_dseg_page(3, resources.actors_page);
            }
            let mut expected_camera = reference(driving_scene_boot_camera(), fix32::zero(), 56);
            expect_block(
                unsafe { Ptr::from_addr(0xc000 + 2880).as_u16_array() },
                &mut expected_camera,
                0,
                56,
            );
            unsafe {
                write_dseg_page(3, camera_page);
            }
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
            player_world::check(&mut player, asset_set, scenery, &state.resources);
            player.expected_offset = offset;
            player.clock_enabled = clock;
        }
    }

    let mut output = frame_output::Renderer::new(1024);
    let mut runtime = game_update::Runtime {
        parents: view_of(&mut state),
        players: view_of(&mut player),
        pickups: view_of(&mut pickups),
        vehicles: view_of(&mut complete),
        impacts: view_of(&mut impacts),
        effects: view_of(&mut effects),
        surface: view_of(&mut surface),
        prefix_shared: view_of(&mut prefix_shared),
        components: view_of(&mut components),
        parent_data,
        deform,
        camera: prefix_camera.as_array(),
        prefix_config: prefix_config.as_array(),
        prefixes: prefixes.as_array(),
        events: events.as_array(),
        binding: binding.as_array(),
        peer_controls: peer_controls.as_array(),
        catalog_scratch: catalog_scratch.as_array(),
        inverse_cosine: unsafe { addr_of(&DRIVING_INVERSE_COSINE).as_i16_array() },
        entity_limit: live[13u16],
        deform_count: DRIVING_DEFORM_COUNT,
        scenery_enabled: meta[21u16] != 0,
        deformation_enabled: meta[18u16] != 0,
        player_enabled: driving_has_player(asset_set),
        prefix_enabled: driving_has_prefix(asset_set),
        multiple_vehicles: driving_world_pass(asset_set),
    };

    let live_asset = driving_scenery(asset_set);
    let mut live_offset = fix32::zero();
    let mut parent_offset = fix32::zero();
    let mut debris_offset = fix32::zero();
    let mut frame = 0u16;
    let frame_asset = driving_frames(asset_set);
    let surface_asset = driving_surfaces(asset_set);
    let mut frames = meta[20u16];
    if DRIVING_VERIFY != 0 {
        frames += driving_finish_frames();
    }
    while frame < frames {
        let finish_tick = DRIVING_VERIFY != 0 && frame >= meta[20u16];
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
            if driving_has_camera_input(asset_set) {
                let saved = read_dseg_page(3);
                unsafe {
                    write_dseg_page(3, player.weapons.effect_page);
                }
                read_mapped(ptr_camera(), driving_camera_input(asset_set), frame * 6, 6);
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
                if driving_world_pass(asset_set) {
                    read(
                        prefix_camera.as_array(),
                        driving_world_view(asset_set),
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
            if driving_has_world_events(asset_set) {
                apply_world_event(
                    scene,
                    &mut state,
                    scenery,
                    &mut player,
                    data,
                    driving_world_events(asset_set),
                );
            }
        } else if first_finish {
            finish_check::setup(&state, &player, &complete, &pickups, asset_set);
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
        let update = game_update::Frame {
            input: record,
            ordered: meta[21u16] & 1 != 0 && !direct_finish,
            finishing: finish_tick,
        };
        game_update::tick(&mut runtime, c, scene, scenery, &update, &mut peers);

        mark(3);
        if DRIVING_VERIFY != 0 {
            if finish_tick {
                if driving_finish_compare(frame - meta[20u16]) {
                    let checkpoint = finish_check::Checkpoint {
                        case: asset_set,
                        index: frame - meta[20u16],
                        live,
                    };
                    finish_check::result(&mut state, scenery, &mut player, &complete, &checkpoint);
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
                    check_resource_versions(deform, driving_mesh_versions(asset_set), frame);
                }
                if driving_has_prefix(asset_set) {
                    let mut expected =
                        reference(driving_prefix_frames(asset_set), prefix_offset, 238);
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
                complete_offset = complete_check::check(&complete, asset_set, complete_offset);
                if driving_has_pickup(asset_set) {
                    check_pickups(
                        &pickups,
                        &state,
                        driving_pickup_frames(asset_set),
                        pickup_offset,
                    );
                    pickup_offset = pickup_offset + fix32::from_words(10040, 0);
                }
                if driving_world_pass(asset_set) {
                    check_pickup_flights(&pickups, driving_world_flights(asset_set), frame);
                }
                if driving_has_debris(asset_set) {
                    let count =
                        check_debris(&state, driving_debris_frames(asset_set), debris_offset);
                    debris_offset = debris_offset + fix32::from_words(count, 0);
                }
                if driving_has_parents(asset_set) {
                    let count = check_parents(
                        parent_actors,
                        parent_data,
                        &state,
                        driving_parent_frames(asset_set),
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
                if driving_has_player(asset_set) {
                    player_world::check(&mut player, asset_set, scenery, &state.resources);
                }
            }
        }
        unsafe {
            addr_of(&DRIVING_CHECKS).write(0, DRIVING_CHECKS + 1);
        }
        output.draw(frame);
        frame += 1;
        serial += 1;
    }
    output.finish();
    if DRIVING_VERIFY != 0 && driving_finish_transfer(asset_set) {
        let checkpoint = finish_check::Checkpoint {
            case: asset_set,
            index: 0,
            live,
        };
        finish_check::qualify(
            &mut state,
            scenery,
            &mut player,
            &mut complete,
            &mut pickups,
            &checkpoint,
        );
    }
    let arena_start = state.resources.arena_start;
    let arena_words = state.resources.arena_words;
    scene_meshes::finish(&mut state.resources);
    if mesh_storage::used(motion_page, arena_start, arena_words) != 0
        || mesh_storage::used(DRIVING_ACTORS.first_sector() >> 8, arena_start, arena_words) != 0
    {
        halt(0x7f9d);
    }
    state.map.terrain.finish();
    if DRIVING_VERIFY != 0 {
        check_world_guards();
    }
    if read_dseg_page(3) != incoming {
        halt(0x7f06);
    }
    free(deform_data_ptr);
    serial
}
