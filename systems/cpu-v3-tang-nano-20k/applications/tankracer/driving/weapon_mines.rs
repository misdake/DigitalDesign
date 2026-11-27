//! Mine ticks and contacts borrow the same car, terrain, shield and scene owners.
#![link_cseg = "world"]
use super::*;
fn ptr(word: u16) -> Array<u16> {
    unsafe { Ptr::from_addr(word).as_u16_array() }
}
fn g(words: Array<u16>, index: u16) -> fix32 {
    attitude::get(words, index)
}
pub fn place(
    scene: &scene_query::Scene,
    state: &mut scene_parent::State,
    world: &mut scene_world::World,
    player: &mut player_world::Player,
    frame: Array<u16>,
    event: Array<u16>,
) -> fix32 {
    let owner = event[2u16];
    let kind = event[3u16];
    if owner >= state.car_count {
        return integer::from_i16(-4);
    }
    let header: Buf<u16, 56> = Buf::new([0; 56]);
    read(header.as_array(), player.context, fix32::zero(), 56);
    let resources: Buf<u16, 18> = Buf::new([0; 18]);
    read(
        resources.as_array(),
        player.context,
        fix32::from_words(header[26u16], 0),
        18,
    );
    let car = &state.cars[owner];
    scene_mines::spawn(
        scene,
        &mut state.lifetime,
        &mut state.runtime,
        world,
        &mut player.weapons,
        &scene_mines::Placement {
            owner: car.entity,
            position: at(car.motion, 6),
            repeat: event[0u16] == 7,
            kind: fix32::from_words(kind, 0),
            frame: g(frame, 26),
            class: fix32::from_words(header[6u16], 0),
            resources_present: true,
            terrain_present: true,
            resources: resources.as_array(),
            map: view_of(&mut state.map),
            tables: projectile::Tables {
                sine: player.sine,
                angle: player.angle,
            },
        },
    )
}
pub fn tick(
    scene: &scene_query::Scene,
    state: &mut scene_parent::State,
    world: &mut scene_world::World,
    player: &mut player_world::Player,
    frame: Array<u16>,
    entity: fix32,
) -> fix32 {
    let header_words: Buf<u16, 56> = Buf::new([0; 56]);
    let header = header_words.as_array();
    read(header, player.context, fix32::zero(), 56);
    let resources: Buf<u16, 18> = Buf::new([0; 18]);
    read(
        resources.as_array(),
        player.context,
        fix32::from_words(header[26u16], 0),
        18,
    );
    let vehicles: Buf<scene_mines::Vehicle, 3> = Buf::new([
        scene_mines::Vehicle {
            present: true,
            position: at(state.cars[0u16].motion, 6),
        },
        scene_mines::Vehicle {
            present: true,
            position: at(state.cars[1u16].motion, 6),
        },
        scene_mines::Vehicle {
            present: state.car_count > 2,
            position: at(state.cars[2u16].motion, 6),
        },
    ]);
    let mut placement = scene_mines::Placement {
        owner: fix32::zero(),
        position: ptr(0),
        repeat: false,
        kind: fix32::zero(),
        frame: g(frame, 26),
        class: fix32::from_words(header[6u16], 0),
        resources_present: true,
        terrain_present: true,
        resources: resources.as_array(),
        map: view_of(&mut state.map),
        tables: projectile::Tables {
            sine: player.sine,
            angle: player.angle,
        },
    };
    scene_mines::dispatch(
        scene,
        &mut state.lifetime,
        &mut state.runtime,
        world,
        &mut player.weapons,
        &scene_mines::Tick {
            call: scene_creation::Call { entity, slot: 12 },
            dt: g(frame, 0),
            placement: view_of(&mut placement),
            vehicles: vehicles.as_array(),
            vehicle_count: state.car_count,
            has_vehicles: true,
        },
    )
}
pub fn contact(
    scene: &scene_query::Scene,
    state: &mut scene_parent::State,
    world: &mut scene_world::World,
    player: &mut player_world::Player,
    frame: Array<u16>,
    object: u16,
) -> fix32 {
    let own = g(frame, 27).hi_bits();
    let zero = fix32::zero();
    let page = read_dseg_page(3);
    if object >= world.count || own >= state.car_count {
        return integer::from_i16(-4);
    }
    let raw: Buf<u16, 60> = Buf::new([0; 60]);
    if !scene_world::load_collider(world, object, raw.as_array()) {
        return integer::from_i16(-4);
    }
    let entity = g(raw.as_array(), 0);
    let class = entities::class(&state.lifetime.identities, entity);
    if class < zero || class >= fix32::from_words(scene.header[4u16], 0) {
        return integer::from_i16(-1);
    }
    let saved_slot = g(state.runtime.slot, 0);
    attitude::put(state.runtime.slot, 0, integer::from_i16(24));
    let method = scene.method(class.lo_bits(), 24);
    if method.status == 2 {
        return zero;
    }
    if method.status == 4 || (method.status == 1 && method.id != 122) {
        return integer::from_i16(-1);
    }
    let mut status = zero;
    if method.status == 1 {
        let header: Buf<u16, 56> = Buf::new([0; 56]);
        read(header.as_array(), player.context, zero, 56);
        let resources: Buf<u16, 44> = Buf::new([0; 44]);
        read(
            resources.as_array(),
            player.context,
            fix32::from_words(header[38u16], 0),
            44,
        );
        let r = resources.as_array();
        unsafe {
            write_dseg_page(3, player.weapons.effect_page);
        }
        let car = &state.cars[own];
        let effects = ptr(0xc000 + complete_check::CAR_WORD + own * complete_check::CAR_WORDS);
        let external = g(effects, 459);
        let mut basis = car.attitude;
        if external == integer::from_i16(2) {
            basis = at(effects, 920);
        }
        let empty = scene_mines::HitPeer {
            present: false,
            entity: zero,
            effect: ptr(0),
        };
        let peers: Buf<scene_mines::HitPeer, 9> = Buf::new([empty; 9]);
        let mut rows = peers.as_array();
        let mut slot = 0u16;
        while slot < 9 {
            let index = state.slots[slot];
            if index != 65535 {
                if index >= state.car_count {
                    unsafe {
                        write_dseg_page(3, page);
                    }
                    return integer::from_i16(-4);
                }
                rows[slot] = scene_mines::HitPeer {
                    present: true,
                    entity: state.cars[index].entity,
                    effect: state.cars[index].ground.effect.as_array(),
                };
            }
            slot += 1;
        }
        status = scene_mines::contact(
            scene,
            &mut state.lifetime,
            &mut state.runtime,
            world,
            &mut player.weapons,
            &scene_mines::Contact {
                target: object,
                frame: g(frame, 26),
                vehicle: scene_mines::HitVehicle {
                    entity: car.entity,
                    player: car.player,
                    protection: at(car.blast, 6),
                    physics: mine::Vehicle {
                        base_position: at(car.motion, 18),
                        velocity: car.motion,
                        angular: at(car.attitude, 42),
                        external: basis,
                        has_external: external != zero,
                        health: car.blast,
                        collision_size: at(player.auxiliary, (own * 36 + 32) as i16),
                    },
                    effect: car.ground.effect.as_array(),
                },
                shield_tails: ptr(0xc000 + 9086 + 88),
                shield_stride: 45,
                peers: peers.as_array(),
                peer_count: 9,
                camera_impulse: ptr(0xc000 + weapon_world::CAMERA_WORD),
                class: g(r, 0),
                stamp: g(at(r, 42), 0),
                resources: at(r, 2),
                sine: player.sine,
                camera: at(r, 12),
                materials: at(r, 30),
            },
        );
        unsafe {
            write_dseg_page(3, page);
        }
    }
    if status == zero {
        attitude::put(state.runtime.slot, 0, saved_slot);
    }
    status
}
