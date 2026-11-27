//! Projectile flight at its position in the persistent entity tick pass.
#![link_cseg = "world"]
use super::*;
pub const OVERLAY_WORD: u16 = 15248;
pub const MARK_WORD: u16 = 15256;
pub const CAMERA_WORD: u16 = 15312;
fn ptr(word: u16) -> Array<u16> {
    unsafe { Ptr::from_addr(word).as_u16_array() }
}
pub fn tick(
    scene: &scene_query::Scene,
    state: &mut scene_parent::State,
    world: &mut scene_world::World,
    player: &mut player_world::Player,
    frame: Array<u16>,
    entity: fix32,
) -> fix32 {
    let zero = integer::from_i16(0);
    let index = scene_lifecycle::object(&state.lifetime, entity);
    if index < zero {
        return index;
    }
    let object = index.lo_bits();
    let class = entities::class(&state.lifetime.identities, entity);
    if class < zero || class >= fix32::from_words(scene.header[4u16], 0) {
        return integer::from_i16(-1);
    }
    let saved_slot = attitude::get(state.runtime.slot, 0);
    attitude::put(state.runtime.slot, 0, integer::from_i16(12));
    let method = scene.method(class.lo_bits(), 12);
    if method.status == 4 {
        return integer::from_i16(-1);
    }
    if method.status == 2 {
        return zero;
    }
    if method.status == 0 {
        attitude::put(state.runtime.slot, 0, saved_slot);
        return zero;
    }
    let kind = scene_creation::component_kind(&player.weapons, object);
    if method.status != 1
        || (kind == 1 && method.id != 117)
        || (kind == 2 && method.id != 120)
        || (kind == 3 && method.id != 93)
        || (kind == 4 && method.id != 125)
        || (kind != 1 && kind != 2 && kind != 3 && kind != 4)
        || state.car_count != 2
    {
        return integer::from_i16(-5);
    }
    if kind == 4 {
        let status = weapon_ufos::tick(scene, state, world, player, frame, entity);
        if status == zero {
            attitude::put(state.runtime.slot, 0, saved_slot);
        }
        return status;
    }
    if kind == 2 {
        let status = weapon_mines::tick(scene, state, world, player, frame, entity);
        if status == zero {
            attitude::put(state.runtime.slot, 0, saved_slot);
        }
        return status;
    }
    let header: Buf<u16, 56> = Buf::new([0; 56]);
    let h = header.as_array();
    read(h, player.context, zero, 56);
    let resources: Buf<u16, 44> = Buf::new([0; 44]);
    let r = resources.as_array();
    read(r, player.context, fix32::from_words(h[38u16], 0), 44);
    if kind == 3 {
        let a = &state.cars[0u16];
        let b = &state.cars[1u16];
        let cars: Buf<scene_explosions::Vehicle, 2> = Buf::new([
            scene_explosions::Vehicle {
                present: true,
                entity: a.entity,
                position: at(a.motion, 18),
                basis: a.attitude,
            },
            scene_explosions::Vehicle {
                present: true,
                entity: b.entity,
                position: at(b.motion, 18),
                basis: b.attitude,
            },
        ]);
        let status = scene_explosions::dispatch(
            scene,
            &mut state.lifetime,
            &mut state.runtime,
            world,
            &mut player.weapons,
            &scene_explosions::Tick {
                call: scene_creation::Call { entity, slot: 12 },
                dt: attitude::get(frame, 0),
                resources: at(r, 2),
                sine: player.sine,
                simulation: true,
                vehicles: cars.as_array(),
                vehicle_count: 2,
            },
        );
        if status == zero {
            attitude::put(state.runtime.slot, 0, saved_slot);
        }
        return status;
    }
    let parameters: Buf<u16, 61> = Buf::new([0; 61]);
    let bounds: Buf<u16, 28> = Buf::new([0; 28]);
    read(
        parameters.as_array(),
        player.context,
        fix32::from_words(h[24u16], 0),
        61,
    );
    read(
        bounds.as_array(),
        player.context,
        fix32::from_words(h[32u16], 0),
        28,
    );
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(3, player.weapons.effect_page);
    }
    let empty = projectile_sweep::Car {
        entity: zero,
        present: false,
        disabled: false,
        visible: false,
        ai_disabled: false,
        position: ptr(0),
        basis: ptr(0),
        turret: ptr(0),
        velocity: ptr(0),
        bounds: ptr(0),
        radius: zero,
    };
    let cars: Buf<projectile_sweep::Car, 2> = Buf::new([empty; 2]);
    let mut rows = cars.as_array();
    let mut i = 0u16;
    while i < 2 {
        let car = &state.cars[i];
        let effects = ptr(0xc000 + 6810 + i * 956);
        rows[i] = projectile_sweep::Car {
            entity: car.entity,
            present: true,
            disabled: attitude::get(at(effects, 950), 0) != zero,
            visible: car.visible,
            ai_disabled: false,
            position: at(car.motion, 18),
            basis: car.attitude,
            turret: effects,
            velocity: car.motion,
            bounds: at(bounds.as_array(), (i * 14) as i16),
            radius: attitude::get(at(bounds.as_array(), (i * 14) as i16), 6),
        };
        i += 1;
    }
    let p = parameters.as_array();
    let mut flight = scene_projectiles::Frame {
        object,
        flight: projectile_flight::Frame {
            dt: attitude::get(frame, 0),
            number: attitude::get(frame, 26),
            entity,
            carrier_present: false,
            carrier_basis: ptr(0),
            carrier_position: ptr(0),
            carrier_released: false,
            cars: projectile_sweep::Cars {
                records: cars.as_array(),
                count: 2,
                slots: state.slots,
                present: true,
            },
            overlay: ptr(0xc000 + OVERLAY_WORD),
        },
        context: projectile::Context {
            shared: projectile::shared(ptr(0xc000 + 10468)),
            resources: projectile::Resources {
                tuning: p,
                main: at(p, 30),
                trail: at(p, 42),
                homing_tail: at(p, 54),
                cup: p[60u16],
            },
            tables: projectile::Tables {
                sine: player.sine,
                angle: player.angle,
            },
            seed: state.runtime.seed,
        },
        map: view_of(&mut state.map),
        terrain_present: true,
        resources_present: true,
    };
    let empty_ufo = scene_ufos::Car {
        present: false,
        player: false,
        slot: 0,
        disabled: false,
        position: ptr(0),
        basis: ptr(0),
    };
    let ufo_cars: Buf<scene_ufos::Car, 2> = Buf::new([empty_ufo; 2]);
    let mut ufo_rows = ufo_cars.as_array();
    i = 0;
    while i < 2 {
        let car = &state.cars[i];
        ufo_rows[i] = scene_ufos::Car {
            present: true,
            player: car.player,
            slot: car.slot,
            disabled: rows[i].disabled,
            position: at(car.motion, 18),
            basis: car.attitude,
        };
        i += 1;
    }
    let carriers = scene_ufos::Cars {
        records: ufo_cars.as_array(),
        count: 2,
        present: true,
    };
    let mut action = projectile_flight::zero();
    let mut status = scene_ufos::carrier(
        &state.lifetime,
        &player.weapons,
        object,
        &carriers,
        &mut flight.flight,
    );
    if status == zero {
        status = scene_projectiles::advance(world, &player.weapons, &flight, &mut action);
    }
    if status == zero {
        if action.kind == 4 {
            let a = &state.cars[0u16];
            let b = &state.cars[1u16];
            let cars: Buf<scene_projectile_impact::Car, 2> = Buf::new([
                scene_projectile_impact::Car {
                    present: true,
                    entity: a.entity,
                    player: a.player,
                    visible: a.visible,
                    base_position: at(a.motion, 18),
                    physical_position: at(a.motion, 6),
                    velocity: a.motion,
                    angular: at(a.attitude, 42),
                    blast: a.blast,
                    marker: a.marker,
                },
                scene_projectile_impact::Car {
                    present: true,
                    entity: b.entity,
                    player: b.player,
                    visible: b.visible,
                    base_position: at(b.motion, 18),
                    physical_position: at(b.motion, 6),
                    velocity: b.motion,
                    angular: at(b.attitude, 42),
                    blast: b.blast,
                    marker: b.marker,
                },
            ]);
            let model = player.hit_model.as_array();
            let hits: Buf<scene_explosions::HitVehicle, 2> = Buf::new([
                scene_explosions::HitVehicle {
                    present: true,
                    entity: a.entity,
                    velocity: a.motion,
                    spin: at(player.prefixes, 34),
                    has_body: model[0u16] != 0,
                    radius: model[6u16] as i16,
                },
                scene_explosions::HitVehicle {
                    present: true,
                    entity: b.entity,
                    velocity: b.motion,
                    spin: at(player.prefixes, 84),
                    has_body: model[7u16] != 0,
                    radius: model[13u16] as i16,
                },
            ]);
            let impact = scene_projectile_impact::Impact {
                object,
                action: view_of(&mut action),
                resources_present: true,
                terrain_present: true,
                simulation: true,
                crater: attitude::get(at(r, 40), 0),
                map: view_of(&mut state.map),
                marks: projectile_marks::State {
                    core: ptr(0x8000 + MARK_WORD),
                },
                class: attitude::get(r, 0),
                resources: at(r, 2),
                camera: at(r, 12),
                materials: at(r, 30),
                frame: attitude::get(frame, 26),
                stamp: attitude::get(at(r, 42), 0),
                sine: player.sine,
                hits: hits.as_array(),
                cars: cars.as_array(),
                vehicle_count: 2,
                has_vehicles: true,
                slots: state.slots,
                camera_impulse: ptr(0xc000 + CAMERA_WORD),
            };
            let page = read_dseg_page(3);
            unsafe {
                write_dseg_page(
                    3,
                    scene_creation::page(
                        &player.weapons,
                        1,
                        player.weapons.directory[object * player.weapons.directory_stride + 1],
                    ),
                );
            }
            let shot = at(scene_creation::record(&player.weapons, object, 3), 30);
            let weapon = attitude::get(shot, 2);
            unsafe {
                write_dseg_page(3, page);
            }
            let mut reflected = zero;
            let mut handled = false;
            if action.has_contact {
                let target = action.contact.vehicle;
                if target.hi_bits() != 0 || target.lo_bits() >= 2 {
                    status = integer::from_i16(-4);
                } else {
                    if weapon == integer::from_i16(3) {
                        let result = weapon_ufos::attach(scene, state, world, player, &impact, h);
                        if result < zero {
                            status = result;
                        } else {
                            handled = result == integer::from_i16(1);
                        }
                    }
                    if status == zero && !handled {
                        reflected = scene_projectiles::reflect(
                            world,
                            &player.weapons,
                            &scene_projectiles::Reflection {
                                object,
                                vehicle: target.lo_bits(),
                                cars: flight.flight.cars,
                                spin: hits.as_array()[target.lo_bits()].spin,
                                context: flight.context,
                                simulation: true,
                            },
                        );
                        if reflected < zero {
                            status = reflected;
                        }
                    }
                }
            }
            if status == zero && !handled && reflected == zero {
                status = scene_projectile_impact::begin(
                    scene,
                    &mut state.lifetime,
                    &mut state.runtime,
                    world,
                    &mut player.weapons,
                    &impact,
                );
                if status >= zero {
                    status = zero;
                    if weapon == integer::from_i16(1) || weapon == integer::from_i16(4) {
                        status =
                            scene_projectile_impact::area(&state.runtime, &player.weapons, &impact);
                    } else {
                        status =
                            weapon_hits::apply(state, player, &impact, p[60u16], h[38u16] + 44);
                    }
                }
            }
        } else {
            status = scene_projectiles::finish(
                scene,
                &mut state.lifetime,
                &mut state.runtime,
                world,
                &mut player.weapons,
                &scene_projectiles::Finish {
                    call: scene_creation::Call { entity, slot: 12 },
                    object,
                    dt: attitude::get(frame, 0),
                    action: action.kind,
                },
            );
        }
    }
    unsafe {
        write_dseg_page(3, saved);
    }
    if status == zero {
        attitude::put(state.runtime.slot, 0, saved_slot);
    }
    status
}
