//! Complete six-kind weapon explosion factories, shared effects and car contact.
use super::{ints, scene_explosions_oracle::snapshot, scene_export};
use serde_json::json;
use std::{fs, path::Path};
use tankracer_game::{
    effect_pool::Quad,
    explosion::{self, blasts},
    math::Matrix,
    object_query::{EntityId, SegmentHit, TankId},
    projectile,
    scene_entity::Handle,
    scene_objects::rcc_lifecycle_probe,
};
pub fn export(root: &Path, out: &Path) {
    let mut cases = Vec::new();
    for branch in 0..23 {
        let mut world = super::reference_world::race(root);
        let position = world.vehicles.entries[0]
            .as_ref()
            .unwrap()
            .physics
            .motion
            .poses
            .base_position
            .map(|v| v.wrapping_add(789));
        let mut old = Vec::new();
        if matches!(branch, 11 | 12 | 13 | 14 | 15) {
            for i in 0..if branch == 14 || branch == 15 { 15 } else { 2 } {
                world.objects.components.frame_group = i + 1;
                old.push(
                    rcc_lifecycle_probe::spawn_explosion(&mut world, position, 100000).unwrap(),
                );
            }
            if matches!(branch, 11 | 12 | 13) {
                for (i, e) in old.iter().enumerate() {
                    let time = if branch == 13 {
                        if i == 0 { i32::MIN } else { i32::MAX }
                    } else {
                        i as i32 + 1
                    };
                    let slot = world
                        .objects
                        .components
                        .blasts
                        .band(
                            blasts::Start {
                                origin: position,
                                owner: *e,
                                time,
                                camera: world.objects.components.camera_basis,
                            },
                            20480,
                            false,
                            &mut world.shared.pool,
                        )
                        .unwrap()
                        .0;
                    let id = world
                        .identity
                        .entities
                        .record(*e)
                        .unwrap()
                        .object
                        .and_then(Handle::obstacle)
                        .unwrap();
                    world.objects.components.explosions[id.0]
                        .as_mut()
                        .unwrap()
                        .band = slot;
                }
                if branch == 11 {
                    world.objects.components.blasts.bands[0].owner = Some(EntityId(0x76543210));
                }
                if branch == 12 {
                    let id = world
                        .identity
                        .entities
                        .record(old[0])
                        .unwrap()
                        .object
                        .and_then(Handle::obstacle)
                        .unwrap();
                    world.objects.components.explosions[id.0] = None;
                }
            }
        }
        if branch == 8 {
            world.shared.pool.quad_count = 160;
        }
        if branch == 9 {
            for _ in 0..155 {
                world.shared.pool.allocate_quad(Quad::ZERO);
            }
        }
        if branch == 10 {
            for q in &mut world.shared.pool.quads {
                q.points[0][1] = 0;
            }
            world.shared.pool.quad_count = 0;
        }
        if branch == 7 || branch == 21 {
            world.objects.components.blasts.materials = None;
        }
        if branch == 16 {
            world.objects.components.explosion_class = None;
        }
        if branch == 14 || branch == 15 {
            world.objects.components.explosion_resources.map_fog = true;
            world.objects.components.explosion_world.flash_timer = 7;
        }
        world.objects.components.camera_basis = if branch == 18 {
            Matrix([i32::MAX, i32::MIN, -1, 0, 4096, 1, i32::MIN, 0, i32::MAX])
        } else {
            Matrix::from_angles([1300, 19000, 2300])
        };
        {
            let car = world.vehicles.entries[0].as_mut().unwrap();
            car.physics.motion.velocity = [11111, -22222, 33333];
            car.prefix.spin.speed = i32::from(branch == 2);
            car.physics.motion.poses.base_position = position.map(|v| v.wrapping_add(12345));
            car.physics.motion.poses.reference = Matrix::from_angles([1234, 23456, 3456]);
            if matches!(branch, 3 | 15 | 20) {
                car.model.parts[tankracer_assets::config::Part::Body.slot()] = None;
            }
        }
        if branch == 5 || branch == 21 {
            world.vehicles.entries[0] = None;
        }
        let has_vehicles = branch != 4;
        let has_pool = branch != 6 && branch != 20;
        let mut poses = Vec::new();
        let mut hits = Vec::new();
        assert!(world.vehicles.entries.len() <= 8);
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
            hits.push(u16::from(car.is_some()));
            ints(
                &mut hits,
                [car.as_ref().map_or(0, |v| v.physics.entity.0 as i32)],
            );
            ints(
                &mut hits,
                car.as_ref().map_or([0; 3], |v| v.physics.motion.velocity),
            );
            ints(&mut hits, [car.as_ref().map_or(0, |v| v.prefix.spin.speed)]);
            let body = car
                .as_ref()
                .and_then(|v| v.model.parts[tankracer_assets::config::Part::Body.slot()].as_ref());
            hits.push(u16::from(body.is_some()));
            hits.push(body.map_or(0, |b| b.source.bounds[1][2]) as u16);
        }
        let count = if has_vehicles {
            world.vehicles.entries.len()
        } else {
            0
        };
        poses.resize(216, 0);
        hits.resize(104, 0);
        let local = [13456, -2345, 45321];
        let point = position.map(|v| v.wrapping_sub(1234));
        let velocity = [123456, -654321, -987654];
        let contact = explosion::Contact {
            hit: projectile::sweep::Hit {
                vehicle: TankId(0),
                local: SegmentHit {
                    point: local,
                    normal: [0; 3],
                },
                world: SegmentHit {
                    point,
                    normal: [0; 3],
                },
            },
            velocity,
        };
        let fixed = world.objects.world.colliders.len();
        let initial = snapshot(&world);
        let kinds: Vec<i32> = if branch == 0 {
            vec![1, 4, 5, 1, 5, 4, 0, 2, 3]
        } else if matches!(branch, 11 | 12 | 13 | 22) {
            vec![5, 1, 4]
        } else if matches!(branch, 14 | 15 | 20 | 21) {
            vec![1]
        } else if branch == 19 {
            vec![1, 5, 2]
        } else {
            vec![0, 1, 2, 3, 4, 5]
        };
        let touching = matches!(branch, 1 | 2 | 3 | 4 | 5 | 14 | 15 | 19 | 20 | 21);
        let mut controls = Vec::new();
        let mut frames = Vec::new();
        for (step, kind) in kinds.into_iter().enumerate() {
            let stamp = if branch == 22 {
                0
            } else if branch == 13 {
                [i32::MIN + 1, -1, i32::MAX][step]
            } else {
                step as i32 + 10
            };
            let number = if branch == 13 {
                u32::MAX.wrapping_sub(step as u32)
            } else {
                step as u32 + 100
            };
            world.objects.components.stamp_units = stamp;
            world.objects.components.frame_group = number;
            let limit = if branch == 17 { 0 } else { 100000 };
            let result = rcc_lifecycle_probe::spawn_weapon_explosion(
                &mut world,
                position,
                super::kind_of(kind),
                touching.then_some(contact),
                limit,
                has_vehicles,
                has_pool,
            );
            let (entity, status) = match result {
                Ok(e) => (e, 0),
                Err(e) => (EntityId(0), e),
            };
            let object = world
                .identity
                .entities
                .record(entity)
                .and_then(|r| r.object)
                .and_then(Handle::obstacle)
                .map(|v| v.0);
            frames.push(
                json!({"entity":entity.0,"status":status,"object":object,"state":snapshot(&world)}),
            );
            controls.push([
                8,
                kind,
                stamp,
                i32::from(touching),
                limit as i32,
                number as i32,
            ]);
            if branch == 19 && status == 0 {
                for dt in [97, 16] {
                    let result = rcc_lifecycle_probe::tick_explosion(
                        &mut world, entity, 12, dt, 100000, true, true, true,
                    );
                    frames.push(json!({"entity":entity.0,"status":result.map_or_else(|e|e,|()|0),"object":object,"state":snapshot(&world)}));
                    controls.push([7, dt, 12, entity.0 as i32, 100000, 0]);
                }
            }
        }
        if branch == 14 {
            assert_eq!(frames[0]["status"], 0);
            assert_eq!(frames[0]["state"]["explosion_world"][2], 0);
            assert_eq!(frames[0]["state"]["explosion_world"][4], 1);
            assert_eq!(frames[0]["state"]["explosion_world"][6], 50);
        }
        if branch == 15 {
            assert_eq!(frames[0]["status"], -4);
            assert_eq!(frames[0]["state"]["explosion_world"][2], 1);
            assert_eq!(frames[0]["state"]["explosion_world"][6], 7);
        }
        if matches!(branch, 11 | 12) {
            assert_eq!(frames[0]["status"], -4);
            let last = frames[0]["state"]["weapons"]
                .as_array()
                .unwrap()
                .last()
                .unwrap();
            assert_eq!(last["payload"][6], 1);
            assert_eq!(last["payload"][14], 65535);
        }
        if branch == 20 || branch == 21 {
            assert_eq!(frames[0]["status"], -5);
        }
        if branch == 1 {
            assert!(frames.iter().all(|f| f["status"] == 0));
            assert_eq!(frames[1]["state"]["weapons"][1]["payload"][34], 15);
        }
        if branch == 2 {
            for f in &frames {
                assert_eq!(
                    f["state"]["weapons"].as_array().unwrap().last().unwrap()["payload"][34],
                    0
                );
            }
        }
        if matches!(branch, 3 | 4 | 5) {
            assert!(frames.iter().all(|f| f["status"] == -4));
        }
        if branch == 6 || branch == 7 {
            assert_eq!(
                frames
                    .iter()
                    .map(|f| f["status"].as_i64().unwrap())
                    .collect::<Vec<_>>(),
                vec![0, -5, 0, 0, -5, -5]
            );
        }
        if branch == 8 || branch == 9 {
            assert_eq!(frames[1]["state"]["weapons"][1]["payload"][12], 65535);
        }
        if branch == 16 || branch == 17 {
            assert!(
                frames
                    .iter()
                    .all(|f| f["state"]["random"] == initial["random"])
            );
        }
        if branch == 19 {
            assert_eq!(frames.len(), 9);
            assert!(frames.iter().all(|f| f["status"] == 0));
        }
        if branch == 22 {
            assert_eq!(
                frames
                    .iter()
                    .map(|f| f["status"].as_i64().unwrap())
                    .collect::<Vec<_>>(),
                vec![0, 0, -4]
            );
        }
        let (scene, _) = scene_export::encode(&world.objects.world);
        let name = format!("scene-weapon-explosion-{branch}.bin");
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
        let mut packed_camera = Vec::new();
        ints(&mut packed_camera, world.objects.components.camera_basis.0);
        let mut pos = Vec::new();
        ints(&mut pos, position);
        let mut contact_data = Vec::new();
        ints(&mut contact_data, local);
        ints(&mut contact_data, point);
        ints(&mut contact_data, velocity);
        cases.push(json!({"tag":format!("weapon-explosion-{branch}"),"fixed_count":fixed,"generic":world.registry.generic.0,"obstacle":world.registry.obstacle.0,"world_class":world.registry.world.0,"projectile_class":world.objects.components.projectiles.class.unwrap().0,"resources_present":true,"homing_mesh":world.objects.components.projectiles.resources.unwrap().homing_mesh.index(),"launch":vec![0u16;93],"explosion_class":world.objects.components.explosion_class.map_or(-1,|v|v.0 as i32),"explosion_resources":resources,"explosion_position":pos,"effect_pool_present":has_pool,"simulation":true,"vehicle_count":count,"vehicle_inputs":poses,"hit_inputs":hits,"hit_vehicle":0,"blast_materials":materials,"blast_camera":packed_camera,"contact":contact_data,"scene":name,"initial":initial,"controls":controls,"frames":frames}));
    }
    fs::write(out.join("scene-weapon-explosion.json"),serde_json::to_vec(&json!({"cases":cases,"scope":"Original six-kind weapon explosion factory with full scene construction, shared blast creation/eviction, contact fragments, flash state and committed failures; includes contact followed by full explosion ticks. Mine/projectile impact and mixed native world frames follow separately."})).unwrap()).unwrap();
}
