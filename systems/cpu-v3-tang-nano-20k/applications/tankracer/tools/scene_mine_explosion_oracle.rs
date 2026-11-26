//! Mine contact explosion attachment, electric rings and shared slot pressure.
use super::{ints, scene_explosions_oracle::snapshot, scene_export};
use serde_json::json;
use std::{fs, path::Path};
use tankracer_game::{
    effect_pool::Quad, explosion::blasts, math::Matrix, object_query::EntityId,
    scene_entity::Handle, scene_objects::rcc_lifecycle_probe,
};
pub fn export(root: &Path, out: &Path) {
    let mut cases = Vec::new();
    for branch in 0..15 {
        let mut world = super::reference_world::race(root);
        let position = world.vehicles.entries[0]
            .as_ref()
            .unwrap()
            .physics
            .motion
            .poses
            .base_position
            .map(|v| v.wrapping_add(789));
        if matches!(branch, 7 | 8 | 9 | 10) {
            for i in 0..if branch == 10 { 15 } else { 2 } {
                world.objects.components.frame_group = i + 1;
                let entity =
                    rcc_lifecycle_probe::spawn_explosion(&mut world, position, 100000).unwrap();
                if branch != 10 {
                    let time = if branch == 9 {
                        if i == 0 { i32::MIN } else { i32::MAX }
                    } else {
                        i as i32 + 1
                    };
                    let slot = world
                        .objects
                        .components
                        .blasts
                        .ring(
                            blasts::Start {
                                origin: position,
                                owner: entity,
                                time,
                                camera: world.objects.components.camera_basis,
                            },
                            1,
                            &mut world.shared.random,
                            &mut world.shared.pool,
                        )
                        .unwrap()
                        .0;
                    let id = world
                        .identity
                        .entities
                        .record(entity)
                        .unwrap()
                        .object
                        .and_then(Handle::obstacle)
                        .unwrap();
                    world.objects.components.explosions[id.0]
                        .as_mut()
                        .unwrap()
                        .ring = slot;
                    if branch == 8 && i == 0 {
                        world.objects.components.explosions[id.0] = None;
                    }
                }
            }
            if branch == 7 {
                world.objects.components.blasts.rings[0].owner = Some(EntityId(0x76543210));
            }
        }
        if branch == 3 {
            world.objects.components.blasts.materials = None;
        }
        if branch == 4 {
            world.shared.pool.quad_count = 160;
        }
        if branch == 5 {
            for _ in 0..155 {
                world.shared.pool.allocate_quad(Quad::ZERO);
            }
        }
        if branch == 6 {
            for quad in &mut world.shared.pool.quads {
                quad.points[0][1] = 0;
            }
            world.shared.pool.quad_count = 0;
        }
        if branch == 10 {
            world.objects.components.explosion_resources.map_fog = true;
            world.objects.components.explosion_world.flash_timer = 7;
        }
        if branch == 11 {
            world.objects.components.explosion_class = None;
        }
        world.objects.components.camera_basis = if branch == 13 {
            Matrix([i32::MAX, i32::MIN, -1, 0, 4096, 1, i32::MIN, 0, i32::MAX])
        } else {
            Matrix::from_angles([1300, 19000, 2300])
        };
        let car = world.vehicles.entries[0].as_mut().unwrap();
        car.physics.motion.velocity = if branch == 13 {
            [i32::MIN, i32::MAX, i32::MIN + 63]
        } else {
            [11111, -22222, 33333]
        };
        car.physics.motion.poses.base_position = position.map(|v| v.wrapping_add(12345));
        car.physics.motion.poses.reference = Matrix::from_angles([1234, 23456, 3456]);
        let mut poses = Vec::new();
        let mut hits = Vec::new();
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
            ints(&mut hits, [0]);
            hits.extend([0, 0]);
        }
        poses.resize(216, 0);
        hits.resize(104, 0);
        let initial = snapshot(&world);
        let fixed = world.objects.world.colliders.len();
        let kinds = if branch == 0 {
            vec![0, 1, 2, 65537, -1, 1, 0]
        } else {
            vec![0, 1, 1]
        };
        let mut frames = Vec::new();
        let mut controls = Vec::new();
        for (step, kind) in kinds.into_iter().enumerate() {
            let stamp = if branch == 14 {
                0
            } else if branch == 9 {
                [i32::MIN + 1, -1, i32::MAX][step]
            } else {
                step as i32 + 10
            };
            let number = if branch == 9 {
                u32::MAX.wrapping_sub(step as u32)
            } else {
                step as u32 + 100
            };
            let limit = if branch == 12 { 0 } else { 100000 };
            world.objects.components.frame_group = number;
            world.objects.components.stamp_units = stamp;
            let result = rcc_lifecycle_probe::spawn_mine_explosion(
                &mut world,
                position,
                kind as u32,
                limit,
                branch != 2,
            );
            let (entity, status) = result.map_or_else(|e| (EntityId(0), e), |e| (e, 0));
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
            controls.push([9, kind, stamp, 0, limit as i32, number as i32]);
            if branch == 1 && status == 0 {
                for dt in [97, 16, 600] {
                    let status = rcc_lifecycle_probe::tick_explosion(
                        &mut world, entity, 12, dt, 100000, true, true, true,
                    )
                    .map_or_else(|e| e, |()| 0);
                    frames.push(json!({"entity":entity.0,"status":status,"object":object,"state":snapshot(&world)}));
                    controls.push([7, dt, 12, entity.0 as i32, 100000, 0]);
                }
            }
        }
        if matches!(branch, 0 | 1 | 4 | 5 | 6 | 9 | 10 | 13) {
            assert!(frames.iter().all(|f| f["status"] == 0), "branch {branch}");
        }
        if branch == 2 || branch == 3 {
            assert!(frames.iter().all(|f| f["status"] == -5));
        }
        if branch == 7 || branch == 8 {
            assert_eq!(frames[0]["status"], -4);
            let payload = &frames[0]["state"]["weapons"]
                .as_array()
                .unwrap()
                .last()
                .unwrap()["payload"];
            assert_eq!(payload[6], 1, "owner/velocity precede ring failure");
            assert_eq!(
                payload[14], 65535,
                "ring reference publishes after eviction owner"
            );
        }
        if branch == 11 || branch == 12 {
            assert!(
                frames
                    .iter()
                    .all(|f| f["state"]["random"] == initial["random"])
            );
        }
        if branch == 10 {
            assert_eq!(
                frames[0]["state"]["explosion_world"][6], 7,
                "mine leaves flash untouched"
            );
        }
        if branch == 14 {
            assert_eq!(frames[2]["status"], -4);
        }
        let (scene, _) = scene_export::encode(&world.objects.world);
        let name = format!("scene-mine-explosion-{branch}.bin");
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
        cases.push(json!({"tag":format!("mine-explosion-{branch}"),"fixed_count":fixed,"generic":world.registry.generic.0,"obstacle":world.registry.obstacle.0,"world_class":world.registry.world.0,"projectile_class":world.objects.components.projectiles.class.unwrap().0,"resources_present":true,"homing_mesh":world.objects.components.projectiles.resources.unwrap().homing_mesh.index(),"launch":vec![0u16;93],"explosion_class":world.objects.components.explosion_class.map_or(-1,|v|v.0 as i32),"explosion_resources":resources,"explosion_position":pos,"effect_pool_present":branch!=2,"simulation":true,"vehicle_count":world.vehicles.entries.len(),"vehicle_inputs":poses,"hit_inputs":hits,"hit_vehicle":0,"blast_materials":materials,"blast_camera":camera,"contact":vec![0u16;18],"scene":name,"initial":initial,"controls":controls,"frames":frames}));
    }
    fs::write(out.join("scene-mine-explosion.json"),serde_json::to_vec(&json!({"cases":cases,"scope":"Original mine explosion factory: live vehicle attachment, normal/electric ring, full-width kind, source velocity shift, slot eviction, pool pressure and contact-to-tick sequences. Vehicle impulse/electric state and outer mine deactivation are separate integration boundaries."})).unwrap()).unwrap();
}
