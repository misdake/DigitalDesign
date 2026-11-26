//! Real explosion constructors, capacity retirement and shared-resource release.
use super::{explosion_blasts_oracle, ints, scene_creation_oracle, scene_export};
use serde_json::{Value, json};
use std::{fs, path::Path};
use tankracer_game::{
    explosion::blasts, object_query::EntityId, scene_entity::Handle,
    scene_objects::rcc_lifecycle_probe, vehicle_runtime::SinglePlayer,
};
pub(super) fn snapshot(world: &SinglePlayer) -> Value {
    let mut out = scene_creation_oracle::snapshot(world);
    let state = &world.objects.components.explosion_world;
    let mut words = Vec::new();
    ints(
        &mut words,
        [
            state.count,
            i32::from(state.overflow),
            i32::from(state.fog),
            i32::from(state.flash_timer),
            i32::from(state.flash_active),
        ],
    );
    for s in state.slots {
        ints(
            &mut words,
            [
                i32::from(s.entity.is_some()),
                s.entity.map_or(0, |e| e.0 as i32),
                s.frame as i32,
            ],
        );
    }
    assert_eq!(words.len(), 100);
    out["explosion_world"] = json!(words);
    out["blast_effects"] = json!(explosion_blasts_oracle::snapshot(
        &world.objects.components.blasts,
        &world.shared.pool,
        world.shared.random
    ));
    out
}
pub fn export(root: &Path, out: &Path) {
    let mut cases = Vec::new();
    for branch in 0..16 {
        let mut world = super::reference_world::race(root);
        let class = world.objects.components.explosion_class.unwrap();
        let position = world.vehicles.entries[0]
            .as_ref()
            .unwrap()
            .physics
            .motion
            .poses
            .base_position
            .map(|v| v.wrapping_add(789));
        let mut existing = Vec::new();
        if branch == 1 {
            let entity = rcc_lifecycle_probe::create_weapon(&mut world, class, 100000).unwrap();
            rcc_lifecycle_probe::release_transient(&mut world, entity).unwrap();
        }
        if branch >= 8 && branch <= 13 {
            for i in 0..if branch == 13 { 15 } else { 1 } {
                world.objects.components.frame_group = i;
                let entity =
                    rcc_lifecycle_probe::spawn_explosion(&mut world, position, 100000).unwrap();
                existing.push(entity);
            }
            if branch <= 12 {
                let entity = existing[0];
                let id = world
                    .identity
                    .entities
                    .record(entity)
                    .unwrap()
                    .object
                    .and_then(Handle::obstacle)
                    .unwrap();
                let slot = world
                    .objects
                    .components
                    .blasts
                    .band(
                        blasts::Start {
                            origin: position,
                            owner: entity,
                            time: 1,
                            camera: world.objects.components.camera_basis,
                        },
                        20480,
                        true,
                        &mut world.shared.pool,
                    )
                    .unwrap()
                    .0;
                world.objects.components.explosions[id.0]
                    .as_mut()
                    .unwrap()
                    .band = slot;
                let slot = world
                    .objects
                    .components
                    .blasts
                    .ring(
                        blasts::Start {
                            origin: position,
                            owner: entity,
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
                    .ring = slot;
                if branch == 10 {
                    world.objects.components.blasts.rings[slot as usize].rays[4].handle =
                        tankracer_game::effect_pool::QuadHandle(999);
                }
                if branch == 11 {
                    world.objects.components.blasts.bands[0].owner = None;
                }
                if branch == 12 {
                    world.objects.components.explosion_world.slots[0].entity =
                        Some(EntityId(0xfedc));
                }
            }
        }
        if branch == 2 {
            world.objects.components.explosion_class = None;
        }
        if branch == 4 {
            let mut table = (*world.registry.classes[class.0]).clone();
            table.methods[0] = None;
            let table = std::sync::Arc::new(table);
            world.registry.classes[class.0] = table.clone();
            world.objects.world.classes[class.0] = table;
        }
        if branch == 5 {
            world.objects.components.explosion_resources.map_fog = true;
        }
        if branch == 6 {
            world.objects.components.explosion_world.count = 15;
        }
        if branch == 7 {
            world.objects.components.explosion_world.count = -1;
        }
        if branch == 13 {
            world.objects.components.special.clear();
        }
        let fixed = world.objects.world.colliders.len();
        let initial = snapshot(&world);
        let mut operations = Vec::new();
        if branch >= 8 && branch <= 12 {
            let e = existing[0].0 as i32;
            operations = vec![
                [2, 0, 5, e, 100000],
                [2, 0, 3, e, 100000],
                [6, 0, 100, 0, 100000],
                [2, 0, 3, e, 100000],
            ];
        } else {
            let count = if branch == 0 || branch == 5 {
                32
            } else if branch == 14 {
                17
            } else {
                3
            };
            for i in 0..count {
                operations.push([
                    6,
                    0,
                    if branch == 5 {
                        u32::MAX.wrapping_sub(i) as i32
                    } else if branch == 14 {
                        (count - i) as i32
                    } else {
                        i as i32 + 1
                    },
                    0,
                    if branch == 3 {
                        0
                    } else if branch == 14 {
                        1
                    } else {
                        100000
                    },
                ]);
            }
        }
        if branch == 15 {
            operations = vec![[0, class.0 as i32, 0, 0, 100000]];
        }
        let mut controls = Vec::new();
        let mut frames = Vec::new();
        let mut made = Vec::new();
        let mut scheduled = false;
        let mut step = 0;
        while step < operations.len() {
            let c = operations[step];
            let mut entity = EntityId(0);
            let result = if c[0] == 6 {
                world.objects.components.frame_group = c[2] as u32;
                match rcc_lifecycle_probe::spawn_explosion(&mut world, position, c[4] as u32) {
                    Ok(e) => {
                        entity = e;
                        made.push(e);
                        Ok(())
                    }
                    Err(e) => Err(e),
                }
            } else if c[0] == 0 {
                match rcc_lifecycle_probe::create_weapon(
                    &mut world,
                    tankracer_game::dispatch::ClassId(c[1] as u32 as usize),
                    c[4] as u32,
                ) {
                    Ok(e) => {
                        entity = e;
                        made.push(e);
                        Ok(())
                    }
                    Err(e) => Err(e),
                }
            } else {
                entity = EntityId(c[3] as u32);
                rcc_lifecycle_probe::dispatch_weapon_effects(
                    &mut world,
                    entity,
                    c[2] as u32,
                    c[4] as u32,
                    branch != 9,
                )
            };
            let object = world
                .identity
                .entities
                .record(entity)
                .and_then(|r| r.object)
                .and_then(Handle::obstacle)
                .map(|i| i.0);
            frames.push(json!({"entity":entity.0,"status":result.map_or_else(|e|e,|()|0),"object":object,"state":snapshot(&world)}));
            controls.push(c);
            step += 1;
            if !scheduled && step == operations.len() && !made.is_empty() {
                scheduled = true;
                for e in &made {
                    operations.push([2, 0, 5, e.0 as i32, 100000]);
                    operations.push([2, 0, 3, e.0 as i32, 100000]);
                }
                if branch == 1 {
                    operations.push([6, 0, 55, 0, 100000]);
                }
            }
        }
        if branch == 0 || branch == 5 {
            assert_eq!(frames[31]["state"]["explosion_world"][0], 15);
            assert_eq!(frames[15]["state"]["explosion_world"][2], 1);
            assert_eq!(
                frames[15]["state"]["explosion_world"][4],
                u16::from(branch == 5)
            );
            assert_eq!(
                frames[33]["state"]["explosion_world"][0],
                if branch == 0 { 15 } else { 14 },
                "unsigned birth order determines whether the released object was evicted"
            );
            assert_eq!(frames.last().unwrap()["state"]["explosion_world"][0], 0);
        }
        if branch == 4 {
            assert!(
                frames
                    .iter()
                    .all(|f| f["status"] == -1 && f["state"]["slot"] == 1)
            );
        }
        if branch == 9 {
            assert_eq!(frames[1]["status"], -5);
        }
        if branch == 10 {
            assert_eq!(frames[1]["status"], -1);
        }
        if branch == 11 {
            assert_eq!(frames[1]["status"], -4);
        }
        if branch == 14 {
            assert_eq!(frames[15]["status"], -3);
        }
        let (scene, _) = scene_export::encode(&world.objects.world);
        let name = format!("scene-explosions-{branch}.bin");
        fs::write(
            out.join(&name),
            scene
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let mut packed = Vec::new();
        let r = world.objects.components.explosion_resources;
        ints(
            &mut packed,
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
        cases.push(json!({"tag":format!("explosion-factory-{branch}"),"fixed_count":fixed,"generic":world.registry.generic.0,"obstacle":world.registry.obstacle.0,"world_class":world.registry.world.0,"projectile_class":world.objects.components.projectiles.class.unwrap().0,"resources_present":true,"homing_mesh":world.objects.components.projectiles.resources.unwrap().homing_mesh.index(),"launch":vec![0u16;93],"explosion_class":world.objects.components.explosion_class.map_or(-1,|v|v.0 as i32),"explosion_resources":packed,"explosion_position":pos,"effect_pool_present":branch!=9,"scene":name,"initial":initial,"controls":controls,"frames":frames}));
    }
    fs::write(out.join("scene-explosions.json"),serde_json::to_vec(&json!({"cases":cases,"scope":"Original explosion class construction, full factory, fifteen-slot capacity eviction, shared effect resource retirement before detach/release and bank reuse. Tick, contact and mixed native weapon scheduling follow separately."})).unwrap()).unwrap();
}
