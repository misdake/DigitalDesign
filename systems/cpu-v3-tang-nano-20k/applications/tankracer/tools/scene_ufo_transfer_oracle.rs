//! Original UFO actor frames with live target methods and full shared state.
use super::{ints, scene_export, scene_mines_oracle, vehicle_pose_oracle, vehicle_transfer_oracle};
use serde_json::{Value, json};
use std::{fs, path::Path, sync::Arc};
use tankracer_game::{
    dispatch::MethodId,
    object_query::{EntityId, ObstacleId, TankId},
    pose,
    scene_entity::Handle,
    scene_objects::rcc_lifecycle_probe,
    vehicle_runtime::SinglePlayer,
};
fn snapshot(world: &SinglePlayer) -> Value {
    let mut out = scene_mines_oracle::snapshot(world);
    let mut cars = Vec::new();
    for i in 0..3 {
        if let Some(Some(car)) = world.vehicles.entries.get(i) {
            cars.extend(vehicle_transfer_oracle::snapshot(car, 0));
            ints(
                &mut cars,
                [
                    i32::from(car.prefix.spin.disabled),
                    i32::from(car.physics.collision.disabled),
                    i32::from(car.race.driving),
                ],
            );
        } else {
            cars.extend([0; 250]);
        }
    }
    out["transfer_cars"] = json!(cars);
    let mut occupied = Vec::new();
    ints(
        &mut occupied,
        world
            .objects
            .components
            .ufo_occupied
            .iter()
            .map(|v| v.0 as i32),
    );
    out["transfer_occupied"] = json!(occupied);
    let mut visible = Vec::new();
    ints(
        &mut visible,
        world.vehicles.visible.iter().map(|v| v.0 as i32),
    );
    out["transfer_visible"] = json!(visible);
    out["overlay"] = json!(match world.objects.components.overlay {
        Some(v) => vec![
            1,
            u16::from(v[0]),
            u16::from(v[1]),
            u16::from(v[2]),
            u16::from(v[3])
        ],
        None => vec![0; 5],
    });
    out
}
pub fn export(root: &Path, out: &Path) {
    let mut cases = Vec::new();
    for branch in 0..62 {
        let mut world = super::reference_world::full_race(root);
        let fixed = world.objects.world.colliders.len();
        let obstacle = matches!(branch, 1 | 23 | 34 | 42 | 43 | 44 | 45 | 46 | 59);
        let car = world.vehicles.entries[0].as_mut().unwrap();
        let car_entity = car.physics.entity;
        car.route.checkpoint = Some(
            world.objects.world.colliders[world.objects.components.waypoints.order[10].0].entity,
        );
        car.route.passed = 12;
        car.race.driving = true;
        if branch == 2 {
            car.physics.player = false;
        }
        let mut c = car.physics.pose.clone();
        if branch == 33 {
            c.track_length = i32::MIN;
        }
        let input = car.physics.pose_input;
        let env = pose::Environment {
            timer: input.timer,
            ambient_wobble: input.ambient_wobble,
            session_active: input.session_active,
            ambient_phase: input.ambient_phase,
            recoil_mode: input.recoil_mode,
            turret_yaw: input.turret_yaw,
            recoil: car.physics.motion.articulation.recoil,
            gun_pitch: car.physics.motion.articulation.pitch,
            external: input.external,
        };
        let mut environment = Vec::new();
        ints(&mut environment, vehicle_pose_oracle::environment(&env));
        let mut travel = Vec::new();
        ints(&mut travel, car.physics.motion.travel);
        let car_class = world
            .identity
            .entities
            .record(car_entity)
            .unwrap()
            .class
            .unwrap();
        if matches!(branch, 35 | 36 | 37 | 39 | 40 | 58) {
            let mut peer = world.vehicles.entries[0].as_ref().unwrap().clone();
            let entity = world.identity.entities.create_record(100000).unwrap();
            world.identity.entities.record_mut(entity).unwrap().class = Some(car_class);
            world.identity.entities.record_mut(entity).unwrap().object =
                Some(Handle::Vehicle(TankId(1)));
            peer.physics.entity = entity;
            peer.physics.player = false;
            peer.slot = 1;
            if branch == 58 {
                peer.model.parts[tankracer_assets::config::Part::Body.slot()]
                    .as_mut()
                    .unwrap()
                    .radius = 60000;
                peer.physics.motion.poses.base_position[0] =
                    peer.physics.motion.poses.base_position[0].wrapping_add(100000);
            }
            if branch == 36 {
                peer.physics.motion.poses.base_position[0] =
                    peer.physics.motion.poses.base_position[0].wrapping_add(100000);
            }
            if branch == 39 || branch == 40 {
                peer.model.parts[tankracer_assets::config::Part::Body.slot()] = None;
            }
            if branch == 40 {
                peer.physics.entity = car_entity;
            }
            world.vehicles.entries.push(Some(peer));
            world.vehicles.racers[1] = Some(TankId(1));
            world.vehicles.visible.push(TankId(1));
            if branch == 37 {
                world.vehicles.entries[1] = None;
            }
        }
        let target_id = world.objects.components.ordinary[0];
        let target = if obstacle {
            let entity = world.objects.world.colliders[target_id.0].entity;
            world.identity.entities.record_mut(entity).unwrap().class =
                Some(world.registry.obstacle);
            entity
        } else {
            car_entity
        };
        let position = if obstacle {
            world.objects.world.colliders[target_id.0].pose.position
        } else {
            world.vehicles.entries[0]
                .as_ref()
                .unwrap()
                .physics
                .motion
                .poses
                .base_position
        };
        let destination = [
            position[0].wrapping_add(1000),
            position[1],
            position[2].wrapping_sub(500),
        ];
        let entity =
            rcc_lifecycle_probe::spawn_ufo(&mut world, None, target, destination, 100000, true)
                .unwrap();
        let id = world
            .identity
            .entities
            .record(entity)
            .unwrap()
            .object
            .and_then(Handle::obstacle)
            .unwrap();
        let ufo_class = world
            .identity
            .entities
            .record(entity)
            .unwrap()
            .class
            .unwrap();
        let mut dt = 40;
        let mut limit = 100000;
        let slot = 12;
        let has_vehicles = !matches!(branch, 25 | 42);
        let s = world.objects.components.ufos[id.0].as_mut().unwrap();
        match branch {
            4 => {
                s.timer = 800;
                dt = 1;
            }
            5 => {
                s.timer = 1000;
                s.fade = 255;
                s.flags |= 8;
                dt = 1;
            }
            6 => {
                s.timer = 1000;
                s.fade = 255;
                dt = 0;
            }
            7 => {
                s.timer = 1399;
                s.flags |= 8;
                dt = 1;
            }
            8 => s.timer = i16::MAX,
            9 => {
                s.timer = 1500;
                s.fade = i16::MAX;
                s.flags |= 8;
                dt = 1;
            }
            10 => {
                s.flags = 34;
                s.timer = 200;
                dt = 1;
            }
            11 => {
                s.flags = 34;
                s.timer = 200;
                dt = 0;
            }
            12 => {
                s.flags = 38;
                s.arrived = true;
            }
            13 => {
                s.flags = 54;
                s.arrived = true;
                s.timer = 700;
                dt = 1;
            }
            14 => {
                s.flags = 54;
                s.arrived = true;
                s.timer = 700;
                s.fade = 40;
            }
            15..=19 | 49 => {
                s.flags = 0;
                s.point[1] = 102400;
                s.reserved = Some(
                    world.objects.world.colliders[world.objects.components.waypoints.order[9].0]
                        .entity,
                );
            }
            27..=33 | 54 | 60 | 61 => {
                s.timer = 1000;
                s.fade = 255;
                dt = 1;
            }
            34 => {
                s.flags = 1;
                s.timer = 2500;
                dt = 1;
                limit = 1;
            }
            35..=42 | 58 | 59 => {
                s.flags = 54;
                s.arrived = true;
                s.timer = 700;
                dt = 1;
            }
            43 => {
                s.flags = 2;
                s.point[0] = position[0].wrapping_add(20000);
                s.point[2] = position[2].wrapping_add(4000);
            }
            44 => {
                s.flags = 2;
                s.point[0] = position[0].wrapping_sub(60000);
                s.point[2] = position[2].wrapping_sub(40000);
            }
            45 => {
                s.flags = 2;
                s.point[0] = position[0].wrapping_add(400);
                s.point[2] = position[2].wrapping_sub(100);
            }
            46 => {
                s.flags = 2;
                s.point[0] = i32::MIN;
                s.point[2] = i32::MAX;
                dt = i32::MAX;
            }
            47 => {
                s.point[1] = i32::MIN;
                dt = i32::MIN;
            }
            48 => {
                s.flags = 0;
                s.departure_speed = i32::MAX;
                dt = -40;
            }
            55 => {
                s.flags = 34;
                s.timer = i16::MAX;
                dt = 1;
            }
            56 => {
                s.flags = 54;
                s.arrived = true;
                s.timer = 701;
                s.fade = i16::MIN;
                dt = 1;
            }
            57 => {
                s.flags = 0;
                s.point[1] = 102400;
                limit = 0;
            }
            _ => {}
        }
        if matches!(branch, 16 | 17 | 18 | 19 | 49) {
            let reserved = world.objects.components.ufos[id.0]
                .as_ref()
                .unwrap()
                .reserved
                .unwrap();
            if branch == 18 {
                world
                    .objects
                    .components
                    .ufo_occupied
                    .push(EntityId(0x12345678));
            }
            world.objects.components.ufo_occupied.push(reserved);
            if branch == 17 {
                limit = 0;
            }
            if branch == 18 {
                limit = 1;
            }
        }
        if branch == 20 {
            rcc_lifecycle_probe::clear_ufo_config(&mut world, true);
        }
        if branch == 21 {
            world.objects.components.ufos[id.0] = None;
        }
        if branch == 22 {
            world.objects.components.ufos[id.0].as_mut().unwrap().target = EntityId(0xfedcba98);
        }
        if branch == 23 {
            world.identity.entities.record_mut(target).unwrap().object =
                Some(Handle::Obstacle(ObstacleId(fixed + 100)));
        }
        if branch == 24 {
            world.vehicles.entries[0] = None;
        }
        if branch == 26 {
            world.identity.entities.record_mut(target).unwrap().object = Some(Handle::Session(
                tankracer_game::scene_entity::SessionObject::Camera,
            ));
        }
        if branch == 32 {
            world.vehicles.entries[0].as_mut().unwrap().route.checkpoint = None;
        }
        if branch == 33 {
            world.vehicles.entries[0]
                .as_mut()
                .unwrap()
                .physics
                .pose
                .track_length = i32::MIN;
        }
        if branch == 34 {
            rcc_lifecycle_probe::transfer_object_setup(&mut world, target_id, 3);
        }
        if branch == 47 {
            rcc_lifecycle_probe::set_ufo_height(&mut world, i32::MIN);
        }
        if branch == 58 {
            world.vehicles.entries[0].as_mut().unwrap().model.parts
                [tankracer_assets::config::Part::Body.slot()]
            .as_mut()
            .unwrap()
            .radius = 60000;
        }
        if branch == 59 {
            world.objects.world.colliders[target_id.0].radius = 65535;
        }
        if branch == 38 {
            world.vehicles.entries[0].as_mut().unwrap().model.parts
                [tankracer_assets::config::Part::Body.slot()] = None;
        }
        if matches!(branch, 19 | 49) {
            let class = world.registry.generic;
            let mut table = (*world.registry.classes[class.0]).clone();
            table.methods[8] = if branch == 19 {
                Some(MethodId(74))
            } else {
                None
            };
            let table = Arc::new(table);
            world.registry.classes[class.0] = table.clone();
            world.objects.world.classes[class.0] = table;
        }
        if (27..=31).contains(&branch) || branch == 60 || branch == 61 {
            let mut table = (*world.registry.classes[car_class.0]).clone();
            let selected = match branch {
                27 | 60 => 9,
                28 => 11,
                29 | 61 => 29,
                30 => 30,
                _ => 10,
            };
            table.methods[selected] = if branch == 60 || branch == 61 {
                Some(MethodId(37))
            } else if branch == 27 {
                None
            } else {
                Some(MethodId(74))
            };
            let table = Arc::new(table);
            world.registry.classes[car_class.0] = table.clone();
            world.objects.world.classes[car_class.0] = table;
        }
        if matches!(branch, 50 | 51 | 52) {
            let mut table = (*world.registry.classes[ufo_class.0]).clone();
            if branch == 50 {
                table.methods[12] = None;
            } else if branch == 51 {
                table.maximum_slot = 11;
            } else {
                table.methods[12] = Some(MethodId(74));
            }
            let table = Arc::new(table);
            world.registry.classes[ufo_class.0] = table.clone();
            world.objects.world.classes[ufo_class.0] = table;
        }
        if branch == 53 {
            world.identity.entities.record_mut(entity).unwrap().class = None;
        }
        if branch == 54 {
            world.identity.entities.record_mut(target).unwrap().class = None;
        }
        world.identity.current_slot = 17;
        let initial = snapshot(&world);
        let mut car_info = Vec::new();
        let mut presence = Vec::new();
        for i in 0..3 {
            if let Some(Some(c)) = world.vehicles.entries.get(i) {
                let body = c.model.parts[tankracer_assets::config::Part::Body.slot()].as_ref();
                ints(
                    &mut car_info,
                    [
                        c.physics.entity.0 as i32,
                        i32::from(c.physics.player),
                        i32::from(body.is_some()),
                        body.map_or(0, |v| i32::from(v.radius)),
                    ],
                );
                presence.push(true);
            } else {
                car_info.extend([0; 8]);
                presence.push(false);
            }
        }
        let racers = world
            .vehicles
            .racers
            .map(|v| v.map_or(65535, |v| v.0 as u16));
        let mut shapes = rcc_lifecycle_probe::transfer_shapes(&world);
        shapes.resize(512, 0);
        let mut points = Vec::new();
        for p in &world.objects.components.waypoints.points {
            ints(
                &mut points,
                [i32::from(p.is_some()), p.as_ref().map_or(0, |p| p.progress)],
            );
        }
        let mut order = Vec::new();
        ints(
            &mut order,
            world
                .objects
                .components
                .waypoints
                .order
                .iter()
                .map(|v| v.0 as i32),
        );
        let (_, resources) = rcc_lifecycle_probe::ufo_config(&world);
        let mut frames = Vec::new();
        let mut controls = Vec::new();
        let count = if branch == 2 {
            160
        } else if branch <= 1 {
            60
        } else if matches!(branch, 3 | 7 | 14 | 35) {
            3
        } else {
            1
        };
        for step in 0..count {
            let time = if branch == 2 {
                200
            } else if branch <= 1 {
                100
            } else if branch == 3 {
                [0, 1, 40][step]
            } else {
                dt
            };
            let status =
                rcc_lifecycle_probe::tick_ufo(&mut world, entity, time, slot, limit, has_vehicles)
                    .err()
                    .unwrap_or(0);
            frames.push(json!({"entity":entity.0,"status":status,"object":world.identity.entities.record(entity).and_then(|r|r.object).and_then(Handle::obstacle).map(|v|v.0),"state":snapshot(&world)}));
            controls.push([18, time, slot as i32, entity.0 as i32, limit as i32, 0]);
            if world.identity.queues.destruction.contains(&entity) {
                break;
            }
        }
        if branch == 60 || branch == 61 {
            assert_eq!(frames[0]["status"], -1);
            assert_eq!(frames[0]["state"]["slot"], 12);
        }
        if branch == 58 || branch == 59 {
            assert_eq!(
                world.objects.components.ufos[id.0].as_ref().unwrap().flags,
                54
            );
        }
        if branch <= 2 {
            assert!(
                frames.iter().all(|v| v["status"] == 0),
                "UFO full sequence {branch}"
            );
            assert!(
                world.identity.queues.destruction.contains(&entity),
                "UFO did not depart {branch}: {:?}",
                world.objects.components.ufos[id.0]
            );
        }
        let (scene, _) = scene_export::encode(&world.objects.world);
        let name = format!("scene-ufo-transfer-{branch}.bin");
        fs::write(
            out.join(&name),
            scene
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        cases.push(json!({"tag":format!("ufo-transfer-{branch}"),"fixed_count":fixed,"generic":world.registry.generic.0,"obstacle":world.registry.obstacle.0,"world_class":world.registry.world.0,"projectile_class":world.objects.components.projectiles.class.unwrap().0,"resources_present":true,"homing_mesh":world.objects.components.projectiles.resources.unwrap().homing_mesh.index(),"launch":vec![0u16;93],"scene":name,"initial":initial,"controls":controls,"frames":frames,"transfer_config":vehicle_pose_oracle::config(&c),"transfer_environment":environment,"transfer_travel":travel,"transfer_points":points,"transfer_order":order,"vehicles_present":has_vehicles,"car_presence":presence,"car_info":car_info,"racers":racers,"shapes":shapes,"ufo_resources_present":resources.is_some(),"ufo_height":resources.map_or(0,|r|r.height)}));
    }
    fs::write(out.join("scene-ufo-transfer.json"),serde_json::to_vec(&json!({"cases":cases,"scope":"Complete original UFO transfer class Tick with real vehicle/obstacle target methods: approach, fade, route restore and reservation, visibility, overlap-controlled release, departure, exact packed arithmetic and committed nested failures. No AI updates or queue drain."})).unwrap()).unwrap();
}
