//! Complete explosion ticks with live vehicle poses and shared blast ownership.
use super::{ints, scene_explosions_oracle::snapshot, scene_export};
use serde_json::json;
use std::{fs, path::Path, sync::Arc};
use tankracer_game::{
    effect_pool::QuadHandle, explosion::blasts, math::Matrix, object_query::EntityId,
    scene_entity::Handle, scene_objects::rcc_lifecycle_probe,
};
pub fn export(root: &Path, out: &Path) {
    let mut cases = Vec::new();
    for branch in 0..26 {
        let mut world = super::reference_world::race(root);
        let class = world.objects.components.explosion_class.unwrap();
        let owner = world.vehicles.entries[0].as_ref().unwrap().physics.entity;
        let position = world.vehicles.entries[0]
            .as_ref()
            .unwrap()
            .physics
            .motion
            .poses
            .base_position
            .map(|v| v.wrapping_add(789));
        let mut others = Vec::new();
        if branch == 12 || branch == 15 || branch == 16 || branch == 22 {
            for _ in 0..2 {
                others.push(
                    rcc_lifecycle_probe::spawn_explosion(&mut world, position, 100000).unwrap(),
                );
            }
        }
        let entity = rcc_lifecycle_probe::spawn_explosion(&mut world, position, 100000).unwrap();
        let id = world
            .identity
            .entities
            .record(entity)
            .unwrap()
            .object
            .and_then(Handle::obstacle)
            .unwrap();
        let has_pool = branch != 9 && branch != 20;
        let simulation = branch != 7;
        let has_vehicles = branch != 4;
        let has_blasts = matches!(branch, 9 | 10 | 12 | 13 | 14 | 16 | 20 | 21 | 22);
        if has_blasts {
            let band_owner = if matches!(branch, 12 | 16 | 22) {
                others[0]
            } else if branch == 13 {
                EntityId(0x76543210)
            } else {
                entity
            };
            let band = world
                .objects
                .components
                .blasts
                .band(
                    blasts::Start {
                        origin: position,
                        owner: band_owner,
                        time: 1,
                        camera: world.objects.components.camera_basis,
                    },
                    if branch == 21 { i32::MIN } else { 100 },
                    false,
                    &mut world.shared.pool,
                )
                .unwrap()
                .0;
            world.objects.components.explosions[id.0]
                .as_mut()
                .unwrap()
                .band = band;
            if branch == 10 || branch == 12 || branch == 14 || branch == 21 || branch == 22 {
                let ring_owner = if branch == 22 { others[1] } else { entity };
                let ring = world
                    .objects
                    .components
                    .blasts
                    .ring(
                        blasts::Start {
                            origin: position,
                            owner: ring_owner,
                            time: 2,
                            camera: world.objects.components.camera_basis,
                        },
                        2,
                        &mut world.shared.random,
                        &mut world.shared.pool,
                    )
                    .unwrap()
                    .0;
                world.objects.components.explosions[id.0]
                    .as_mut()
                    .unwrap()
                    .ring = ring;
                if branch == 12 {
                    world.objects.components.blasts.rings[ring as usize].rays[4].handle =
                        QuadHandle(999);
                    world.objects.components.blasts.rings[ring as usize].life = 1;
                }
                if branch == 22 {
                    world.objects.components.blasts.rings[ring as usize].life = 1;
                    world.identity.entities.detach(others[1]).unwrap();
                }
            }
            if branch == 12 || branch == 16 || branch == 22 {
                let other = world
                    .identity
                    .entities
                    .record(others[0])
                    .unwrap()
                    .object
                    .and_then(Handle::obstacle)
                    .unwrap();
                world.objects.components.explosions[other.0]
                    .as_mut()
                    .unwrap()
                    .band = band;
                if branch == 16 {
                    world.identity.entities.detach(others[0]).unwrap();
                }
            }
        }
        {
            let s = world.objects.components.explosions[id.0].as_mut().unwrap();
            s.velocity = [123456, -234567, 345678];
            if matches!(branch, 2 | 3 | 4 | 5 | 6) {
                s.owner = Some(if branch == 3 {
                    EntityId(0x12345678)
                } else {
                    owner
                });
                s.attachment = [2048, -1024, 512];
                s.smoke_timer = 64;
            }
            if matches!(branch, 0 | 1 | 14 | 15 | 20 | 23 | 24) {
                s.sprite.uv = [96, 96, 127, 127];
            }
            if branch == 1 {
                s.smoke[0].active = 1;
                s.smoke[0].sprite.color = [1; 3];
                s.smoke_count = 10;
            }
            if branch == 11 {
                s.band = 2;
            }
            if branch == 13 || branch == 16 || branch == 22 {
                s.smoke_timer = 96;
            }
            if branch == 21 {
                s.timer = i32::MAX;
                s.angular_velocity = i32::MIN;
                s.fragment_count = 2;
                s.fragment_time = i32::MAX;
            }
        }
        if branch == 5 {
            world.vehicles.entries[0] = None;
        }
        if branch == 6 {
            world.identity.entities.detach(owner).unwrap();
        }
        if let Some(car) = world.vehicles.entries[0].as_mut() {
            car.physics.motion.poses.base_position = position.map(|v| v.wrapping_add(12345));
            car.physics.motion.poses.physical_position = position.map(|v| v.wrapping_sub(54321));
            car.physics.motion.poses.reference = Matrix::from_angles([1234, 23456, 3456]);
        }
        if branch == 17 || branch == 18 {
            let mut table = (*world.registry.classes[class.0]).clone();
            if branch == 17 {
                table.methods[12] = None;
            } else {
                table.maximum_slot = 11;
            }
            let table = Arc::new(table);
            world.registry.classes[class.0] = table.clone();
            world.objects.world.classes[class.0] = table;
        }
        if branch == 19 {
            world.objects.components.explosions[id.0] = None;
        }
        if branch == 23 {
            world.identity.ticks.next = Some(entity);
        }
        let fixed = world.objects.world.colliders.len();
        let initial = snapshot(&world);
        let mut vehicles = Vec::new();
        assert!(world.vehicles.entries.len() <= 8);
        for car in &world.vehicles.entries {
            vehicles.push(u16::from(car.is_some()));
            ints(
                &mut vehicles,
                [car.as_ref().map_or(0, |v| v.physics.entity.0 as i32)],
            );
            ints(
                &mut vehicles,
                car.as_ref()
                    .map_or([0; 3], |v| v.physics.motion.poses.base_position),
            );
            ints(
                &mut vehicles,
                car.as_ref()
                    .map_or(Matrix([0; 9]), |v| v.physics.motion.poses.reference)
                    .0,
            );
        }
        let vehicle_count = world.vehicles.entries.len();
        vehicles.resize(216, 0);
        let times = if branch == 1 || branch == 24 {
            vec![31, 31]
        } else if branch == 21 {
            vec![1, -1, i32::MAX, i32::MIN, 33]
        } else if matches!(branch, 2 | 3 | 4 | 5 | 6) {
            vec![1, 65, 97]
        } else if branch == 10 {
            vec![1, 97, 152, 1]
        } else {
            vec![97, 1]
        };
        let mut frames = Vec::new();
        let mut controls = Vec::new();
        let mut target = entity;
        if branch == 25 {
            let number = world.objects.components.frame_group as i32;
            target = rcc_lifecycle_probe::spawn_explosion(&mut world, position, 100000).unwrap();
            let object = world
                .identity
                .entities
                .record(target)
                .unwrap()
                .object
                .and_then(Handle::obstacle)
                .unwrap();
            assert!(object.0 >= fixed);
            frames.push(
                json!({"entity":target.0,"status":0,"object":object.0,"state":snapshot(&world)}),
            );
            controls.push([6, 0, number, 0, 100000]);
        }
        for dt in times {
            let limit = if branch == 8 {
                0
            } else if branch == 15 {
                1
            } else {
                100000
            };
            let result = rcc_lifecycle_probe::tick_explosion(
                &mut world,
                target,
                12,
                dt,
                limit,
                simulation,
                has_vehicles,
                has_pool,
            );
            let object = world
                .identity
                .entities
                .record(target)
                .unwrap()
                .object
                .and_then(Handle::obstacle)
                .unwrap();
            frames.push(json!({"entity":target.0,"status":result.map_or_else(|e|e,|()|0),"object":object.0,"state":snapshot(&world)}));
            controls.push([7, dt, 12, target.0 as i32, limit as i32]);
        }
        if matches!(branch, 0 | 14 | 23 | 24) {
            assert_eq!(frames[0]["state"]["explosion_world"][0], 0);
            assert!(
                frames[0]["state"]["lists"][2]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|v| v == entity.0)
            );
        }
        if branch == 1 {
            assert_eq!(frames[0]["state"]["explosion_world"][0], 1);
            assert_eq!(frames[1]["state"]["explosion_world"][0], 0);
        }
        if matches!(branch, 3 | 4 | 5 | 7 | 8) {
            assert_eq!(frames[0]["state"]["weapons"], initial["weapons"]);
            assert_eq!(frames[0]["state"]["random"], initial["random"]);
        }
        if branch == 6 {
            assert!(frames.iter().all(|f| f["status"] == 0));
        }
        if branch == 12 {
            assert_eq!(frames[0]["status"], -1);
            assert_eq!(frames[0]["state"]["random"], initial["random"]);
            assert_eq!(frames[0]["state"]["weapons"][0]["payload"][12], 0);
            assert_eq!(frames[0]["state"]["weapons"][2]["payload"][12], 65535);
        }
        if matches!(branch, 13 | 16 | 22) {
            assert_eq!(frames[0]["status"], -4);
            assert_ne!(frames[0]["state"]["random"], initial["random"]);
        }
        if branch == 15 {
            assert_eq!(frames[0]["status"], -3);
        }
        if branch == 17 {
            assert_eq!(frames[0]["state"], initial);
        }
        if branch == 18 {
            assert_eq!(frames[0]["status"], 0);
            assert_eq!(frames[0]["state"]["slot"], 12);
        }
        if branch == 20 {
            assert_eq!(frames[0]["status"], -5);
            assert_eq!(frames[0]["state"]["explosion_world"][0], 1);
            assert_eq!(frames[0]["state"]["slot"], 12);
        }
        if branch == 22 {
            assert_eq!(frames[0]["state"]["weapons"][0]["payload"][12], 65535);
        }
        if branch == 23 {
            assert_eq!(frames[0]["state"]["next"], initial["next"]);
            assert_eq!(frames[0]["state"]["next"], entity.0);
        }
        let (scene, _) = scene_export::encode(&world.objects.world);
        let name = format!("scene-explosion-tick-{branch}.bin");
        fs::write(
            out.join(&name),
            scene
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let mut resources = Vec::new();
        let r = world.objects.components.explosion_resources;
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
        let mut pos = Vec::new();
        ints(&mut pos, position);
        cases.push(json!({"tag":format!("explosion-tick-{branch}"),"fixed_count":fixed,"generic":world.registry.generic.0,"obstacle":world.registry.obstacle.0,"world_class":world.registry.world.0,"projectile_class":world.objects.components.projectiles.class.unwrap().0,"resources_present":true,"homing_mesh":world.objects.components.projectiles.resources.unwrap().homing_mesh.index(),"launch":vec![0u16;93],"explosion_class":class.0,"explosion_resources":resources,"explosion_position":pos,"effect_pool_present":has_pool,"simulation":simulation,"vehicle_count":if has_vehicles{vehicle_count}else{0},"vehicle_inputs":vehicles,"scene":name,"initial":initial,"controls":controls,"frames":frames}));
    }
    fs::write(out.join("scene-explosion-tick.json"),serde_json::to_vec(&json!({"cases":cases,"scope":"Original complete explosion Tick with live vehicle base/reference lookup, shared blast updates and owner clearing, smoke RNG, natural retirement and committed failures. Whole mixed native weapon scheduling follows separately."})).unwrap()).unwrap();
}
