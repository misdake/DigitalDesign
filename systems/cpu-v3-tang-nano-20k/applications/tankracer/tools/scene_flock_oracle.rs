//! Original first-map flocks with live bird, model and mesh ownership.
use super::{ints, scene_export, scene_mines_oracle};
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, path::Path, sync::Arc};
use tankracer_game::{
    actor_startup::{Actor, flock::Flock},
    dispatch::MethodId,
    math::Matrix,
    object_query::ObstacleId,
    scene_meshes::rcc_probe,
    scene_objects::rcc_lifecycle_probe,
    vehicle_runtime::SinglePlayer,
};
const IDS: [usize; 3] = [183, 184, 185];
fn state(f: &Flock) -> Vec<u16> {
    let mut out = Vec::new();
    super::projectile_snapshot::flock(&mut out, f);
    assert_eq!(out.len(), 288);
    out
}
fn snapshot(world: &SinglePlayer) -> Value {
    let mut out = scene_mines_oracle::snapshot(world);
    let mut states = Vec::new();
    let mut config = Vec::new();
    let mut models = Vec::new();
    let mut meshes = BTreeSet::new();
    for id in IDS {
        let Some(Actor::Flock(f)) = world.objects.components.actors.get(ObstacleId(id)) else {
            panic!("Flock owner")
        };
        states.extend(state(f));
        ints(&mut config, [f.range, f.kind as i32]);
        config.extend([
            f.maneuver as u16,
            f.models.len() as u16,
            f.meshes.len() as u16,
            f.takeoff_sample as u16,
            (f.takeoff_sample >> 16) as u16,
            0,
        ]);
        for (i, m) in f.models.iter().enumerate() {
            let vertices: Vec<_> = m
                .geometry
                .vertices
                .iter()
                .flatten()
                .map(|&v| v as u16)
                .collect();
            models.push(json!({"low":f.extrema[i][0],"high":f.extrema[i][1],"vertices":vertices,"mesh":f.meshes[i].index()}));
        }
        meshes.extend(f.meshes.iter().map(|m| m.index()));
    }
    let mut geometry = Vec::new();
    for id in meshes {
        let exists = id < world.objects.components.surfaces.meshes.len();
        let (vertices, version, present) = if exists {
            let (g, _, _, v) =
                rcc_probe::inspect(&world.objects.components.surfaces.meshes, rcc_probe::id(id));
            let present = g.is_some();
            (
                g.map_or(vec![], |g| {
                    g.vertices.iter().flatten().map(|&v| v as u16).collect()
                }),
                v,
                present,
            )
        } else {
            (vec![], 0, false)
        };
        geometry.push(json!({"id":id,"exists":exists,"present":present,"version":[version as u16,(version>>16) as u16,(version>>32) as u16,(version>>48) as u16],"vertices":vertices}));
    }
    out["flock_state"] = json!(states);
    out["flock_config"] = json!(config);
    out["flock_models"] = json!(models);
    out["flock_meshes"] = json!(geometry);
    out
}
pub fn export(root: &Path, out: &Path) {
    let mut cases = Vec::new();
    for branch in 0..48 {
        let mut world = super::reference_world::full_race(root);
        let fixed = world.objects.world.colliders.len();
        let id = IDS[branch % 3];
        let entity = world.objects.world.colliders[id].entity;
        let position = world.objects.world.colliders[id].pose.position;
        let mut player = position;
        player[2] = player[2].wrapping_sub(1000);
        let mut dt = 40i32;
        let mut number = entity.0;
        let mut limit = 100000;
        let mut vehicles = true;
        let mut simulation = true;
        let mut slot = 12;
        let mut calls = 3;
        let mut failed_mesh = None;
        let mut overflow_mesh = None;
        {
            let Some(Actor::Flock(f)) = world.objects.components.actors.get_mut(ObstacleId(id))
            else {
                unreachable!()
            };
            if (3..6).contains(&branch) {
                number ^= 1;
            }
            if (6..9).contains(&branch) {
                player[0] = position[0].wrapping_add(f.range);
            }
            if branch >= 9 && branch < 38 {
                f.flight.phase = 1;
                f.flight.timer = 2000;
                f.flight.stage = 0;
                f.flight.target = [4096, 2048, -8192];
                for b in &mut f.birds {
                    b.active = false;
                }
                if (9..12).contains(&branch) {
                    for b in &mut f.birds {
                        b.active = true;
                    }
                }
                if (12..15).contains(&branch) {
                    f.flight.timer = dt;
                }
                if (15..18).contains(&branch) {
                    f.flight.timer = 0;
                }
                if (18..21).contains(&branch) {
                    f.flight.timer = 0;
                    f.flight.stage = 1;
                }
                if branch == 21 {
                    f.flight.stage = u32::MAX;
                }
                if branch == 22 {
                    f.flight.phase = 2;
                }
                if branch == 23 {
                    dt = -40;
                }
                if branch == 24 {
                    dt = i32::MAX;
                }
                if branch == 25 {
                    for m in &mut f.models {
                        let m = Arc::make_mut(m);
                        for v in &mut m.geometry.vertices {
                            v[1] = 32760;
                        }
                    }
                }
                if branch == 26 {
                    f.extrema[0][1] = f.extrema[0][0];
                }
                if branch == 27 || branch == 28 {
                    failed_mesh = Some(f.meshes[usize::from(branch == 28)]);
                }
                if branch == 29 {
                    f.meshes[0] = rcc_probe::id(65535);
                }
                if branch == 30 {
                    f.meshes[1] = f.meshes[0];
                }
                if branch == 31 {
                    overflow_mesh = Some(f.meshes[0]);
                }
                if branch == 32 {
                    vehicles = false;
                }
                if branch == 33 {
                    world.vehicles.entries[0] = None;
                }
                if branch == 34 {
                    simulation = false;
                }
                if branch == 35 || branch == 36 || branch == 37 {
                    f.flight.timer = 0;
                    f.flight.stage = 1;
                    limit = if branch == 35 {
                        0
                    } else if branch == 36 {
                        1
                    } else {
                        100000
                    };
                }
            }
            if branch == 43 {
                f.flight.phase = 1;
                f.flight.timer = 3000;
                f.flight.stage = 1;
                f.flight.target = [i32::MIN, 0, 0];
                f.flight.velocities[3] = [i32::MIN, 0, 0];
                for b in &mut f.birds {
                    b.active = false;
                    b.speed = 0;
                }
            }
        }
        if let Some(mesh) = failed_mesh {
            world
                .objects
                .components
                .surfaces
                .meshes
                .release_private(mesh)
                .unwrap();
        }
        if let Some(mesh) = overflow_mesh {
            rcc_probe::version(
                &mut world.objects.components.surfaces.meshes,
                mesh,
                u64::MAX,
            );
        }
        if branch == 37 || branch == 38 || branch == 39 || branch == 40 {
            let class = world
                .identity
                .entities
                .record(entity)
                .unwrap()
                .class
                .unwrap();
            let mut t = (*world.registry.classes[class.0]).clone();
            if branch == 37 {
                t.methods[5] = Some(MethodId(74));
            } else if branch == 38 {
                t.methods[12] = None;
            } else if branch == 39 {
                t.maximum_slot = 11;
            } else {
                t.methods[12] = Some(MethodId(74));
            }
            let t = Arc::new(t);
            world.registry.classes[class.0] = t.clone();
            world.objects.world.classes[class.0] = t;
        }
        if branch == 41 {
            slot = 4;
        }
        if branch == 42 {
            slot = 5;
        }
        if branch == 44 {
            world.objects.world.colliders[id].external_basis = None;
        }
        if branch == 45 {
            world.objects.world.colliders[id].external_basis =
                Some(Matrix::from_angles([1024, 1234, 512]));
        }
        if branch == 46 {
            player[0] = position[0].wrapping_sub(i32::MIN);
        }
        if branch == 47 {
            calls = 9;
            for id in IDS {
                world.objects.world.colliders[id].pose.position = position;
                let Some(Actor::Flock(f)) = world.objects.components.actors.get_mut(ObstacleId(id))
                else {
                    unreachable!()
                };
                f.range = i32::MAX;
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
            let called_id = if branch == 47 {
                IDS[step as usize % 3]
            } else {
                id
            };
            let entity = world.objects.world.colliders[called_id].entity;
            let frame_number = if branch == 47 {
                entity.0
            } else {
                number.wrapping_add(step as u32)
            };
            world.objects.components.frame_group = frame_number;
            let status = rcc_lifecycle_probe::tick_scenery(
                &mut world, entity, dt, slot, limit, simulation, vehicles,
            )
            .err()
            .unwrap_or(0);
            frames.push(json!({"entity":entity.0,"object":called_id,"status":status,"state":snapshot(&world)}));
            controls.push([
                23,
                dt,
                slot as i32,
                entity.0 as i32,
                limit as i32,
                frame_number as i32,
            ]);
        }
        let expected = match branch {
            22 | 32 | 34 | 37 | 40 => -5,
            27 | 28 | 29 | 35 => -1,
            33 => -4,
            36 => -3,
            43 => -2,
            _ => 0,
        };
        assert!(
            frames.iter().all(|f| f["status"] == expected),
            "Flock branch {branch}"
        );
        let mut player_words = Vec::new();
        ints(&mut player_words, player);
        let (scene, _) = scene_export::encode(&world.objects.world);
        let name = format!("scene-flock-{branch}.bin");
        fs::write(
            out.join(&name),
            scene
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        cases.push(json!({"tag":format!("flock-{branch}"),"scene":name,"fixed_count":fixed,"generic":world.registry.generic.0,"obstacle":world.registry.obstacle.0,"world_class":world.registry.world.0,"projectile_class":-1,"resources_present":false,"homing_mesh":0,"launch":vec![0u16;93],"initial":initial,"frames":frames,"controls":controls,"flock_objects":IDS,"flock_banked":matches!(branch,26|31|47),"flock_player":player_words,"flock_player_present":world.vehicles.entries[0].is_some(),"flock_vehicles":vehicles,"flock_simulation":simulation}));
    }
    fs::write(out.join("scene-flock.json"),serde_json::to_vec(&json!({"cases":cases,"scope":"Actual first-map flock class ticks: activation, ordered RNG/bird motion, flight stages and angle failures, mutable wing models, shared mesh aliases/revisions, stop publication and partial failures, with alternating actors."})).unwrap()).unwrap();
}
