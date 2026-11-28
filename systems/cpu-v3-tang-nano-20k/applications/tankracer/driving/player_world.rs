//! Player preparation over the persistent car, scene and weapon owners.
#![link_cseg = "world"]
use super::*;
pub const BLAST_WORD: u16 = 16000;
pub const TAIL_WORD: u16 = 8234;
pub const COMBAT_WORD: u16 = 15320;
pub const COMBAT_ROW: u16 = 334;
pub struct Player {
    pub context: AssetBytes,
    // Immutable, frequently used parameters stay valid across both DSEG windows.
    pub context_header: Buf<u16, 60>,
    pub projectile_params: Buf<u16, 61>,
    pub mine_params: Buf<u16, 18>,
    pub explosion_params: Buf<u16, 44>,
    pub collision_bounds: Buf<u16, 28>,
    pub slide_materials: Buf<u16, 10>,
    pub weapons: scene_creation::Weapons,
    pub fields: Array<u16>,
    pub inventory: Array<u16>,
    pub expected_offset: fix32,
    pub base_words: u16,
    pub ambient_word: u16,
    pub ambient_sounds: u16,
    pub context_end: u16,
    pub hit_fields: Buf<u16, 72>,
    pub hit_model: Buf<u16, 21>,
    pub body: Array<u16>,
    pub body_selection: Buf<u16, 4>,
    pub auxiliary: Array<u16>,
    pub prefixes: Array<u16>,
    pub clock: Buf<u16, 6>,
    pub clock_enabled: bool,
    pub clock_frame: u16,
    pub sine: Array<i16>,
    pub angle: Array<i16>,
    pub drive: Array<u16>,
    pub surfaces: Array<deform_contact::Component>,
    pub pickup_bindings: Array<u16>,
    pub present: Array<u16>,
    pub finish_entity: fix32,
    pub finish_old: u16,
    pub finish_stage: u16,
    pub finish_rank: u16,
}
pub fn combat(owner: u16) -> Array<u16> {
    if owner == 2 {
        return ptr(0xc000 + car_runtime::FINISH_MESH_WORD);
    }
    ptr(0xc000 + COMBAT_WORD + owner * COMBAT_ROW)
}
fn ptr(word: u16) -> Array<u16> {
    unsafe { Ptr::from_addr(word).as_u16_array() }
}
fn g(data: Array<u16>, i: u16) -> fix32 {
    attitude::get(data, i)
}
fn p(data: Array<u16>, i: u16, value: fix32) {
    attitude::put(data, i, value);
}
fn n(value: i16) -> fix32 {
    integer::from_i16(value)
}
fn section(h: Array<u16>, index: u16) -> Array<u16> {
    ptr(0xc000 + 10000 + h[8 + index * 2])
}
pub fn load(
    case: u16,
    world: &scene_world::World,
    fields: Array<u16>,
    mines: Array<u16>,
    explosion: Array<u16>,
) -> Player {
    let page = DRIVING_WEAPONS.first_sector() >> 8;
    let mut result = Player {
        context: driving_player_context(case),
        context_header: Buf::new([0; 60]),
        projectile_params: Buf::new([0; 61]),
        mine_params: Buf::new([0; 18]),
        explosion_params: Buf::new([0; 44]),
        collision_bounds: Buf::new([0; 28]),
        slide_materials: Buf::new([0; 10]),
        fields,
        inventory: ptr(0),
        expected_offset: fix32::from_words(677, 0),
        base_words: 677,
        ambient_word: 0,
        ambient_sounds: 0,
        context_end: 0,
        hit_fields: Buf::new([0; 72]),
        hit_model: Buf::new([
            1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ]),
        body: ptr(0),
        body_selection: Buf::new([0; 4]),
        auxiliary: ptr(0),
        prefixes: ptr(0),
        clock: Buf::new([0; 6]),
        clock_enabled: driving_has_clock(case),
        clock_frame: 0,
        sine: unsafe { Ptr::from_addr(0).as_i16_array() },
        angle: unsafe { Ptr::from_addr(0).as_i16_array() },
        drive: ptr(0),
        surfaces: unsafe { Ptr::from_addr(0).as_array::<deform_contact::Component>() },
        pickup_bindings: ptr(0),
        present: ptr(0),
        finish_entity: n(-1),
        finish_old: 65535,
        finish_stage: 0,
        finish_rank: 0,
        weapons: scene_creation::Weapons {
            directory: ptr(0),
            directory_stride: 8,
            object_capacity: DRIVING_OBJECT_CAPACITY,
            projectile_page: page,
            projectile_pages: 2,
            mine_page: page + 2,
            mine_pages: 1,
            explosion_page: page + 3,
            explosion_pages: 2,
            ufo_page: page + 5,
            ufo_pages: 1,
            explosion_world: explosion,
            blasts: explosion_blasts::view(ptr(0x8000 + BLAST_WORD)),
            effect_page: DRIVING_PICKUP_EFFECTS.first_sector() >> 8,
            quads: unsafe { Ptr::from_addr(0).as_array::<quad_pool::Pool>() },
            lines: unsafe { Ptr::from_addr(0).as_array::<effect_pool::Pool>() },
            has_effect_pool: true,
            world_class: 0,
            mine_order: ordered::List {
                items: mines,
                count: 0,
                capacity: 32,
            },
            mine_count: at(mines, 64),
        },
    };
    // The lower meshes share this immutable source header, including after cloning.
    read(
        result.body_selection.as_array(),
        DRIVING_MESH,
        fix32::zero(),
        4,
    );
    if !driving_has_player(case) {
        return result;
    }
    if result.clock_enabled {
        read(
            result.clock.as_array(),
            driving_clock(case),
            fix32::zero(),
            6,
        );
    }
    let h = result.context_header.as_array();
    read(h, result.context, fix32::zero(), 60);
    if h[0u16] != 0x5053
        || h[1u16] != 5
        || h[2u16] != world.fixed_count
        || h[42u16] < 60
        || h[42u16] >= DRIVING_LIFE_ENTER_OFFSET - 10000
    {
        halt(0x7f61);
    }
    let mut cursor = 60u16;
    let mut i = 0u16;
    while i < 16 {
        if cursor > h[42u16] || h[8 + i * 2] != cursor || h[9 + i * 2] > h[42u16] - cursor {
            halt(0x7f61);
        }
        cursor += h[9 + i * 2];
        i += 1;
    }
    if cursor != h[42u16]
        || h[9u16] != 18
        || h[11u16] % 14 != 0
        || h[21u16] != world.fixed_count * 6
        || h[25u16] != 61
        || h[27u16] != 18
        || h[29u16] != 54
        || h[35u16] != 100
        || h[37u16] != 374
        || h[39u16] < 91
        || h[43u16] != 69
        || h[45u16] < 725
    {
        halt(0x7f61);
    }
    read(
        result.projectile_params.as_array(),
        result.context,
        fix32::from_words(h[24u16], 0),
        61,
    );
    read(
        result.mine_params.as_array(),
        result.context,
        fix32::from_words(h[26u16], 0),
        18,
    );
    read(
        result.explosion_params.as_array(),
        result.context,
        fix32::from_words(h[38u16], 0),
        44,
    );
    read(
        result.collision_bounds.as_array(),
        result.context,
        fix32::from_words(h[32u16], 0),
        28,
    );
    // Preserve prepare's original error branch for an oversized slide table.
    if h[19u16] <= 10 {
        read(
            result.slide_materials.as_array(),
            result.context,
            fix32::from_words(h[18u16], 0),
            h[19u16],
        );
    }
    result.base_words = h[45u16];
    result.expected_offset = fix32::from_words(result.base_words, 0);
    let saved = spatial::select(&world.grid);
    read_mapped(ptr(0xc000 + 10000), result.context, 0, h[42u16]);
    scene_ambient_world::load(&mut result, h);
    scene_flock_world::load();
    unsafe {
        write_dseg_page(3, result.weapons.effect_page);
    }
    read_mapped(ptr(0xc000 + 10468), result.context, h[28u16], 54);
    read_mapped(ptr(0xc000 + BLAST_WORD), result.context, h[36u16], 374);
    let mut effect_words = ptr(0xc000);
    effect_words[BLAST_WORD - 1] = 0x71b3;
    effect_words[BLAST_WORD + 374] = 0x3b17;
    unsafe {
        write_dseg_page(3, saved);
    }
    let packet: Buf<u16, 2> = Buf::new([0; 2]);
    read(packet.as_array(), driving_player(case), fix32::zero(), 2);
    if packet[0u16] != result.base_words || packet[1u16] != 0 {
        halt(0x7f64);
    }
    read(fields, driving_player(case), fix32::from_words(2, 0), 60);
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(3, result.weapons.effect_page);
    }
    read_mapped(ptr(0xc000 + TAIL_WORD), driving_player(case), 124, 14);
    let mut raw = ptr(0xc000);
    raw[TAIL_WORD - 1] = 0x19c7;
    let mut reserved_tail = ptr(0xc000 + TAIL_WORD + 14);
    let mut tail_word = 0u16;
    while tail_word < 7 {
        reserved_tail[tail_word] = 0;
        tail_word += 1;
    }
    raw[TAIL_WORD + 21] = 0x7c91;
    read_mapped(
        ptr(0xc000 + weapon_world::OVERLAY_WORD),
        driving_player(case),
        138,
        5,
    );
    raw[weapon_world::OVERLAY_WORD - 1] = 0x395d;
    raw[weapon_world::OVERLAY_WORD + 5] = 0x5d93;
    read_mapped(
        ptr(0xc000 + weapon_world::MARK_WORD),
        driving_player(case),
        143,
        54,
    );
    read_mapped(
        ptr(0xc000 + weapon_world::CAMERA_WORD),
        driving_player(case),
        671,
        6,
    );
    raw[weapon_world::MARK_WORD - 1] = 0x483b;
    raw[weapon_world::MARK_WORD + 54] = 0x3b48;
    raw[weapon_world::CAMERA_WORD - 1] = 0x967d;
    raw[weapon_world::CAMERA_WORD + 6] = 0x7d96;
    read(
        result.hit_fields.as_array(),
        driving_player(case),
        fix32::from_words(677, 0),
        48,
    );
    let mut cursor = 725u16;
    let mut model = result.hit_model.as_array();
    let metadata: Buf<u16, 11> = Buf::new([0; 11]);
    i = 0;
    while i < 2 {
        let start = h[43 + i];
        let m = metadata.as_array();
        if start > h[39u16] || h[39u16] - start < 11 {
            halt(0x7f6c);
        }
        read(
            m,
            result.context,
            fix32::from_words(h[38u16] + start, 0),
            11,
        );
        let vertices = m[8u16];
        let faces = m[9u16];
        if vertices > 110
            || faces > 55
            || 4 + vertices * 3 + faces * 6 > COMBAT_ROW
            || start > h[39u16]
            || h[39u16] - start < 11 + faces * 3
        {
            halt(0x7f6c);
        }
        let mut j = 0u16;
        while j < 4 {
            model[i * 7 + j] = m[j];
            j += 1;
        }
        model[i * 7 + 4] = vertices;
        model[i * 7 + 5] = faces;
        model[i * 7 + 6] = m[10u16];
        let geometry = ptr(0xc000 + COMBAT_WORD + i * COMBAT_ROW);
        read_mapped(geometry, result.context, h[38u16] + start + 4, 4);
        read_mapped(at(geometry, 4), driving_player(case), cursor, vertices * 3);
        read_mapped(
            at(geometry, (4 + vertices * 3) as i16),
            result.context,
            h[38u16] + start + 11,
            faces * 3,
        );
        read_mapped(
            at(geometry, (4 + vertices * 3 + faces * 3) as i16),
            driving_player(case),
            cursor + vertices * 3,
            faces * 3,
        );
        cursor += (vertices + faces) * 3;
        if (i == 0 && start + 11 + faces * 3 != h[44u16])
            || (i == 1 && start + 11 + faces * 3 != h[53u16])
        {
            halt(0x7f6c);
        }
        i += 1;
    }
    if cursor + 68 + 86 + 250 + 120 + 430 + 1188 != result.base_words {
        halt(0x7f6c);
    }
    let mine_header: Buf<u16, 4> = Buf::new([0; 4]);
    read(
        mine_header.as_array(),
        driving_player(case),
        fix32::from_words(cursor, 0),
        4,
    );
    if mine_header[0u16] != 0
        || mine_header[1u16] != 0
        || mine_header[2u16] != 0
        || mine_header[3u16] != 0
    {
        halt(0x7f64);
    }
    raw[COMBAT_WORD - 1] = 0x4a67;
    raw[COMBAT_WORD + 2 * COMBAT_ROW] = 0x764a;
    unsafe {
        write_dseg_page(3, saved);
    }
    weapon_ufos::load(&result, h);
    weapon_emitters::load(&result, h);
    scene_trains::load(&result, h);
    let saved_ufo = read_dseg_page(3);
    unsafe {
        write_dseg_page(3, result.weapons.effect_page);
    }
    read_mapped(
        ptr(0xc000 + weapon_ufos::COUNT_WORD),
        driving_player(case),
        cursor + 68,
        4,
    );
    let counts = ptr(0xc000 + weapon_ufos::COUNT_WORD);
    if counts[0u16] > 32 || counts[1u16] > 9 || counts[2u16] != 0 || counts[3u16] != 0 {
        halt(0x7f72);
    }
    read_mapped(
        ptr(0xc000 + weapon_ufos::OCCUPIED_WORD),
        driving_player(case),
        cursor + 72,
        64,
    );
    read_mapped(
        ptr(0xc000 + weapon_ufos::VISIBLE_WORD),
        driving_player(case),
        cursor + 136,
        18,
    );
    unsafe {
        write_dseg_page(3, saved_ufo);
    }
    read(
        explosion,
        result.context,
        fix32::from_words(h[34u16], 0),
        100,
    );
    p(at(mines, 64), 0, fix32::zero());
    result.weapons.world_class = h[4u16];
    let incoming = read_dseg_page(3);
    let mut bank = 0u16;
    while bank < 6 {
        let mut count = 25u16;
        let mut stride = 640u16;
        if bank == 2 {
            count = 341;
            stride = 48;
        } else if bank == 3 || bank == 4 {
            count = 23;
            stride = 706;
        } else if bank == 5 {
            count = 190;
            stride = 86;
        }
        unsafe {
            write_dseg_page(3, page + bank);
        }
        let mut data = ptr(0xc000);
        i = 0;
        while i < count {
            data[i * stride] = 0;
            i += 1;
        }
        data[16383u16] = 0x468c;
        bank += 1;
    }
    unsafe {
        write_dseg_page(3, incoming);
    }
    result
}
// Raw race time advances once before any entity, including hidden-player frames.
pub fn clock(state: &scene_parent::State, player: &mut Player, frame: Array<u16>) -> fix32 {
    let packed = g(frame, 29);
    if player.clock_enabled {
        let clock = player.clock.as_array();
        let raw = g(frame, 0);
        let running = g(clock, 0) == n(0);
        let launch = player_route::clock_time(clock, raw);
        let mut slot = 0u16;
        while slot < 9 {
            let id = state.slots[slot];
            if id != 65535 {
                if id >= state.car_count {
                    return n(-1);
                }
                let race = at(player.fields, (id * 30 + 22) as i16);
                if running {
                    p(race, 0, g(race, 0) + raw);
                } else if launch {
                    p(race, 2, n(1));
                }
            }
            slot += 1;
        }
        p(frame, 0, player_route::physics_dt(raw));
        let command = (packed.lo_bits() & 57855) | ((g(clock, 2).lo_bits() & 15) << 9);
        p(frame, 29, fix32::from_words(command, packed.hi_bits()));
    }
    n(0)
}
pub fn tank_input(car: &driving::Car, player: &Player, frame: Array<u16>, mut out: Array<u16>) {
    let mut i = 0u16;
    while i < 60 {
        out[i] = 0;
        i += 1;
    }
    p(out, 0, g(frame, 0));
    p(out, 1, g(frame, 1));
    p(out, 2, fix32::from_words(car.player as u16, 0));
    p(
        out,
        3,
        g(at(player.fields, (car.owner * 30 + 22) as i16), 2),
    );
    p(out, 26, g(frame, 26));
    // Visibility and owner are transport metadata; column 28 is physical yaw lock.
    p(
        out,
        27,
        fix32::from_words(256 | car.visible as u16, car.owner),
    );
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(3, player.weapons.effect_page);
    }
    p(
        out,
        28,
        g(ptr(0xc000 + car_runtime::INPUT_WORD + car.owner * 26), 4),
    );
    copy(
        at(out, 38),
        ptr(0xc000 + car_runtime::METADATA_WORD + 2 + car.owner * 28),
        14,
    );
    unsafe {
        write_dseg_page(3, saved);
    }
}
pub fn prepare(
    scene: &scene_query::Scene,
    state: &mut scene_parent::State,
    world: &mut scene_world::World,
    complete: &car_runtime::State,
    player: &mut Player,
    frame: Array<u16>,
) -> fix32 {
    let command = g(frame, 29).lo_bits();
    let own = g(frame, 27).hi_bits();
    if own >= state.car_count || !state.cars[own].player {
        return n(-1);
    }
    let h = player.context_header.as_array();
    if h[19u16] > 10 {
        return n(-1);
    }
    let mine_resources = player.mine_params.as_array();
    let slides = player.slide_materials.as_array();
    let bounds = player.collision_bounds.as_array();
    let camera: Buf<u16, 6> = Buf::new([0; 6]);
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(3, complete.page);
    }
    let mut actors = state.cars;
    let car = &mut actors[own];
    copy(camera.as_array(), ptr(0xc000 + 10174), 6);
    let fields = ptr(0xc000 + car_runtime::INPUT_WORD + own * 26);
    let stored = ptr(0xc000 + car_runtime::METADATA_WORD + 2 + own * 28);
    let core = at(complete.prefixes, (own * 50) as i16);
    let effects = ptr(0xc000 + car_runtime::CAR_WORD + own * car_runtime::CAR_WORDS);
    let aux = at(complete.auxiliary, (own * 36) as i16);
    let tables = projectile::Tables {
        sine: player.sine,
        angle: player.angle,
    };
    let resource = player.projectile_params.as_array();
    let mut launch = scene_creation::Launch {
        owner: projectile::Owner {
            entity: car.entity,
            turret: effects,
            gun: at(effects, 42),
            velocity: car.motion,
        },
        context: projectile::Context {
            shared: projectile::shared(ptr(0xc000 + 10468)),
            resources: projectile::Resources {
                tuning: resource,
                main: at(resource, 30),
                trail: at(resource, 42),
                homing_tail: at(resource, 54),
                cup: resource[60u16],
            },
            tables,
            seed: state.runtime.seed,
        },
        kind: 0,
        class: fix32::from_words(h[5u16], 0),
        resources_present: true,
        homing_mesh: fix32::from_words(h[40u16], h[41u16]),
    };
    let mut placement = scene_mines::Placement {
        owner: car.entity,
        position: at(car.motion, 6),
        repeat: true,
        kind: fix32::zero(),
        frame: g(frame, 26),
        class: fix32::from_words(h[6u16], 0),
        resources_present: true,
        terrain_present: true,
        resources: mine_resources,
        map: view_of(&mut state.map),
        tables,
    };
    let empty = projectile_sweep::Car {
        entity: fix32::zero(),
        present: false,
        disabled: false,
        visible: false,
        ai_disabled: false,
        position: ptr(0),
        basis: ptr(0),
        turret: ptr(0),
        velocity: ptr(0),
        bounds: ptr(0),
        radius: fix32::zero(),
    };
    let targets: Buf<projectile_sweep::Car, 3> = Buf::new([empty; 3]);
    let mut rows = targets.as_array();
    let mut i = 0u16;
    while i < state.car_count {
        let c = &state.cars[i];
        let e = ptr(0xc000 + car_runtime::CAR_WORD + i * car_runtime::CAR_WORDS);
        rows[i] = projectile_sweep::Car {
            entity: c.entity,
            present: true,
            disabled: g(at(e, 950), 0) != fix32::zero(),
            visible: c.visible,
            ai_disabled: false,
            position: at(c.motion, 18),
            basis: c.attitude,
            turret: e,
            velocity: c.motion,
            bounds: at(bounds, (c.profile * 14) as i16),
            radius: g(at(bounds, (c.profile * 14) as i16), 6),
        };
        i += 1;
    }
    let shot: Buf<u16, 3> = Buf::new([0; 3]);
    let mut fire = scene_fire::Fire {
        car: scene_fire::Car {
            player: car.player,
            inventory: at(player.inventory, (own * 4) as i16),
            cooldown: at(car.suspension, 66),
            impact: at(aux, 30),
            collision_size: at(aux, 32),
            recoil_rate: at(car.articulation.as_array(), 2),
            pitch_rate: at(car.articulation.as_array(), 6),
            recoil_mode: at(effects, 910),
            boost_control: at(core, 38),
            spin: at(core, 20),
        },
        mode: g(frame, 1),
        launch: view_of(&mut launch),
        mine: view_of(&mut placement),
        boost_feedback: at(complete.prepared[0u16].boost, 12),
        spin_shared: complete.prepared[0u16].spin,
        cars: projectile_sweep::Cars {
            records: targets.as_array(),
            count: state.car_count,
            slots: state.slots,
            present: true,
        },
        shot: shot.as_array(),
    };
    let attachments: Buf<scene_laps::Attachment, 10> = Buf::new(
        [scene_laps::Attachment {
            object: 0,
            present: ptr(0),
            attached: ptr(0),
        }; 10],
    );
    let mut a = attachments.as_array();
    let mut surfaces = player.surfaces;
    i = 0;
    while i < DRIVING_DEFORM_COUNT {
        let s = &mut surfaces[i];
        a[i] = scene_laps::Attachment {
            object: s.object,
            present: at(state.lifetime.records, (s.object * 8 + 7) as i16),
            attached: unsafe { view_of(&mut s.sampling.attached).as_ptr().as_u16_array() },
        };
        i += 1;
    }
    let mut lap = scene_laps::Context {
        objects: section(h, 6),
        count: world.fixed_count,
        backups: section(h, 7),
        backup_count: h[23u16] / 19,
        attachments: attachments.as_array(),
        attachment_count: DRIVING_DEFORM_COUNT,
        camera: camera.as_array(),
        lap: 0,
        flush: false,
    };
    let tuning: Buf<u16, 12> = Buf::new([800, 0, 70, 0, 800, 0, 720, 0, 720, 0, 700, 0]);
    let eligible: Buf<u16, 2> = Buf::new([0; 2]);
    let routes = at(player.fields, (own * 30) as i16);
    let stage = scene_player::Stage {
        car: scene_player::Car {
            input: player_control::Input {
                drive: stored,
                state: fields,
                countdown: at(core, 2),
                analog_latch: ptr(0xc000 + 10466),
            },
            collision: player_control::Collision {
                counters: at(aux, 30),
                ground_delta: at(aux, 10),
                tank_delta: at(aux, 4),
                obstacle_impulse: at(aux, 34),
            },
            route: player_route::Car {
                route: routes,
                race: at(routes, 22),
            },
            brake: player_control::Brake {
                saved_turn: at(fields, 4),
                reverse_braking: at(effects, 948),
                drive: stored,
                max_turn: at(fields, 6),
                yaw_lock: at(fields, 8),
            },
            turret: at(fields, 10),
            angle: at(effects, 912),
            speed: at(car.drive, 4),
            effect_timer: car.ground.effect.as_array(),
            temporary_timer: at(core, 4),
            material: unsafe { view_of(&mut car.ground.material0).as_ptr().as_u16_array() },
            weather: at(routes, 28),
            position: at(car.motion, 18),
            rotational_drag: g(player.drive, 35).lo_bits() as i16,
        },
        frame: scene_player::Frame {
            entity: car.entity,
            number: g(frame, 26),
            dt: g(frame, 0),
            keys: command & 511,
            has_keys: true,
            start_stage: fix32::from_words((command >> 9) & 15, 0),
            race_active: !player.clock_enabled || g(player.clock.as_array(), 0) == n(0),
            mode: g(frame, 1),
        },
        routes: player_route::Routes {
            header: section(h, 0),
            points: section(h, 1),
            count: h[11u16] / 14,
            ranges: section(h, 2),
            items: section(h, 3),
        },
        route_count: h[3u16],
        route_page: world.grid.page,
        weather_table: section(h, 4),
        slides: slides,
        slide_count: h[19u16],
        tuning: tuning.as_array(),
        laps: view_of(&mut lap),
        fire: view_of(&mut fire),
        launch_eligible: eligible.as_array(),
    };
    let status = scene_player::prepare(
        scene,
        &mut state.lifetime,
        &mut state.runtime,
        world,
        &mut player.weapons,
        &stage,
    );
    if status == fix32::zero() {
        p(frame, 3, g(at(routes, 22), 2));
        p(frame, 4, g(eligible.as_array(), 0));
        p(frame, 28, g(fields, 4));
        copy(at(frame, 38), stored, 14);
        p(frame, 29, fix32::from_words((command >> 13) & 1, 0));
    }
    unsafe {
        write_dseg_page(3, saved);
    }
    status
}
pub fn finish(
    state: &scene_parent::State,
    player: &Player,
    frame: Array<u16>,
    before: Array<u16>,
    keys: u16,
    success: bool,
) -> fix32 {
    let own = g(frame, 27).hi_bits();
    if own >= state.car_count {
        return n(-1);
    }
    let car = &state.cars[own];
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(3, player.weapons.effect_page);
    }
    let fields = ptr(0xc000 + TAIL_WORD + own * 7);
    let tail = player_tail::State {
        gun: car.gun,
        binding: car.gun_binding,
        fields,
    };
    let mut flags = 4u16;
    if car.visible {
        flags |= 1;
    }
    if own == state.slots[0u16] {
        flags |= 2;
    }
    if keys & 256 != 0 {
        flags |= 8;
    }
    if player_tail::finish(&tail, before, g(frame, 0), flags, success) {
        let mut slot = 2u16;
        while slot < 9 {
            let peer = state.slots[slot];
            if peer != 65535 && peer >= state.car_count {
                unsafe {
                    write_dseg_page(3, saved);
                }
                return n(-1);
            }
            slot += 1;
        }
        let mut fields = fields;
        fields[2u16] = 0;
    }
    unsafe {
        write_dseg_page(3, saved);
    }
    n(0)
}
#[link_cseg = "frame"]
pub fn check(
    player: &mut Player,
    case: u16,
    world: &scene_world::World,
    store: &scene_meshes::Store,
) {
    if player.clock_enabled {
        let mut expected = reference(
            driving_clock(case),
            fix32::from_words((player.clock_frame + 1) * 6, 0),
            6,
        );
        expect_block(player.clock.as_array(), &mut expected, 0, 6);
        player.clock_frame += 1;
    }
    let header: Buf<u16, 2> = Buf::new([0; 2]);
    read(
        header.as_array(),
        driving_player(case),
        player.expected_offset,
        2,
    );
    if header[1u16] > 96 || header[0u16] < player.base_words {
        halt(0x7f64);
    }
    let mut expected = reference(driving_player(case), player.expected_offset, header[0u16]);
    expect_block(player.fields, &mut expected, 2, 60);
    expect_block(player.inventory, &mut expected, 62, 8);
    expect_block(player.hit_fields.as_array(), &mut expected, 677, 48);
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(3, player.weapons.effect_page);
    }
    expect_block(ptr(0xc000 + 10468), &mut expected, 70, 54);
    expect_block(ptr(0xc000 + TAIL_WORD), &mut expected, 124, 14);
    expect_block(
        ptr(0xc000 + weapon_world::MARK_WORD),
        &mut expected,
        143,
        54,
    );
    expect_block(player.weapons.explosion_world, &mut expected, 197, 100);
    expect_block(ptr(0xc000 + BLAST_WORD), &mut expected, 297, 374);
    expect_block(
        ptr(0xc000 + weapon_world::CAMERA_WORD),
        &mut expected,
        671,
        6,
    );
    if ptr(0xc000)[weapon_world::MARK_WORD - 1] != 0x483b
        || ptr(0xc000)[weapon_world::MARK_WORD + 54] != 0x3b48
        || ptr(0xc000)[weapon_world::CAMERA_WORD - 1] != 0x967d
        || ptr(0xc000)[weapon_world::CAMERA_WORD + 6] != 0x7d96
    {
        halt(0x7f6b);
    }
    let overlay = ptr(0xc000 + weapon_world::OVERLAY_WORD);
    expect_word(overlay[0u16], &mut expected, 138);
    // Option::None owns no color payload; the kernel keeps the reusable lanes.
    if overlay[0u16] != 0 {
        expect_block(at(overlay, 1), &mut expected, 139, 4);
    }
    if ptr(0xc000)[weapon_world::OVERLAY_WORD - 1] != 0x395d
        || ptr(0xc000)[weapon_world::OVERLAY_WORD + 5] != 0x5d93
    {
        halt(0x7f6a);
    }
    if ptr(0xc000)[TAIL_WORD - 1] != 0x19c7 || ptr(0xc000)[TAIL_WORD + 21] != 0x7c91 {
        halt(0x7f69);
    }
    if ptr(0xc000)[BLAST_WORD - 1] != 0x71b3 || ptr(0xc000)[BLAST_WORD + 374] != 0x3b17 {
        halt(0x7f66);
    }
    let mut cursor = 725u16;
    let mut part = 0u16;
    let model = player.hit_model.as_array();
    while part < 2 {
        let geometry = ptr(0xc000 + COMBAT_WORD + part * COMBAT_ROW);
        let vertices = model[part * 7 + 4];
        let faces = model[part * 7 + 5];
        expect_block(at(geometry, 4), &mut expected, cursor, vertices * 3);
        expect_block(
            at(geometry, (4 + vertices * 3 + faces * 3) as i16),
            &mut expected,
            cursor + vertices * 3,
            faces * 3,
        );
        cursor += (vertices + faces) * 3;
        part += 1;
    }
    if cursor + 68 + 86 + 250 + 120 + 430 + 1188 != player.base_words
        || ptr(0xc000)[COMBAT_WORD - 1] != 0x4a67
        || ptr(0xc000)[COMBAT_WORD + 2 * COMBAT_ROW] != 0x764a
    {
        halt(0x7f6d);
    }
    expect_block(player.weapons.mine_count, &mut expected, cursor, 2);
    expect_word(player.weapons.mine_order.count, &mut expected, cursor + 2);
    expect_word(0, &mut expected, cursor + 3);
    if player.weapons.mine_order.count > 32 {
        halt(0x7f6e);
    }
    expect_block(
        player.weapons.mine_order.items,
        &mut expected,
        cursor + 4,
        player.weapons.mine_order.count * 2,
    );
    cursor += 68;
    let counts = ptr(0xc000 + weapon_ufos::COUNT_WORD);
    if counts[0u16] > 32 || counts[1u16] > 9 {
        halt(0x7f72);
    }
    expect_word(counts[0u16], &mut expected, cursor);
    expect_word(counts[1u16], &mut expected, cursor + 1);
    expect_word(0, &mut expected, cursor + 2);
    expect_word(0, &mut expected, cursor + 3);
    expect_block(
        ptr(0xc000 + weapon_ufos::OCCUPIED_WORD),
        &mut expected,
        cursor + 4,
        counts[0u16] * 2,
    );
    expect_block(
        ptr(0xc000 + weapon_ufos::VISIBLE_WORD),
        &mut expected,
        cursor + 68,
        counts[1u16] * 2,
    );
    if ptr(0xc000)[weapon_ufos::POINT_WORD - 1] != 0x1f7a
        || ptr(0xc000)[weapon_ufos::COUNT_WORD + 4] != 0xa7f1
    {
        halt(0x7f73);
    }
    cursor += 86;
    expect_block(
        ptr(0xc000 + weapon_emitters::STATE_WORD),
        &mut expected,
        cursor,
        250,
    );
    if ptr(0xc000)[weapon_emitters::GUARD_WORD] != 0x5e19 {
        halt(0x7f79);
    }
    cursor += 250;
    expect_block(
        ptr(0xc000 + scene_trains::STATE_WORD),
        &mut expected,
        cursor,
        120,
    );
    if ptr(0xc000)[scene_trains::STATE_WORD - 1] != 0x2a31
        || ptr(0xc000)[scene_trains::GUARD_WORD] != 0x13a2
    {
        halt(0x7f7b);
    }
    cursor += 120;
    scene_ambient_world::check(player, world, &mut expected, cursor);
    cursor += 430;
    scene_flock_world::check(store, &mut expected, cursor);
    cursor += 1188;
    let mut index = 0u16;
    let mut id = 0u16;
    while id < world.count {
        let kind = scene_creation::component_kind(&player.weapons, id);
        if kind != 0 {
            if index >= header[1u16] {
                halt(0x7f65);
            }
            let mut words = 640u16;
            if kind == 2 {
                words = 48;
            } else if kind == 3 {
                words = 706;
            } else if kind == 4 {
                words = 86;
            } else if kind != 1 {
                halt(0x7f65);
            }
            if cursor > header[0u16] || header[0u16] - cursor < words + 2 {
                halt(0x7f64);
            }
            expect_word(id, &mut expected, cursor);
            expect_word(kind, &mut expected, cursor + 1);
            unsafe {
                write_dseg_page(
                    3,
                    scene_creation::page(
                        &player.weapons,
                        kind,
                        player.weapons.directory[id * player.weapons.directory_stride + 1],
                    ),
                );
            }
            expect_block(
                scene_creation::record(&player.weapons, id, 3),
                &mut expected,
                cursor + 2,
                words,
            );
            index += 1;
            cursor += words + 2;
        }
        id += 1;
    }
    if index != header[1u16] || cursor != header[0u16] {
        halt(0x7f65);
    }
    let mut bank = 0u16;
    while bank < 6 {
        unsafe {
            write_dseg_page(3, (DRIVING_WEAPONS.first_sector() >> 8) + bank);
        }
        if ptr(0xffff)[0u16] != 0x468c {
            halt(0x7f64);
        }
        bank += 1;
    }
    player.expected_offset = player.expected_offset + fix32::from_words(header[0u16], 0);
    unsafe {
        write_dseg_page(3, saved);
    }
}
