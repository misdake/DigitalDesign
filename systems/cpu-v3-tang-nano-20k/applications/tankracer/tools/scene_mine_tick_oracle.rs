//! Original complete mine ticks with live owner lookup and nested child creation.
use super::{scene_export, scene_mines_oracle as shared};
use serde_json::json;
use std::{fs, path::Path, sync::Arc};
use tankracer_game::{
    mine::Owner, object_query::EntityId, scene_entity::Handle, scene_objects::rcc_lifecycle_probe,
};
pub fn export(root: &Path, out: &Path) {
    let mut cases = Vec::new();
    for branch in 0..20 {
        let mut world = super::reference_world::race(root);
        let resources = world.objects.components.mines.resources.unwrap();
        let class = world.objects.components.mines.class.unwrap();
        let car = world.vehicles.entries[0].as_ref().unwrap();
        let owner = Owner {
            entity: car.physics.entity,
            position: car.physics.motion.poses.base_position,
        };
        if branch == 1 {
            let entity = rcc_lifecycle_probe::create_weapon(&mut world, class, 100000).unwrap();
            rcc_lifecycle_probe::release_transient(&mut world, entity).unwrap();
        }
        if branch == 10 || branch == 11 || branch == 17 || branch == 19 {
            for _ in 0..29 {
                rcc_lifecycle_probe::spawn_mine(&mut world, owner, false, 0, 100000).unwrap();
            }
        }
        let entity = if branch == 4 {
            rcc_lifecycle_probe::create_weapon(&mut world, class, 100000).unwrap()
        } else {
            rcc_lifecycle_probe::spawn_mine(
                &mut world,
                owner,
                branch != 2 && branch != 18,
                if branch == 2 { 1 } else { 0 },
                100000,
            )
            .unwrap()
        };
        let id = world
            .identity
            .entities
            .record(entity)
            .unwrap()
            .object
            .and_then(Handle::obstacle)
            .unwrap();
        // Repeat drops read physical_position, independently of the initial drop/base.
        let position = [
            owner.position[0] + 4096,
            owner.position[1] + 2000,
            owner.position[2] - 2048,
        ];
        world.vehicles.entries[0]
            .as_mut()
            .unwrap()
            .physics
            .motion
            .poses
            .physical_position = position;
        if branch == 3 {
            world.vehicles.entries[0] = None;
        }
        if branch == 5 {
            rcc_lifecycle_probe::set_mine_owner(&mut world, id, Some(EntityId(0x12345678)));
        }
        if branch == 6 {
            world.objects.components.mines.resources = None;
        }
        if branch == 7 {
            world.objects.components.mines.class = None;
        }
        if branch == 8 {
            rcc_lifecycle_probe::clear_terrain(&mut world.objects.components);
        }
        if branch == 9 {
            rcc_lifecycle_probe::set_mine_kind(&mut world, id, 2);
        }
        if branch == 14 {
            world.identity.entities.detach(owner.entity).unwrap();
        }
        if branch == 17 || branch == 19 {
            let oldest = world.objects.components.mines.order[0];
            world.identity.ticks.next = Some(world.objects.world.colliders[oldest.0].entity);
        }
        if branch == 18 {
            world.identity.ticks.next = Some(entity);
        }
        if branch == 15 || branch == 16 {
            let mut table = (*world.registry.classes[class.0]).clone();
            if branch == 15 {
                table.methods[12] = None;
            } else {
                table.maximum_slot = 11;
            }
            let table = Arc::new(table);
            world.registry.classes[class.0] = table.clone();
            world.objects.world.classes[class.0] = table;
        }
        let fixed = world.objects.world.colliders.len();
        let initial = shared::snapshot(&world);
        let times: Vec<i32> = if branch == 2 || branch == 18 {
            vec![20000, 1]
        } else if branch == 4 {
            vec![1, -1, 251, 20001]
        } else if branch == 12 {
            vec![0, -1, i32::MAX, i32::MIN, 20001]
        } else {
            vec![250, 1, 251, 251, 19749, 1]
        };
        let mut controls = Vec::new();
        let mut frames = Vec::new();
        for (step, dt) in times.into_iter().enumerate() {
            let number = if branch == 13 {
                u32::MAX.wrapping_sub(2).wrapping_add(step as u32)
            } else {
                step as u32
            };
            let limit = if branch == 11 || branch == 19 {
                1
            } else {
                100000
            };
            let result = rcc_lifecycle_probe::tick_mine(&mut world, entity, dt, number, limit);
            controls.push([5, dt, number as i32, entity.0 as i32, limit as i32]);
            frames.push(json!({"entity":entity.0,"status":result.map_or_else(|v|v,|_|0),"object":id.0,"state":shared::snapshot(&world)}));
        }
        if branch == 0 || branch == 1 {
            assert_eq!(frames[1]["state"]["mine_count"], 2);
            assert_eq!(frames[2]["state"]["mine_count"], 3);
            assert_eq!(frames[5]["state"]["mine_count"], 2);
        }
        if branch == 11 || branch == 19 {
            assert_eq!(frames[1]["status"], -3);
            assert_eq!(frames[1]["state"]["slot"], 8);
            assert_eq!(frames[1]["state"]["mine_count"], 31);
        }
        if branch == 17 || branch == 18 || branch == 19 {
            assert!(frames.iter().all(|f| f["state"]["next"] == initial["next"]));
        }
        let (scene, _) = scene_export::encode(&world.objects.world);
        let name = format!("scene-mine-tick-{branch}.bin");
        fs::write(
            out.join(&name),
            scene
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let mut vehicle_position = Vec::new();
        super::ints(&mut vehicle_position, position);
        cases.push(shared::bindings(&world,owner,resources,json!({"tag":format!("mine-tick-{branch}"),"fixed_count":fixed,"scene":name,"initial":initial,"controls":controls,"frames":frames,"terrain_present":branch!=8,"vehicle_position":vehicle_position,"vehicle_present":branch!=3,"has_vehicles":true,"vehicle_count":1})));
    }
    fs::write(out.join("scene-mine-tick.json"),serde_json::to_vec(&json!({"cases":cases,"scope":"Original complete mine Tick: source dt/owner order, live physical vehicle view, nested repeat child factory, retirement, shared memberships/identity/RNG and committed lookup/resource/limit failure prefixes; hit/explosion and complete mixed world scheduling remain separate"})).unwrap()).unwrap();
}
