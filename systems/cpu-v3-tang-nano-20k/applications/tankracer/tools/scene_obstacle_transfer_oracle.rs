//! Original obstacle transfer calls, grid prefixes and class output semantics.
use super::{ints, scene_export, scene_mines_oracle};
use serde_json::{Value, json};
use std::{fs, path::Path, sync::Arc};
use tankracer_game::{
    dispatch::{ClassId, MethodId},
    scene_objects::rcc_lifecycle_probe,
};
fn snapshot(world: &tankracer_game::vehicle_runtime::SinglePlayer, value: u32) -> Value {
    let mut out = scene_mines_oracle::snapshot(world);
    let mut result = Vec::new();
    ints(&mut result, [value as i32]);
    out["transfer_result"] = json!(result);
    out
}
pub fn export(root: &Path, out: &Path) {
    let mut cases = Vec::new();
    for branch in 0..41 {
        let mut world = super::reference_world::full_race(root);
        let fixed = world.objects.world.colliders.len();
        let mut id = world.objects.components.ordinary[0];
        if (36..39).contains(&branch) {
            let entity = world.identity.entities.create_record(100000).unwrap();
            world.identity.entities.record_mut(entity).unwrap().class =
                Some(world.registry.obstacle);
            id = rcc_lifecycle_probe::allocate_transient(&mut world, entity).unwrap();
        }
        let entity = world.objects.world.colliders[id.0].entity;
        let class = world.registry.obstacle;
        world.identity.entities.record_mut(entity).unwrap().class = Some(class);
        let position = world.objects.world.colliders[id.0].pose.position;
        let destination = match branch {
            10 => position,
            11 => [-999999, 123456, 999999],
            12 => [i32::MIN, i32::MAX, i32::MAX],
            34 => [position[0], i32::MIN, position[2]],
            _ => [
                position[0].wrapping_add(40000),
                -12345,
                position[2].wrapping_add(5000),
            ],
        };
        let mode = if branch == 39 || branch == 40 {
            4
        } else if matches!(branch, 16 | 17 | 18) {
            1
        } else if branch == 13 {
            2
        } else if branch == 15 {
            3
        } else {
            0
        };
        let shape = rcc_lifecycle_probe::transfer_object_setup(&mut world, id, mode);
        if branch == 22 {
            world.identity.entities.record_mut(entity).unwrap().object = None;
        }
        if branch == 23 {
            world.identity.entities.record_mut(entity).unwrap().class = None;
        }
        if branch == 24 {
            world.identity.entities.record_mut(entity).unwrap().class =
                Some(ClassId(world.registry.classes.len() + 1));
        }
        if matches!(branch, 25 | 26 | 27 | 28 | 31 | 32) {
            let mut table = (*world.registry.classes[class.0]).clone();
            match branch {
                25 => table.methods[9] = None,
                26 => table.maximum_slot = 8,
                27 | 32 => table.methods[9] = Some(MethodId(35)),
                28 => table.methods[10] = Some(MethodId(33)),
                31 => table.methods[9] = Some(MethodId(74)),
                _ => {}
            }
            let table = Arc::new(table);
            world.registry.classes[class.0] = table.clone();
            world.objects.world.classes[class.0] = table;
        }
        if branch == 19 {
            world.objects.components.ordinary.remove(0);
            world.objects.components.ordinary.insert(1, id);
        }
        world.identity.current_slot = 17;
        let mut value = 0x12345678;
        let initial = snapshot(&world, value);
        let requests = match branch {
            0 => vec![
                (9, 1, 1, 100000),
                (11, 1, 0, 100000),
                (10, 2, 0, 100000),
                (11, 1, 1, 100000),
                (9, 1, 0, 100000),
            ],
            1 => vec![(9, 1, 1, 100000); 2],
            2 => vec![(11, 1, 1, 100000); 2],
            3 => vec![(9, 1, u32::MAX, 100000)],
            4 => vec![(9, 0, 1, 100000)],
            5 => vec![(9, 2, 1, 100000)],
            6 => vec![(9, 1, 1, 0)],
            7 => vec![(11, 1, 1, 0)],
            8 => vec![(10, 2, 0, 0)],
            9 => vec![(10, 0, 0, 100000)],
            10 | 11 | 12 | 13 | 18 | 21 | 28 | 34 => {
                vec![(10, 2, 0, if branch == 21 { 1 } else { 100000 })]
            }
            14 => vec![(11, 1, 2, 100000)],
            15 | 17 => vec![(11, 1, 0, 100000)],
            19 => vec![(9, 1, 1, 1)],
            20 => vec![(11, 1, 0, 1)],
            29 => vec![(11, 0, 1, 100000)],
            30 => vec![(9, 1, 1, 100000), (9, 1, 0, 100000), (9, 1, 1, 100000)],
            39 => vec![(11, 1, 0, 1)],
            40 => vec![(10, 2, 0, 1)],
            36 => vec![
                (9, 1, 1, 100000),
                (11, 1, 1, 100000),
                (10, 2, 0, 100000),
                (11, 1, 0, 100000),
                (9, 1, 0, 100000),
            ],
            37 => vec![(10, 2, 0, 100000)],
            38 => vec![(11, 1, 1, 100000); 2],
            32 => vec![(9, 1, 1, 0)],
            33 => vec![(5, 0, 0, 100000)],
            35 => vec![(29, 1, 1, 100000)],
            _ => vec![(9, 1, 1, 100000)],
        };
        let mut frames = Vec::new();
        let mut controls = Vec::new();
        for (slot, payload, argument, limit) in requests {
            // The bridge supplies the source Position payload; publish this case's destination.
            let result = rcc_lifecycle_probe::dispatch_obstacle_transfer(
                &mut world,
                entity,
                slot,
                payload,
                argument,
                destination,
                limit,
            );
            let status = match result {
                Ok(v) => {
                    value = v;
                    0
                }
                Err(e) => e,
            };
            frames.push(json!({"entity":entity.0,"status":status,"object":world.identity.entities.record(entity).and_then(|r|r.object).and_then(|h|h.obstacle()).map(|v|v.0),"state":snapshot(&world,value)}));
            controls.push([
                17,
                slot as i32,
                i32::from(payload),
                argument as i32,
                limit as i32,
                entity.0 as i32,
            ]);
        }
        if matches!(
            branch,
            0 | 1
                | 2
                | 3
                | 10
                | 11
                | 12
                | 13
                | 14
                | 15
                | 16
                | 25
                | 26
                | 27
                | 28
                | 30
                | 33
                | 34
                | 35
        ) {
            assert!(
                frames.iter().all(|f| f["status"] == 0),
                "obstacle methods branch {branch}"
            );
        }
        if branch == 18 {
            assert_ne!(
                initial["colliders"][id.0],
                frames[0]["state"]["colliders"][id.0]
            );
            assert_eq!(frames[0]["status"], -1);
        }
        if branch == 19 || branch == 39 || branch == 40 {
            assert_eq!(frames[0]["status"], -3);
        }
        if branch == 39 || branch == 40 {
            assert_ne!(initial["grid"], frames[0]["state"]["grid"]);
        }
        let mut point = Vec::new();
        ints(&mut point, destination);
        let (scene, _) = scene_export::encode(&world.objects.world);
        let name = format!("scene-obstacle-transfer-{branch}.bin");
        fs::write(
            out.join(&name),
            scene
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        cases.push(json!({"tag":format!("obstacle-methods-{branch}"),"fixed_count":fixed,"generic":world.registry.generic.0,"obstacle":world.registry.obstacle.0,"world_class":world.registry.world.0,"projectile_class":world.objects.components.projectiles.class.unwrap().0,"resources_present":true,"homing_mesh":world.objects.components.projectiles.resources.unwrap().homing_mesh.index(),"launch":vec![0u16;93],"scene":name,"initial":initial,"controls":controls,"frames":frames,"transfer_position":point,"shape_present":shape}));
    }
    fs::write(out.join("scene-obstacle-transfer.json"),serde_json::to_vec(&json!({"cases":cases,"scope":"Actual obstacle MODE/COLLIDABLE/MOVE class dispatch: live ordered memberships, duplicate insert/remove, signed/world-boundary positions, resource-dependent shape radius, grid metadata and partial mutations before work-limit/component failure. UFO actor orchestration follows separately."})).unwrap()).unwrap();
}
