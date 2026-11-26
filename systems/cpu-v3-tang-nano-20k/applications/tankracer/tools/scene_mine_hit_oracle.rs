//! Complete original mine contact on live cars, shared explosion and shield owners.
use super::{ints, scene_explosions_oracle, scene_export, scene_mines_oracle};
use serde_json::{Value, json};
use std::{fs, path::Path};
use tankracer_game::{
    explosion::blasts,
    math::Matrix,
    mine::Owner,
    object_query::{EntityId, TankId},
    pose::ExternalBasis,
    scene_entity::Handle,
    scene_objects::rcc_lifecycle_probe,
    vehicle_runtime::SinglePlayer,
};
fn snapshot(world: &SinglePlayer) -> Value {
    let mut out = scene_explosions_oracle::snapshot(world);
    let membership = scene_mines_oracle::snapshot(world);
    for key in ["grid", "mine_order", "mine_count"] {
        out[key] = membership[key].clone();
    }
    let mut cars = Vec::new();
    for car in &world.vehicles.entries {
        let start = cars.len();
        if let Some(car) = car {
            let v = &car.physics;
            ints(
                &mut cars,
                [
                    v.entity.0 as i32,
                    i32::from(v.player),
                    v.blast.protection_timer,
                ],
            );
            ints(&mut cars, v.motion.poses.base_position);
            ints(&mut cars, v.motion.velocity);
            ints(&mut cars, v.motion.angular);
            ints(&mut cars, [i32::from(v.external_basis().is_some())]);
            ints(&mut cars, v.external_basis().map_or([0; 9], |v| v.0));
            ints(
                &mut cars,
                [v.blast.health, v.collision.collision_size as i32],
            );
            let e = &v.contact.sampling.effect;
            ints(
                &mut cars,
                [
                    e.timer,
                    e.restore,
                    e.active,
                    e.phase,
                    e.impulse[0],
                    e.impulse[1],
                    e.pose,
                    i32::from(e.clear_player_slot),
                ],
            );
            assert_eq!(cars.len() - start, 64);
        } else {
            cars.resize(start + 64, 0);
        }
    }
    assert!(cars.len() <= 512);
    cars.resize(512, 0);
    out["mine_cars"] = json!(cars);
    let mut tails = Vec::new();
    ints(
        &mut tails,
        world.shared.shield.banks.iter().map(|v| v.tail as i32),
    );
    out["shield_tails"] = json!(tails);
    let (duration, magnitude) = world.objects.components.camera_impulse.unwrap_or((0, 0));
    let mut camera = Vec::new();
    ints(
        &mut camera,
        [
            i32::from(world.objects.components.camera_impulse.is_some()),
            duration as i32,
            magnitude,
        ],
    );
    out["camera_impulse"] = json!(camera);
    out
}
pub fn export(root: &Path, out: &Path) {
    let mut cases = Vec::new();
    for branch in 0..29 {
        let mut world = super::reference_world::race(root);
        for _ in 0..2 {
            let car = world.vehicles.entries[0].as_ref().unwrap();
            let request = tankracer_game::vehicle_loading::Request {
                model: car.model.clone(),
                spawn: car.physics.motion.poses.body,
                static_allocation: true,
                environment: tankracer_game::vehicle_startup::Environment {
                    ambient_wobble: false,
                    session_active: false,
                    ambient_phase: 0,
                    gun_pivot: [0; 3],
                },
            };
            world.add_vehicle(request).unwrap();
        }
        let car = world.vehicles.entries[0].as_ref().unwrap();
        let owner = Owner {
            entity: car.physics.entity,
            position: car.physics.motion.poses.base_position,
        };
        let kind = if branch == 27 {
            65537
        } else {
            u32::from(matches!(
                branch,
                1 | 3 | 5 | 7 | 8 | 9 | 10 | 11 | 12 | 13 | 20 | 23 | 26
            ))
        };
        let entity =
            rcc_lifecycle_probe::spawn_mine(&mut world, owner, false, u32::from(kind == 1), 100000)
                .unwrap();
        let id = world
            .identity
            .entities
            .record(entity)
            .unwrap()
            .object
            .and_then(Handle::obstacle)
            .unwrap();
        let position = world.objects.world.colliders[id.0].pose.position;
        let state = world.objects.components.mines.get_mut(id).unwrap();
        state.kind = kind;
        state.armed = !matches!(branch, 14 | 15 | 28);
        state.born = if branch == 15 { u32::MAX } else { 5 };
        state.last_contact = state.born;
        for (i, car) in world.vehicles.entries.iter_mut().enumerate() {
            let Some(car) = car else {
                continue;
            };
            car.slot = i as u8;
            world.vehicles.racers[i] = Some(TankId(i));
            let e = &mut car.physics.contact.sampling.effect;
            e.timer = 777;
            e.restore = -111;
            e.active = 7;
            e.phase = 8193;
            e.impulse = [-123456, 234567];
            e.pose = if branch == 13 { 65535 } else { 34567 };
            e.clear_player_slot = true;
        }
        let has_peers = branch != 10;
        let mut previous = 0;
        if matches!(branch, 8 | 9 | 10 | 11 | 12) {
            previous = if branch == 8 {
                owner.entity.0
            } else if branch == 11 {
                0x76543210
            } else {
                world.vehicles.entries[1].as_ref().unwrap().physics.entity.0
            };
        }
        if branch == 12 {
            world.vehicles.entries[2].as_mut().unwrap().physics.entity = EntityId(previous);
            world.vehicles.entries[1].as_mut().unwrap().slot = 8;
            world.vehicles.entries[2].as_mut().unwrap().slot = 1;
            world.vehicles.racers[8] = Some(TankId(1));
            world.vehicles.racers[1] = Some(TankId(2));
            world.vehicles.racers[2] = None;
        }
        world.shared.shield.banks[0].tail = previous;
        world.shared.shield.banks[1].tail = 0x12345678;
        let v = &mut world.vehicles.entries[0].as_mut().unwrap().physics;
        v.player = !matches!(branch, 2 | 3 | 8 | 9 | 10 | 11 | 12);
        v.blast.protection_timer = if branch == 4 || branch == 23 {
            1
        } else if branch == 5 {
            -1
        } else {
            0
        };
        v.motion.poses.base_position = [position[0] - 700, position[1] + 321, position[2] + 1400];
        v.motion.poses.reference = Matrix::from_angles([700, 11000, 1300]);
        v.pose_input.external = if branch == 4 || branch == 6 || branch == 7 {
            ExternalBasis::None
        } else {
            ExternalBasis::Body
        };
        v.motion.velocity = if branch == 22 {
            [i32::MIN, i32::MAX, -1234567]
        } else {
            [300000, 20000, -400000]
        };
        v.motion.angular = if branch == 22 {
            [i32::MAX, i32::MIN, 1234567]
        } else {
            [0; 3]
        };
        v.blast.health = if branch == 22 { -123 } else { 5000 };
        v.collision.collision_size = 13;
        world.objects.components.camera_impulse = Some((17, -250));
        world.objects.components.stamp_units = 100;
        if branch == 16 {
            rcc_lifecycle_probe::remove_mine_component(&mut world, id);
        }
        if branch == 17 {
            world.objects.components.explosion_class = None;
        }
        if branch == 19 {
            world.shared.pool.quad_count = 160;
        }
        if branch == 20 {
            world.objects.components.blasts.materials = None;
        }
        if branch == 21 {
            world.objects.components.mines.order.clear();
        }
        if branch == 25 {
            // Null mine Deactivate leaves the gameplay contact prefix intact.
            let class = world.objects.components.mines.class.unwrap();
            let mut table = (*world.registry.classes[class.0]).clone();
            table.methods[8] = None;
            let table = std::sync::Arc::new(table);
            world.registry.classes[class.0] = table.clone();
            world.objects.world.classes[class.0] = table;
        }
        let mut peers = vec![65535u16; 9];
        if has_peers {
            for (i, car) in world.vehicles.entries.iter().enumerate().skip(1) {
                if let Some(car) = car {
                    let slot = usize::from(car.slot);
                    if world.vehicles.racers[slot] == Some(TankId(i)) {
                        peers[slot] = i as u16;
                    }
                }
            }
        }
        let mut poses = Vec::new();
        for car in &world.vehicles.entries {
            poses.push(u16::from(car.is_some()));
            ints(
                &mut poses,
                [car.as_ref().map_or(0, |v| v.physics.entity.0 as i32)],
            );
            ints(
                &mut poses,
                car.as_ref()
                    .map_or([0; 3], |v| v.physics.motion.poses.base_position),
            );
            ints(
                &mut poses,
                car.as_ref()
                    .map_or(Matrix([0; 9]), |v| v.physics.motion.poses.reference)
                    .0,
            );
        }
        poses.resize(216, 0);
        let fixed = world.objects.world.colliders.len();
        let initial = snapshot(&world);
        let numbers: Vec<u32> = if branch == 28 {
            vec![5]
        } else if branch == 14 {
            vec![5, 6, 8, 9]
        } else if branch == 15 {
            vec![u32::MAX, 0, 2, 3]
        } else {
            vec![100]
        };
        let mut frames = Vec::new();
        let mut controls = Vec::new();
        for number in numbers {
            let limit = if branch == 18 || branch == 28 {
                0
            } else if branch == 24 {
                1
            } else {
                100000
            };
            let result = rcc_lifecycle_probe::hit_mine(&mut world, id, number, limit, has_peers);
            let status = result.map_or_else(|e| e, |()| 0);
            frames.push(
                json!({"entity":entity.0,"status":status,"object":id.0,"state":snapshot(&world)}),
            );
            controls.push([
                10,
                id.0 as i32,
                number as i32,
                entity.0 as i32,
                limit as i32,
                100,
            ]);
            if branch == 26 && status == 0 {
                let exploded = world
                    .objects
                    .components
                    .explosion_world
                    .slots
                    .iter()
                    .find_map(|s| s.entity)
                    .unwrap();
                let explosion_id = world
                    .identity
                    .entities
                    .record(exploded)
                    .unwrap()
                    .object
                    .and_then(Handle::obstacle)
                    .unwrap();
                for dt in [97, 16, 600] {
                    let status = rcc_lifecycle_probe::tick_explosion(
                        &mut world, exploded, 12, dt, 100000, true, true, true,
                    )
                    .map_or_else(|e| e, |()| 0);
                    frames.push(json!({"entity":exploded.0,"status":status,"object":explosion_id.0,"state":snapshot(&world)}));
                    controls.push([7, dt, 12, exploded.0 as i32, 100000, 0]);
                }
            }
        }
        if matches!(
            branch,
            0 | 1 | 2 | 3 | 4 | 5 | 8 | 9 | 10 | 11 | 12 | 13 | 19 | 22 | 23 | 25 | 26 | 27
        ) {
            assert!(frames.iter().all(|f| f["status"] == 0), "branch {branch}");
        }
        if branch == 6 || branch == 7 {
            assert_eq!(frames[0]["status"], -4);
            assert_eq!(frames[0]["state"]["mine_count"], 1);
            assert_eq!(frames[0]["state"]["explosion_world"][0], 1);
            assert_eq!(
                frames[0]["state"]["camera_impulse"],
                initial["camera_impulse"]
            );
            assert_eq!(frames[0]["state"]["mine_cars"][46], 13);
            assert_ne!(
                frames[0]["state"]["mine_cars"][12],
                initial["mine_cars"][12]
            );
        }
        if branch == 1 {
            assert_eq!(frames[0]["state"]["mine_cars"][48], 6000);
        }
        if branch == 8 {
            assert_eq!(frames[0]["state"]["mine_cars"][52], 0);
        }
        if branch == 9 {
            assert_eq!(frames[0]["state"]["mine_cars"][64 + 52], 0);
        }
        if branch == 10 {
            assert_eq!(frames[0]["state"]["mine_cars"][64 + 52], 7);
        }
        if branch == 12 {
            assert_eq!(frames[0]["state"]["mine_cars"][64 + 52], 7);
            assert_eq!(frames[0]["state"]["mine_cars"][128 + 52], 0);
        }
        if branch == 13 {
            assert_eq!(frames[0]["state"]["mine_cars"][50], 65535);
            assert_eq!(frames[0]["state"]["mine_cars"][51], 65535);
        }
        if branch == 14 || branch == 15 {
            assert!(
                frames[..3]
                    .iter()
                    .all(|f| f["state"]["explosion_world"][0] == 0)
            );
            assert_eq!(frames[3]["state"]["explosion_world"][0], 1);
        }
        if branch == 4 || branch == 23 {
            assert_eq!(
                frames[0]["state"]["camera_impulse"],
                initial["camera_impulse"]
            );
            assert_eq!(frames[0]["state"]["mine_cars"], initial["mine_cars"]);
        }
        if branch == 24 {
            assert_eq!(frames[0]["status"], -3);
            assert_eq!(frames[0]["state"]["slot"], 8);
            assert_eq!(frames[0]["state"]["mine_count"], 1);
            assert_eq!(frames[0]["state"]["camera_impulse"][2], 600);
        }
        if branch == 28 {
            assert_eq!(frames[0]["status"], 0);
            assert_eq!(frames[0]["state"]["random"], initial["random"]);
            assert_eq!(frames[0]["state"]["explosion_world"][0], 0);
        }
        let (scene, _) = scene_export::encode(&world.objects.world);
        let name = format!("scene-mine-hit-{branch}.bin");
        fs::write(
            out.join(&name),
            scene
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let r = world.objects.components.explosion_resources;
        let mut resources = Vec::new();
        ints(
            &mut resources,
            [
                r.main as i32,
                r.smoke as i32,
                i32::from(r.small),
                r.damping as i32,
                i32::from(r.map_fog),
            ],
        );
        let m = world
            .objects
            .components
            .blasts
            .materials
            .unwrap_or(blasts::Materials {
                band: 0,
                ring: [0; 3],
            });
        let mut materials = Vec::new();
        ints(
            &mut materials,
            [
                i32::from(world.objects.components.blasts.materials.is_some()),
                m.band as i32,
            ],
        );
        ints(&mut materials, m.ring.map(|v| v as i32));
        let mut camera = Vec::new();
        ints(&mut camera, world.objects.components.camera_basis.0);
        let mut pos = Vec::new();
        ints(&mut pos, position);
        cases.push(json!({"tag":format!("mine-hit-{branch}"),"fixed_count":fixed,"generic":world.registry.generic.0,"obstacle":world.registry.obstacle.0,"world_class":world.registry.world.0,"projectile_class":world.objects.components.projectiles.class.unwrap().0,"resources_present":true,"homing_mesh":world.objects.components.projectiles.resources.unwrap().homing_mesh.index(),"launch":vec![0u16;93],"explosion_class":world.objects.components.explosion_class.map_or(-1,|v|v.0 as i32),"explosion_resources":resources,"explosion_position":pos,"effect_pool_present":true,"simulation":true,"vehicle_count":world.vehicles.entries.len(),"vehicle_inputs":poses,"blast_materials":materials,"blast_camera":camera,"peer_indices":peers,"scene":name,"initial":initial,"controls":controls,"frames":frames}));
    }
    fs::write(out.join("scene-mine-hit.json"),serde_json::to_vec(&json!({"cases":cases,"scope":"Complete original mine contact method 122: arming, shared explosion before impulse, player camera impulse, electric surface effect and ordered peer/shield tails, final nested deactivation, committed failures and subsequent explosion ticks. Outer collision selection and mixed native scheduling remain integration work."})).unwrap()).unwrap();
}
