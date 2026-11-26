//! Original Player pre-Tank body, including live lap actions and weapon factories.
use super::{ints, scene_export, scene_fire_oracle, scene_mines_oracle};
use serde_json::{Value, json};
use std::{fs, path::Path};
use tankracer_game::{
    object_query::EntityId,
    player_input,
    projectile::launch::Owner,
    scene_objects::rcc_lifecycle_probe,
    symbols::Binding,
    vehicle_runtime::{SinglePlayer, rcc_player_probe},
};

fn snapshot(world: &SinglePlayer, launch: bool) -> Value {
    let mut out = scene_fire_oracle::snapshot(world, None);
    out["lap_bindings"] = json!(rcc_lifecycle_probe::lap_bindings(world));
    out["lap_backups"] = json!(rcc_lifecycle_probe::lap_backups(world));
    out["lap_attachments"] = json!(rcc_lifecycle_probe::lap_attachments(world));
    let mut deform = Vec::new();
    ints(
        &mut deform,
        world.objects.world.deformables.iter().map(|id| id.0 as i32),
    );
    out["lap_deformables"] = json!(deform);
    let car = world.vehicles.entries[0].as_ref().unwrap();
    let v = &car.physics;
    let d = car.input.drive;
    let t = car.turret;
    let r = car.route;
    let mut data = Vec::new();
    ints(
        &mut data,
        [
            d.throttle,
            d.brake,
            d.steering,
            d.action_flags as i32,
            i32::from(d.alternate),
            i32::from(d.alternate_lateral),
            i32::from(d.alternate_rotation),
            i32::from(car.input.start_held),
            i32::from(car.input.fast_start),
            car.prefix.countdown_throttle,
            i32::from(world.shared.input.steering_analog_latch),
            t.velocity,
            t.cooldown,
            i32::from(t.centering),
            i32::from(t.moving),
            i32::from(t.center_lock),
            i32::from(t.pressed[0]),
            i32::from(t.pressed[1]),
            i32::from(v.pose_input.turret_yaw),
            i32::from(car.brake.saved_turn),
            i32::from(car.brake.reverse_braking),
            car.model.config.tank.drive.max_turn,
            v.yaw_lock,
            v.collision.ground_delta[0],
            v.collision.ground_delta[1],
            v.collision.ground_delta[2],
            v.collision.tank_delta[0],
            v.collision.tank_delta[1],
            v.collision.tank_delta[2],
            v.collision.obstacle_impulse,
            v.powertrain.local_velocity[2],
            v.contact.sampling.effect.timer,
            i32::from(car.prefix.temporary_timer),
            v.contact.sampling.materials[0] as i32,
            car.weather as i32,
            i32::from(r.checkpoint.is_some()),
            r.checkpoint.map_or(0, |id| id.0 as i32),
            r.passed as i32,
            r.completed,
            r.records[0],
            r.records[1],
            r.records[2],
            r.score,
            r.previous_score,
            i32::from(r.wrong_way),
            i32::from(r.warning_timer),
            car.race.elapsed,
            i32::from(car.race.laps),
            i32::from(car.race.driving),
            i32::from(launch),
        ],
    );
    assert_eq!(data.len(), 100);
    out["player_fields"] = json!(data);
    out
}
fn route_bindings(world: &SinglePlayer, value: &mut Value) {
    let r = &world.objects.components.waypoints;
    let mut header = Vec::new();
    ints(
        &mut header,
        [
            r.origin[0],
            r.origin[1],
            r.shifts[0] as i32,
            r.shifts[1] as i32,
            r.radius[0],
            r.radius[1],
            r.backwards_index,
            i32::from(r.fallback.is_some()),
            r.fallback.map_or(0, |id| id.0 as i32),
        ],
    );
    value["player_route_header"] = json!(header);
    let mut points = Vec::new();
    for p in &r.points {
        let mut row = [0; 7];
        if let Some(p) = p {
            let (kind, id) = match p.next[0] {
                Binding::Direct(None) => (0, 0),
                Binding::Direct(Some(id)) => (1, id.0 as i32),
                Binding::Named(_) => (2, 0),
            };
            row = [
                1,
                p.progress,
                kind,
                id,
                i32::from(p.lap),
                i32::from(p.hairpin),
                p.width,
            ];
        }
        ints(&mut points, row);
    }
    value["player_route_points"] = json!(points);
    let mut ranges = Vec::new();
    let mut items = Vec::new();
    for cell in &r.cells {
        ranges.extend([(items.len() / 2) as u16, cell.len() as u16]);
        ints(&mut items, cell.iter().map(|id| id.0 as i32));
    }
    value["player_route_ranges"] = json!(ranges);
    value["player_route_items"] = json!(items);
    value["player_route_count"] = json!(r.order.len());
    value["player_weather"] = json!(r.weather.iter().flatten().copied().collect::<Vec<_>>());
}
pub fn export(root: &Path, out: &Path) {
    let mut cases = Vec::new();
    for branch in 0..48 {
        let mut world = super::reference_world::full_race(root);
        world.vehicles.entries.resize_with(3, || None);
        let fixed = world.objects.world.colliders.len();
        let order = world.objects.components.waypoints.order.clone();
        let current = order[10];
        let current_entity = world.objects.world.colliders[current.0].entity;
        let Binding::Direct(Some(target)) = world
            .objects
            .components
            .waypoints
            .point(current)
            .unwrap()
            .next[0]
        else {
            panic!("linked checkpoint")
        };
        let target_id = world
            .identity
            .entities
            .record(target)
            .unwrap()
            .object
            .unwrap()
            .obstacle()
            .unwrap();
        let mut camera = world.camera_position;
        if matches!(branch, 14 | 15 | 16 | 17 | 29) {
            let c = world.objects.world.colliders[target_id.0];
            let offset = c
                .external_basis
                .map_or([0, 0, 1024], |m| m.local_to_world([0, 0, 1024]));
            let position = std::array::from_fn(|i| c.pose.position[i].wrapping_add(offset[i]));
            let car = world.vehicles.entries[0].as_mut().unwrap();
            car.physics.motion.poses.base_position = position;
            car.physics.motion.poses.physical_position = position;
            car.route.checkpoint = Some(current_entity);
            world.objects.components.waypoints.points[target_id.0]
                .as_mut()
                .unwrap()
                .lap = 1;
            camera = if branch == 15 {
                position.map(|v| v >> 3)
            } else {
                [1000000, 0, 1000000]
            };
        }
        let mut has_keys = branch != 1 && branch != 32 && branch != 46;
        let mut limit = 100000;
        let mut launch = false;
        {
            let car = world.vehicles.entries[0].as_mut().unwrap();
            car.race.driving = !matches!(branch, 2 | 36 | 37);
            car.race.elapsed = 12345;
            car.physics.contact.response.timer = 0;
            car.physics.collision.impact_accumulator = 120;
            car.physics.collision.collision_size = 200;
            car.physics.collision.ground_delta = [40000, 0, 0];
            car.physics.collision.tank_delta = [0, 50000, 0];
            car.physics.collision.obstacle_impulse = 60000;
            car.physics.powertrain.local_velocity[2] = if matches!(branch, 5 | 6 | 7 | 38) {
                300001
            } else {
                100000
            };
            car.input.drive.throttle = 20000;
            car.input.drive.brake = 1000;
            car.input.drive.steering = -1000;
            car.turret.cooldown = -1;
            car.physics.pose_input.turret_yaw = 1000;
            car.inventory.ammo = 2;
            if matches!(branch, 5 | 6 | 7 | 38 | 39) {
                car.input.drive.alternate = true;
                car.input.drive.alternate_lateral = 200;
                car.input.drive.alternate_rotation = 20;
                car.brake.saved_turn = 250;
                car.model.config.tank.drive.max_turn = 310;
            }
            if matches!(branch, 5 | 6 | 7 | 19) {
                car.physics.contact.sampling.materials[0] = -1;
            }
            if branch == 7 {
                car.prefix.temporary_timer = 5;
            }
            if branch == 8 {
                car.physics.contact.response.timer = 1;
            }
            if matches!(branch, 10 | 32 | 33) {
                car.route.checkpoint = None;
            }
            if branch == 11 {
                car.route.checkpoint = Some(EntityId(0xfffffffb));
            }
            if branch == 13 {
                car.prefix.temporary_timer = 32767;
                car.physics.contact.sampling.effect.timer = -1;
            }
            if branch == 16 {
                car.race.laps = 32767;
                car.route.passed = u32::MAX;
            }
            if branch == 17 {
                car.race.laps = -1;
                car.route.completed = -1;
            }
            if branch == 18 {
                car.visible = false;
            }
            if branch == 20 {
                car.inventory.weapon = 7;
            }
            if branch == 21 || branch == 41 {
                car.inventory.weapon = 8;
            }
            if branch == 22 {
                car.inventory.weapon = 1;
            }
            if branch == 23 {
                car.inventory.weapon = 4;
            }
            if branch == 24 {
                car.inventory.weapon = 5;
            }
            if branch == 25 {
                car.inventory.weapon = 10;
            }
            if branch == 31 {
                car.weather = u32::MAX;
            }
            if branch == 35 {
                car.physics.contact.sampling.effect.timer = 2;
            }
            if branch == 39 {
                car.model.config.tank.drive.max_turn = i32::MIN;
                car.brake.saved_turn = -32768;
            }
            if branch == 40 {
                car.physics.player = false;
            }
        }
        if matches!(branch, 3 | 33 | 41) {
            world.race_active = false;
        }
        if branch == 4 {
            world.objects.components.waypoints.order.clear();
        }
        if branch == 12 {
            let checkpoint = world.vehicles.entries[0]
                .as_ref()
                .unwrap()
                .route
                .checkpoint
                .unwrap();
            let id = world
                .identity
                .entities
                .record(checkpoint)
                .unwrap()
                .object
                .unwrap()
                .obstacle()
                .unwrap();
            world.objects.components.waypoints.points[id.0]
                .as_mut()
                .unwrap()
                .next[0] = Binding::Direct(None);
            for id in &order {
                world.objects.world.colliders[id.0].external_basis = None;
            }
        }
        if branch == 29 {
            limit = 1;
        }
        if branch == 30 {
            world.objects.components.waypoints.weather[0][0] = 1;
        }
        if branch == 45 {
            let e = |id: usize| world.objects.world.colliders[id].entity;
            world.objects.components.lap_events.enter = vec![e(75), e(76), e(77)];
            camera = [1000000, 0, 1000000];
        }
        if branch == 47 {
            let symbol = world.identity.symbols.intern("RCC_PENDING_GATE").unwrap();
            let cp = world.vehicles.entries[0]
                .as_ref()
                .unwrap()
                .route
                .checkpoint
                .unwrap();
            let id = world
                .identity
                .entities
                .record(cp)
                .unwrap()
                .object
                .unwrap()
                .obstacle()
                .unwrap();
            world.objects.components.waypoints.points[id.0]
                .as_mut()
                .unwrap()
                .next[0] = Binding::Named(symbol);
        }
        world.camera_position = camera;
        world.objects.components.camera_position = camera;
        let car = world.vehicles.entries[0].as_ref().unwrap();
        let owner = Owner::from(car);
        let physical = car.physics.motion.poses.physical_position;
        let player = car.physics.player;
        let entity = car.physics.entity;
        let rotation = car.model.config.tank.drive.rotational_drag.step;
        let pr = world.objects.components.projectiles.resources.unwrap();
        let mr = world.objects.components.mines.resources.unwrap();
        let mut value = scene_mines_oracle::bindings(
            &world,
            tankracer_game::mine::Owner {
                entity,
                position: physical,
            },
            mr,
            json!({}),
        );
        route_bindings(&world, &mut value);
        let mut position = Vec::new();
        ints(&mut position, car.physics.motion.poses.base_position);
        value["player_position"] = json!(position);
        value["player_rotation"] = json!(rotation);
        value["player_race_active"] = json!(world.race_active);
        value["player_slides"] = json!(
            world
                .surfaces()
                .entries
                .iter()
                .map(|s| s.slide[0].multiplier)
                .collect::<Vec<_>>()
        );
        if branch == 26 {
            world.objects.components.projectiles.resources = None;
        }
        if branch == 27 {
            world.objects.components.projectiles.class = None;
        }
        let mut cars = Vec::new();
        super::cars(&mut cars, &world);
        value["flight_cars"] = json!(cars);
        value["fire_player"] = json!(player);
        value["launch"] = json!(super::launch_input(pr, owner));
        value["projectile_class"] = json!(
            world
                .objects
                .components
                .projectiles
                .class
                .map_or(-1, |v| v.0 as i32)
        );
        value["resources_present"] =
            json!(world.objects.components.projectiles.resources.is_some());
        value["terrain_present"] = json!(true);
        let mut cameras = Vec::new();
        ints(&mut cameras, camera);
        value["lap_cameras"] = json!(cameras);
        world.identity.current_slot = 12;
        let initial = snapshot(&world, launch);
        let mut frames = Vec::new();
        let mut controls = Vec::new();
        for step in 0..3 {
            let mut keys = 4 | 32 | 128;
            if matches!(branch, 5 | 38) {
                keys |= 16;
            }
            if matches!(branch, 34 | 35) {
                keys |= 64;
            }
            if branch == 36 {
                keys = 128;
            }
            if step == 1 && !matches!(branch, 21 | 22 | 23 | 24 | 25 | 26 | 27 | 30 | 47) {
                keys = 0;
            }
            if branch == 37 {
                keys = if step == 1 { 0 } else { 4 };
            }
            if branch == 46 {
                has_keys = step == 2;
            }
            let stage = if branch == 37 {
                if step == 0 { 1 } else { 2 }
            } else {
                4
            };
            let number = if step == 0 {
                1
            } else if step == 1 {
                2
            } else {
                entity.0
            };
            let dt = if branch == 43 {
                10
            } else if branch == 42 {
                100
            } else {
                40
            };
            let sampled = player_input::Keys {
                left: keys & 1 != 0,
                right: keys & 2 != 0,
                throttle: keys & 4 != 0,
                brake: keys & 8 != 0,
                auxiliary: keys & 16 != 0,
                turret_left: keys & 32 != 0,
                turret_right: keys & 64 != 0,
                fire: keys & 128 != 0,
                ..Default::default()
            };
            let status = rcc_player_probe::before_tank(
                &mut world,
                if has_keys {
                    Some((sampled, stage))
                } else {
                    None
                },
                dt,
                number,
                limit,
                &mut launch,
            )
            .err()
            .unwrap_or(0);
            controls.push([
                25,
                dt,
                number as i32,
                keys | if has_keys { 512 } else { 0 },
                limit as i32,
                stage,
            ]);
            frames.push(
                json!({"entity":0,"object":null,"status":status,"state":snapshot(&world,launch)}),
            );
        }
        let (scene, _) = scene_export::encode(&world.objects.world);
        let name = format!("scene-player-{branch}.bin");
        fs::write(
            out.join(&name),
            scene
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        value["tag"] = json!(format!("player-{branch}"));
        value["fixed_count"] = json!(fixed);
        value["scene"] = json!(name);
        value["initial"] = initial;
        value["controls"] = json!(controls);
        value["frames"] = json!(frames);
        cases.push(value);
    }
    fs::write(out.join("scene-player.json"),serde_json::to_vec(&json!({"cases":cases,"scope":"Original extracted Player body after finish takeover and before Tank visibility/prepare: shared input, collision decay, real lap actions, turret/fire, brake, route progress and first-map weather, with actual factories and error prefixes. Finish takeover, parent Tank and mixed world scheduling remain separate."})).unwrap()).unwrap();
}
