//! Original first-map emitters with real ownerless projectile creation.
use super::{ints, scene_export, scene_mines_oracle};
use serde_json::{Value, json};
use std::{fs, path::Path, sync::Arc};
use tankracer_game::{
    actor_startup::{Actor, emitter::Emitter},
    dispatch::MethodId,
    math::Matrix,
    object_query::ObstacleId,
    scene_objects::rcc_lifecycle_probe,
    vehicle_runtime::SinglePlayer,
};
const IDS: [usize; 5] = [81, 112, 113, 114, 115];
fn state(e: &Emitter) -> Vec<u16> {
    let mut out = Vec::new();
    super::projectile_snapshot::emitter(&mut out, e);
    assert_eq!(out.len(), 50);
    out
}
fn snapshot(world: &SinglePlayer) -> Value {
    let mut out = scene_mines_oracle::snapshot(world);
    let mut states = Vec::new();
    for id in IDS {
        let Some(Actor::Emitter(e)) = world.objects.components.actors.get(ObstacleId(id)) else {
            panic!("Emitter owner")
        };
        states.extend(state(e));
    }
    out["emitter_state"] = json!(states);
    out
}
pub fn export(root: &Path, out: &Path) {
    let mut cases = Vec::new();
    for branch in 0..50 {
        let mut world = super::reference_world::full_race(root);
        let fixed = world.objects.world.colliders.len();
        let id = IDS[branch % 5];
        let original_entity = world.objects.world.colliders[id].entity;
        let pr = world.objects.components.projectiles.resources.unwrap();
        let pc = world.objects.components.projectiles.class.unwrap();
        let position = world.objects.world.colliders[id].pose.position;
        let mut camera = position.map(|v| v >> 3);
        let mut player = position;
        let mut dt = 40i32;
        let mut limit = 100000;
        let mut simulation = true;
        let mut vehicles = true;
        let mut calls = 3;
        {
            let Some(Actor::Emitter(e)) = world.objects.components.actors.get_mut(ObstacleId(id))
            else {
                unreachable!()
            };
            if e.trigger == 1 {
                player[0] = e.bounds[0].wrapping_add(e.bounds[2].wrapping_sub(e.bounds[0]) >> 1);
                player[2] = e.bounds[1].wrapping_add(e.bounds[3].wrapping_sub(e.bounds[1]) >> 1);
            }
            if !(5..10).contains(&branch) {
                e.timer = -1;
            }
            if branch == 15 {
                e.gun = true;
                e.pitch = 3000;
                world.objects.world.colliders[id].external_basis =
                    Some(Matrix::from_angles([1024, 12345, 456]));
            }
            if branch == 16 {
                e.gun = false;
                e.pitch = i32::MAX;
            }
            if branch == 17 {
                e.period = -2048;
            }
            if branch == 18 {
                dt = -40;
            }
            if branch == 19 {
                e.timer = dt;
            }
            if branch == 20 {
                e.timer = dt - 1;
            }
            if branch == 21 || branch == 22 || branch == 45 {
                e.trigger = 0;
                camera[0] = camera[0].wrapping_sub(if branch == 21 {
                    19200
                } else if branch == 22 {
                    19199
                } else {
                    i32::MIN
                });
            }
            if branch == 23 || branch == 24 || branch == 34 {
                e.trigger = 0;
            }
            if branch == 25 {
                simulation = false;
            }
            if branch == 26 {
                world.objects.components.projectiles.resources = None;
            }
            if branch == 27 {
                world.objects.components.projectiles.class = None;
            }
            if branch == 28 {
                limit = 0;
            }
            if branch == 29 {
                limit = 1;
            }
            if branch == 32 {
                e.period = -2048;
                dt = 0;
                calls = 35;
            }
            if branch == 35 {
                e.trigger = 1;
                e.timer = 31415;
                world.vehicles.entries[0] = None;
            }
            if branch == 36 {
                e.trigger = 2;
            }
            if (37..40).contains(&branch) {
                e.alpha = [255, 128, 0][branch - 37];
            }
            if branch == 40 {
                e.mesh = None;
            }
            if branch == 41 {
                e.material = None;
            }
            if branch == 42 {
                e.material = Some(0x87654321);
            }
            if branch == 43 {
                e.glow = [i32::MIN, i32::MAX, 12345];
                e.offset = [i32::MIN, i32::MAX, -50000];
            }
            if branch == 44 {
                e.timer = i32::MIN;
                e.period = i32::MAX;
                dt = -30;
            }
            if branch == 46 {
                e.trigger = 1;
                player[0] = e.bounds[2];
            }
        }
        if branch == 23 || branch == 34 {
            world.vehicles.entries[0] = None;
        }
        if branch == 24 {
            vehicles = false;
        }
        if branch == 30 || branch == 31 {
            let mut table = (*world.registry.classes[pc.0]).clone();
            if branch == 30 {
                table.methods[4] = Some(MethodId(74));
            } else {
                table.methods[0] = None;
            }
            let table = Arc::new(table);
            world.registry.classes[pc.0] = table.clone();
            world.objects.world.classes[pc.0] = table;
        }
        if branch >= 47 {
            let class = world
                .identity
                .entities
                .record(original_entity)
                .unwrap()
                .class
                .unwrap();
            let mut table = (*world.registry.classes[class.0]).clone();
            if branch == 47 {
                table.methods[12] = None;
            } else if branch == 48 {
                table.maximum_slot = 11;
            } else {
                table.methods[12] = Some(MethodId(74));
            }
            let table = Arc::new(table);
            world.registry.classes[class.0] = table.clone();
            world.objects.world.classes[class.0] = table;
        }
        if branch == 33 {
            calls = 10;
            for id in IDS {
                let Some(Actor::Emitter(e)) =
                    world.objects.components.actors.get_mut(ObstacleId(id))
                else {
                    unreachable!()
                };
                e.timer = -1;
                e.trigger = 0;
                world.objects.world.colliders[id].pose.position = position;
            }
        }
        if let Some(car) = world.vehicles.entries[0].as_mut() {
            car.physics.motion.poses.base_position = player;
        }
        world.identity.current_slot = 17;
        let initial = snapshot(&world);
        let mut controls = Vec::new();
        let mut frames = Vec::new();
        for step in 0..calls {
            let called_id = if branch == 33 {
                IDS[step as usize % 5]
            } else {
                id
            };
            let entity = world.objects.world.colliders[called_id].entity;
            world.objects.components.camera_position = camera;
            let status = rcc_lifecycle_probe::tick_scenery(
                &mut world, entity, dt, 12, limit, simulation, vehicles,
            )
            .err()
            .unwrap_or(0);
            frames.push(json!({"entity":entity.0,"object":called_id,"status":status,"state":snapshot(&world)}));
            controls.push([22, dt, 12, entity.0 as i32, limit as i32, 0]);
        }
        for (step, frame) in frames.iter().enumerate() {
            let expected = if branch == 25 || branch == 49 {
                -5
            } else if step == 0 {
                match branch {
                    26 | 27 | 30 => -5,
                    28 | 31 => -1,
                    29 => -3,
                    _ => 0,
                }
            } else {
                0
            };
            assert_eq!(
                frame["status"], expected,
                "Emitter branch {branch} step {step}"
            );
        }
        if branch == 32 {
            assert_eq!(
                frames.last().unwrap()["state"]["weapons"]
                    .as_array()
                    .unwrap()
                    .len(),
                35
            );
        }
        if branch == 33 {
            assert_eq!(
                frames.last().unwrap()["state"]["weapons"]
                    .as_array()
                    .unwrap()
                    .len(),
                5
            );
        }
        let mut camera_words = Vec::new();
        ints(&mut camera_words, camera);
        let mut player_words = Vec::new();
        ints(&mut player_words, player);
        let (scene, _) = scene_export::encode(&world.objects.world);
        let name = format!("scene-emitter-{branch}.bin");
        fs::write(
            out.join(&name),
            scene
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        cases.push(json!({"tag":format!("emitter-{branch}"),"scene":name,"fixed_count":fixed,"generic":world.registry.generic.0,"obstacle":world.registry.obstacle.0,"world_class":world.registry.world.0,"projectile_class":world.objects.components.projectiles.class.map_or(-1,|v|v.0 as i32),"resources_present":world.objects.components.projectiles.resources.is_some(),"homing_mesh":0,"launch":vec![0u16;93],"initial":initial,"frames":frames,"controls":controls,"emitter_objects":IDS,"emitter_camera":camera_words,"emitter_player":player_words,"emitter_player_present":vehicles&&world.vehicles.entries[0].is_some(),"emitter_simulation":simulation,"emitter_material":pr.main[4]}));
    }
    fs::write(out.join("scene-emitter.json"),serde_json::to_vec(&json!({"cases":cases,"scope":"Actual first-map emitter class ticks and configured Projectile factories: camera/BOX gates, exact timer and RNG, gun launch, ownerless Debris state, mesh/sprite/emitter publication, ordered activation/list changes and partial failures, including shared emitters and cross-bank creation. Audio and extra-map asteroid construction remain outside this path."})).unwrap()).unwrap();
}
