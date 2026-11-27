//! UFO transfers over live vehicle, obstacle, route and lifecycle owners.
#![link_cseg = "parents"]
use super::*;
pub const POINT_WORD: u16 = 10524;
pub const ORDER_WORD: u16 = 11484;
pub const OCCUPIED_WORD: u16 = 11740;
pub const VISIBLE_WORD: u16 = 11804;
pub const COUNT_WORD: u16 = 11822;
fn ptr(word: u16) -> Array<u16> {
    unsafe { Ptr::from_addr(word).as_u16_array() }
}
fn n(value: i16) -> fix32 {
    integer::from_i16(value)
}
fn g(data: Array<u16>, i: u16) -> fix32 {
    attitude::get(data, i)
}
fn p(data: Array<u16>, i: u16, value: fix32) {
    attitude::put(data, i, value);
}
pub fn load(player: &player_world::Player, h: Array<u16>) {
    if h[51u16] > 240
        || h[52u16] > 128
        || h[48u16] != h[53u16]
        || h[49u16] != h[48u16] + 6
        || h[50u16] != h[49u16] + h[51u16] * 4
        || h[54u16] != h[50u16] + h[52u16] * 2
        || h[55u16] != 5
        || h[7u16] != h[54u16] + h[55u16] * 51
        || h[56u16] != h[7u16] + 124
        || h[39u16] != h[56u16] + h[57u16]
    {
        halt(0x7f71);
    }
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(3, player.weapons.effect_page);
    }
    pickup_check::read_pickup_stage(
        ptr(0xc000 + POINT_WORD),
        player.context,
        h[38u16] + h[49u16],
        h[51u16] * 4,
    );
    pickup_check::read_pickup_stage(
        ptr(0xc000 + ORDER_WORD),
        player.context,
        h[38u16] + h[50u16],
        h[52u16] * 2,
    );
    let mut words = ptr(0xc000);
    words[POINT_WORD - 1] = 0x1f7a;
    words[COUNT_WORD + 4] = 0xa7f1;
    unsafe {
        write_dseg_page(3, saved);
    }
}
pub fn place(
    scene: &scene_query::Scene,
    state: &mut scene_parent::State,
    world: &mut scene_world::World,
    player: &mut player_world::Player,
    event: Array<u16>,
) -> fix32 {
    let destination: Buf<u16, 6> = Buf::new([0; 6]);
    let mut target = n(0);
    if event[0u16] == 6 {
        let collider: Buf<u16, 60> = Buf::new([0; 60]);
        if !scene_world::load_collider(world, event[2u16], collider.as_array()) {
            return n(-4);
        }
        target = g(collider.as_array(), 0);
        copy(destination.as_array(), at(collider.as_array(), 2), 6);
        p(
            destination.as_array(),
            0,
            g(destination.as_array(), 0) + n(event[3u16] as i16),
        );
    } else {
        if event[2u16] >= state.car_count {
            return n(-4);
        }
        target = state.cars[event[2u16]].entity;
    }
    let header: Buf<u16, 56> = Buf::new([0; 56]);
    let h = header.as_array();
    read(h, player.context, fix32::zero(), 56);
    let resources: Buf<u16, 6> = Buf::new([0; 6]);
    let r = resources.as_array();
    read(
        r,
        player.context,
        fix32::from_words(h[38u16] + h[48u16], 0),
        6,
    );
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(3, player.weapons.effect_page);
    }
    let empty = scene_ufos::Car {
        present: false,
        player: false,
        slot: 0,
        disabled: false,
        position: ptr(0),
        basis: ptr(0),
    };
    let cars: Buf<scene_ufos::Car, 2> = Buf::new([empty; 2]);
    let mut rows = cars.as_array();
    let mut i = 0u16;
    while i < state.car_count && i < 2 {
        let car = &state.cars[i];
        rows[i] = scene_ufos::Car {
            present: true,
            player: car.player,
            slot: car.slot,
            disabled: g(ptr(0xc000 + 6810 + i * 956 + 950), 0) != n(0),
            position: at(car.motion, 18),
            basis: car.attitude,
        };
        i += 1;
    }
    let entity = scene_ufos::spawn(
        scene,
        &mut state.lifetime,
        &mut state.runtime,
        world,
        &mut player.weapons,
        &scene_ufos::Spawn {
            class: fix32::from_words(h[46u16], 0),
            resources_present: h[47u16] != 0,
            mesh: g(r, 0),
            material: g(r, 1),
            target,
            emitter_present: false,
            emitter: n(0),
            destination: destination.as_array(),
            cars: scene_ufos::Cars {
                records: cars.as_array(),
                count: state.car_count,
                present: true,
            },
        },
    );
    unsafe {
        write_dseg_page(3, saved);
    }
    if entity < n(0) {
        return entity;
    }
    n(0)
}
pub fn tick(
    scene: &scene_query::Scene,
    state: &mut scene_parent::State,
    world: &mut scene_world::World,
    player: &mut player_world::Player,
    frame: Array<u16>,
    entity: fix32,
) -> fix32 {
    if state.car_count != 2 {
        return n(-4);
    }
    let header: Buf<u16, 56> = Buf::new([0; 56]);
    let h = header.as_array();
    read(h, player.context, fix32::zero(), 56);
    let resources: Buf<u16, 6> = Buf::new([0; 6]);
    let r = resources.as_array();
    read(
        r,
        player.context,
        fix32::from_words(h[38u16] + h[48u16], 0),
        6,
    );
    let bounds: Buf<u16, 28> = Buf::new([0; 28]);
    read(
        bounds.as_array(),
        player.context,
        fix32::from_words(h[32u16], 0),
        28,
    );
    let index = scene_lifecycle::object(&state.lifetime, entity);
    if index < n(0) {
        return index;
    }
    let id = index.lo_bits();
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(
            3,
            scene_creation::page(
                &player.weapons,
                4,
                player.weapons.directory[id * player.weapons.directory_stride + 1],
            ),
        );
    }
    let target = g(at(scene_creation::record(&player.weapons, id, 3), 30), 0);
    unsafe {
        write_dseg_page(3, saved);
    }
    let shape_present = target_shape(state, world, player, target);
    unsafe {
        write_dseg_page(3, player.weapons.effect_page);
    }
    let travel: Buf<u16, 16> = Buf::new([0; 16]);
    let t = travel.as_array();
    let car_info: Buf<u16, 16> = Buf::new([0; 16]);
    let info = car_info.as_array();
    let cars: Buf<scene_vehicle_transfer::Car, 2> = Buf::new([
        transfer_car(&state.cars[0u16], player, at(t, 0), 0),
        transfer_car(&state.cars[1u16], player, at(t, 8), 1),
    ]);
    let mut i = 0u16;
    while i < 2 {
        let car = &state.cars[i];
        let mut point = 0u16;
        while point < 4 {
            p(t, i * 4 + point, g(car.suspension, point * 6 + 3));
            point += 1;
        }
        p(info, i * 4, car.entity);
        let mut player_flag = n(0);
        if car.player {
            player_flag = n(1);
        }
        p(info, i * 4 + 1, player_flag);
        p(info, i * 4 + 2, n(player.hit_model[i * 7u16] as i16));
        // The source uses the original body radius, independent of dents.
        p(
            info,
            i * 4 + 3,
            integer::sar(g(bounds.as_array(), i * 7 + 6), 4),
        );
        i += 1;
    }
    let count = ptr(0xc000 + COUNT_WORD);
    let routes: Buf<vehicle_transfer::Routes, 1> = Buf::new([vehicle_transfer::Routes {
        order: ordered::List {
            items: ptr(0xc000 + ORDER_WORD),
            count: h[52u16],
            capacity: 128,
        },
        points: ptr(0xc000 + POINT_WORD),
        count: h[51u16],
        occupied: ordered::List {
            items: ptr(0xc000 + OCCUPIED_WORD),
            count: count[0u16],
            capacity: 32,
        },
    }]);
    let result: Buf<u16, 2> = Buf::new([0; 2]);
    let mut vehicles = scene_vehicle_transfer::View {
        cars: cars.as_array(),
        count: 2,
        present: true,
        routes: routes.as_array(),
        sine: player.sine,
        angle: player.angle,
        result: result.as_array(),
    };
    let visible: Buf<ordered::List, 1> = Buf::new([ordered::List {
        items: ptr(0xc000 + VISIBLE_WORD),
        count: count[1u16],
        capacity: 9,
    }]);
    let mut tick = scene_ufo_transfer::Tick {
        call: scene_creation::Call { entity, slot: 12 },
        dt: g(frame, 0),
        component_window: 2,
        collision_page: read_dseg_page(2),
        resources_present: h[47u16] != 0,
        height: g(r, 2),
        vehicles: view_of(&mut vehicles),
        car_info: info,
        racers: state.slots,
        visible: visible.as_array(),
        shape_present,
        overlay: ptr(0xc000 + weapon_world::OVERLAY_WORD),
    };
    let status = scene_ufo_transfer::dispatch(
        scene,
        &mut state.lifetime,
        &mut state.runtime,
        world,
        &mut player.weapons,
        &mut tick,
    );
    let mut counts = count;
    counts[0u16] = routes[0u16].occupied.count;
    counts[1u16] = visible[0u16].count;
    unsafe {
        write_dseg_page(3, saved);
    }
    status
}
fn target_shape(
    state: &scene_parent::State,
    world: &scene_world::World,
    player: &player_world::Player,
    target: fix32,
) -> bool {
    if entities::handle_kind(&state.lifetime.identities, target) != 1 {
        return false;
    }
    let object = entities::handle(&state.lifetime.identities, target);
    if object.hi_bits() != 0 || object.lo_bits() >= world.count {
        return false;
    }
    let id = object.lo_bits();
    let mut mesh = n(-1);
    let mut found = false;
    let mut i = 0u16;
    while i < state.actor_count {
        let actor = &state.actors[i];
        if actor.fragments.id == id {
            mesh = g(actor.fragments.current_mesh, 0);
            found = true;
            break;
        }
        i += 1;
    }
    if !found {
        i = 0;
        while i < 30 {
            let row = at(player.pickup_bindings, (i * 26) as i16);
            if row[0u16] == id {
                mesh = g(at(row, 6), 0);
                found = true;
                break;
            }
            i += 1;
        }
    }
    if !found {
        let kind = scene_creation::component_kind(&player.weapons, id);
        if kind != 0 {
            let saved = read_dseg_page(3);
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
            let row = scene_creation::record(&player.weapons, id, 3);
            if row[2u16] != 0 {
                mesh = g(at(row, 3), 0);
            }
            unsafe {
                write_dseg_page(3, saved);
            }
        } else if id < world.fixed_count {
            let word: Buf<u16, 1> = Buf::new([65535]);
            if DRIVING_OBJECT_MESHES.byte_len_hi != 0
                || DRIVING_OBJECT_MESHES.byte_len_lo != world.fixed_count * 2
            {
                halt(0x7f83);
            }
            read(
                word.as_array(),
                DRIVING_OBJECT_MESHES,
                fix32::from_words(id, 0),
                1,
            );
            mesh = fix32::from_words(word[0u16], 0);
        }
    }
    mesh.hi_bits() == 0
        && mesh.lo_bits() < state.resources.count
        && state.resources.records[mesh.lo_bits() * 13] & 1 != 0
}
fn transfer_car(
    car: &driving::Car,
    player: &player_world::Player,
    travel: Array<u16>,
    own: u16,
) -> scene_vehicle_transfer::Car {
    let b = ptr(0xc000 + 6810 + own * 956);
    let core = at(player.prefixes, (own * 50) as i16);
    let route = at(player.fields, (own * 30) as i16);
    scene_vehicle_transfer::Car {
        present: true,
        transfer: vehicle_transfer::Car {
            pose: vehicle_pose::State {
                physical_position: at(car.motion, 6),
                reference: car.attitude,
                base_position: at(car.motion, 18),
                body_basis: at(car.attitude, 18),
                body_position: at(car.attitude, 36),
                tracks: car.tracks,
                turret: b,
                gun: at(b, 24),
                attachments: at(b, 48),
                bottom_position: at(b, 96),
                recoil: at(b, 102),
            },
            checkpoint: route,
            passed: at(route, 4),
            laps: at(route, 24),
            controls: ptr(0xc000 + 10360 + own * 28),
            velocity: car.motion,
            acceleration: at(car.drive, 26),
            angular: at(car.attitude, 42),
            track_slip: at(car.motion, 30),
            temporary_timer: at(core, 8),
            config: at(b, 848),
            travel,
            environment: at(b, 902),
        },
        spin_disabled: at(core, 36),
        collision_disabled: at(b, 950),
        driving: at(route, 26),
    }
}

// 1 consumes the contact, 0 continues through normal reflection/impact.
pub fn attach(
    scene: &scene_query::Scene,
    state: &mut scene_parent::State,
    world: &mut scene_world::World,
    player: &mut player_world::Player,
    impact: &scene_projectile_impact::Impact,
    h: Array<u16>,
) -> fix32 {
    let action = &impact.action[0u16];
    let target = action.contact.vehicle;
    if target.hi_bits() != 0 || target.lo_bits() >= state.car_count {
        return n(-4);
    }
    let index = target.lo_bits();
    let id = impact.object;
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(
            3,
            scene_creation::page(
                &player.weapons,
                1,
                player.weapons.directory[id * player.weapons.directory_stride + 1],
            ),
        );
    }
    let shot = at(scene_creation::record(&player.weapons, id, 3), 30);
    let flags = g(shot, 4);
    let attached = g(shot, 25) != n(0);
    unsafe {
        write_dseg_page(3, saved);
    }
    if flags.lo_bits() & 4 != 0 {
        return n(1);
    }
    if g(impact.cars[index].blast, 3) >= n(1) || attached {
        return n(0);
    }
    let resources: Buf<u16, 6> = Buf::new([0; 6]);
    let r = resources.as_array();
    read(
        r,
        player.context,
        fix32::from_words(h[38u16] + h[48u16], 0),
        6,
    );
    let empty = scene_ufos::Car {
        present: false,
        player: false,
        slot: 0,
        disabled: false,
        position: ptr(0),
        basis: ptr(0),
    };
    let cars: Buf<scene_ufos::Car, 2> = Buf::new([empty; 2]);
    let mut rows = cars.as_array();
    let mut i = 0u16;
    while i < state.car_count && i < 2 {
        let car = &state.cars[i];
        rows[i] = scene_ufos::Car {
            present: true,
            player: car.player,
            slot: car.slot,
            disabled: g(ptr(0xc000 + 6810 + i * 956 + 950), 0) != n(0),
            position: at(car.motion, 18),
            basis: car.attitude,
        };
        i += 1;
    }
    let destination: Buf<u16, 6> = Buf::new([0; 6]);
    let entity = scene_ufos::spawn(
        scene,
        &mut state.lifetime,
        &mut state.runtime,
        world,
        &mut player.weapons,
        &scene_ufos::Spawn {
            class: fix32::from_words(h[46u16], 0),
            resources_present: h[47u16] != 0,
            mesh: g(r, 0),
            material: g(r, 1),
            target: state.cars[index].entity,
            emitter_present: false,
            emitter: n(0),
            destination: destination.as_array(),
            cars: scene_ufos::Cars {
                records: cars.as_array(),
                count: state.car_count,
                present: true,
            },
        },
    );
    if entity < n(0) {
        return entity;
    }
    if scene_creation::component_kind(&player.weapons, id) != 1 {
        return n(-4);
    }
    unsafe {
        write_dseg_page(
            3,
            scene_creation::page(
                &player.weapons,
                1,
                player.weapons.directory[id * player.weapons.directory_stride + 1],
            ),
        );
    }
    let shot = at(scene_creation::record(&player.weapons, id, 3), 30);
    p(shot, 25, n(1));
    p(shot, 26, entity);
    copy(at(shot, 54), action.contact.local.point.as_array(), 6);
    let flags = g(shot, 4);
    p(
        shot,
        4,
        fix32::from_words(flags.lo_bits() | 4, flags.hi_bits()),
    );
    unsafe {
        write_dseg_page(3, saved);
    }
    n(1)
}
