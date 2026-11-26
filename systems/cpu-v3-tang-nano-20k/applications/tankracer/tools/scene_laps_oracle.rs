//! Original lap actions, delayed queues and restoration on the first map.
use super::{ints, scene_export, scene_mines_oracle};
use serde_json::{Value, json};
use std::{fs, path::Path, sync::Arc};
use tankracer_game::{
    dispatch::MethodId,
    math::Matrix,
    object_query::{EntityId, ObstacleId},
    scene_objects::rcc_lifecycle_probe,
    vehicle_runtime::SinglePlayer,
};
fn snapshot(world: &SinglePlayer) -> Value {
    let mut out = scene_mines_oracle::snapshot(world);
    out["lap_bindings"] = json!(rcc_lifecycle_probe::lap_bindings(world));
    out["lap_backups"] = json!(rcc_lifecycle_probe::lap_backups(world));
    out["lap_attachments"] = json!(rcc_lifecycle_probe::lap_attachments(world));
    let mut deformables = Vec::new();
    ints(
        &mut deformables,
        world.objects.world.deformables.iter().map(|v| v.0 as i32),
    );
    out["lap_deformables"] = json!(deformables);
    out
}
pub fn export(root: &Path, out: &Path) {
    let mut cases = Vec::new();
    for branch in 0..74 {
        let mut world = super::reference_world::full_race(root);
        let fixed = world.objects.world.colliders.len();
        let lap_objects = world.objects.components.lap_objects.clone();
        let far = [1000000, 0, 1000000];
        let id = if branch < 8 {
            75
        } else {
            [75, 217, 186, 185][(branch - 8) % 4]
        };
        let position = world.objects.world.colliders[id].pose.position;
        let near = position.map(|v| v >> 3);
        let mut limit = 100000;
        let mut lap = branch as i16 % 4;
        let mut camera = far;
        let mut flush = false;
        let mut calls = 1;
        if (4..8).contains(&branch) {
            camera = near;
            calls = 3;
        }
        if branch >= 8 {
            world.objects.components.lap_objects = vec![ObstacleId(id)];
            let v = &mut world.objects.components.objects[id]
                .as_mut()
                .unwrap()
                .data
                .visibility;
            v.add_lap = 5;
            v.remove_lap = 6;
            lap = if (branch - 8) / 4 % 2 == 0 { 5 } else { 6 };
        }
        if (16..24).contains(&branch) {
            camera = near;
            calls = 3;
        }
        if (24..32).contains(&branch) {
            limit = if branch < 28 { 0 } else { 1 };
        }
        if branch >= 32 {
            world.objects.components.lap_objects.clear();
            world.objects.components.lap_events.enter.clear();
            world.objects.components.lap_events.leave.clear();
        }
        if (32..40).contains(&branch) {
            flush = true;
            let e = |id: usize| world.objects.world.colliders[id].entity;
            world.objects.components.lap_events.enter = vec![e(75), e(76), e(77)];
            world.objects.components.lap_events.leave = vec![e(77), e(76)];
            if branch == 33 {
                limit = 1;
            }
            if branch == 34 {
                limit = 4;
            }
            if branch == 35 {
                limit = 0;
            }
            if branch == 36 {
                camera = world.objects.world.colliders[75]
                    .pose
                    .position
                    .map(|v| v >> 3);
            }
            if branch == 37 {
                world
                    .objects
                    .components
                    .lap_events
                    .enter
                    .insert(0, EntityId(u32::MAX));
            }
            if branch == 38 {
                world.objects.components.objects[75] = None;
            }
            if branch == 39 {
                let class = world
                    .identity
                    .entities
                    .record(e(75))
                    .unwrap()
                    .class
                    .unwrap();
                let mut table = (*world.registry.classes[class.0]).clone();
                table.methods[6] = Some(MethodId(74));
                let table = Arc::new(table);
                world.registry.classes[class.0] = table.clone();
                world.objects.world.classes[class.0] = table;
            }
        }
        if (40..48).contains(&branch) {
            let backups = rcc_lifecycle_probe::lap_backups(&world);
            let fid = usize::from(backups[0]);
            let c = &mut world.objects.world.colliders[fid];
            c.pose.matrix = Matrix([4096, 1, 0, 0, 4096, 2, 3, 0, 4096]);
            c.external_basis = if branch == 41 {
                Some(Matrix::IDENTITY)
            } else if branch == 42 {
                None
            } else {
                Some(c.pose.matrix)
            };
            world.objects.world.objects[fid].flags = if branch == 44 { 0 } else { 1 };
            world.objects.world.objects[fid].activity = if branch == 45 { 1 } else { 0 };
            if branch == 43 {
                camera = c.pose.position.map(|v| v >> 3);
            }
            if branch == 46 {
                world.objects.components.objects[fid] = None;
            }
            if branch == 47 {
                limit = 0;
            }
        }
        if branch >= 48 {
            let id = if (60..66).contains(&branch) { 217 } else { 75 };
            let entity = world.objects.world.colliders[id].entity;
            world.objects.components.lap_objects = vec![ObstacleId(id)];
            let v = &mut world.objects.components.objects[id]
                .as_mut()
                .unwrap()
                .data
                .visibility;
            v.add_lap = 5;
            v.remove_lap = 6;
            lap = 5;
            match branch {
                48 => v.remove_lap = 5,
                49 => lap = 7,
                50 => {
                    world.objects.components.objects[id]
                        .as_mut()
                        .unwrap()
                        .data
                        .mesh = None
                }
                51 => world.objects.components.objects[id] = None,
                52 => world.identity.entities.release_id(entity).unwrap(),
                53 => world.identity.entities.record_mut(entity).unwrap().object = None,
                54 => {
                    world.identity.entities.record_mut(entity).unwrap().object =
                        Some(tankracer_game::scene_entity::Handle::Vehicle(
                            tankracer_game::object_query::TankId(0),
                        ))
                }
                55 | 69 => {
                    world.identity.entities.record_mut(entity).unwrap().object = Some(
                        tankracer_game::scene_entity::Handle::Obstacle(ObstacleId(76)),
                    );
                    world.objects.components.objects[76]
                        .as_mut()
                        .unwrap()
                        .data
                        .visibility
                        .add_lap = 5;
                }
                56 => world.identity.entities.record_mut(entity).unwrap().class = None,
                57..=59 | 64 => {
                    let class = world
                        .identity
                        .entities
                        .record(entity)
                        .unwrap()
                        .class
                        .unwrap();
                    let mut table = (*world.registry.classes[class.0]).clone();
                    if branch == 57 {
                        table.methods[6] = None;
                    } else if branch == 58 {
                        table.maximum_slot = 5;
                    } else if branch == 59 {
                        table.methods[6] = Some(MethodId(74));
                    } else {
                        table.methods[4] = Some(MethodId(74));
                    }
                    let table = Arc::new(table);
                    world.registry.classes[class.0] = table.clone();
                    world.objects.world.classes[class.0] = table;
                }
                _ => {}
            }
            if (60..66).contains(&branch) {
                rcc_lifecycle_probe::lap_surface_mode(
                    &mut world,
                    id,
                    if branch == 62 {
                        2
                    } else if branch == 63 {
                        3
                    } else {
                        1
                    },
                );
                if branch == 61 || branch == 65 {
                    lap = 6;
                }
                if branch == 65 {
                    limit = 1;
                }
            }
            if (66..69).contains(&branch) {
                camera = world.objects.world.colliders[id]
                    .pose
                    .position
                    .map(|v| v >> 3);
                camera[0] = camera[0].wrapping_sub(if branch == 66 {
                    19199
                } else if branch == 67 {
                    19200
                } else {
                    i32::MIN
                });
            }
            if (69..72).contains(&branch) {
                flush = true;
                world.objects.components.lap_objects.clear();
                world.objects.components.lap_events.enter = vec![entity];
                if branch == 70 {
                    world.identity.entities.record_mut(entity).unwrap().object =
                        Some(tankracer_game::scene_entity::Handle::Vehicle(
                            tankracer_game::object_query::TankId(0),
                        ));
                }
                if branch == 71 {
                    world.identity.entities.record_mut(entity).unwrap().object = None;
                }
            }
            if branch == 72 {
                world.objects.components.lap_objects.clear();
                limit = 0;
            }
            if branch == 73 {
                world.objects.components.lap_objects = lap_objects;
                calls = 4;
            }
        }
        world.identity.current_slot = 17;
        let initial = snapshot(&world);
        let mut frames = Vec::new();
        let mut controls = Vec::new();
        let mut cameras = Vec::new();
        for step in 0..calls {
            if branch == 73 {
                lap = step as i16;
            } else if step > 0 {
                camera = far;
                flush = true;
            }
            let status = rcc_lifecycle_probe::lap_step(&mut world, lap, flush, camera, limit)
                .err()
                .unwrap_or(0);
            frames.push(json!({"entity":0,"object":null,"status":status,"state":snapshot(&world)}));
            controls.push([20, lap as i32, i32::from(flush), step, limit as i32, 0]);
            ints(&mut cameras, camera);
        }
        let statuses: Vec<_> = frames
            .iter()
            .map(|f| f["status"].as_i64().unwrap())
            .collect();
        let expected = match branch {
            24..=31 | 33..=35 | 65 => -3,
            38 | 46 | 56 | 63 => -1,
            39 | 53 | 54 | 59 | 64 | 70 => -5,
            _ => 0,
        };
        assert!(
            statuses.iter().all(|&v| v == expected),
            "lap branch {branch}: {statuses:?}"
        );
        let (scene, _) = scene_export::encode(&world.objects.world);
        let name = format!("scene-laps-{branch}.bin");
        fs::write(
            out.join(&name),
            scene
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        cases.push(json!({"tag":format!("laps-{branch}"),"fixed_count":fixed,"scene":name,
            "generic":world.registry.generic.0,"obstacle":world.registry.obstacle.0,"world_class":world.registry.world.0,
            "initial":initial,"controls":controls,"frames":frames,"lap_cameras":cameras,
            "projectile_class":-1,"resources_present":false,"homing_mesh":0,"launch":vec![0u16;93]}));
    }
    fs::write(out.join("scene-laps.json"),serde_json::to_vec(&json!({"cases":cases,"scope":"Original first-map lap enter/leave actions, live camera-delayed queues, ordered shared work limit and off-camera Fall basis restoration, with actual class dispatch, grid, memberships, state and partial failures."})).unwrap()).unwrap();
}
