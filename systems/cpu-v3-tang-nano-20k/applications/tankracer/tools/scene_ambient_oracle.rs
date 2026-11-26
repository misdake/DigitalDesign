//! Actual first-map chimney and sound-region actor updates; playback is external.
use super::{ints, scene_export, scene_mines_oracle};
use serde_json::{Value, json};
use std::{fs, path::Path, sync::Arc};
use tankracer_game::{
    actor_startup::Actor, dispatch::MethodId, math::Matrix, object_query::ObstacleId,
    scene_objects::rcc_lifecycle_probe, vehicle_runtime::SinglePlayer,
};
fn snapshot(world: &SinglePlayer) -> Value {
    let mut out = scene_mines_oracle::snapshot(world);
    let Some(Actor::Smoker(s)) = world.objects.components.actors.get(ObstacleId(1)) else {
        panic!("Smoker")
    };
    let mut state = Vec::new();
    ints(
        &mut state,
        [
            i32::from(s.white),
            i32::from(s.billow),
            s.timer,
            i32::from(s.visible),
            s.material as i32,
        ],
    );
    for p in s.particles.iter() {
        ints(&mut state, [i32::from(p.spin)]);
        ints(&mut state, p.velocity);
        ints(&mut state, p.size);
        ints(&mut state, [i32::from(p.rotation)]);
        ints(&mut state, p.position);
        ints(
            &mut state,
            [
                i32::from(p.active),
                i32::from(p.clockwise),
                i32::from(p.life),
            ],
        );
    }
    assert_eq!(state.len(), 426);
    out["smoker_state"] = json!(state);
    let mut sprites = Vec::new();
    for sprite in s.sprites() {
        super::sprite(&mut sprites, sprite);
    }
    out["smoke_sprites"] = json!(sprites);
    out["smoke_sequence"] = json!(world.objects.components.actors.smoke_sequence.0);
    let Some(Actor::Noisy { sounds, selected }) =
        world.objects.components.actors.get(ObstacleId(0))
    else {
        panic!("Noisy")
    };
    let mut state = Vec::new();
    ints(&mut state, [*selected]);
    out["noisy_selected"] = json!(state);
    let mut records = Vec::new();
    for sound in sounds {
        ints(&mut records, sound.values);
        ints(&mut records, [sound.sample as i32]);
    }
    out["noisy_sounds"] = json!(records);
    out
}
pub fn export(root: &Path, out: &Path) {
    let mut cases = Vec::new();
    for branch in 0..61 {
        let mut world = super::reference_world::full_race(root);
        let fixed = world.objects.world.colliders.len();
        let id = usize::from(branch < 40 || branch == 60);
        let entity = world.objects.world.colliders[id].entity;
        let position = world.objects.world.colliders[id].pose.position;
        let mut camera = position.map(|v| v >> 3);
        let mut dt = 40i32;
        let mut number = entity.0;
        let mut calls = 3;
        let mut vehicles = true;
        let mut simulation = true;
        let mut limit = 100000;
        if branch < 40 || branch == 60 {
            world.objects.components.actors.smoke_sequence.0 = (branch % 10) as u8;
            let Some(Actor::Smoker(s)) = world.objects.components.actors.get_mut(ObstacleId(1))
            else {
                unreachable!()
            };
            if matches!(branch, 10 | 11 | 12 | 13 | 22 | 24 | 27 | 37 | 38) {
                for p in s.particles.iter_mut() {
                    p.active = 1;
                    p.life = 50;
                    p.spin = 65530;
                    p.rotation = 65530;
                    p.velocity = [1, -2, 3];
                    p.size = [100; 2];
                }
            }
            if branch == 10 {
                s.timer = -1;
            }
            if branch == 11 {
                for p in s.particles.iter_mut() {
                    p.life = 1;
                }
            }
            if branch == 12 {
                for p in s.particles.iter_mut() {
                    p.life = 2;
                }
            }
            if branch == 13 {
                s.particles[2].active = 0;
                s.particles[7].active = 0;
            }
            if branch == 14 {
                s.billow = true;
            }
            if branch == 15 {
                s.billow = false;
            }
            if branch == 16 {
                s.white = true;
            }
            if branch == 17 {
                s.white = false;
            }
            if branch == 18 || branch == 19 || branch == 20 {
                camera[0] = camera[0].wrapping_sub(if branch == 18 {
                    19200
                } else if branch == 19 {
                    19199
                } else {
                    i32::MIN
                });
            }
            if branch == 21 {
                dt = 0;
            }
            if branch == 22 {
                dt = -40;
                for p in s.particles.iter_mut() {
                    p.life = 0;
                }
            }
            if branch == 23 {
                dt = i32::MAX;
            }
            if branch == 24 {
                dt = i32::MIN;
            }
            if branch == 25 {
                s.timer = dt;
            }
            if branch == 27 {
                simulation = false;
                s.visible = true;
                camera = [1000000, 0, 1000000];
            }
            if branch == 28 {
                vehicles = false;
            }
            if branch == 29 {
                world.vehicles.entries[0] = None;
            }
            if branch == 33 {
                world.objects.world.colliders[1].external_basis = None;
            }
            if branch == 34 {
                world.objects.world.colliders[1].external_basis =
                    Some(Matrix::from_angles([1000, 2000, 3000]));
            }
            if branch == 35 {
                limit = 0;
            }
            if branch == 36 {
                limit = 1;
            }
            if branch == 37 {
                for p in s.particles.iter_mut() {
                    p.active = 255;
                    p.clockwise = 255;
                    p.life = 255;
                    p.spin = 65535;
                    p.rotation = 65535;
                }
            }
            if branch == 38 {
                s.white = true;
                s.material = 0x87654321;
            }
            if branch == 39 {
                world.objects.world.colliders[1].pose.position = [i32::MIN, i32::MAX, -12345];
                camera = world.objects.world.colliders[1]
                    .pose
                    .position
                    .map(|v| v >> 3);
            }
        } else {
            let Some(Actor::Noisy { sounds, selected }) =
                world.objects.components.actors.get_mut(ObstacleId(0))
            else {
                unreachable!()
            };
            assert!(!sounds.is_empty());
            camera = [sounds[0].values[0] >> 3, 0, sounds[0].values[2] >> 3];
            if branch == 41 || branch == 45 {
                number ^= 4;
            }
            if branch == 42 || branch == 43 || branch == 51 || branch == 52 {
                *selected = 0;
            }
            if branch == 43 {
                camera = [1000000, 0, 1000000];
            }
            if branch == 44 || branch == 45 {
                *selected = -2;
            }
            if branch == 46 {
                *selected = sounds.len() as i32;
            }
            if branch == 47 {
                sounds.clear();
                *selected = -1;
            }
            if branch == 48 {
                sounds.clear();
                *selected = 0;
            }
            if branch == 49 {
                vehicles = false;
                simulation = false;
            }
            if branch == 50 {
                world.vehicles.entries[0] = None;
                simulation = false;
            }
            if branch == 51 {
                vehicles = false;
                simulation = false;
                camera[0] = camera[0].wrapping_sub(sounds[0].values[3]);
            }
            if branch == 52 {
                sounds[0].values[3] = 0;
            }
            if branch == 53 {
                sounds[0].values[3] = -1;
                camera[0] = camera[0].wrapping_sub(i32::MIN);
                camera[2] = camera[2].wrapping_sub(i32::MIN);
            }
            if branch == 54 {
                camera[1] = i32::MAX;
            }
            if branch == 55 {
                let pos = sounds[0].values;
                for s in &mut *sounds {
                    s.values[..3].copy_from_slice(&pos[..3]);
                }
            }
            if branch == 56 && sounds.len() > 1 {
                camera = [sounds[1].values[0] >> 3, 0, sounds[1].values[2] >> 3];
                sounds[0].values[0] = i32::MAX;
            }
        }
        if matches!(branch, 30 | 31 | 32 | 57 | 58 | 59) {
            let class = world
                .identity
                .entities
                .record(entity)
                .unwrap()
                .class
                .unwrap();
            let mut t = (*world.registry.classes[class.0]).clone();
            if branch == 30 || branch == 57 {
                t.methods[12] = None;
            } else if branch == 31 || branch == 58 {
                t.maximum_slot = 11;
            } else {
                t.methods[12] = Some(MethodId(74));
            }
            let t = Arc::new(t);
            world.registry.classes[class.0] = t.clone();
            world.objects.world.classes[class.0] = t;
        }
        if branch == 60 {
            calls = 6;
        }
        world.identity.current_slot = 17;
        let initial = snapshot(&world);
        let mut frames = Vec::new();
        let mut controls = Vec::new();
        let mut cameras = Vec::new();
        for step in 0..calls {
            let called_id = if branch == 60 {
                (step as usize) % 2
            } else {
                id
            };
            let entity = world.objects.world.colliders[called_id].entity;
            let current_camera = if branch == 26 && step == 1 {
                [1000000, 0, 1000000]
            } else {
                camera
            };
            world.objects.components.camera_position = current_camera;
            let frame_number = if branch == 60 {
                entity.0
            } else {
                number.wrapping_add(step as u32)
            };
            world.objects.components.frame_group = frame_number;
            let status = rcc_lifecycle_probe::tick_scenery(
                &mut world, entity, dt, 12, limit, simulation, vehicles,
            )
            .err()
            .unwrap_or(0);
            frames.push(json!({"entity":entity.0,"object":called_id,"status":status,"state":snapshot(&world)}));
            controls.push([
                24,
                dt,
                12,
                entity.0 as i32,
                limit as i32,
                frame_number as i32,
            ]);
            ints(&mut cameras, current_camera);
        }
        let expected = match branch {
            27 | 28 | 32 | 59 => -5,
            29 | 44 | 46 | 48 => -4,
            _ => 0,
        };
        assert!(
            frames.iter().all(|f| f["status"] == expected),
            "Ambient branch {branch}"
        );
        let (scene, _) = scene_export::encode(&world.objects.world);
        let name = format!("scene-ambient-{branch}.bin");
        fs::write(
            out.join(&name),
            scene
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        cases.push(json!({"tag":format!("ambient-{branch}"),"scene":name,"fixed_count":fixed,"generic":world.registry.generic.0,"obstacle":world.registry.obstacle.0,"world_class":world.registry.world.0,"projectile_class":-1,"resources_present":false,"homing_mesh":0,"launch":vec![0u16;93],"initial":initial,"frames":frames,"controls":controls,"ambient_cameras":cameras,"ambient_banked":matches!(branch,14|27|60),"ambient_vehicles":vehicles,"ambient_player_present":world.vehicles.entries[0].is_some(),"ambient_simulation":simulation}));
    }
    fs::write(out.join("scene-ambient.json"),serde_json::to_vec(&json!({"cases":cases,"scope":"Actual first-map Smoker/Noisy class ticks: independent shared smoke sequence, exact packed particle/narrow arithmetic, camera freezing, region selection and context/class errors, with read-only sprite derivation. Audio playback is external."})).unwrap()).unwrap();
}
