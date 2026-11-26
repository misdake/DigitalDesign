//! Original weapon constructors, nested parent initialization and shared release.
use super::{scene_allocation_oracle, scene_export};
use serde_json::json;
use std::{fs, path::Path};
use tankracer_game::{
    dispatch::ClassId, math::Matrix, object_query::EntityId, pose::Pose, projectile::launch::Owner,
    scene_entity::Handle, scene_objects::rcc_lifecycle_probe, vehicle_runtime::SinglePlayer,
};

pub(super) fn snapshot(world: &SinglePlayer) -> serde_json::Value {
    let mut out = scene_allocation_oracle::snapshot(world);
    let mut weapons = Vec::new();
    for (id, state) in world.objects.components.projectiles.entries() {
        let mut payload = Vec::new();
        super::state(&mut payload, state);
        weapons.push((id.0, 1, payload));
    }
    for (id, state) in world.objects.components.mines.entries() {
        let mut payload = Vec::new();
        super::ints(
            &mut payload,
            [
                i32::from(state.owner.is_some()),
                state.owner.map_or(0, |v| v.0 as i32),
                state.kind as i32,
                i32::from(state.count),
                i32::from(state.repeat),
                i32::from(state.armed),
                state.born as i32,
                state.last_contact as i32,
                state.elapsed,
            ],
        );
        weapons.push((id.0, 2, payload));
    }
    for (id, state) in world.objects.components.explosions.iter().enumerate() {
        if let Some(state) = state {
            let mut payload = super::explosion_oracle::snapshot(state, 0);
            payload.truncate(676);
            weapons.push((id, 3, payload));
        }
    }
    for (id, state) in world.objects.components.ufos.iter().enumerate() {
        if let Some(s) = state {
            let mut payload = Vec::new();
            super::ints(&mut payload, [s.target.0 as i32, s.flags as i32]);
            super::ints(&mut payload, s.destination);
            super::ints(&mut payload, s.point);
            super::ints(
                &mut payload,
                [
                    s.departure_speed,
                    i32::from(s.arrived),
                    i32::from(s.timer),
                    i32::from(s.fade),
                    i32::from(s.view),
                    i32::from(s.reserved.is_some()),
                    s.reserved.map_or(0, |v| v.0 as i32),
                    i32::from(s.was_special),
                    i32::from(s.was_collidable),
                ],
            );
            super::sprite(&mut payload, s.sprite);
            assert_eq!(payload.len(), 56);
            weapons.push((id, 4, payload));
        }
    }
    weapons.sort_by_key(|r| r.0);
    out["weapons"] = json!(
        weapons
            .into_iter()
            .map(|(id, kind, payload)| {
                let meta = super::projectile_snapshot::metadata(world, id);
                json!({"id":id,"kind":kind,"meta":meta,"payload":payload})
            })
            .collect::<Vec<_>>()
    );
    let mut shared = Vec::new();
    super::projectile_snapshot::flight(&mut shared, &world.objects.components.projectiles.flight);
    out["flight"] = json!(shared);
    out
}
pub fn export(root: &Path, out: &Path) {
    let mut cases = Vec::new();
    for branch in 0..16 {
        let mut world = super::reference_world::race(root);
        let projectile = world.objects.components.projectiles.class.unwrap();
        let mine = world.objects.components.mines.class.unwrap();
        if branch >= 14 {
            let class = if branch == 14 { projectile } else { mine };
            let mut table = (*world.registry.classes[class.0]).clone();
            table.methods[0] = None;
            let table = std::sync::Arc::new(table);
            world.registry.classes[class.0] = table.clone();
            world.objects.world.classes[class.0] = table;
        }
        if branch == 2 || branch == 8 {
            // Reused transient IDs become part of the qualifier's fixed prefix.
            let a = rcc_lifecycle_probe::create_weapon(&mut world, projectile, 100000).unwrap();
            let b = rcc_lifecycle_probe::create_weapon(&mut world, mine, 100000).unwrap();
            rcc_lifecycle_probe::release_transient(&mut world, a).unwrap();
            rcc_lifecycle_probe::release_transient(&mut world, b).unwrap();
        }
        let resources = world.objects.components.projectiles.resources.unwrap();
        if branch == 13 {
            world
                .objects
                .components
                .projectiles
                .resources
                .as_mut()
                .unwrap()
                .cup = 2;
        }
        let resources = world
            .objects
            .components
            .projectiles
            .resources
            .unwrap_or(resources);
        let car = world.vehicles.entries[0].as_ref().unwrap();
        let position = car.physics.motion.poses.base_position;
        let owner = Owner {
            entity: car.physics.entity,
            turret: Pose {
                position,
                matrix: if branch == 9 {
                    Matrix([4096, 0, 0, 0, 4096, 0, 0, i32::MIN, 4096])
                } else {
                    Matrix::from_angles([65000, 13000, 1200])
                },
            },
            gun: [position[0], position[1] + 50000, position[2]],
            velocity: [100001, -12345, -567890],
        };
        let launch = super::launch_input(resources, owner);
        if branch == 10 {
            world.objects.components.projectiles.class = None;
        }
        if branch == 11 {
            world.objects.components.projectiles.resources = None;
        }
        let projectile_class = world
            .objects
            .components
            .projectiles
            .class
            .map_or(-1, |c| c.0 as i32);
        let resources_present = world.objects.components.projectiles.resources.is_some();
        let fixed = world.objects.world.colliders.len();
        let initial = snapshot(&world);
        let mut operations = Vec::new();
        if branch >= 14 {
            operations.push([
                0,
                if branch == 14 {
                    projectile.0 as i32
                } else {
                    mine.0 as i32
                },
                0,
                0,
                100000,
            ]);
        } else if branch == 0 {
            for _ in 0..30 {
                operations.push([0, projectile.0 as i32, 0, 0, 100000]);
            }
        } else if branch == 1 {
            for _ in 0..8 {
                operations.push([0, mine.0 as i32, 0, 0, 100000]);
            }
        } else if branch == 2 {
            for i in 0..12 {
                operations.push([
                    0,
                    if i % 2 == 0 {
                        projectile.0 as i32
                    } else {
                        mine.0 as i32
                    },
                    0,
                    0,
                    100000,
                ]);
            }
        } else if branch == 3 {
            operations.push([0, 99, 0, 0, 100000]);
        } else if branch == 4 {
            operations.push([0, mine.0 as i32, 0, 0, 0]);
        } else if branch < 7 {
            operations.push([
                0,
                if branch == 5 {
                    mine.0 as i32
                } else {
                    projectile.0 as i32
                },
                0,
                0,
                100000,
            ]);
        } else {
            for index in 0..if branch == 7 { 30 } else { 6 } {
                let kind = index % 6;
                operations.push([3, kind, 0, 0, if branch == 12 { 1 } else { 100000 }]);
            }
        }
        let mut frames = Vec::new();
        let mut controls = Vec::new();
        let mut created = Vec::new();
        let mut step = 0;
        let mut scheduled = false;
        while step < operations.len() {
            let c = operations[step];
            let (entity, result) = if c[0] == 0 {
                let result = rcc_lifecycle_probe::create_weapon(
                    &mut world,
                    ClassId(c[1] as usize),
                    c[4] as u32,
                );
                // Failed constructors retain their allocated identity; snapshot checks it.
                let entity = result.as_ref().map_or(0, |e| e.0);
                if result.is_ok() {
                    created.push(entity);
                }
                (EntityId(entity), result.map(|_| ()))
            } else if c[0] == 3 {
                let result = rcc_lifecycle_probe::spawn_projectile(
                    &mut world,
                    owner,
                    super::kind_of(c[1]),
                    c[4] as u32,
                );
                let entity = result.as_ref().map_or(0, |e| e.0);
                if result.is_ok() {
                    created.push(entity);
                }
                (EntityId(entity), result.map(|_| ()))
            } else {
                let entity = EntityId(c[3] as u32);
                (
                    entity,
                    rcc_lifecycle_probe::dispatch_weapon(
                        &mut world,
                        entity,
                        c[2] as u32,
                        c[4] as u32,
                    ),
                )
            };
            let object = world
                .identity
                .entities
                .record(entity)
                .and_then(|r| r.object)
                .and_then(Handle::obstacle)
                .map(|id| id.0);
            frames.push(json!({"entity":entity.0,"status":result.as_ref().map_or_else(|&v|v,|_|0),"object":object,"state":snapshot(&world)}));
            controls.push(c);
            step += 1;
            if !scheduled
                && step == operations.len()
                && (c[0] == 0 || c[0] == 3)
                && !created.is_empty()
            {
                scheduled = true;
                for &entity in &created {
                    operations.push([2, 0, 4, entity as i32, 100000]);
                    if branch == 0 || branch >= 7 {
                        operations.push([2, 0, 8, entity as i32, 100000]);
                    }
                    if branch == 5 {
                        operations.push([2, 0, 1, entity as i32, 0]);
                    }
                    if branch == 6 {
                        operations.push([2, 0, 28, entity as i32, 100000]);
                    }
                    operations.push([2, 0, 5, entity as i32, 100000]);
                    operations.push([2, 0, 3, entity as i32, 100000]);
                }
                // A complete release makes both identity and component slots reusable.
                if branch == 2 {
                    operations.push([0, projectile.0 as i32, 0, 0, 100000]);
                    created.clear();
                }
            }
        }
        if branch == 7 {
            assert_eq!(frames[29]["state"]["weapons"].as_array().unwrap().len(), 30);
        }
        if branch == 9 || branch == 12 {
            let expected = if branch == 9 { -2 } else { -3 };
            assert!(
                frames
                    .iter()
                    .all(|f| f["status"].as_i64() == Some(expected))
            );
        }
        if branch >= 14 {
            assert_eq!(frames[0]["status"], -1);
            assert_eq!(frames[0]["state"]["slot"], 1);
        }
        let (scene, _) = scene_export::encode(&world.objects.world);
        let name = format!("scene-creation-{branch}.bin");
        fs::write(
            out.join(&name),
            scene
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        cases.push(json!({"tag":format!("constructor-{branch}"),"fixed_count":fixed,"generic":world.registry.generic.0,"obstacle":world.registry.obstacle.0,"world_class":world.registry.world.0,"projectile_class":projectile_class,"resources_present":resources_present,"homing_mesh":resources.homing_mesh.index(),"launch":launch,"scene":name,"initial":initial,"controls":controls,"frames":frames}));
    }
    fs::write(out.join("scene-creation.json"),serde_json::to_vec(&json!({"cases":cases,"scope":"Original projectile/mine constructors and six-kind projectile factory, nested initialization, launch/sprite, shared flight/RNG, activation/deactivation/stop/release, banked component reuse and committed failure prefixes; mixed flight/impact/explosion scheduling remains separate"})).unwrap()).unwrap();
}
