//! Ordered world update over persistent game-owned storage.
#![link_cseg = "frame"]
use super::*;

// These views are bound once after construction. They borrow the existing
// owners in place; frame updates neither clone state nor install callbacks.
pub struct Runtime {
    pub parents: Array<scene_parent::State>,
    pub players: Array<player_world::Player>,
    pub pickups: Array<Pickups>,
    pub vehicles: Array<car_runtime::State>,
    pub impacts: Array<impact::State>,
    pub effects: Array<impact::Context>,
    pub surface: Array<deformation::Surface>,
    pub prefix_shared: Array<prefix::Shared>,
    pub components: Array<scenery_contact::Components>,
    pub parent_data: Array<u16>,
    pub deform: Array<deform_contact::Component>,
    pub camera: Array<u16>,
    pub prefix_config: Array<u16>,
    pub prefixes: Array<prefix::State>,
    pub events: Array<u16>,
    pub binding: Array<u16>,
    pub peer_controls: Array<u16>,
    pub catalog_scratch: Array<u16>,
    pub inverse_cosine: Array<i16>,
    pub entity_limit: u16,
    pub deform_count: u16,
    pub scenery_enabled: bool,
    pub deformation_enabled: bool,
    pub player_enabled: bool,
    pub prefix_enabled: bool,
    pub multiple_vehicles: bool,
}

pub struct Frame {
    pub input: Array<u16>,
    pub ordered: bool,
    pub finishing: bool,
}

pub fn tick(
    runtime: &mut Runtime,
    c: &driving::Config,
    scene: &scene_query::Scene,
    scenery: &mut scene_world::World,
    frame: &Frame,
    peers: &mut tank_response::World,
) {
    let record = frame.input;
    let state = &mut runtime.parents[0u16];
    let player = &mut runtime.players[0u16];
    let pickups = &mut runtime.pickups[0u16];
    let complete = &mut runtime.vehicles[0u16];
    let impacts = &mut runtime.impacts[0u16];
    let effects = &mut runtime.effects[0u16];
    let surface = &mut runtime.surface[0u16];
    let prefix_shared = &runtime.prefix_shared[0u16];
    let components = &runtime.components[0u16];
    let mut actors = state.cars;
    let mut parent_actors = state.actors;
    let parent_data = runtime.parent_data;
    let mut deform = runtime.deform;
    let aux = player.auxiliary;
    let mut geo = peers.geometry;
    let saved_slot = attitude::get(state.runtime.slot, 0);
    let mut pass = lifecycle::begin(&state.runtime.ticks, components.work_limit);
    loop {
        let mut data = record;
        let mut own = attitude::get(data, 27).hi_bits();
        if frame.ordered {
            let progress =
                lifecycle::prepare(&mut state.runtime.ticks, &mut pass, components.work_limit);
            if progress == 0 {
                break;
            }
            if progress != 1 {
                halt(0x7f1e);
            }
            let kind = entities::handle_kind(&state.lifetime.identities, pass.current);
            if kind == 1 {
                let entity = pass.current;
                if entity.hi_bits() != 0 || entity.lo_bits() >= runtime.entity_limit {
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
                    number: if runtime.player_enabled {
                        attitude::get(data, 26)
                    } else {
                        fix32::zero()
                    },
                    camera: runtime.camera,
                    peer_basis: live_peer(
                        components,
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
                    status = weapon_world::tick(scene, state, scenery, player, data, entity);
                } else if scene_ambient_world::contains(object) {
                    status = scene_ambient_world::tick(scene, state, scenery, player, data, entity);
                } else if scene_flock_world::contains(object) {
                    status = scene_flock_world::tick(scene, state, scenery, player, data, entity);
                } else if scene_trains::contains(object) {
                    status = scene_trains::tick(scene, state, scenery, player, data, entity);
                } else if weapon_emitters::contains(object) {
                    status = weapon_emitters::tick(scene, state, scenery, player, data, entity);
                } else if pickup_index(pickups, object) != 65535 {
                    status = pickup_tick(scene, state, scenery, components, pickups, &frame);
                } else if selected == 65535 {
                    status = scenery_contact::tick(
                        scene,
                        scenery,
                        components,
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
                        inverse_cosine: runtime.inverse_cosine,
                    };
                    status = scene_parent::tick(scene, state, &mut scenery.grid, actor, &tick);
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
                if !runtime.multiple_vehicles {
                    halt(0x7f74);
                }
                player_world::tank_input(&actors[own], player, record, runtime.peer_controls);
                data = runtime.peer_controls;
            }
            attitude::put(state.runtime.slot, 0, integer::from_i16(12));
        }
        if own == state.slots[0u16]
            && player.finish_stage == 0
            && attitude::get(at(player.fields, (own * 30) as i16), 12) == integer::from_i16(3)
        {
            let entity = finish_world::entity(state);
            if entity >= fix32::zero() {
                if finish_world::transfer(state, player, complete, pickups, own, entity)
                    != fix32::zero()
                {
                    halt(0x7f98);
                }
                own = 2;
                attitude::put(state.runtime.slot, 0, integer::from_i16(12));
                player_world::tank_input(&actors[own], player, record, runtime.peer_controls);
                data = runtime.peer_controls;
                peers.geometry_count = state.car_count;
                peers.vehicle_count = state.car_count;
            }
        }
        let mut far_finish = false;
        if own == 2 && player.finish_stage != 0 {
            let status = finish_driver::before(scene, state, scenery, player, complete, data);
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
            let status = player_world::prepare(scene, state, scenery, complete, player, data);
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
            let status = player_input::sample(complete, &mut actors[own], c, data);
            if status != fix32::zero() {
                unsafe {
                    addr_of(&DRIVING_FIELD).write(0, status.lo_bits());
                }
                halt(0x7f58);
            }
        }
        let mut peer_rows = peers.vehicles;
        let mut peer = 0u16;
        while peer < state.car_count {
            peer_rows[peer] =
                collision_car(&mut actors[peer], player, peer, at(aux, (peer * 36) as i16));
            peer += 1;
        }
        if !far_finish {
            if complete.enabled && !actors[own].visible {
                let status = car_runtime::inactive(
                    complete,
                    &mut actors[own],
                    c,
                    data,
                    &mut state.map,
                    effects,
                );
                if status != fix32::zero() {
                    unsafe {
                        addr_of(&DRIVING_FIELD).write(0, status.lo_bits());
                    }
                    halt(0x7f55);
                }
            } else {
                if runtime.prefix_enabled {
                    let prefix_page = read_dseg_page(3);
                    if complete.enabled {
                        unsafe {
                            write_dseg_page(3, complete.page);
                        }
                        copy(
                            runtime.camera,
                            unsafe { Ptr::from_addr(0xc000 + 10174).as_u16_array() },
                            6,
                        );
                    }
                    if complete.enabled {
                        let fields = unsafe {
                            Ptr::from_addr(0xc000 + car_runtime::INPUT_WORD + own * 26)
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
                        camera: runtime.camera,
                        sine: c.sine,
                    };
                    let status = prefix::prepare(
                        &runtime.prefixes[own],
                        prefix_shared,
                        &step,
                        runtime.prefix_config,
                        scene,
                        &state.lifetime.identities,
                    );
                    if complete.enabled {
                        car_runtime::publish_prefix(
                            &actors[own],
                            c,
                            &runtime.prefixes[own],
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
                if runtime.player_enabled {
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
                    runtime.deform_count,
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
                        runtime.events,
                        surface,
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
                        runtime.events,
                        &mut deform[selected].sampling,
                    );
                }
                if status != 0 {
                    unsafe {
                        addr_of(&DRIVING_FIELD).write(0, status);
                    }
                    halt(0x7f04);
                }
                if runtime.scenery_enabled {
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
                    let mut status = scenery_contact::query(&v, scenery, components, &mut hit);
                    if status == integer::from_i16(1) {
                        let selected = scene_auxiliaries::find(parent_data, hit.object.lo_bits());
                        if scene_creation::component_kind(&player.weapons, hit.object.lo_bits())
                            == 2
                        {
                            status = weapon_mines::contact(
                                scene,
                                state,
                                scenery,
                                player,
                                data,
                                hit.object.lo_bits(),
                            );
                        } else if pickup_index(pickups, hit.object.lo_bits()) != 65535 {
                            let call = PickupContact {
                                vehicle: v,
                                target: hit.object.lo_bits(),
                                own: own,
                                mode: attitude::get(data, 1),
                                number: if runtime.player_enabled {
                                    attitude::get(data, 26)
                                } else {
                                    fix32::zero()
                                },
                                camera: runtime.camera,
                            };
                            status =
                                pickup_contact(scene, state, scenery, components, pickups, &call);
                        } else if selected == 65535 {
                            status = scenery_contact::method(
                                &v,
                                scene,
                                scenery,
                                components,
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
                                            runtime.catalog_scratch,
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
                                        state,
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
                            attitude::get(car.motion, i) - attitude::get(before.as_array(), i),
                        );
                        i += 1;
                    }
                    attitude::put(actor_aux, 17, collision::distance(before.as_array()));
                }
                effects.frame = attitude::get(data, 26);
                let mut peer = 0u16;
                let mut views = peers.vehicles;
                while peer < state.car_count {
                    query_car(
                        &mut actors[peer],
                        &mut geo[peer],
                        runtime.binding,
                        peers.present,
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
                let status = tank_response::cars(own, peers, &env, impacts, effects);
                if status != fix32::zero() {
                    unsafe {
                        addr_of(&DRIVING_FIELD).write(0, status.lo_bits());
                    }
                    halt(0x7f0a);
                }
                let current = collision_car(&mut actors[own], player, own, actor_aux);
                let car = &mut actors[own];
                let mut contact = deform_contact::Vehicle {
                    component_count: runtime.deform_count,
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
                    current_slot: state.runtime.slot,
                    impact: current.impact,
                };
                if runtime.deformation_enabled {
                    let status = deform_contact::respond(
                        &mut contact,
                        scene,
                        scenery,
                        deform,
                        impacts,
                        effects,
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
                let status = ground_response::step(&mut response, impacts, effects);
                if status != 0 {
                    unsafe {
                        addr_of(&DRIVING_FIELD).write(0, status);
                    }
                    halt(0x7f07);
                }
                if complete.enabled {
                    let motion_status =
                        car_runtime::advance(complete, car, c, data, &mut response.impact, effects);
                    if runtime.player_enabled {
                        let post = player_world::finish(
                            state,
                            player,
                            data,
                            gun_before.as_array(),
                            keys,
                            motion_status == fix32::zero(),
                        );
                        if post != fix32::zero() {
                            unsafe {
                                addr_of(&DRIVING_FIELD).write(0, post.lo_bits());
                            }
                            halt(0x7f68);
                        }
                    }
                    if motion_status != fix32::zero() {
                        unsafe {
                            addr_of(&DRIVING_FIELD).write(0, motion_status.lo_bits());
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
                        pickups,
                        car,
                        attitude::get(data, 26),
                        attitude::get(data, 0),
                    ) != fix32::zero()
                    {
                        halt(0x7f50);
                    }
                    let status = impact::tick(
                        impacts,
                        &mut response.impact,
                        attitude::get(data, 0),
                        effects,
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
            let status = finish_driver::after(state, scenery, player, data);
            if status != fix32::zero() {
                unsafe {
                    addr_of(&DRIVING_FIELD).write(0, (-status).lo_bits());
                }
                halt(0x7f9a);
            }
            if player.finish_stage == 1 {
                finish_world::started(state, player);
            }
        }
        if frame.ordered {
            attitude::put(state.runtime.slot, 0, saved_slot);
            lifecycle::resume(&mut pass, &state.runtime.ticks);
        } else {
            if frame.finishing {
                attitude::put(state.runtime.slot, 0, saved_slot);
            }
            break;
        }
    }
    if frame.ordered {
        if scene_auxiliaries::drain(scene, state, scenery, parent_data, &mut player.weapons)
            != fix32::zero()
        {
            halt(0x7f18);
        }
    }
}
