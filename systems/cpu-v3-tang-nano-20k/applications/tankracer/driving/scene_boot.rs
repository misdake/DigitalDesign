//! First-map constructors publish directly into the continuous world owners.
#![link_cseg = "parents"]
use super::*;
const INPUT: u16 = 1088;
const OUTPUT: u16 = 1280;
const CAMERA: u16 = 2880;
fn ptr(word: u16) -> Array<u16> {
    unsafe { Ptr::from_addr(word).as_u16_array() }
}
fn sub(data: Array<u16>, word: u16) -> Array<u16> {
    unsafe { data.as_ptr().add(word as i16).as_u16_array() }
}
fn g(data: Array<u16>, column: u16) -> fix32 {
    attitude::get(data, column)
}
fn p(data: Array<u16>, column: u16, value: fix32) {
    attitude::put(data, column, value);
}
fn n(value: i16) -> fix32 {
    integer::from_i16(value)
}
fn require(ok: bool) {
    if !ok {
        halt(0x7fa0);
    }
}
fn reset(state: &mut scene_parent::State, world: &mut scene_world::World) {
    state.runtime.ticks.order.count = 0;
    state.runtime.ticks.next = n(-1);
    let mut flags = state.runtime.ticks.flags;
    let mut i = 0u16;
    while i < state.runtime.ticks.flag_count {
        flags[i * state.runtime.ticks.flag_words] &= 65487;
        i += 1;
    }
    state.runtime.queues.notifications.count = 0;
    state.runtime.queues.destruction.count = 0;
    state.runtime.ordinary.count = 0;
    state.runtime.special.count = 0;
    world.deformables.count = 0;
    state.lifetime.lap_objects.count = 0;
    state.lifetime.signs.count = 0;
    state.lifetime.enter.count = 0;
    state.lifetime.leave.count = 0;
    i = 0;
    while i < world.fixed_count {
        let mut row = sub(state.lifetime.records, i * 8);
        row[4u16] = 0;
        row[5u16] = 0;
        row[6u16] = 1;
        i += 1;
    }
    let saved = spatial::select(&world.grid);
    let mut ranges = world.grid.ranges;
    i = 0;
    while i < 64 {
        ranges[i * 3 + 1] = 0;
        i += 1;
    }
    unsafe {
        write_dseg_page(3, saved);
    }
}
fn pose(world: &scene_world::World, object: u16, data: Array<u16>) {
    let row = sub(world.colliders, object * 60);
    copy(sub(row, 2), sub(data, 18), 6);
    copy(sub(row, 8), data, 18);
    copy(sub(row, 26), data, 18);
}
fn debris(state: &scene_parent::State, object: u16, count: u16, actors_page: u16) {
    if count == 0 {
        return;
    }
    let mut i = 0u16;
    while i < state.actor_count && state.actors[i].fragments.id != object {
        i += 1;
    }
    require(i < state.actor_count);
    let actor = &state.actors[i];
    require(actor.debris.as_ptr().addr() != 0);
    let s = &actor.debris[0u16];
    require(s.count == count);
    let fixed = read_dseg_page(2);
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(2, actors_page);
        write_dseg_page(3, actor.fragments.motion_page);
    }
    i = 0;
    while i < count {
        copy(sub(s.pieces, i * 42), ptr(0x8000 + OUTPUT + 8 + i * 48), 42);
        i += 1;
    }
    unsafe {
        write_dseg_page(2, fixed);
        write_dseg_page(3, saved);
    }
}
pub fn construct(
    scene: &scene_query::Scene,
    state: &mut scene_parent::State,
    world: &mut scene_world::World,
    player: &mut player_world::Player,
    complete: &complete_check::State,
) {
    let asset = driving_scene_boot();
    let saved = read_dseg_page(3);
    let fixed = read_dseg_page(2);
    let actors_page = DRIVING_ACTORS.first_sector() >> 8;
    let header: Buf<u16, 16> = Buf::new([0; 16]);
    let h = header.as_array();
    read(h, asset, fix32::zero(), 16);
    require(
        h[0u16] == 0x4353
            && h[1u16] == 1
            && h[2u16] == world.fixed_count
            && h[4u16] == 16
            && h[5u16] >= 16
            && h[5u16] <= 16342
            && h[6u16] == 30
            && h[7u16] == h[5u16] + 30
            && h[8u16] == 12
            && h[3u16] == h[7u16] + 12
            && asset.byte_len_hi == 0
            && asset.byte_len_lo == h[3u16] * 2,
    );
    require((CAMERA + 57) * 2 <= DRIVING_ACTORS.sector_count() * 128);
    let setup: Buf<u16, 12> = Buf::new([0; 12]);
    read(setup.as_array(), asset, fix32::from_words(h[7u16], 0), 12);
    let context_header: Buf<u16, 60> = Buf::new([0; 60]);
    read(context_header.as_array(), player.context, fix32::zero(), 60);
    let lap_objects = ptr(0xc000 + 10000 + context_header[20u16]);
    let visibility: Buf<u16, 8> = Buf::new([0; 8]);
    let v = visibility.as_array();
    let attachments: Buf<scene_laps::Attachment, 10> = Buf::new(
        [scene_laps::Attachment {
            object: 0,
            present: ptr(0),
            attached: ptr(0),
        }; 10],
    );
    let mut a = attachments.as_array();
    let mut surfaces = player.surfaces;
    let mut i = 0u16;
    while i < DRIVING_DEFORM_COUNT {
        let s = &mut surfaces[i];
        a[i] = scene_laps::Attachment {
            object: s.object,
            present: sub(state.lifetime.records, s.object * 8 + 7),
            attached: unsafe { view_of(&mut s.sampling.attached).as_ptr().as_u16_array() },
        };
        i += 1;
    }
    let camera: Buf<u16, 6> = Buf::new([0; 6]);
    let mut lap = scene_laps::Context {
        objects: lap_objects,
        count: world.fixed_count,
        backups: ptr(0),
        backup_count: 0,
        attachments: a,
        attachment_count: DRIVING_DEFORM_COUNT,
        camera: camera.as_array(),
        lap: 0,
        flush: false,
    };
    let mut tilts = driving_scene_boot_tilts();
    let tilt_view = unsafe { view_of(&mut tilts).as_ptr().as_i16_array() };
    unsafe {
        write_dseg_page(3, complete.page);
    }
    let mut shield = complete.shared.shield;
    i = 0;
    while i < 810 {
        shield[i] = 0;
        i += 1;
    }
    vehicle_shield::initialize(shield, state.runtime.seed);
    reset(state, world);
    let row_header: Buf<u16, 5> = Buf::new([0; 5]);
    let r = row_header.as_array();
    let mut cursor = 16u16;
    let mut object = 0u16;
    let mut pickup = 0u16;
    let mut train_index = 0u16;
    let mut emitter_index = 0u16;
    while object < world.fixed_count {
        read(r, asset, fix32::from_words(cursor, 0), 5);
        cursor += 5;
        let family = r[1u16];
        let words = r[2u16];
        let extra = r[3u16];
        let flags = r[4u16];
        require(
            r[0u16] == object
                && family >= 1
                && family <= 15
                && words >= 37
                && words <= OUTPUT - INPUT
                && extra <= OUTPUT - INPUT - words
                && flags <= 3
                && cursor <= h[5u16]
                && words + extra <= h[5u16] - cursor,
        );
        unsafe {
            write_dseg_page(3, actors_page);
        }
        let input = ptr(0xc000 + INPUT);
        let out = ptr(0xc000 + OUTPUT);
        pickup_check::read_pickup_stage(input, asset, cursor, words + extra);
        cursor += words + extra;
        require(input[2u16] == 0 && input[34u16] <= 13 && input[36u16] <= 1);
        let piece_count = input[34u16];
        require(words == 37 + input[36u16] * piece_count * 6);
        let mut kind = 0u16;
        if family == 2 {
            kind = 1;
        }
        if family == 11 {
            kind = 3;
        }
        if family == 13 {
            kind = 2;
        }
        if family == 14 {
            kind = 5;
        }
        if family == 15 {
            kind = 4;
        }
        require(input[0u16] == kind);
        if family >= 8 {
            require(piece_count == 0);
        }
        if family == 12 {
            require(extra == 41);
        } else if family == 14 {
            require(extra == 54);
        } else {
            require(extra == 0);
        }
        let mut surface_index = 65535u16;
        if family == 2 {
            surface_index = deform_contact::find(surfaces, DRIVING_DEFORM_COUNT, object);
            require(
                surface_index < DRIVING_DEFORM_COUNT
                    && surfaces[surface_index].mesh.flag_count == input[1u16],
            );
        } else {
            require(input[1u16] == 0);
        }
        require(input[1u16] <= INPUT + vehicle_startup::WORDS - OUTPUT - 8 - piece_count * 48);
        let remove_rule = flags & 2 != 0;
        // Generic allocation starts with Some(Cell { x: 0, z: 0 }).
        // Obstacle initialization clears it; Crush overrides that initializer.
        if (family <= 7 && family != 2) || family == 14 {
            let mut record = sub(state.lifetime.records, object * 8);
            record[4u16] = 0;
            record[5u16] = 0;
            record[6u16] = 0;
        }
        if family == 5 {
            let mut actor = 0u16;
            while actor < state.actor_count && state.actors[actor].fragments.id != object {
                actor += 1;
            }
            require(actor < state.actor_count && state.actors[actor].has_state);
            let closer = state.actors[actor].state;
            p(closer, 0, n(1820));
            p(closer, 1, n(4));
            p(closer, 2, n(1));
        }
        let produced =
            scene_initial::random_fields(out, input, state.runtime.seed, player.sine, player.angle);
        p(v, 0, n(150));
        p(v, 1, n(0));
        p(v, 2, n(0));
        p(v, 3, n(1));
        if produced >= 8 {
            copy(v, out, 8);
        }
        debris(state, object, piece_count, actors_page);
        if family == 2 {
            let s = &mut surfaces[surface_index];
            require(
                s.mesh.flag_count == input[1u16]
                    && produced == 8 + piece_count * 48 + s.mesh.flag_count,
            );
            copy(
                s.mesh.flags,
                sub(out, 8 + piece_count * 48),
                s.mesh.flag_count,
            );
        }
        if family == 13 {
            require(object >= 183 && object <= 185 && produced == 296);
            copy(
                ptr(0xc000 + 16 + (object - 183) * 348 + 4),
                sub(out, 8),
                288,
            );
        }
        if family == 11 {
            require(produced == 10 && emitter_index < 5);
            unsafe {
                write_dseg_page(2, actors_page);
                write_dseg_page(3, complete.page);
            }
            copy(
                ptr(0xc000 + 11828 + emitter_index * 50),
                ptr(0x8000 + OUTPUT + 8),
                2,
            );
            unsafe {
                write_dseg_page(2, fixed);
                write_dseg_page(3, actors_page);
            }
            emitter_index += 1;
        }
        if family == 12 {
            require(extra == 41 && train_index < 2);
            let data = sub(input, words);
            copy(sub(out, 56), data, 24);
            let path = train::State {
                travel: out,
                path: sub(out, 38),
                position: sub(out, 74),
                basis: sub(out, 56),
                speed: g(sub(data, 36), 0),
                range: g(sub(data, 38), 0),
                sound: false,
                seed: state.runtime.seed,
                sine: player.sine,
            };
            train::initialize(&path, sub(data, 24), sub(data, 30), data[40u16] != 0);
            pose(world, object, sub(out, 56));
            unsafe {
                write_dseg_page(2, actors_page);
                write_dseg_page(3, complete.page);
            }
            let row = ptr(0xc000 + 12086 + train_index * 60);
            let from = ptr(0x8000 + OUTPUT);
            let data = ptr(0x8000 + INPUT + words);
            copy(row, from, 38);
            copy(sub(row, 38), sub(data, 36), 4);
            copy(sub(row, 42), sub(from, 38), 18);
            unsafe {
                write_dseg_page(2, fixed);
                write_dseg_page(3, actors_page);
            }
            train_index += 1;
        }
        if family == 14 {
            require(extra == 54 && pickup < 30);
            scene_actor_initial::pickup(
                out,
                sub(input, words),
                setup.as_array(),
                player.sine,
                tilt_view,
            );
            pose(world, object, out);
            p(v, 1, g(out, 40));
            p(v, 2, g(out, 39));
            p(v, 3, n(1));
            if g(out, 39) == n(2) || g(out, 39) == n(3) {
                p(v, 3, n(0));
            }
            unsafe {
                write_dseg_page(2, actors_page);
            }
            spatial::select(&world.grid);
            let from = ptr(0x8000 + OUTPUT);
            copy(
                ptr(0xc000 + pickup_check::FLIGHT_WORD + pickup * 54),
                sub(from, 24),
                54,
            );
            unsafe {
                write_dseg_page(2, fixed);
                write_dseg_page(3, actors_page);
            }
            pickup += 1;
        }
        if family == 8 || family == 9 {
            unsafe {
                write_dseg_page(2, actors_page);
            }
            spatial::select(&world.grid);
            let ambient = ptr(0xc000 + player.ambient_word);
            if family == 8 {
                let mut row = ambient;
                row[3u16] = 0;
                p(sub(ambient, 4), 0, n(-1));
            }
            if family == 9 {
                let smoke = sub(ambient, 6 + player.ambient_sounds * 10);
                p(smoke, 2, n(0));
                p(smoke, 3, n(0));
                let mut particle = 0u16;
                while particle < 16 {
                    let row = sub(smoke, 10 + particle * 26);
                    let mut col = 0u16;
                    while col < 13 {
                        p(row, col, n(0));
                        col += 1;
                    }
                    copy(sub(row, 14), ptr(0x8000 + INPUT + 26), 6);
                    particle += 1;
                }
            }
            unsafe {
                write_dseg_page(2, fixed);
                write_dseg_page(3, actors_page);
            }
        }
        spatial::select(&world.grid);
        let mut fields = sub(lap_objects, object * 6);
        fields[0u16] = g(v, 1).lo_bits();
        fields[1u16] = g(v, 2).lo_bits();
        fields[2u16] = g(v, 3).lo_bits();
        let entity = g(world.colliders, object * 30);
        require(lifecycle::enable(&mut state.runtime.ticks, entity) < 2);
        if family != 15 {
            let status = scene_laps::action(
                scene,
                &mut state.lifetime,
                &mut state.runtime,
                world,
                &mut player.weapons,
                &scene_laps::Action {
                    context: view_of(&mut lap),
                    entity,
                    leave: g(v, 3) == n(0),
                },
            );
            require(status == n(0));
            if g(v, 1) != n(0) || remove_rule || family == 14 {
                require(
                    ordered::append(
                        &mut state.lifetime.lap_objects,
                        fix32::from_words(object, 0),
                    ) == 0,
                );
            }
        }
        if family == 9 {
            require(ordered::append(&mut state.runtime.special, fix32::from_words(object, 0)) == 0);
        }
        if family == 3 && flags & 1 != 0 {
            require(ordered::append(&mut state.lifetime.signs, fix32::from_words(object, 0)) == 0);
        }
        if family <= 7 || family == 10 || family == 15 {
            require(
                lifecycle::queue(
                    &mut state.runtime.queues,
                    &mut state.runtime.ticks,
                    entity,
                    false,
                    state.lifetime.limit,
                ) == 0,
            );
        }
        if family == 5 {
            require(
                lifecycle::cancel(
                    &mut state.runtime.queues,
                    &mut state.runtime.ticks,
                    entity,
                    state.lifetime.limit,
                ) == 0,
            );
        }
        object += 1;
    }
    require(cursor == h[5u16] && pickup == 30 && train_index == 2 && emitter_index == 5);
    while state.runtime.queues.notifications.count != 0 {
        let entity = g(state.runtime.queues.notifications.items, 0);
        ordered::remove_at(&mut state.runtime.queues.notifications, 0);
        require(lifecycle::disable(&mut state.runtime.ticks, entity, state.lifetime.limit) < 2);
    }
    i = 0;
    while i < state.car_count {
        require(lifecycle::enable(&mut state.runtime.ticks, state.cars[i].entity) < 2);
        i += 1;
    }
    let meta = sub(player.pickup_bindings, 780);
    copy(meta, sub(setup.as_array(), 8), 4);
    unsafe {
        write_dseg_page(3, actors_page);
    }
    pickup_check::read_pickup_stage(ptr(0xc000 + INPUT), asset, h[5u16], 30);
    let input = ptr(0xc000 + INPUT);
    require(scene_initial::camera(
        ptr(0xc000 + CAMERA),
        input,
        sub(input, 24),
        state.runtime.seed,
        player.sine,
        unsafe { addr_of(&DRIVING_INVERSE_COSINE).as_i16_array() },
    ));
    let mut camera_words = ptr(0xc000 + CAMERA);
    camera_words[56u16] = 0x2c71;
    let mut expected_camera = reference(driving_scene_boot_camera(), fix32::zero(), 56);
    expect_block(ptr(0xc000 + CAMERA), &mut expected_camera, 0, 56);
    // The finish surrogate is declared after the camera and queues its initial
    // Stop for the first world drain, independently of the scene's earlier drain.
    i = 0;
    while i < g(state.lifetime.identities.header, 0).lo_bits() {
        let entity = fix32::from_words(i, 0);
        if entities::handle_kind(&state.lifetime.identities, entity) == 4 {
            require(
                lifecycle::queue(
                    &mut state.runtime.queues,
                    &mut state.runtime.ticks,
                    entity,
                    false,
                    state.lifetime.limit,
                ) == 0,
            );
        }
        i += 1;
    }
    unsafe {
        write_dseg_page(3, saved);
    }
    require(read_dseg_page(2) == fixed);
}
