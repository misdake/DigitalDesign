//! Original banked flight phase and ordered retirement before impact callbacks.
use super::{ints, scene_creation_oracle, scene_export};
use serde_json::{Value, json};
use std::{fs, path::Path, sync::Arc};
use tankracer_game::{
    dispatch::MethodId,
    math::Matrix,
    pose::Pose,
    projectile::{
        flight::{Carrier, Frame},
        launch::Owner,
    },
    scene_entity::Handle,
    scene_objects::rcc_lifecycle_probe,
    vehicle_runtime::SinglePlayer,
};
fn snapshot(world: &SinglePlayer) -> Value {
    let mut value = scene_creation_oracle::snapshot(world);
    let mut overlay = vec![u16::from(world.objects.components.overlay.is_some())];
    overlay.extend(
        world
            .objects
            .components
            .overlay
            .unwrap_or([0; 4])
            .map(u16::from),
    );
    value["overlay"] = json!(overlay);
    value
}
pub fn export(root: &Path, out: &Path) {
    let mut cases = Vec::new();
    for branch in 0..33 {
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
        let class = world.objects.components.projectiles.class.unwrap();
        let resources = world.objects.components.projectiles.resources.unwrap();
        let car = world.vehicles.entries[0].as_ref().unwrap();
        let position = car.physics.motion.poses.base_position;
        let owner = Owner {
            entity: car.physics.entity,
            turret: Pose {
                position,
                matrix: Matrix::from_angles([65000, 13000, 1200]),
            },
            gun: [position[0], position[1] + 50000, position[2]],
            velocity: [100001, -12345, -567890],
        };
        let kind = super::kind_of(if branch >= 23 {
            (branch - 23) % 6
        } else if branch < 6 {
            branch
        } else if branch == 8 {
            1
        } else if matches!(branch, 9 | 15 | 16 | 17 | 18) {
            3
        } else if branch == 14 {
            2
        } else {
            0
        });
        if branch == 21 {
            for _ in 0..25 {
                rcc_lifecycle_probe::create_weapon(&mut world, class, 100000).unwrap();
            }
        }
        let mut frames = Vec::new();
        let mut controls = Vec::new();
        let mut fixed = 0;
        let mut initial = Value::Null;
        if branch == 21 {
            fixed = world.objects.world.colliders.len();
            initial = snapshot(&world);
        }
        let entity =
            rcc_lifecycle_probe::spawn_projectile(&mut world, owner, kind, 100000).unwrap();
        let id = world
            .identity
            .entities
            .record(entity)
            .unwrap()
            .object
            .and_then(Handle::obstacle)
            .unwrap();
        if branch == 21 {
            frames.push(json!({"entity":entity.0,"status":0,"object":id.0,"state":snapshot(&world),"action":vec![0u16;41]}));
            controls.push([3, kind as i32, 0, 0, 100000, 0]);
        } else {
            let state = rcc_lifecycle_probe::projectile_state(&mut world, id);
            state.velocity = [12000, -800, 9000];
            if matches!(branch, 6 | 7 | 8 | 9 | 11 | 15 | 22) {
                state.flags = if branch == 8 { 3 } else { 1 };
            }
            if matches!(branch, 6 | 9 | 15) {
                state.trail_count = 1;
                state.trails[0].active = true;
                state.trails[0].time = 29;
                state.trails[0].sprite.color = [200, 180, 160];
                state.trails[0].angular = -1234;
                state.trails[0].velocity = [12345, -23456, 34567];
                if branch == 9 || branch == 15 {
                    state.trails[0].channel = 3;
                }
            }
            if branch == 8 {
                state.flash_views = [1, 1];
                state.flash_alpha = 17;
            }
            if branch == 10 {
                state.time = 6000;
            }
            if branch == 14 {
                state.reference = None;
            }
            if matches!(branch, 16 | 17 | 18) {
                state.flags = 4;
                state.attachment = [1111, -2222, 3333];
            }
            if branch == 19 {
                state.velocity = [0, 0, 0];
            }
            if branch == 20 {
                state.owner = None;
                state.velocity = [0, 0, 384000];
            }
            if branch == 19 {
                let mut height = 0;
                assert!(
                    tankracer_game::terrain::height(
                        world.terrain(),
                        position[0],
                        position[2],
                        &mut height
                    )
                    .unwrap()
                );
                world.objects.world.colliders[id.0].pose.position =
                    [position[0], height - 8192, position[2]];
                for car in world.vehicles.entries.iter_mut().flatten() {
                    car.physics.collision.disabled = true;
                }
            }
            if branch == 20 {
                world.objects.world.colliders[id.0].pose.position =
                    [position[0], position[1], position[2] - 24000];
            }
            if branch == 12 {
                world.objects.components.projectiles.resources = None;
            }
            if branch == 13 {
                rcc_lifecycle_probe::clear_terrain(&mut world.objects.components);
            }
            if branch == 22 {
                let mut table = (*world.registry.classes[class.0]).clone();
                table.methods[8] = Some(MethodId(38));
                let table = Arc::new(table);
                world.registry.classes[class.0] = table.clone();
                world.objects.world.classes[class.0] = table;
            }
            // An advance error must preserve this independently published matrix.
            world.objects.world.colliders[id.0].external_basis =
                Some(Matrix::from_angles([1234, 23456, 3456]));
            fixed = world.objects.world.colliders.len();
            initial = snapshot(&world);
        }
        if branch >= 23 {
            let state = rcc_lifecycle_probe::projectile_state(&mut world, id);
            if branch == 28 {
                state.owner = None;
            }
            world.vehicles.entries[1]
                .as_mut()
                .unwrap()
                .prefix
                .spin
                .speed = if branch == 30 { 0 } else { 1234 };
            if branch == 31 {
                world.vehicles.entries[1] = None;
            }
            initial = snapshot(&world);
        }
        let mut spins = Vec::new();
        for car in &world.vehicles.entries {
            ints(
                &mut spins,
                [car.as_ref().map_or(0, |v| v.prefix.spin.speed)],
            );
        }
        let mut cars = Vec::new();
        super::cars(&mut cars, &world);
        assert_eq!(cars.len(), 214);
        if branch == 32 {
            cars[213] = 0;
        }
        let carrier = Pose {
            position: [position[0] + 5000, position[1] + 7000, position[2] - 9000],
            matrix: Matrix::from_angles([1700, 21000, 3100]),
        };
        let mut carrier_pose = Vec::new();
        ints(&mut carrier_pose, carrier.matrix.0);
        ints(&mut carrier_pose, carrier.position);
        for step in 0..if matches!(branch, 0 | 1 | 2 | 3 | 4 | 5 | 6 | 8 | 10 | 17 | 18 | 21) {
            5
        } else {
            1
        } {
            if branch >= 23 {
                let simulation = branch != 29 && branch != 30 && branch != 25 && branch != 26;
                let result = rcc_lifecycle_probe::reflect_projectile(
                    &mut world,
                    id,
                    tankracer_game::object_query::TankId(1),
                    simulation,
                    branch != 32,
                );
                let (status, reflected) = result.map_or_else(|e| (e, false), |v| (0, v));
                frames.push(json!({"entity":entity.0,"status":status,"object":id.0,"state":snapshot(&world),"action":vec![0u16;41],"reflection":reflected}));
                controls.push([12, 1, 0, entity.0 as i32, 100000, i32::from(simulation)]);
                continue;
            }
            let number = if branch == 19 {
                entity.0
            } else if branch == 18 {
                u32::MAX.wrapping_sub(2).wrapping_add(step)
            } else {
                step
            };
            let dt = if branch == 20 { 32 } else { 33 };
            let flags = if branch == 17 {
                1
            } else if branch == 18 {
                3
            } else {
                0
            };
            let frame = Frame {
                dt,
                number,
                entity,
                carrier: (flags & 1 != 0).then_some(Carrier {
                    pose: carrier,
                    released: flags & 2 != 0,
                }),
            };
            let limit = if branch == 11 { 1 } else { 100000 };
            let mut action = None;
            let status =
                rcc_lifecycle_probe::projectile_flight(&mut world, id, frame, limit, &mut action)
                    .map_or_else(|e| e, |()| 0);
            let mut packed = Vec::new();
            super::action(&mut packed, action);
            let bound = world
                .identity
                .entities
                .record(entity)
                .and_then(|r| r.object)
                .and_then(Handle::obstacle)
                .map(|v| v.0);
            frames.push(json!({"entity":entity.0,"status":status,"object":bound,"state":snapshot(&world),"action":packed}));
            controls.push([11, dt, number as i32, entity.0 as i32, limit as i32, flags]);
        }
        if branch == 10 {
            assert_eq!(frames[0]["action"][0], 3);
            assert_eq!(frames[1]["action"][0], 2);
        }
        if branch == 14 {
            assert_eq!(frames[0]["status"], -4);
            assert_eq!(
                frames[0]["state"]["colliders"][id.0][26],
                initial["colliders"][id.0][26]
            );
        }
        if branch == 19 {
            assert_eq!(frames[0]["action"][0], 4);
            assert_eq!(frames[0]["action"][1], 1);
        }
        if branch == 20 {
            assert_eq!(frames[0]["action"][14], 1);
        }
        if branch == 22 {
            assert_eq!(frames[0]["status"], -4);
            assert!(frames[0]["state"]["weapons"].as_array().unwrap().is_empty());
        }
        if matches!(branch, 23 | 24 | 27 | 28) {
            assert_eq!(frames[0]["reflection"], true);
        }
        if matches!(branch, 25 | 26 | 30) {
            assert_eq!(frames[0]["status"], 0);
            assert_eq!(frames[0]["reflection"], false);
        }
        if branch == 29 {
            assert_eq!(frames[0]["status"], -5);
        }
        if branch == 31 || branch == 32 {
            assert_eq!(frames[0]["status"], -4);
        }
        let (scene, _) = scene_export::encode(&world.objects.world);
        let name = format!("scene-projectile-flight-{branch}.bin");
        fs::write(
            out.join(&name),
            scene
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        cases.push(json!({"tag":format!("projectile-flight-{branch}"),"fixed_count":fixed,"generic":world.registry.generic.0,"obstacle":world.registry.obstacle.0,"world_class":world.registry.world.0,"projectile_class":class.0,"resources_present":world.objects.components.projectiles.resources.is_some(),"homing_mesh":resources.homing_mesh.index(),"launch":super::launch_input(resources,owner),"scene":name,"initial":initial,"controls":controls,"frames":frames,"terrain_present":branch!=13,"flight_cars":cars,"carrier_pose":carrier_pose,"spin_inputs":spins}));
    }
    fs::write(out.join("scene-projectile-flight.json"),serde_json::to_vec(&json!({"cases":cases,"scope":"Original selected projectile flight phase with shared component banks, real terrain and live car sweeps, pose/error publication, overlay, trails, shield reflection and ordered Deactivate/special-list retirement. Returns ground/car impact packets for the synchronous callback phase; attached-UFO lookup and impact callbacks remain separate integration work."})).unwrap()).unwrap();
}
