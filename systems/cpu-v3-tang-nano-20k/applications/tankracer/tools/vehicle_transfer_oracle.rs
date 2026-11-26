//! Actual route restoration with live scene/identity data and full car snapshots.
use super::{identity_assets, ints, scene_export, vehicle_pose_oracle};
use serde_json::json;
use std::{fs, path::Path};
use tankracer_game::{
    math::Matrix,
    object_query::{EntityId, TankId},
    pose,
    scene_entity::Handle,
    vehicle_startup::Player,
};
pub(super) fn snapshot(car: &Player, height: i32) -> Vec<u16> {
    let mut out = vehicle_pose_oracle::snapshot(&car.physics.motion.poses);
    ints(
        &mut out,
        [
            i32::from(car.route.checkpoint.is_some()),
            car.route.checkpoint.map_or(0, |v| v.0 as i32),
            car.route.passed as i32,
            i32::from(car.race.laps),
        ],
    );
    ints(
        &mut out,
        [
            car.input.drive.throttle,
            car.input.drive.brake,
            car.input.drive.steering,
        ],
    );
    ints(&mut out, car.physics.motion.velocity);
    ints(&mut out, car.physics.motion.acceleration);
    ints(&mut out, car.physics.motion.angular);
    ints(
        &mut out,
        [
            car.physics.motion.rates.track_slip,
            i32::from(car.prefix.temporary_timer),
            height,
        ],
    );
    assert_eq!(out.len(), 244);
    out
}
pub fn export(root: &Path, out: &Path) {
    let mut cases = Vec::new();
    for branch in 0..34 {
        let mut world = super::reference_world::race(root);
        let order = world.objects.components.waypoints.order.clone();
        assert!(order.len() > 12);
        let checkpoint = order[if matches!(branch, 1 | 10 | 20 | 21) {
            0
        } else if branch == 11 {
            1
        } else {
            10
        }];
        let index = order.iter().position(|v| *v == checkpoint).unwrap();
        let prev = order[if index == 0 {
            order.len() - 1
        } else {
            index - 1
        }];
        let entity = world.objects.world.colliders[checkpoint.0].entity;
        let previous = world.objects.world.colliders[prev.0].entity;
        let mut car = world.vehicles.entries[0].as_ref().unwrap().clone();
        car.route.checkpoint = Some(entity);
        car.route.passed = 123;
        car.race.laps = 2;
        car.input.drive.throttle = 123;
        car.input.drive.brake = -234;
        car.input.drive.steering = 345;
        car.physics.motion.velocity = [12345, -23456, 34567];
        car.physics.motion.angular = [45678, -56789, 67890];
        car.physics.motion.acceleration = [11111, -22222];
        car.physics.motion.rates.track_slip = -33333;
        car.physics.pose_input.timer = 321;
        car.prefix.temporary_timer = 127;
        let mut limit = 100000;
        match branch {
            2 => {
                world.objects.components.waypoints.points[prev.0]
                    .as_mut()
                    .unwrap()
                    .progress = -1
            }
            3 => world.objects.components.ufo_occupied.push(previous),
            4 => {
                world.objects.components.ufo_occupied.extend([
                    previous,
                    world.objects.world.colliders[order[index - 2].0].entity,
                ]);
                car.route.passed = 0;
            }
            5 => world.objects.components.waypoints.order.clear(),
            6 => car.route.checkpoint = None,
            7 => car.route.checkpoint = Some(EntityId(0xfedcba98)),
            8 => limit = 0,
            9 | 10 | 11 => limit = 1,
            12 => {
                limit = 32;
                world.objects.components.ufo_occupied = order
                    .iter()
                    .map(|id| world.objects.world.colliders[id.0].entity)
                    .collect();
            }
            13 => world.objects.components.waypoints.points[checkpoint.0] = None,
            14 => world.objects.components.waypoints.points[prev.0] = None,
            15 => world.objects.world.colliders[checkpoint.0].entity = EntityId(0xfedcba98),
            16 => car.physics.pose.track_length = i32::MIN,
            17 => {
                car.physics.pose.travel_gain.shift = 0;
                car.physics.motion.travel[2] = i32::MIN;
                car.physics.motion.travel[3] = 0;
            }
            18 => {
                car.physics.motion.poses.body.matrix = Matrix([i32::MIN; 9]);
            }
            19 => {
                limit = 16;
                for p in world
                    .objects
                    .components
                    .waypoints
                    .points
                    .iter_mut()
                    .flatten()
                {
                    p.progress = -1;
                }
            }
            20 => {
                world.objects.components.ufo_occupied =
                    vec![EntityId(0xfedcba98), EntityId(0x76543210)]
            }
            21 => car.race.laps = i16::MIN,
            22 => world.objects.world.colliders[prev.0].pose.position[1] = -12345,
            23 => world
                .objects
                .components
                .waypoints
                .order
                .retain(|id| *id != checkpoint),
            24 => car.route.passed = u32::MAX,
            25 => {
                car.prefix.temporary_timer = -1;
                car.physics.pose_input.ambient_wobble = true;
                car.physics.pose_input.session_active = false;
                car.physics.pose_input.ambient_phase = 197;
            }
            26 => car.physics.pose.travel_gain.shift = u16::MAX,
            27 => car.physics.motion.poses.body.matrix = Matrix::from_angles([1700, 30000, 1900]),
            28 => {
                world.identity.entities.record_mut(previous).unwrap().object =
                    Some(Handle::Vehicle(TankId(0)))
            }
            29 => {
                world.objects.components.waypoints.order[0] =
                    tankracer_game::object_query::ObstacleId(
                        world.objects.world.colliders.len() + 1,
                    )
            }
            30 => {
                world.objects.components.waypoints.points[checkpoint.0]
                    .as_mut()
                    .unwrap()
                    .progress = 999999;
                world.objects.components.waypoints.points[prev.0]
                    .as_mut()
                    .unwrap()
                    .progress = 0;
            }
            31 => car.route.passed = 0,
            32 => limit = 0x80000000,
            33 => limit = u32::MAX,
            _ => {}
        }
        let mut height = 0x12345678;
        let initial = snapshot(&car, height);
        let config = vehicle_pose_oracle::config(&car.physics.pose);
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
        let mut colliders = Vec::new();
        for i in 0..world.objects.world.colliders.len() {
            colliders.extend_from_slice(&scene_export::record(&world.objects.world, i)[..60]);
        }
        let mut points = Vec::new();
        for p in &world.objects.components.waypoints.points {
            ints(
                &mut points,
                [i32::from(p.is_some()), p.as_ref().map_or(0, |p| p.progress)],
            );
        }
        let mut order_words = Vec::new();
        ints(
            &mut order_words,
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
        let ids = identity_assets::encode(&world.identity.entities);
        let mut frames = Vec::new();
        for _ in 0..2 {
            let routes = tankracer_game::waypoint::View {
                routes: &world.objects.components.waypoints,
                world: &world.objects.world,
                entities: &world.identity.entities,
            };
            let result = tankracer_game::vehicle_transfer::restore(
                &mut car,
                &routes,
                &world.objects.components.ufo_occupied,
                limit,
            );
            let status = match result {
                Ok(y) => {
                    height = y;
                    0
                }
                Err(e) => e,
            };
            let mut frame = Vec::new();
            ints(&mut frame, [status]);
            frame.extend(snapshot(&car, height));
            frames.push(frame);
        }
        if branch == 0 || branch == 22 || branch == 25 || branch == 27 {
            assert_eq!(frames[0][0], 0, "transfer branch {branch}");
        }
        if branch == 22 {
            assert_eq!(&frames[0][244..246], &[(-12345i32) as u16, 65535]);
        }
        cases.push(json!({"tag":format!("transfer-{branch}"),"state":initial,"config":config,"environment":environment,"travel":travel,"identity":ids,"colliders":colliders,"points":points,"order":order_words,"occupied":occupied,"limit":limit,"frames":frames}));
    }
    fs::write(out.join("vehicle-transfer.json"),serde_json::to_vec(&json!({"cases":cases,"scope":"Original vehicle_transfer::restore on real ordered ENGLAND checkpoints: occupied/disabled skips, lap and passed wrap, body-local yaw, full pose publication, motion/control reset, negative returned heights and ordered work-limit/error prefixes."})).unwrap()).unwrap();
}
