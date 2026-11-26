//! Original complete UFO projectile ticks, target attachment and carrier resolution.
use super::{ints, scene_export};
use serde_json::{Value, json};
use std::{fs, path::Path, sync::Arc};
use tankracer_game::{
    math::{Axis, Matrix},
    object_query::{EntityId, ObstacleId, TankId},
    pose::Pose,
    projectile::{self, launch::Owner},
    scene_entity::Handle,
    scene_objects::rcc_lifecycle_probe,
    vehicle_runtime::SinglePlayer,
};
fn snapshot(world: &SinglePlayer) -> Value {
    let mut value = super::scene_projectile_area_oracle::snapshot(world);
    let memberships = super::scene_mines_oracle::snapshot(world);
    for key in ["grid", "mine_count", "mine_order"] {
        value[key] = memberships[key].clone();
    }
    let mut basis = Vec::new();
    for car in &world.vehicles.entries {
        ints(
            &mut basis,
            car.as_ref()
                .map_or([0; 9], |c| c.physics.motion.poses.reference.0),
        );
    }
    value["ufo_car_basis"] = json!(basis);
    value
}
fn object(world: &SinglePlayer, entity: EntityId) -> Option<usize> {
    world
        .identity
        .entities
        .record(entity)
        .and_then(|r| r.object)
        .and_then(Handle::obstacle)
        .map(|v| v.0)
}
pub fn export(root: &Path, out: &Path) {
    let mut cases = Vec::new();
    for branch in 0..34 {
        let mut world = super::reference_world::full_race(root);
        for _ in 0..2 {
            let car = world.vehicles.entries[0].as_ref().unwrap();
            world
                .add_vehicle(tankracer_game::vehicle_loading::Request {
                    model: car.model.clone(),
                    spawn: car.physics.motion.poses.body,
                    static_allocation: true,
                    environment: tankracer_game::vehicle_startup::Environment {
                        ambient_wobble: false,
                        session_active: false,
                        ambient_phase: 0,
                        gun_pivot: [0; 3],
                    },
                })
                .unwrap();
        }
        let position = world.vehicles.entries[0]
            .as_ref()
            .unwrap()
            .physics
            .motion
            .poses
            .base_position;
        for (i, entry) in world.vehicles.entries.iter_mut().enumerate() {
            let car = entry.as_mut().unwrap();
            assert!(car.ai.is_none());
            world.vehicles.racers[i] = Some(TankId(i));
            car.slot = if branch == 30 && i == 0 { 7 } else { i as u8 };
            car.physics.player = i == 0 && branch != 1;
            car.physics.motion.poses.base_position =
                [position[0] + i as i32 * 200000, position[1], position[2]];
            car.physics.motion.poses.physical_position = car.physics.motion.poses.base_position;
            car.physics.motion.poses.reference = Matrix::IDENTITY;
            car.physics.motion.velocity = [11111, -22222, 33333];
            car.physics.blast.protection_timer = if branch == 2 {
                1
            } else if branch == 3 {
                -1
            } else {
                0
            };
        }
        if branch == 4 {
            world.vehicles.entries[0]
                .as_mut()
                .unwrap()
                .prefix
                .spin
                .speed = 1234;
        }
        let config = rcc_lifecycle_probe::ufo_config(&world);
        let ufo_class = config.0.unwrap();
        let ufo_resources = config.1.unwrap();
        let projectile_class = world.objects.components.projectiles.class.unwrap();
        let resources = world.objects.components.projectiles.resources.unwrap();
        let target = world.vehicles.entries[0].as_ref().unwrap().physics.entity;
        let preset = matches!(branch, 9 | 10 | 11 | 14 | 15 | 16 | 17 | 18 | 20 | 21);
        let ufo = if preset {
            Some(
                rcc_lifecycle_probe::spawn_ufo(&mut world, None, target, [0; 3], 100000, true)
                    .unwrap(),
            )
        } else {
            None
        };
        let owner = Owner {
            entity: target,
            turret: Pose {
                position,
                matrix: Matrix::IDENTITY,
            },
            gun: [position[0], position[1] + 50000, position[2]],
            velocity: [0; 3],
        };
        let entity =
            rcc_lifecycle_probe::spawn_projectile(&mut world, owner, projectile::Kind::Ufo, 100000)
                .unwrap();
        let id = ObstacleId(object(&world, entity).unwrap());
        let fallback = world.objects.world.colliders[169].entity;
        let state = rcc_lifecycle_probe::projectile_state(&mut world, id);
        state.owner = None;
        state.target = None;
        state.velocity = [0, 0, 384000];
        if preset {
            state.attached_ufo = ufo;
            state.attachment = [1234, 2048, -3456];
            state.flags |= 4;
        }
        if branch == 20 {
            state.flags &= !4;
        }
        if matches!(branch, 12 | 13 | 14 | 19 | 24 | 25) {
            state.flags |= 4;
            state.attached_ufo = Some(if branch == 13 {
                target
            } else if branch == 14 {
                fallback
            } else {
                EntityId(0xfedcba98)
            });
        }
        if branch == 19 {
            rcc_lifecycle_probe::projectile_state(&mut world, id).flags |= 1;
        }
        world.objects.world.colliders[id.0].pose = Pose {
            position: [position[0], position[1], position[2] - 24000],
            matrix: Matrix::IDENTITY,
        };
        if branch == 31 {
            world.objects.components.objects[id.0]
                .as_mut()
                .unwrap()
                .emitter = Some(EntityId(0x76543210));
        }
        if let Some(ufo) = ufo {
            let object = object(&world, ufo).unwrap();
            if branch == 11 {
                world.objects.components.ufos[object]
                    .as_mut()
                    .unwrap()
                    .flags |= 2;
            }
            if branch == 15 {
                world.objects.components.ufos[object]
                    .as_mut()
                    .unwrap()
                    .target = world.objects.world.colliders[169].entity;
            }
            if branch == 16 {
                world.objects.components.ufos[object]
                    .as_mut()
                    .unwrap()
                    .target = EntityId(0xfedcba98);
            }
            if branch == 21 {
                world.identity.entities.record_mut(ufo).unwrap().object = None;
            }
        }
        if branch == 7 {
            rcc_lifecycle_probe::clear_ufo_config(&mut world, true);
        }
        if branch == 8 {
            rcc_lifecycle_probe::clear_ufo_config(&mut world, false);
        }
        if branch == 18 {
            world.vehicles.entries[0] = None;
        }
        if branch == 24 {
            rcc_lifecycle_probe::clear_terrain(&mut world.objects.components);
        }
        if branch == 25 {
            world.objects.components.projectiles.resources = None;
        }
        if matches!(branch, 22 | 23 | 26 | 27 | 33) {
            let class = if matches!(branch, 26 | 27 | 33) {
                ufo_class
            } else {
                projectile_class
            };
            let mut table = (*world.registry.classes[class.0]).clone();
            match branch {
                22 => table.methods[12] = None,
                23 => table.maximum_slot = 11,
                26 => table.methods[1] = None,
                27 => table.methods[0] = None,
                _ => table.maximum_slot = 3,
            }
            let table = Arc::new(table);
            world.registry.classes[class.0] = table.clone();
            world.objects.world.classes[class.0] = table;
        }
        if branch == 28 {
            world.objects.world.colliders[id.0].pose.position =
                [position[0], position[1] - 8192, position[2]];
            rcc_lifecycle_probe::projectile_state(&mut world, id).velocity = [0; 3];
        }
        if branch == 29 {
            let state = rcc_lifecycle_probe::projectile_state(&mut world, id);
            state.flags |= 4;
            state.attached_ufo = None;
        }
        world.objects.components.camera_impulse = Some((17, -250));
        let initial = snapshot(&world);
        let mut cars = Vec::new();
        super::cars(&mut cars, &world);
        let has_vehicles = !matches!(branch, 9 | 17);
        cars[213] = u16::from(has_vehicles);
        let mut poses = Vec::new();
        let mut spins = Vec::new();
        let mut slots = Vec::new();
        for entry in &world.vehicles.entries {
            poses.push(u16::from(entry.is_some()));
            ints(
                &mut poses,
                [entry.as_ref().map_or(0, |c| c.physics.entity.0 as i32)],
            );
            ints(
                &mut poses,
                entry
                    .as_ref()
                    .map_or([0; 3], |c| c.physics.motion.poses.base_position),
            );
            ints(
                &mut poses,
                entry
                    .as_ref()
                    .map_or([0; 9], |c| c.physics.motion.poses.reference.0),
            );
            ints(
                &mut spins,
                [entry.as_ref().map_or(0, |c| c.prefix.spin.speed)],
            );
            slots.push(entry.as_ref().map_or(0, |c| c.slot as u16));
        }
        poses.resize(216, 0);
        let has_pool = branch != 6;
        let simulation = branch != 5;
        let fixed = world.objects.world.colliders.len();
        let mut frames = Vec::new();
        let mut controls = Vec::new();
        for step in 0..if matches!(branch, 0 | 10 | 11 | 19) {
            3
        } else {
            1
        } {
            if branch == 0 && step == 1 {
                let car = world.vehicles.entries[0].as_mut().unwrap();
                car.physics.motion.poses.base_position[0] += 5000;
                car.physics.motion.poses.base_position[2] -= 2000;
                car.physics.motion.poses.reference.rotate(Axis::Y, 4000);
                frames.push(json!({"entity":0,"status":0,"object":object(&world,EntityId(0)),"state":snapshot(&world)}));
                controls.push([15, 0, 5000, -2000, 100000, 4000]);
            }
            let number = if branch == 28 {
                entity.0
            } else {
                (entity.0 ^ 1).wrapping_add(step * 4)
            };
            let dt = if step == 0 { 32 } else { 97 };
            let limit = if branch == 32 { 0 } else { 100000 };
            world.objects.components.frame_group = number;
            world.objects.components.stamp_units = 100;
            let result = rcc_lifecycle_probe::tick_projectile(
                &mut world,
                entity,
                dt,
                limit,
                simulation,
                has_vehicles,
                has_pool,
                true,
            );
            frames.push(json!({"entity":entity.0,"status":result.map_or_else(|e|e,|()|0),"object":object(&world,entity),"state":snapshot(&world)}));
            controls.push([13, dt, number as i32, entity.0 as i32, limit as i32, 100]);
        }
        if matches!(branch, 0 | 1 | 3 | 4 | 5 | 6 | 26 | 30 | 31 | 33) {
            assert_eq!(frames[0]["status"], 0, "UFO hit branch {branch}");
            let weapons = frames[0]["state"]["weapons"].as_array().unwrap();
            assert_eq!(
                weapons.iter().filter(|w| w["kind"] == 4).count(),
                1,
                "UFO creation branch {branch}"
            );
        }
        if matches!(
            branch,
            7 | 8 | 9 | 12 | 13 | 14 | 15 | 16 | 17 | 18 | 21 | 24 | 25 | 27 | 29 | 32
        ) {
            assert!(
                frames[0]["status"].as_i64().unwrap() < 0,
                "UFO error branch {branch}"
            );
        }
        if branch == 0 {
            assert!(frames.iter().all(|f| f["status"] == 0));
            assert_ne!(
                frames[0]["state"]["colliders"][id.0],
                frames[2]["state"]["colliders"][id.0]
            );
            assert_ne!(
                frames[0]["state"]["ufo_car_basis"],
                frames[2]["state"]["ufo_car_basis"]
            );
        }
        if branch == 11 || branch == 19 {
            assert!(frames.iter().all(|f| f["status"] == 0));
        }
        if branch == 31 {
            let child = frames[0]["state"]["weapons"]
                .as_array()
                .unwrap()
                .iter()
                .find(|w| w["kind"] == 4)
                .unwrap();
            assert_eq!(child["meta"][7], 0);
        }
        let (scene, _) = scene_export::encode(&world.objects.world);
        let name = format!("scene-projectile-ufo-{branch}.bin");
        fs::write(
            out.join(&name),
            scene
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let r = world.objects.components.explosion_resources;
        let mut er = Vec::new();
        ints(
            &mut er,
            [
                r.main as i32,
                r.smoke as i32,
                i32::from(r.small),
                r.damping as i32,
                i32::from(r.map_fog),
            ],
        );
        let m = world.objects.components.blasts.materials.unwrap();
        let mut materials = Vec::new();
        ints(&mut materials, [1, m.band as i32]);
        ints(&mut materials, m.ring.map(|v| v as i32));
        let mut camera = Vec::new();
        ints(&mut camera, world.objects.components.camera_basis.0);
        let mut pos = Vec::new();
        ints(&mut pos, position);
        cases.push(json!({"tag":format!("projectile-ufo-{branch}"),"fixed_count":fixed,"generic":world.registry.generic.0,"obstacle":world.registry.obstacle.0,"world_class":world.registry.world.0,"projectile_class":projectile_class.0,"resources_present":world.objects.components.projectiles.resources.is_some(),"homing_mesh":resources.homing_mesh.index(),"launch":super::launch_input(resources,owner),"scene":name,"initial":initial,"controls":controls,"frames":frames,"terrain_present":branch!=24,"flight_cars":cars,"carrier_pose":vec![0u16;24],"spin_inputs":spins,"explosion_class":world.objects.components.explosion_class.map_or(-1,|v|v.0 as i32),"explosion_resources":er,"explosion_position":pos,"effect_pool_present":has_pool,"simulation":simulation,"vehicle_count":if has_vehicles {3}else{0},"vehicle_inputs":poses,"blast_materials":materials,"blast_camera":camera,"crater":resources.crater,"ufo_class":rcc_lifecycle_probe::ufo_config(&world).0.map_or(-1,|v|v.0 as i32),"ufo_resources_present":rcc_lifecycle_probe::ufo_config(&world).1.is_some(),"ufo_mesh":ufo_resources.mesh.index(),"ufo_material":ufo_resources.material,"ufo_car_slots":slots}));
    }
    fs::write(out.join("scene-projectile-ufo.json"),serde_json::to_vec(&json!({"cases":cases,"scope":"Complete original UFO projectile Tick: live carrier lookup before flight, shared UFO creation and local hit attachment, moving/rotating target follow and release, protection/already-attached explosion fallback, retirement and committed nested failures. UFO transfer actor Tick remains a separate integration stage."})).unwrap()).unwrap();
}
