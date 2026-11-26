//! Original vehicle transfer interfaces, shared ticks/queues and nested activation.
use super::{ints, scene_export, scene_mines_oracle, vehicle_pose_oracle, vehicle_transfer_oracle};
use serde_json::{Value, json};
use std::{fs, path::Path, sync::Arc};
use tankracer_game::{
    dispatch::{ClassId, MethodId},
    object_query::EntityId,
    pose,
    scene_entity::Handle,
    scene_objects::rcc_lifecycle_probe,
    vehicle_runtime::SinglePlayer,
};
fn snapshot(world: &SinglePlayer, value: u32) -> Value {
    let mut out = scene_mines_oracle::snapshot(world);
    let mut car_words = if let Some(car) = world.vehicles.entries[0].as_ref() {
        vehicle_transfer_oracle::snapshot(car, 0)
    } else {
        vec![0; 244]
    };
    let flags = world.vehicles.entries[0].as_ref().map_or([0; 3], |c| {
        [
            i32::from(c.prefix.spin.disabled),
            i32::from(c.physics.collision.disabled),
            i32::from(c.race.driving),
        ]
    });
    ints(&mut car_words, flags);
    out["transfer_car"] = json!(car_words);
    let mut result = Vec::new();
    ints(&mut result, [value as i32]);
    out["transfer_result"] = json!(result);
    out
}
pub fn export(root: &Path, out: &Path) {
    let mut cases = Vec::new();
    for branch in 0..60 {
        let mut world = super::reference_world::full_race(root);
        let original = world.vehicles.entries[0].as_ref().unwrap().clone();
        let entity = original.physics.entity;
        let class = world
            .identity
            .entities
            .record(entity)
            .unwrap()
            .class
            .unwrap();
        let checkpoint = world.objects.components.waypoints.order[10];
        let previous = world.objects.components.waypoints.order[9];
        let checkpoint_entity = world.objects.world.colliders[checkpoint.0].entity;
        let car = world.vehicles.entries[0].as_mut().unwrap();
        car.route.checkpoint = Some(checkpoint_entity);
        car.prefix.spin.disabled = 123;
        car.physics.collision.disabled = true;
        car.race.driving = false;
        car.input.drive.throttle = 123;
        car.input.drive.brake = -234;
        car.input.drive.steering = 345;
        car.physics.motion.velocity = [12345, -23456, 34567];
        car.physics.motion.angular = [45678, -56789, 67890];
        car.physics.motion.acceleration = [11111, -22222];
        car.physics.motion.rates.track_slip = -33333;
        car.prefix.temporary_timer = 127;
        if branch == 23 {
            car.route.checkpoint = None;
        }
        if branch == 24 {
            car.physics.pose.track_length = i32::MIN;
        }
        let c = car.physics.pose.clone();
        let input = car.physics.pose_input;
        let e = pose::Environment {
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
        ints(&mut environment, vehicle_pose_oracle::environment(&e));
        let mut travel = Vec::new();
        ints(&mut travel, car.physics.motion.travel);
        if branch == 25 {
            world.objects.components.waypoints.points[previous.0] = None;
        }
        if matches!(branch, 26 | 27 | 28 | 50) {
            world.vehicles.entries[0] = None;
        }
        if branch == 51 {
            world
                .objects
                .components
                .ufo_occupied
                .push(world.objects.world.colliders[previous.0].entity);
        }
        if branch == 39 {
            world.identity.ticks.next = Some(entity);
        }
        if branch == 53 {
            world.identity.ticks.disable(entity, 100000).unwrap();
        }
        if branch == 58 {
            world.identity.entities.record_mut(entity).unwrap().object = None;
        }
        if branch == 59 {
            world.identity.entities.record_mut(entity).unwrap().object = Some(Handle::Session(
                tankracer_game::scene_entity::SessionObject::Camera,
            ));
        }
        if branch == 47 {
            world.identity.entities.record_mut(entity).unwrap().class = None;
        }
        if branch == 49 {
            world.identity.entities.record_mut(entity).unwrap().class =
                Some(ClassId(world.registry.classes.len() + 1));
        }
        if matches!(
            branch,
            29 | 30 | 31 | 32 | 33 | 34 | 35 | 36 | 37 | 41 | 42 | 43 | 44 | 45 | 46 | 55 | 56 | 57
        ) {
            let mut table = (*world.registry.classes[class.0]).clone();
            match branch {
                29 => table.methods[9] = None,
                30 => table.maximum_slot = 8,
                31 => table.methods[9] = Some(MethodId(74)),
                32 => table.methods[4] = None,
                33 => table.methods[4] = Some(MethodId(33)),
                34 => table.methods[4] = Some(MethodId(129)),
                35 => {
                    table.methods[0] = Some(MethodId(128));
                    table.maximum_slot = 3;
                }
                36 => table.methods[4] = Some(MethodId(36)),
                37 => table.methods[4] = Some(MethodId(34)),
                41 => table.methods[8] = Some(MethodId(36)),
                42 => table.methods[3] = Some(MethodId(38)),
                43 => table.methods[3] = Some(MethodId(37)),
                44 => table.methods[9] = Some(MethodId(32)),
                45 => table.methods[9] = Some(MethodId(33)),
                46 => table.methods[9] = Some(MethodId(34)),
                55 => table.methods[4] = Some(MethodId(74)),
                56 => table.methods[4] = Some(MethodId(47)),
                57 => table.methods[4] = Some(MethodId(35)),
                _ => {}
            }
            let table = Arc::new(table);
            world.registry.classes[class.0] = table.clone();
            world.objects.world.classes[class.0] = table;
        }
        let has_vehicles = !matches!(branch, 11 | 12 | 13 | 14 | 15 | 16 | 29 | 38);
        world.identity.current_slot = 17;
        let mut value = 0x12345678;
        let initial = snapshot(&world, value);
        let fixed = world.objects.world.colliders.len();
        let requests = match branch {
            0 => vec![
                (9, 1, 1, 100000),
                (11, 1, 0, 100000),
                (30, 0, 0, 100000),
                (29, 1, 0, 100000),
                (5, 0, 0, 100000),
                (29, 1, 1, 100000),
                (10, 0, 0, 100000),
                (11, 1, 1, 100000),
                (9, 1, 0, 100000),
                (30, 0, 0, 100000),
            ],
            1 => vec![(9, 1, 1, 0)],
            2 => vec![(9, 0, 1, 100000)],
            3 => vec![(9, 2, 1, 100000)],
            4 => vec![(9, 1, u32::MAX, 100000)],
            5 => vec![(11, 1, 1, 100000)],
            6 => vec![(11, 1, 2, 100000)],
            7 => vec![(11, 0, 1, 100000)],
            8 => vec![(11, 1, 0, 0)],
            9 => vec![(30, 0, 0, 100000); 2],
            10 => vec![(29, 1, 0, 100000); 2],
            11 | 27 => vec![(29, 1, 0, 100000)],
            12 | 28 | 32 | 33 | 34 | 36 | 37 | 55 | 56 | 57 => vec![(29, 1, 1, 100000)],
            13 | 29 | 30 | 31 | 44 | 45 | 46 | 47 | 48 | 49 | 58 | 59 => vec![(9, 1, 1, 100000)],
            14 => vec![(11, 1, 1, 100000)],
            15 => vec![(30, 0, 0, 100000)],
            17 => vec![(29, 1, 0, 0)],
            18 => vec![(29, 1, 1, 0)],
            19 => vec![(10, 0, 0, 0)],
            20 => vec![(10, 2, 0, 100000)],
            21 => vec![(10, 1, 99, 100000)],
            35 => vec![(0, 1, 1, 100000)],
            38 | 53 | 54 => vec![(4, 0, 0, 100000)],
            39 => vec![(5, 0, 0, 100000)],
            40 => vec![(5, 0, 0, 0)],
            41 => vec![(8, 0, 0, 100000)],
            42 | 43 => vec![(3, 0, 0, 100000)],
            52 => vec![(10, 0, 0, 1)],
            _ => vec![(10, 0, 0, 100000)],
        };
        let selected = if branch == 48 {
            EntityId(0xfedcba98)
        } else {
            entity
        };
        let mut frames = Vec::new();
        let mut controls = Vec::new();
        for (slot, payload, argument, limit) in requests {
            let result = rcc_lifecycle_probe::dispatch_vehicle_transfer(
                &mut world,
                selected,
                slot,
                payload,
                argument,
                limit,
                has_vehicles,
            );
            let status = match result {
                Ok(v) => {
                    value = v;
                    0
                }
                Err(e) => e,
            };
            let object = world
                .identity
                .entities
                .record(selected)
                .and_then(|r| r.object)
                .and_then(Handle::obstacle)
                .map(|v| v.0);
            frames.push(json!({"entity":selected.0,"status":status,"object":object,"state":snapshot(&world,value)}));
            controls.push([
                16,
                slot as i32,
                i32::from(payload),
                argument as i32,
                limit as i32,
                selected.0 as i32,
            ]);
        }
        if matches!(
            branch,
            0 | 1
                | 2
                | 3
                | 5
                | 8
                | 9
                | 10
                | 11
                | 12
                | 20
                | 21
                | 22
                | 27
                | 28
                | 29
                | 30
                | 32
                | 33
                | 35
                | 36
                | 37
                | 38
                | 39
                | 41
                | 42
                | 43
                | 44
                | 45
                | 46
                | 51
                | 53
                | 54
        ) {
            assert!(
                frames.iter().all(|f| f["status"] == 0),
                "vehicle methods branch {branch}"
            );
        }
        if branch == 34 {
            assert_eq!(frames[0]["state"]["slot"], 17);
            assert_eq!(frames[0]["status"], 0);
        }
        if branch == 55 {
            assert_eq!(frames[0]["status"], -5);
            assert_eq!(frames[0]["state"]["slot"], 4);
        }
        if branch == 56 || branch == 57 {
            assert_eq!(frames[0]["status"], 0);
            assert_eq!(frames[0]["state"]["slot"], 17);
        }
        if branch == 58 || branch == 59 {
            assert_eq!(frames[0]["status"], -1);
            assert_eq!(frames[0]["state"]["slot"], 9);
        }
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
        let (scene, _) = scene_export::encode(&world.objects.world);
        let name = format!("scene-vehicle-transfer-{branch}.bin");
        fs::write(
            out.join(&name),
            scene
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        cases.push(json!({"tag":format!("vehicle-methods-{branch}"),"fixed_count":fixed,"generic":world.registry.generic.0,"obstacle":world.registry.obstacle.0,"world_class":world.registry.world.0,"projectile_class":world.objects.components.projectiles.class.unwrap().0,"resources_present":true,"homing_mesh":world.objects.components.projectiles.resources.unwrap().homing_mesh.index(),"launch":vec![0u16;93],"scene":name,"initial":initial,"controls":controls,"frames":frames,"transfer_config":vehicle_pose_oracle::config(&c),"transfer_environment":environment,"transfer_travel":travel,"transfer_points":points,"transfer_order":order,"transfer_occupied":occupied,"vehicles_present":has_vehicles,"car_present":world.vehicles.entries[0].is_some()}));
    }
    fs::write(out.join("scene-vehicle-transfer.json"),serde_json::to_vec(&json!({"cases":cases,"scope":"Actual vehicle class MODE/COLLIDABLE/SHOW/RESET/MOVE and shared base methods: payload defaults, flags, ordered route restoration, live ticks/queues/cursor, nested activation, source slot/result semantics and committed failures. UFO actor orchestration follows separately."})).unwrap()).unwrap();
}
