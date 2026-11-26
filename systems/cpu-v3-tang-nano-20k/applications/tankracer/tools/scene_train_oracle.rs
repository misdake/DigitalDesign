//! Actual first-map Train class ticks, including endpoint and failed-pose prefixes.
use super::{ints, scene_export, scene_mines_oracle};
use serde_json::{Value, json};
use std::{fs, path::Path, sync::Arc};
use tankracer_game::{
    actor_startup::{
        Actor,
        train::{Motion, Train},
    },
    dispatch::MethodId,
    math::Matrix,
    object_query::{EntityId, ObstacleId},
    scene_objects::rcc_lifecycle_probe,
    vehicle_runtime::SinglePlayer,
};
fn state(train: &Train) -> Vec<u16> {
    let t = &train.travel;
    let mut out = Vec::new();
    ints(
        &mut out,
        [
            i32::from(t.active),
            i32::from(t.running),
            i32::from(t.waiting),
            t.wait,
            t.angle,
        ],
    );
    ints(&mut out, t.tilt);
    ints(&mut out, t.angular_velocity);
    ints(
        &mut out,
        [i32::from(t.sound_active), i32::from(t.lander_flying)],
    );
    ints(&mut out, t.lander_direction);
    ints(&mut out, t.lander_velocity);
    ints(&mut out, [train.speed, train.range]);
    let Motion::Path {
        start,
        end,
        velocity,
        ..
    } = train.motion
    else {
        panic!("first-map Train path")
    };
    ints(&mut out, start);
    ints(&mut out, end);
    ints(&mut out, velocity);
    assert_eq!(out.len(), 60);
    out
}
fn snapshot(world: &SinglePlayer) -> Value {
    let mut out = scene_mines_oracle::snapshot(world);
    let mut trains = Vec::new();
    for id in [125, 239] {
        let Some(Actor::Train(train)) = world.objects.components.actors.get(ObstacleId(id)) else {
            panic!("Train owner")
        };
        trains.extend(state(train));
    }
    out["train_state"] = json!(trains);
    out
}
pub fn export(root: &Path, out: &Path) {
    let mut cases = Vec::new();
    for branch in 0..47 {
        let mut world = super::reference_world::full_race(root);
        let fixed = world.objects.world.colliders.len();
        let id = if branch < 8 || branch >= 16 { 125 } else { 239 };
        let mut entity = world.objects.world.colliders[id].entity;
        let position = world.objects.world.colliders[id].pose.position;
        let mut player = position;
        let mut dt = 40i32;
        let mut slot = 12;
        let mut limit = 100000;
        let mut simulation = true;
        let mut vehicles = true;
        let mut number = entity.0;
        let mut calls = 3;
        {
            let Some(Actor::Train(t)) = world.objects.components.actors.get_mut(ObstacleId(id))
            else {
                panic!("Train")
            };
            t.travel.sound_active = true;
            if branch < 16 {
                match branch % 8 {
                    0 => {}
                    1 => player[0] = player[0].wrapping_add(t.range),
                    2 => number ^= 1,
                    3 => dt = 16,
                    4 => t.travel.running = true,
                    5 => {
                        t.travel.active = true;
                        t.travel.running = true;
                        player[2] = player[2].wrapping_add(t.range);
                    }
                    6 => {
                        t.travel.waiting = true;
                        t.travel.wait = dt;
                    }
                    _ => {
                        t.travel.waiting = true;
                        t.travel.wait = dt + 1;
                    }
                }
            } else {
                t.travel.active = true;
                t.travel.running = true;
                let Motion::Path { end, .. } = t.motion else {
                    unreachable!()
                };
                if (16..30).contains(&branch) {
                    world.objects.world.colliders[id].pose.position = end;
                    player = end;
                    dt = 0;
                    world.shared.random = if branch % 2 == 0 { 0 } else { 1 };
                }
                if branch == 18 || branch == 19 {
                    dt = 1;
                }
                if branch == 20 || branch == 21 {
                    dt = -40;
                }
                if branch == 22 || branch == 23 {
                    t.speed = -17;
                }
                if branch == 24 || branch == 25 {
                    let Motion::Path { start, end, .. } = &mut t.motion else {
                        unreachable!()
                    };
                    *start = [0; 3];
                    *end = [0; 3];
                    world.objects.world.colliders[id].pose.position = [0; 3];
                    player = [0; 3];
                }
                if branch == 26 || branch == 27 {
                    let Motion::Path { start, .. } = &mut t.motion else {
                        unreachable!()
                    };
                    *start = [i32::MIN, i32::MAX, -123456789];
                }
                if branch == 28 || branch == 29 {
                    dt = 0;
                    world.shared.random = 1;
                    let bad = Matrix([0, 0, 0, 16753, 63226, 0, 0, 0, 0]);
                    world.objects.world.colliders[id].pose.matrix = bad;
                    world.objects.world.colliders[id].external_basis =
                        if branch == 29 { Some(bad) } else { None };
                    calls = 2;
                }
                if branch == 30 {
                    simulation = false;
                }
                if branch == 31 {
                    vehicles = false;
                }
                if branch == 32 {
                    world.vehicles.entries[0] = None;
                }
                if branch == 33 {
                    world.objects.world.colliders[id].external_basis =
                        Some(Matrix::from_angles([2000, 3000, 4000]));
                }
                if branch == 34 {
                    world.objects.world.colliders[id].external_basis = None;
                }
                if branch == 35 {
                    t.range = 0;
                }
                if branch == 36 {
                    t.travel.running = false;
                    player[0] = player[0].wrapping_sub(i32::MIN);
                }
                if branch == 37 {
                    dt = i32::MAX;
                }
                if branch == 38 {
                    limit = 0;
                }
                if (39..44).contains(&branch) {
                    let class = world
                        .identity
                        .entities
                        .record(entity)
                        .unwrap()
                        .class
                        .unwrap();
                    let mut table = (*world.registry.classes[class.0]).clone();
                    if branch == 39 {
                        table.methods[12] = None;
                    } else if branch == 40 {
                        table.maximum_slot = 11;
                    } else if branch == 41 {
                        table.methods[12] = Some(MethodId(74));
                    } else if branch == 42 {
                        slot = 4;
                    } else {
                        slot = 8;
                    }
                    let table = Arc::new(table);
                    world.registry.classes[class.0] = table.clone();
                    world.objects.world.classes[class.0] = table;
                }
            }
        }
        if branch >= 44 {
            dt = 0;
            calls = 6;
            player = [0; 3];
            world.shared.random = if branch == 44 { 0 } else { 1 };
            for id in [125, 239] {
                let Some(Actor::Train(t)) = world.objects.components.actors.get_mut(ObstacleId(id))
                else {
                    unreachable!()
                };
                t.travel.active = true;
                t.travel.running = true;
                t.range = i32::MAX;
                let Motion::Path { end, .. } = t.motion else {
                    unreachable!()
                };
                world.objects.world.colliders[id].pose.position = end;
            }
            if branch == 46 {
                let c = &mut world.objects.world.colliders[125];
                c.pose.matrix = Matrix([0, 0, 0, 16753, 63226, 0, 0, 0, 0]);
                c.external_basis = None;
            }
        }
        if let Some(car) = world.vehicles.entries[0].as_mut() {
            car.physics.motion.poses.body.position = player;
        }
        world.identity.current_slot = 17;
        let initial = snapshot(&world);
        let mut frames = Vec::new();
        let mut controls = Vec::new();
        for step in 0..calls {
            let called_id = if branch >= 44 && step % 2 != 0 {
                239
            } else {
                id
            };
            entity = world.objects.world.colliders[called_id].entity;
            world.objects.components.frame_group = number.wrapping_add(step as u32);
            let status = rcc_lifecycle_probe::tick_scenery(
                &mut world, entity, dt, slot, limit, simulation, vehicles,
            )
            .err()
            .unwrap_or(0);
            frames.push(
                json!({"entity":entity.0,"object":called_id,"status":status,"state":snapshot(&world)}),
            );
            controls.push([
                21,
                dt,
                slot as i32,
                entity.0 as i32,
                limit as i32,
                number.wrapping_add(step as u32) as i32,
            ]);
        }
        let expected = if branch == 28 || branch == 29 || branch == 46 {
            -2
        } else if branch == 30 || branch == 31 || branch == 41 {
            -5
        } else if branch == 32 {
            -4
        } else {
            0
        };
        assert!(
            frames.iter().enumerate().all(|(step, f)| f["status"]
                == if (branch == 28 || branch == 29 || branch == 46) && step > 0 {
                    0
                } else {
                    expected
                }),
            "Train branch {branch}: {:?}",
            frames.iter().map(|f| &f["status"]).collect::<Vec<_>>()
        );
        let sounds: Vec<_> = [125, 239]
            .into_iter()
            .map(|id| {
                let Some(Actor::Train(t)) = world.objects.components.actors.get(ObstacleId(id))
                else {
                    unreachable!()
                };
                t.sound.is_some()
            })
            .collect();
        let mut player_words = Vec::new();
        ints(&mut player_words, player);
        let (scene, _) = scene_export::encode(&world.objects.world);
        let name = format!("scene-train-{branch}.bin");
        fs::write(
            out.join(&name),
            scene
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        cases.push(json!({"tag":format!("train-{branch}"),"scene":name,"fixed_count":fixed,"generic":world.registry.generic.0,"obstacle":world.registry.obstacle.0,"world_class":world.registry.world.0,"projectile_class":-1,"resources_present":false,"homing_mesh":0,"launch":vec![0u16;93],"initial":initial,"frames":frames,"controls":controls,"train_objects":[125,239],"train_sounds":sounds,"train_simulation":simulation,"train_vehicles":vehicles,"train_player_present":world.vehicles.entries[0].is_some(),"train_player":player_words}));
    }
    fs::write(out.join("scene-train.json"),serde_json::to_vec(&json!({"cases":cases,"scope":"Actual first-map Train class ticks on both authored paths and alternating shared-RNG calls: staggered activation, range, waiting, exact movement and endpoint RNG, reversal and basis normalization, live collider publication, shared lifecycle and committed failure prefixes. Audio is external; extra-map motions have separate implementations."})).unwrap()).unwrap();
}
