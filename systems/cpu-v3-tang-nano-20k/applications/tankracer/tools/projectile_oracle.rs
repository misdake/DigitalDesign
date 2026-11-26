//! Original projectile methods, with full compact state after each call.
mod car_effects_oracle;
mod car_motion_oracle;
mod contact_effect_oracle;
mod driving_devices;
mod explosion_blasts_oracle;
mod explosion_oracle;
#[allow(dead_code)]
mod identity_assets;
mod mine_oracle;
#[allow(dead_code)]
#[path = "terrain_assets.rs"]
mod pages;
mod pickup_snapshot;
mod player_control_oracle;
mod player_route_oracle;
mod player_tail_oracle;
mod projectile_marks_oracle;
#[allow(dead_code)]
mod reference_world;
mod scene_allocation_oracle;
mod scene_ambient_oracle;
mod scene_creation_oracle;
mod scene_emitter_oracle;
mod scene_explosion_tick_oracle;
mod scene_explosions_oracle;
#[allow(dead_code)]
mod scene_export;
mod scene_fire_oracle;
mod scene_flock_oracle;
mod scene_laps_oracle;
#[allow(dead_code)]
mod scene_lifecycle_assets;
mod scene_mine_explosion_oracle;
mod scene_mine_hit_oracle;
mod scene_mine_tick_oracle;
mod scene_mines_oracle;
mod scene_obstacle_transfer_oracle;
mod scene_player_oracle;
mod scene_projectile_area_oracle;
mod scene_projectile_direct_oracle;
mod scene_projectile_flight_oracle;
mod scene_projectile_ufo_oracle;
mod scene_train_oracle;
mod scene_ufo_transfer_oracle;
mod scene_ufos_oracle;
mod scene_vehicle_transfer_oracle;
mod scene_weapon_explosion_oracle;
mod tank_hit_oracle;
mod temporary_oracle;
mod track_effects_oracle;
mod vehicle_effects_oracle;
mod vehicle_inactive_oracle;
mod vehicle_pose_oracle;
mod vehicle_startup_oracle;
mod vehicle_transfer_oracle;
use serde_json::{Value, json};
use std::{fs, path::Path};
use tankracer_game::{
    math::Matrix,
    object_query::{EntityId, TankId},
    pose::Pose,
    projectile::{
        self, Kind,
        flight::{Action, Carrier, Frame},
        launch::Owner,
    },
    vehicle_runtime::{SinglePlayer, rcc_car_probe},
};
mod projectile_snapshot;
use projectile_snapshot::{ints, sprite, state, wide};
fn mutable(
    s: &projectile::State,
    shared: &projectile::Flight,
    p: Pose,
    seed: u32,
    overlay: Option<[u8; 4]>,
) -> Vec<u16> {
    let mut out = Vec::new();
    state(&mut out, s);
    assert_eq!(out.len(), 610);
    projectile_snapshot::flight(&mut out, shared);
    ints(&mut out, p.matrix.0);
    ints(&mut out, p.position);
    wide(&mut out, seed as i32);
    out.push(u16::from(overlay.is_some()));
    out.extend(overlay.unwrap_or([0; 4]).map(u16::from));
    assert_eq!(out.len(), 695);
    out
}
fn action(out: &mut Vec<u16>, a: Option<Action>) {
    let mut words = vec![0; 41];
    if let Some(a) = a {
        words[0] = match a {
            Action::Flying => 0,
            Action::Trails => 1,
            Action::Retire => 2,
            Action::Expired => 3,
            Action::Impact { .. } => 4,
        };
        if let Action::Impact {
            origin,
            mark,
            contact,
        } = a
        {
            let mut packed = Vec::new();
            ints(&mut packed, origin);
            words[2..8].copy_from_slice(&packed);
            if let Some(mark) = mark {
                words[1] = 1;
                packed.clear();
                ints(&mut packed, mark);
                words[8..14].copy_from_slice(&packed);
            }
            if let Some(hit) = contact {
                words[14] = 1;
                packed.clear();
                wide(&mut packed, hit.vehicle.0 as i32);
                ints(&mut packed, hit.world.point);
                ints(&mut packed, hit.world.normal);
                ints(&mut packed, hit.local.point);
                ints(&mut packed, hit.local.normal);
                words[15..41].copy_from_slice(&packed);
            }
        }
    }
    out.extend(words);
}
fn cars(out: &mut Vec<u16>, world: &SinglePlayer) {
    for i in 0..3 {
        let Some(p) = world.vehicles.entries[i].as_ref() else {
            out.extend([0; 68]);
            continue;
        };
        let v = &p.physics;
        out.extend([
            1,
            u16::from(v.collision.disabled),
            u16::from(p.visible),
            u16::from(p.ai.as_ref().is_some_and(|a| a.flags & 1 != 0)),
        ]);
        wide(out, v.entity.0 as i32);
        ints(out, v.motion.poses.base_position);
        ints(out, v.motion.poses.reference.0);
        ints(out, v.motion.poses.turret.matrix.0);
        ints(out, v.motion.velocity);
        ints(out, p.model.query_bounds[0].min);
        ints(out, p.model.query_bounds[0].max);
        wide(
            out,
            p.model.parts[tankracer_assets::config::Part::Body.slot()]
                .as_ref()
                .map_or(0, |m| i32::from(m.radius) * 16),
        );
    }
    out.extend(
        world
            .vehicles
            .racers
            .map(|r| r.map_or(65535, |r| r.0 as u16)),
    );
    out.push(1);
}
fn input(
    s: &projectile::State,
    shared: &projectile::Flight,
    p: Pose,
    seed: u32,
    overlay: Option<[u8; 4]>,
    r: projectile::Resources,
    owner: Owner,
    world: &SinglePlayer,
) -> Vec<u16> {
    let mut out = mutable(s, shared, p, seed, overlay);
    out.extend(launch_input(r, owner));
    assert_eq!(out.len(), 788);
    cars(&mut out, world);
    assert_eq!(out.len(), 1002);
    out
}
fn launch_input(r: projectile::Resources, owner: Owner) -> Vec<u16> {
    let mut out = Vec::new();
    ints(&mut out, r.tuning.gravity);
    ints(&mut out, r.tuning.speed);
    ints(&mut out, r.tuning.elevation);
    ints(&mut out, r.tuning.snap);
    ints(
        &mut out,
        [
            r.tuning.homing_speed,
            r.tuning.seek_time,
            r.tuning.max_steer,
        ],
    );
    ints(&mut out, r.main.map(|x| x as i32));
    ints(&mut out, r.trail.map(|x| x as i32));
    ints(&mut out, r.homing_tail);
    out.push(r.cup as u16);
    assert_eq!(out.len(), 61);
    wide(&mut out, owner.entity.0 as i32);
    ints(&mut out, owner.turret.matrix.0);
    ints(&mut out, owner.gun);
    ints(&mut out, owner.velocity);
    assert_eq!(out.len(), 93);
    out
}
fn run_seed(
    tag: String,
    mut s: projectile::State,
    mut shared: projectile::Flight,
    mut p: Pose,
    r: projectile::Resources,
    owner: Owner,
    controls: Vec<[i32; 6]>,
    world: &SinglePlayer,
    mut seed: u32,
) -> Value {
    let mut overlay = Some([17, 18, 19, 20]);
    let initial = input(&s, &shared, p, seed, overlay, r, owner, world);
    let mut frames = Vec::new();
    for c in &controls {
        let [op, kind, dt, number, entity, carrier] = *c;
        let mut a = None;
        let status = match op {
            0 => s
                .launch(&mut p, owner, kind_of(kind), r.tuning, &mut seed)
                .map(|drift| {
                    if let Some(d) = drift {
                        shared.homing_drift = d;
                    }
                    s.sprite(r, &mut seed);
                }),
            1 => s
                .emit_trail(p, r, &mut shared.sequence, dt)
                .and_then(|()| s.fade_trails(dt)),
            2 => s.fade_trails(dt),
            3 | 4 => s
                .advance_with_vehicles(
                    &mut p,
                    Frame {
                        dt,
                        number: number as u32,
                        entity: EntityId(entity as u32),
                        carrier: (carrier & 1 != 0).then_some(Carrier {
                            pose: Pose {
                                matrix: owner.turret.matrix,
                                position: owner.gun,
                            },
                            released: carrier & 2 != 0,
                        }),
                    },
                    &mut shared,
                    r,
                    world.terrain(),
                    &mut overlay,
                    Some(&world.vehicles),
                )
                .and_then(|v| {
                    a = Some(v);
                    if op == 4 && matches!(v, Action::Trails | Action::Retire) {
                        s.fade_trails(dt)
                    } else {
                        Ok(())
                    }
                }),
            5 => rcc_car_probe::projectile_target(&mut s, world),
            6 => s.reflect(&mut p, TankId(carrier as usize), &world.vehicles, &mut seed),
            7 => s.reflect_ownerless(EntityId(entity as u32), &mut seed),
            8 => projectile::sweep::cars(&world.vehicles, s.owner, owner.gun, p.position, dt).map(
                |hit| {
                    if let Some(hit) = hit {
                        a = Some(Action::Impact {
                            origin: owner.gun,
                            mark: None,
                            contact: Some(hit),
                        });
                    }
                },
            ),
            _ => panic!("operation"),
        }
        .map_or_else(|e| e, |()| 0);
        let mut out = Vec::new();
        wide(&mut out, status);
        action(&mut out, a);
        out.extend(mutable(&s, &shared, p, seed, overlay));
        assert_eq!(out.len(), 738);
        frames.push(out);
    }
    json!({"tag":tag,"input":initial,"controls":controls,"frames":frames})
}
fn run(
    tag: String,
    s: projectile::State,
    shared: projectile::Flight,
    p: Pose,
    r: projectile::Resources,
    owner: Owner,
    controls: Vec<[i32; 6]>,
    world: &SinglePlayer,
) -> Value {
    run_seed(tag, s, shared, p, r, owner, controls, world, 0x19876543)
}
fn kind_of(kind: i32) -> Kind {
    match kind {
        0 => Kind::Shell,
        1 => Kind::Concussion,
        2 => Kind::Homing,
        3 => Kind::Ufo,
        4 => Kind::Debris,
        5 => Kind::Warp,
        _ => panic!("kind"),
    }
}
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let root = Path::new(&args[0]);
    let out = Path::new(&args[1]);
    let world = &mut reference_world::race(root);
    mine_oracle::export(out, world);
    explosion_oracle::export(out, world);
    explosion_blasts_oracle::export(out, world);
    scene_explosions_oracle::export(root, out);
    scene_explosion_tick_oracle::export(root, out);
    scene_weapon_explosion_oracle::export(root, out);
    scene_mine_explosion_oracle::export(root, out);
    scene_mine_hit_oracle::export(root, out);
    scene_projectile_flight_oracle::export(root, out);
    scene_projectile_area_oracle::export(root, out);
    scene_projectile_direct_oracle::export(root, out);
    scene_ufos_oracle::export(root, out);
    scene_projectile_ufo_oracle::export(root, out);
    projectile_marks_oracle::export(root, out);
    tank_hit_oracle::export(out, world);
    temporary_oracle::export(out);
    player_control_oracle::export(out);
    player_route_oracle::export(root, out);
    player_tail_oracle::export(root, out);
    track_effects_oracle::export(out);
    vehicle_effects_oracle::export(out);
    car_effects_oracle::export(root, out);
    car_motion_oracle::export(out, &reference_world::full_race(root));
    vehicle_startup_oracle::export(out, &reference_world::full_race(root));
    vehicle_inactive_oracle::export(root, out);
    contact_effect_oracle::export(out);
    scene_player_oracle::export(root, out);
    vehicle_pose_oracle::export(out, world);
    vehicle_transfer_oracle::export(root, out);
    scene_vehicle_transfer_oracle::export(root, out);
    scene_obstacle_transfer_oracle::export(root, out);
    scene_ufo_transfer_oracle::export(root, out);
    scene_fire_oracle::export(root, out);
    scene_laps_oracle::export(root, out);
    scene_train_oracle::export(root, out);
    scene_emitter_oracle::export(root, out);
    scene_flock_oracle::export(root, out);
    scene_ambient_oracle::export(root, out);
    scene_allocation_oracle::export(root, out);
    scene_creation_oracle::export(root, out);
    scene_mines_oracle::export(root, out);
    scene_mine_tick_oracle::export(root, out);
    // Three snapshots of the authored model distinguish racer order from nearest hit.
    let mut peer = world.vehicles.entries[0].clone().unwrap();
    peer.physics.entity = EntityId(998);
    peer.physics.player = false;
    peer.slot = 1;
    world.vehicles.entries.push(Some(peer));
    let mut third = world.vehicles.entries[1].clone().unwrap();
    third.physics.entity = EntityId(997);
    third.slot = 2;
    world.vehicles.entries.push(Some(third));
    let original = world.vehicles.entries[..3].to_vec();
    let resources = world.objects.components.projectiles.resources.unwrap();
    let start = world.vehicles.entries[0]
        .as_ref()
        .unwrap()
        .physics
        .motion
        .poses
        .base_position;
    let mut cases = Vec::new();
    for kind in 0..6 {
        for variant in 0..18 {
            for (i, p) in original.iter().enumerate() {
                world.vehicles.entries[i] = p.clone();
                let car = world.vehicles.entries[i].as_mut().unwrap();
                car.ai = None;
                car.physics.motion.poses.base_position =
                    [start[0] + 300000 + i as i32 * 50000, start[1], start[2]];
            }
            world.vehicles.racers = [None; 9];
            for i in 0..3 {
                world.vehicles.racers[i] = Some(TankId(i));
            }
            let mut r = resources;
            r.cup = (variant % 3) as usize;
            let owner = Owner {
                entity: world.vehicles.entries[0].as_ref().unwrap().physics.entity,
                turret: Pose {
                    matrix: Matrix::from_angles([
                        if variant % 4 == 0 {
                            65000
                        } else {
                            variant as u32 * 700
                        },
                        13000,
                        1200,
                    ]),
                    position: start,
                },
                gun: [start[0], start[1] + 50000, start[2]],
                velocity: [100001, -12345, -567890],
            };
            let mut owner = owner;
            let mut s = projectile::State {
                kind: kind_of(kind),
                ..Default::default()
            };
            let mut shared = projectile::Flight::default();
            let mut p = Pose {
                matrix: Matrix::IDENTITY,
                position: owner.gun,
            };
            let control = |op, dt, number, carrier| [op, kind, dt, number, 247, carrier];
            let mut controls = vec![control(0, 0, 0, 0)];
            controls.extend(
                [1, 32, 33, 100, 33, 31, 1, 33]
                    .into_iter()
                    .enumerate()
                    .map(|(i, dt)| control(4, dt, i as i32 + 1, 0)),
            );
            match variant {
                0..=2 => {}
                3 => {
                    owner.gun[1] = start[1] + 100;
                    p.position = owner.gun;
                }
                4 => {
                    owner.gun[0] = i32::MAX / 2;
                    p.position = owner.gun;
                }
                5 => {
                    s.flags = 1;
                    s.trail_count = 15;
                    for t in &mut s.trails {
                        t.active = true;
                        t.sprite.color = [9, 1, 255];
                    }
                    controls = vec![control(4, 31, 1, 0); 8];
                }
                6 => {
                    s.flags = 3;
                    s.flash_views = [1, 1];
                    s.flash_alpha = 3;
                    controls = vec![control(4, 33, 1, 0); 5];
                }
                7 => {
                    s.flags = 4;
                    s.attachment = [12, 45, 78];
                    controls = vec![
                        control(4, 33, 1, 1),
                        control(4, 33, 2, 3),
                        control(4, 33, 3, 0),
                    ];
                }
                8 => {
                    s.flags = 4;
                    controls = vec![control(4, 33, 1, 0)];
                }
                9 => {
                    s.trail_cursor = 15;
                    shared.sequence = 10;
                    controls = vec![control(1, 33, 1, 0)];
                }
                10 => {
                    s.trail_time = 32;
                    shared.sequence = 11;
                    controls = vec![control(1, 1, 1, 0)];
                }
                11 => {
                    s.trail_count = 2;
                    for i in 0..2 {
                        s.trails[i].active = true;
                        s.trails[i].channel = if i == 0 { 0 } else { 99 };
                        s.trails[i].sprite.color = [128, 25, 255];
                    }
                    controls = vec![control(2, 33, 1, 0); 4];
                }
                12 => {
                    s.trails[0].active = true;
                    s.trails[0].channel = u32::MAX;
                    s.trail_count = i16::MIN;
                    controls = vec![control(2, 33, 1, 0); 2];
                }
                13 => {
                    s.time = 6000;
                    s.velocity = [0, 0, 100];
                    controls = vec![control(4, 1, 1, 0), control(4, 33, 2, 0)];
                }
                14 => {
                    s.reference = None;
                    s.time = 33;
                    s.velocity = [0; 3];
                    controls = vec![control(4, 33, 2, 0)];
                }
                15 => {
                    controls = vec![
                        control(0, 0, 0, 0),
                        control(4, -33, 1, 0),
                        control(1, 32767, 2, 0),
                        control(2, 32767, 3, 0),
                    ];
                }
                16 => {
                    s.flags = 16;
                    p.matrix.0 = [0; 9];
                    s.velocity = [12345, -23456, 34567];
                    controls = vec![control(4, 33, 1, 0), control(4, 100, 2, 0)];
                }
                _ => {
                    s.trail_count = i16::MIN;
                    s.trail_cursor = -1;
                    s.trails[0].active = true;
                    s.trails[0].time = i16::MAX;
                    s.trails[0].sprite.color = [0, 1, 255];
                    controls = vec![control(2, 1, 1, 0), control(2, 32767, 2, 0)];
                }
            }
            cases.push(run(
                format!("kind-{kind}-branch-{variant}"),
                s,
                shared,
                p,
                r,
                owner,
                controls,
                world,
            ));
        }
    }
    // Flight with live targets, including steering and actual contact records.
    for kind in 0..6 {
        for variant in 0..5 {
            for (i, p) in original.iter().enumerate() {
                world.vehicles.entries[i] = p.clone();
                let car = world.vehicles.entries[i].as_mut().unwrap();
                car.ai = None;
                car.physics.motion.poses.reference = Matrix::IDENTITY;
                car.physics.motion.poses.turret.matrix = Matrix::IDENTITY;
                car.physics.motion.poses.base_position =
                    [start[0] + i as i32 * 100000, start[1] + 4000, start[2]];
            }
            world.vehicles.racers = [None; 9];
            for i in 0..3 {
                world.vehicles.racers[i] = Some(TankId(i));
            }
            let target = world.vehicles.entries[1].as_mut().unwrap();
            target.physics.motion.poses.base_position =
                [start[0] + 2000, start[1] + 4000, start[2] + 25000];
            if variant == 1 {
                target.physics.motion.poses.base_position[2] += 300000;
            }
            if variant == 3 {
                target.physics.motion.poses.base_position =
                    [start[0], start[1] + 3850, start[2] + 1500];
            }
            let target_entity = target.physics.entity;
            let owner = Owner {
                entity: world.vehicles.entries[0].as_ref().unwrap().physics.entity,
                turret: Pose {
                    matrix: Matrix::IDENTITY,
                    position: start,
                },
                gun: [start[0], start[1] + 4000, start[2]],
                velocity: [0; 3],
            };
            let mut seed = 0x19876543;
            let mut state = projectile::State::default();
            let mut pose = Pose {
                matrix: Matrix::IDENTITY,
                position: start,
            };
            let drift = state
                .launch(&mut pose, owner, kind_of(kind), resources.tuning, &mut seed)
                .unwrap();
            state.sprite(resources, &mut seed);
            state.target = Some(if variant == 2 {
                EntityId(123456)
            } else {
                target_entity
            });
            state.time = resources.tuning.seek_time.wrapping_add(1);
            let shared = projectile::Flight {
                homing_drift: drift.unwrap_or(0),
                ..Default::default()
            };
            let controls = (0..8)
                .map(|i| {
                    [
                        4,
                        kind,
                        33,
                        if variant == 4 { i / 2 + 1 } else { i + 1 },
                        247,
                        0,
                    ]
                })
                .collect();
            cases.push(run_seed(
                format!("target-flight-{kind}-{variant}"),
                state,
                shared,
                pose,
                resources,
                owner,
                controls,
                world,
                seed,
            ));
        }
    }
    // Source-slot order, moving targets, inside-start backtracking and misses.
    for variant in 0..12 {
        for (i, p) in original.iter().enumerate() {
            world.vehicles.entries[i] = p.clone();
            let car = world.vehicles.entries[i].as_mut().unwrap();
            car.ai = None;
            car.physics.motion.poses.base_position =
                [start[0], start[1], start[2] + i as i32 * 100];
            car.physics.motion.poses.reference = Matrix::IDENTITY;
            car.physics.motion.poses.turret.matrix = Matrix::IDENTITY;
            car.physics.motion.velocity = [0; 3];
        }
        world.vehicles.racers = [None; 9];
        for i in 0..3 {
            world.vehicles.racers[i] = Some(TankId(i));
        }
        let mut owner = Owner {
            entity: EntityId(999),
            turret: Pose {
                matrix: Matrix::IDENTITY,
                position: start,
            },
            gun: [start[0], start[1], start[2] + 10000],
            velocity: [0; 3],
        };
        let mut p = Pose {
            matrix: Matrix::IDENTITY,
            position: [start[0], start[1], start[2] - 10000],
        };
        let mut s = projectile::State {
            kind: Kind::Homing,
            ..Default::default()
        };
        match variant {
            0 => {}
            1 => {
                owner.gun = start;
            }
            2 => {
                p.position = owner.gun;
            }
            3 => {
                world.vehicles.entries[0]
                    .as_mut()
                    .unwrap()
                    .physics
                    .motion
                    .velocity = [0, 0, 300000];
            }
            4 => {
                s.owner = Some(world.vehicles.entries[0].as_ref().unwrap().physics.entity);
            }
            5 => {
                world.vehicles.entries[0]
                    .as_mut()
                    .unwrap()
                    .physics
                    .collision
                    .disabled = true;
            }
            6 => {
                world.vehicles.entries[0] = None;
            }
            7 => {
                owner.gun[0] += 100000;
                p.position[0] += 100000;
            }
            8 => {
                s.owner = Some(world.vehicles.entries[0].as_ref().unwrap().physics.entity);
                world.vehicles.entries[1]
                    .as_mut()
                    .unwrap()
                    .physics
                    .motion
                    .poses
                    .base_position[2] += 60000;
            }
            9 => {
                s.owner = Some(EntityId(999));
                s.target = Some(EntityId(77));
            }
            10 => {
                s.kind = Kind::Shell;
            }
            _ => {
                s.owner = Some(world.vehicles.entries[0].as_ref().unwrap().physics.entity);
                world.vehicles.entries[1]
                    .as_mut()
                    .unwrap()
                    .physics
                    .motion
                    .poses
                    .base_position[0] += 100000;
            }
        }
        let op = if variant >= 8 { 5 } else { 8 };
        cases.push(run(
            format!("sweep-target-{variant}"),
            s,
            projectile::Flight::default(),
            p,
            resources,
            owner,
            vec![[op, 0, 33, 1, 247, 0]],
            world,
        ));
    }
    // Exercise both deflection branches and the rarer roll-zero redirection.
    let mut seeds = Vec::new();
    let mut candidate = 1u32;
    while seeds.len() < 8 {
        let mut random = candidate;
        let roll = (tankracer_game::random::next(&mut random) >> 4) & 65535;
        if roll % 10 == 0 || seeds.len() < 4 {
            seeds.push(candidate);
        }
        candidate = candidate.wrapping_mul(1664525).wrapping_add(1013904223);
    }
    for kind in 0..6 {
        for variant in 0..9 {
            for (seed_index, seed) in seeds.iter().copied().enumerate() {
                for (i, p) in original.iter().enumerate() {
                    world.vehicles.entries[i] = p.clone();
                    let car = world.vehicles.entries[i].as_mut().unwrap();
                    car.ai = None;
                    car.visible = true;
                    car.physics.motion.poses.base_position = [
                        start[0] - i as i32 * 5000,
                        start[1],
                        start[2] - i as i32 * 10000,
                    ];
                }
                world.vehicles.racers = [None; 9];
                for i in 0..3 {
                    world.vehicles.racers[i] = Some(TankId(i));
                }
                let mut s = projectile::State {
                    kind: kind_of(kind),
                    owner: Some(world.vehicles.entries[0].as_ref().unwrap().physics.entity),
                    velocity: [123456, -23456, 345678],
                    ..Default::default()
                };
                let owner = Owner {
                    entity: EntityId(999),
                    turret: Pose {
                        matrix: Matrix::IDENTITY,
                        position: start,
                    },
                    gun: start,
                    velocity: [0; 3],
                };
                let mut target = 1;
                match variant {
                    0 => {}
                    1 => {
                        s.owner = None;
                    }
                    2 => {
                        s.owner = Some(EntityId(123456));
                    }
                    3 => {
                        world.vehicles.racers[0] = None;
                    }
                    4 => {
                        world.vehicles.entries[2].as_mut().unwrap().visible = false;
                    }
                    5 => {
                        world.vehicles.entries[2] = None;
                    }
                    6 => {
                        s.velocity = [i32::MIN, i32::MAX, 0];
                    }
                    7 => {
                        world.vehicles.entries[0]
                            .as_mut()
                            .unwrap()
                            .physics
                            .motion
                            .poses
                            .base_position[0] += 100000;
                    }
                    _ => {
                        target = 2;
                        world.vehicles.entries[2] = None;
                    }
                }
                cases.push(run_seed(
                    format!("reflect-{kind}-{variant}-{seed_index}"),
                    s,
                    projectile::Flight::default(),
                    Pose {
                        matrix: Matrix::IDENTITY,
                        position: start,
                    },
                    resources,
                    owner,
                    vec![[6, kind, 0, 0, 0, target]; 3],
                    world,
                    seed,
                ));
            }
        }
    }
    for kind in 0..6 {
        for present in 0..2 {
            let s = projectile::State {
                kind: kind_of(kind),
                owner: (present != 0).then_some(EntityId(123)),
                velocity: [123456, -23456, 345678],
                ..Default::default()
            };
            let owner = Owner {
                entity: EntityId(999),
                turret: Pose {
                    matrix: Matrix::IDENTITY,
                    position: start,
                },
                gun: start,
                velocity: [0; 3],
            };
            cases.push(run(
                format!("ownerless-{kind}-{present}"),
                s,
                projectile::Flight::default(),
                Pose {
                    matrix: Matrix::IDENTITY,
                    position: start,
                },
                resources,
                owner,
                vec![[7, kind, 0, 0, 42, 0]; 4],
                world,
            ));
        }
    }
    let mut cells = Vec::new();
    for i in 0..64 {
        cells.extend(pages::cell_bytes(world.terrain(), i));
    }
    fs::write(out.join("projectile-cells.bin"), cells).unwrap();
    fs::write(out.join("projectile.json"),serde_json::to_vec(&json!({"cases":cases,"scope":"Original launch/sprite, full free-flight/vehicle sweeps, targeting, reflection, trails and shared glow state on real ENGLAND terrain; entity creation and impact callbacks are separate"})).unwrap()).unwrap();
}
