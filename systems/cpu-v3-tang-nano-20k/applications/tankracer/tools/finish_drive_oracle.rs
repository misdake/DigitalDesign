//! Finish controls invoked on the original Player and actual ENGLAND route.
mod finish_scene_assets;
#[path = "reference_world.rs"]
mod reference_world;
use serde_json::{Value, json};
use std::{fs, path::Path};
use tankracer_game::{
    ai, math::Matrix, object_query::EntityId, scene_entity::Handle, symbols::Binding,
    vehicle_runtime::SinglePlayer, vehicle_startup::Player, waypoint,
};

fn controls(car: &Player) -> [i32; 3] {
    let i = car.input.drive;
    [i.throttle, i.brake, i.steering]
}
fn route(car: &Player) -> [i32; 11] {
    let r = car.route;
    [
        i32::from(r.checkpoint.is_some()),
        r.checkpoint.map_or(0, |e| e.0 as i32),
        r.passed as i32,
        r.completed,
        r.records[0],
        r.records[1],
        r.records[2],
        r.score,
        r.previous_score,
        i32::from(r.wrong_way),
        i32::from(r.warning_timer),
    ]
}
fn race(car: &Player) -> [i32; 3] {
    [
        car.race.elapsed,
        i32::from(car.race.laps),
        i32::from(car.race.driving),
    ]
}
fn capture(world: &SinglePlayer, car: &Player, op: u32, checkpoint: EntityId, dt: &[i32]) -> Value {
    let source: Vec<_> = world
        .objects
        .components
        .waypoints
        .points
        .iter()
        .enumerate()
        .filter_map(|(i, p)| p.as_ref().map(|p| (i, p)))
        .collect();
    let mut points = Vec::new();
    let mut colliders = Vec::new();
    let capacity = world.identity.entities.capacity() as usize;
    assert!(capacity <= 2048);
    let mut ids = vec![0u16; capacity * 4];
    for (compact, &(i, p)) in source.iter().enumerate() {
        points.extend(finish_scene_assets::point(p));
        let c = &world.objects.world.colliders[i];
        assert_eq!(
            world.identity.entities.record(c.entity).unwrap().object,
            Some(Handle::Obstacle(tankracer_game::object_query::ObstacleId(
                i
            )))
        );
        let at = c.entity.0 as usize * 4;
        ids[at] = 5;
        ids[at + 2] = compact as u16;
        colliders.push(c.entity.0 as i32);
        colliders.extend(c.pose.position);
        colliders.extend(c.pose.matrix.0);
        colliders.extend(c.external_basis.unwrap_or(Matrix([0; 9])).0);
        colliders.extend(c.bounds.min);
        colliders.extend(c.bounds.max);
        colliders.extend([i32::from(c.radius), i32::from(c.external_basis.is_some())]);
    }
    let tuning = finish_scene_assets::tuning(world);
    json!({"state":finish_scene_assets::state(car),"controls":controls(car),"horn":[i32::from(car.horn_down)],"route":route(car),"race":race(car),"position":car.physics.motion.poses.base_position,
        "velocity":car.physics.motion.velocity,"reference":car.physics.motion.poses.reference.0,
        "params":[op as i32,car.physics.powertrain.local_velocity[2],i32::from(car.race.laps),
            i32::from(car.visible),i32::from(car.ai.is_some()),checkpoint.0 as i32],
        "dt":dt,"seed":0x1234abcd_u32,"points":points,"colliders":colliders,"ids":ids,
        "capacity":capacity,"count":source.len(),"object_ids":source.iter().map(|(id,_)| *id).collect::<Vec<_>>(),"world_count":world.objects.world.colliders.len(),"config":tuning})
}
fn case(
    world: &SinglePlayer,
    mut car: Player,
    op: u32,
    checkpoint: EntityId,
    dt: &[i32],
    tag: &str,
) -> Value {
    let mut record = capture(world, &car, op, checkpoint, dt);
    let routes = waypoint::View {
        routes: &world.objects.components.waypoints,
        world: &world.objects.world,
        entities: &world.identity.entities,
    };
    let config = world.vehicles.ai_config[world.vehicles.cup * 5];
    let global = world.vehicles.ai_settings;
    let rules = ai::driving::Rules {
        cup: world.vehicles.cup,
        difficulty: world.vehicles.difficulty,
        player_rank: 0,
        finish_owner: true,
    };
    let mut seed = 0x1234abcd;
    let mut frames = Vec::new();
    for &elapsed in dt {
        let status = match op {
            0 => ai::driving::throttle(&mut car, &routes, &config, &global, rules, elapsed)
                .map(|_| 0),
            1 => ai::driving::steer(&mut car, &routes, &config, &mut seed).map(|_| 0),
            2 => ai::driving::reverse(&mut car, &routes, global.reverse_time, elapsed).map(|_| 0),
            3 => {
                ai::driving::branch(&routes, checkpoint, car.race.laps, &mut seed).map(|n| n as i32)
            }
            4 => ai::routes::waypoints(&mut car, &routes, &mut seed).map(i32::from),
            5 => tankracer_game::vehicle_runtime::rcc_finish_controls(
                &mut car, &routes, config, global, rules, elapsed, &mut seed,
            )
            .map(|_| 0),
            6 => tankracer_game::vehicle_runtime::rcc_finish_controls(
                &mut car, &routes, config, global, rules, elapsed, &mut seed,
            )
            .and_then(|_| ai::routes::waypoints(&mut car, &routes, &mut seed).map(i32::from)),
            _ => unreachable!(),
        }
        .unwrap_or_else(|e| e);
        let mut out = vec![status];
        out.extend(finish_scene_assets::state(&car));
        out.extend(controls(&car));
        out.push(seed as i32);
        out.extend(route(&car));
        out.extend(race(&car));
        out.push(i32::from(car.horn_down));
        frames.push(out);
    }
    record["tag"] = json!(tag);
    record["frames"] = json!(frames);
    record
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let mut world = reference_world::full_race(Path::new(&args[1]));
    fs::write(
        Path::new(&args[2]).join("finish-scene.bin"),
        finish_scene_assets::encode(&world)
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let mut car = world.vehicles.entries[world.player.0]
        .as_ref()
        .unwrap()
        .clone();
    let checkpoints: Vec<_> = world
        .objects
        .components
        .waypoints
        .points
        .iter()
        .enumerate()
        .filter_map(|(i, p)| {
            p.as_ref()
                .map(|_| (i, world.objects.world.colliders[i].entity))
        })
        .collect();
    let mut cases = Vec::new();
    for &(id, entity) in &checkpoints {
        let point = world.objects.components.waypoints.points[id]
            .as_ref()
            .unwrap();
        let Binding::Direct(Some(target)) = point.next[0] else {
            continue;
        };
        let Some(Handle::Obstacle(next_id)) = world
            .identity
            .entities
            .record(target)
            .and_then(|e| e.object)
        else {
            continue;
        };
        let next = world.objects.components.waypoints.points[next_id.0]
            .as_ref()
            .unwrap();
        let collider = &world.objects.world.colliders[next_id.0];
        let basis = collider.external_basis.unwrap_or(Matrix::IDENTITY).0;
        for flags in [0, 1] {
            for (x, z) in [(0i32, 1i32), (0, 0), (next.width >> 1, 100), (0, 51200)] {
                car.ai = Some(ai::State {
                    flags,
                    look_checkpoint: Some(entity),
                    drive_checkpoint: Some(entity),
                    ..Default::default()
                });
                car.route.checkpoint = Some(entity);
                car.route.passed = u32::MAX;
                car.route.completed = 3;
                car.race.laps = 3;
                car.race.elapsed = 12345;
                car.physics.motion.poses.base_position = std::array::from_fn(|i| {
                    collider.pose.position[i].wrapping_add(
                        x.wrapping_mul(basis[i])
                            .wrapping_add(z.wrapping_mul(basis[6 + i]))
                            >> 12,
                    )
                });
                car.ai.as_mut().unwrap().look_position = car.physics.motion.poses.base_position;
                cases.push(case(
                    &world,
                    car.clone(),
                    4,
                    entity,
                    &[40, 40],
                    &format!("waypoint-{id}-flags-{flags}-x{x}-z{z}"),
                ));
            }
        }
    }
    for &(id, entity) in &checkpoints {
        car.ai = Some(ai::State {
            flags: 32,
            look_checkpoint: Some(entity),
            drive_checkpoint: Some(entity),
            ..Default::default()
        });
        car.physics.motion.poses.base_position = world.objects.world.colliders[id].pose.position;
        car.physics.motion.velocity = [0, 0, 0];
        car.race.laps = 3;
        for (visible, speed) in [(true, 0), (true, 6000), (false, -3000)] {
            car.visible = visible;
            car.physics.powertrain.local_velocity[2] = speed;
            for op in [0, 1, 2, 3, 5] {
                cases.push(case(
                    &world,
                    car.clone(),
                    op,
                    entity,
                    &[40, 1500, 1, 3000],
                    &format!("point-{id}-visible-{visible}-speed-{speed}-op-{op}"),
                ));
            }
        }
    }
    let (id, entity) = checkpoints[0];
    let (other, other_entity) = checkpoints[1];
    let (third, third_entity) = checkpoints[2];
    // Controlled planes make strict width/Z/range boundaries and partial
    // updates observable while retaining the original IDs and source methods.
    let saved_points = world.objects.components.waypoints.points.clone();
    let saved_collider = world.objects.world.colliders[other].clone();
    world.objects.world.colliders[other].pose.position = [0; 3];
    world.objects.world.colliders[other].external_basis = Some(Matrix::IDENTITY);
    world.objects.components.waypoints.points[id]
        .as_mut()
        .unwrap()
        .next[0] = Binding::Direct(Some(other_entity));
    world.objects.components.waypoints.points[other]
        .as_mut()
        .unwrap()
        .width = 200;
    world.objects.components.waypoints.points[other]
        .as_mut()
        .unwrap()
        .lap = 1;
    for lap in [-1, 0, 1, 2, 3, 32767] {
        for completed in [-1, 0, 1, 2, 3] {
            car.ai = Some(ai::State {
                flags: 0,
                look_checkpoint: Some(entity),
                drive_checkpoint: Some(entity),
                look_position: [0, 0, 100],
                ..Default::default()
            });
            car.route.checkpoint = Some(entity);
            car.route.completed = completed;
            car.route.records = [11, 22, 33];
            car.route.passed = u32::MAX;
            car.race.laps = lap;
            car.race.elapsed = 9876;
            car.physics.motion.poses.base_position = [0, 0, 100];
            cases.push(case(
                &world,
                car.clone(),
                4,
                entity,
                &[40, 40],
                &format!("waypoint-lap-{lap}-completed-{completed}"),
            ));
        }
    }
    car.race.laps = 3;
    car.route.completed = 3;
    for (x, z) in [
        (-101, 100),
        (-100, 100),
        (-99, 100),
        (99, 100),
        (100, 100),
        (0, -1),
        (0, 0),
        (0, 1),
        (0, 51199),
        (0, 51200),
        (i32::MIN, 100),
    ] {
        car.ai = Some(ai::State {
            look_checkpoint: Some(entity),
            drive_checkpoint: Some(entity),
            look_position: [x, 0, z],
            ..Default::default()
        });
        car.route.checkpoint = Some(entity);
        car.physics.motion.poses.base_position = [x, 0, z];
        cases.push(case(
            &world,
            car.clone(),
            4,
            entity,
            &[40],
            &format!("waypoint-plane-x{x}-z{z}"),
        ));
    }
    for which in 0..3 {
        car.ai = Some(ai::State {
            look_checkpoint: Some(entity),
            drive_checkpoint: Some(entity),
            look_position: [0, 0, 100],
            ..Default::default()
        });
        car.route.checkpoint = Some(entity);
        car.physics.motion.poses.base_position = [0, 0, 100];
        if which == 0 {
            car.ai.as_mut().unwrap().look_checkpoint = None;
        }
        if which == 1 {
            car.route.checkpoint = None;
        }
        if which == 2 {
            car.ai.as_mut().unwrap().drive_checkpoint = None;
        }
        cases.push(case(
            &world,
            car.clone(),
            4,
            entity,
            &[40],
            &format!("waypoint-missing-checkpoint-{which}"),
        ));
    }
    // A named target on the newly entered look point fails after that point
    // has been published, before the race and driving gates are visited.
    let named = tankracer_game::symbols::Symbols::default()
        .intern("waypoint-missing")
        .unwrap();
    world.objects.components.waypoints.points[other]
        .as_mut()
        .unwrap()
        .next[1] = Binding::Named(named);
    car.ai = Some(ai::State {
        look_checkpoint: Some(entity),
        drive_checkpoint: Some(entity),
        look_position: [0, 0, 100],
        ..Default::default()
    });
    car.route.checkpoint = Some(entity);
    car.physics.motion.poses.base_position = [0, 0, 100];
    cases.push(case(
        &world,
        car.clone(),
        4,
        entity,
        &[40],
        "waypoint-look-published-before-error",
    ));
    world.objects.components.waypoints.points = saved_points;
    world.objects.world.colliders[other] = saved_collider;
    car.ai = Some(ai::State {
        flags: 32,
        look_checkpoint: Some(entity),
        drive_checkpoint: Some(entity),
        stuck_time: 1499,
        stuck_distance: 0,
        ..Default::default()
    });
    car.physics.motion.poses.base_position = world.objects.world.colliders[id].pose.position;
    car.physics.motion.velocity = [0, 0, 0];
    car.physics.powertrain.local_velocity[2] = 0;
    car.visible = true;
    // Two untagged branches exercise both calls to the original selector.
    world.objects.components.waypoints.points[id]
        .as_mut()
        .unwrap()
        .next = [
        Binding::Direct(Some(other_entity)),
        Binding::Direct(Some(third_entity)),
    ];
    world.objects.components.waypoints.points[other]
        .as_mut()
        .unwrap()
        .activate_lap = 255;
    world.objects.components.waypoints.points[third]
        .as_mut()
        .unwrap()
        .activate_lap = 255;
    world.objects.components.waypoints.points[other]
        .as_mut()
        .unwrap()
        .next = [
        Binding::Direct(Some(entity)),
        Binding::Direct(Some(third_entity)),
    ];
    world.objects.components.waypoints.points[id]
        .as_mut()
        .unwrap()
        .activate_lap = 255;
    for lap in [-1, 0, 2, 3, 4] {
        car.race.laps = lap;
        cases.push(case(
            &world,
            car.clone(),
            3,
            entity,
            &[40; 5],
            &format!("random-branch-lap-{lap}"),
        ));
        cases.push(case(
            &world,
            car.clone(),
            1,
            entity,
            &[40; 5],
            &format!("double-selector-lap-{lap}"),
        ));
    }
    for tag in [0, 2, 255] {
        world.objects.components.waypoints.points[other]
            .as_mut()
            .unwrap()
            .activate_lap = tag;
        for lap in [0, 1, 2, 3] {
            car.race.laps = lap;
            cases.push(case(
                &world,
                car.clone(),
                3,
                entity,
                &[40],
                &format!("tag-{tag}-lap-{lap}"),
            ));
        }
    }
    for flag in [4, 32] {
        car.ai.as_mut().unwrap().flags = flag;
        cases.push(case(
            &world,
            car.clone(),
            1,
            entity,
            &[40],
            &format!("steer-flag-{flag}"),
        ));
    }
    car.ai.as_mut().unwrap().flags = 32;
    for braking in [0, 32767, 32768, 65535] {
        world.objects.components.waypoints.points[other]
            .as_mut()
            .unwrap()
            .braking_range = braking;
        world.objects.components.waypoints.points[other]
            .as_mut()
            .unwrap()
            .max_speed = 0;
        car.physics.powertrain.local_velocity[2] = 6000;
        cases.push(case(
            &world,
            car.clone(),
            0,
            entity,
            &[1, 1, 40],
            &format!("braking-range-{braking}"),
        ));
    }
    let period = world.vehicles.ai_settings.reverse_time;
    for timer in [
        0,
        (period.wrapping_mul(5) >> 3) - 1,
        period - 1,
        period.wrapping_mul(2),
        i32::MAX,
    ] {
        car.ai.as_mut().unwrap().reverse_time = timer;
        cases.push(case(
            &world,
            car.clone(),
            2,
            entity,
            &[0, 1, 40],
            &format!("reverse-time-{timer}"),
        ));
    }
    car.ai = None;
    for op in 0..3 {
        cases.push(case(
            &world,
            car.clone(),
            op,
            entity,
            &[40],
            &format!("missing-ai-{op}"),
        ));
    }
    car.ai = Some(ai::State::default());
    for op in 0..3 {
        cases.push(case(
            &world,
            car.clone(),
            op,
            entity,
            &[40],
            &format!("missing-checkpoint-{op}"),
        ));
    }
    car.ai = Some(ai::State {
        vehicle: 1,
        colour: 2,
        flags: 0x80000020,
        spacing: u32::MAX,
        look_checkpoint: Some(entity),
        drive_checkpoint: Some(entity),
        turn_time: 123,
        confusion: 456,
        anger: 789,
        awareness: -45,
        aim_time: 33,
        warn_time: 44,
        mine_time: 55,
        view_timer: 66,
        view_span: 77,
        behind_span: 88,
        shell_error: -99,
        ..Default::default()
    });
    let saved_points = world.objects.components.waypoints.points.clone();
    let named = tankracer_game::symbols::Symbols::default()
        .intern("unresolved")
        .unwrap();
    for (tag, links) in [
        ("none", [Binding::Direct(None), Binding::Direct(None)]),
        (
            "named-first",
            [Binding::Named(named), Binding::Direct(Some(other_entity))],
        ),
        (
            "named-second",
            [Binding::Direct(Some(other_entity)), Binding::Named(named)],
        ),
        (
            "missing-entity",
            [
                Binding::Direct(Some(EntityId(0))),
                Binding::Direct(Some(other_entity)),
            ],
        ),
    ] {
        world.objects.components.waypoints.points[id]
            .as_mut()
            .unwrap()
            .next = links;
        for op in 0..4 {
            cases.push(case(
                &world,
                car.clone(),
                op,
                entity,
                &[40],
                &format!("binding-{tag}-{op}"),
            ));
        }
    }
    world.objects.components.waypoints.points = saved_points;
    for side in [2, usize::MAX] {
        car.ai.as_mut().unwrap().look_branch = side;
        car.ai.as_mut().unwrap().drive_branch = side;
        for op in 0..3 {
            cases.push(case(
                &world,
                car.clone(),
                op,
                entity,
                &[40],
                &format!("invalid-side-{side}-{op}"),
            ));
        }
    }
    car.ai.as_mut().unwrap().look_branch = 0;
    car.ai.as_mut().unwrap().drive_branch = 0;
    let original_basis = world.objects.world.colliders[other].external_basis;
    world.objects.world.colliders[other].external_basis = None;
    cases.push(case(
        &world,
        car.clone(),
        1,
        entity,
        &[40],
        "missing-external-before-look-position",
    ));
    world.objects.world.colliders[other].external_basis = original_basis;
    // Force exact local coordinates to distinguish the checked divide overflow
    // prefix, strict rule thresholds and all nine speed-dependent rules.
    world.objects.world.colliders[other].external_basis = Some(Matrix::IDENTITY);
    world.vehicles.ai_config[world.vehicles.cup * 5].look_ahead = 0;
    let target = world.objects.world.colliders[other].pose.position;
    car.physics.motion.poses.reference = Matrix::IDENTITY;
    car.physics.motion.poses.base_position = [target[0], target[1], target[2].wrapping_add(32768)];
    car.physics.motion.velocity = [4194304, 0, -64];
    cases.push(case(
        &world,
        car.clone(),
        1,
        entity,
        &[40],
        "checked-divide-overflow-after-distance",
    ));
    for (x, z) in [
        (0, 0),
        (1, 0),
        (-1, 1),
        (32767, 32768),
        (-500000, 31000),
        (i32::MIN, i32::MAX),
    ] {
        car.physics.motion.poses.base_position = [
            target[0].wrapping_add(x),
            target[1],
            target[2].wrapping_add(z),
        ];
        car.physics.motion.velocity = [640, 320, 1024];
        for speed in [0, 999, 1000, 6000, i32::MIN] {
            car.physics.powertrain.local_velocity[2] = speed;
            car.visible = false;
            cases.push(case(
                &world,
                car.clone(),
                1,
                entity,
                &[40],
                &format!("outer-x{x}-z{z}-speed{speed}"),
            ));
        }
    }
    for elapsed in [1500, 1501, i32::MAX, i32::MIN] {
        car.ai.as_mut().unwrap().stuck_time = 0;
        car.ai.as_mut().unwrap().stuck_distance = i32::MIN;
        cases.push(case(
            &world,
            car.clone(),
            0,
            entity,
            &[elapsed],
            &format!("stuck-wrapping-{elapsed}"),
        ));
    }
    world.objects.world.colliders[id].pose.position = [1000, 0, 0];
    world.objects.world.colliders[other].pose.position = [0, 0, 0];
    world.objects.world.colliders[other].external_basis = Some(Matrix::IDENTITY);
    world.objects.components.waypoints.points[id]
        .as_mut()
        .unwrap()
        .next[0] = Binding::Direct(Some(other_entity));
    world.objects.components.waypoints.points[other]
        .as_mut()
        .unwrap()
        .next = [Binding::Direct(Some(entity)), Binding::Direct(None)];
    let config = &mut world.vehicles.ai_config[world.vehicles.cup * 5];
    for rule in &mut config.inner_steering {
        rule.threshold = -1;
        rule.multiplier = 100;
        rule.shift = 0;
    }
    config.look_ahead = 0;
    car.visible = true;
    car.physics.motion.poses.base_position = [0; 3];
    car.physics.motion.velocity = [0; 3];
    car.physics.motion.poses.reference = Matrix::IDENTITY;
    car.physics.powertrain.local_velocity[2] = 6000;
    for flags in [0, 4] {
        for confusion in [0, 1, -1] {
            for anger in [5000, 5001] {
                for spacing in [0, 1, 2] {
                    car.ai = Some(ai::State {
                        vehicle: 1,
                        flags,
                        look_checkpoint: Some(entity),
                        drive_checkpoint: Some(entity),
                        confusion,
                        anger,
                        spacing,
                        turn_time: 49,
                        ..Default::default()
                    });
                    car.horn_down = true;
                    car.input.drive.throttle = i32::MAX;
                    car.input.drive.brake = -123;
                    car.input.drive.steering = i32::MIN;
                    cases.push(case(&world,car.clone(),5,entity,&[1,1,40,1501],
                        &format!("controls-flags{flags}-confusion{confusion}-anger{anger}-spacing{spacing}")));
                }
            }
        }
    }
    world.objects.components.waypoints.points[other]
        .as_mut()
        .unwrap()
        .lap = 1;
    for lap in [2, 3] {
        car.ai = Some(ai::State {
            vehicle: 1,
            flags: 32,
            look_checkpoint: Some(entity),
            drive_checkpoint: Some(entity),
            anger: 5001,
            turn_time: 49,
            ..Default::default()
        });
        car.route.checkpoint = Some(entity);
        car.route.completed = i32::from(lap);
        car.route.passed = u32::MAX;
        car.race.laps = lap;
        car.race.elapsed = 7654;
        car.physics.motion.poses.base_position = [0, 0, 100];
        cases.push(case(
            &world,
            car.clone(),
            6,
            entity,
            &[1, 1, 40, 40],
            &format!("controls-then-waypoints-lap-{lap}"),
        ));
    }
    car.ai = None;
    cases.push(case(
        &world,
        car.clone(),
        5,
        entity,
        &[40],
        "controls-missing-ai",
    ));
    let output = json!({"scope":"Original finish-owner controls; no whole-finish takeover claim", "cases":cases});
    fs::write(
        Path::new(&args[2]).join("finish-drive.json"),
        serde_json::to_vec_pretty(&output).unwrap(),
    )
    .unwrap();
}
