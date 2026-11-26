//! Original UFO factory, target kinds, partial construction and shared retirement.
use super::{ints, scene_export, scene_mines_oracle};
use serde_json::json;
use std::{fs, path::Path, sync::Arc};
use tankracer_game::{
    object_query::EntityId, scene_entity::Handle, scene_objects::rcc_lifecycle_probe,
};

pub fn export(root: &Path, out: &Path) {
    let mut cases = Vec::new();
    for branch in 0..22 {
        let mut world = super::reference_world::full_race(root);
        let config = rcc_lifecycle_probe::ufo_config(&world);
        let class = config.0.unwrap();
        let resources = config.1.unwrap();
        let car = world.vehicles.entries[0].as_mut().unwrap();
        assert!(car.ai.is_none());
        if branch == 1 {
            car.physics.player = false;
        }
        if branch == 5 {
            car.physics.collision.disabled = true;
        }
        if branch == 6 {
            car.slot = 7;
        }
        if branch == 8 {
            car.physics.motion.poses.base_position[1] = i32::MAX - 12345;
        }
        let mut target = car.physics.entity;
        if matches!(branch, 2 | 3 | 4) {
            let id = tankracer_game::object_query::ObstacleId(169);
            target = world.objects.world.colliders[id.0].entity;
            if branch == 3 {
                world.objects.components.special.push(id);
            }
            if branch == 4 {
                for cell in &mut world.objects.world.grid.cells {
                    cell.retain(|v| *v != id);
                }
            }
        }
        if branch == 9 {
            target = EntityId(0xfedcba98);
        }
        if branch == 10 {
            rcc_lifecycle_probe::clear_ufo_config(&mut world, true);
        }
        if branch == 11 {
            rcc_lifecycle_probe::clear_ufo_config(&mut world, false);
        }
        if matches!(branch, 13 | 14 | 15 | 16) {
            let mut table = (*world.registry.classes[class.0]).clone();
            match branch {
                13 => table.methods[0] = None,
                14 => table.methods[1] = None,
                15 => table.methods[4] = None,
                _ => table.maximum_slot = 3,
            }
            let table = Arc::new(table);
            world.registry.classes[class.0] = table.clone();
            world.objects.world.classes[class.0] = table;
        }
        let initial = scene_mines_oracle::snapshot(&world);
        let fixed = world.objects.world.colliders.len();
        let mut cars = Vec::new();
        for i in 0..3 {
            let start = cars.len();
            if let Some(Some(car)) = world.vehicles.entries.get(i) {
                cars.extend([
                    1,
                    u16::from(car.physics.player),
                    u16::from(car.slot),
                    u16::from(car.physics.collision.disabled),
                ]);
                ints(&mut cars, car.physics.motion.poses.base_position);
                ints(&mut cars, car.physics.motion.poses.reference.0);
            }
            cars.resize(start + 32, 0);
        }
        let mut destination = Vec::new();
        ints(&mut destination, [12345, -23456, 34567]);
        let emitter = if branch == 7 {
            Some(EntityId(0x76543210))
        } else {
            None
        };
        let limit = if branch == 17 {
            0
        } else if branch == 18 {
            1
        } else {
            100000
        };
        let has_vehicles = branch != 12;
        let mut frames = Vec::new();
        let mut controls = Vec::new();
        let count = if branch == 0 { 3 } else { 1 };
        let mut last = EntityId(0);
        for _ in 0..count {
            let result = if branch == 20 {
                rcc_lifecycle_probe::create_weapon(&mut world, class, limit)
            } else {
                rcc_lifecycle_probe::spawn_ufo(
                    &mut world,
                    emitter,
                    target,
                    [12345, -23456, 34567],
                    limit,
                    has_vehicles,
                )
            };
            let (entity, status) = result.map_or_else(|e| (EntityId(0), e), |id| (id, 0));
            last = entity;
            let object = world
                .identity
                .entities
                .record(entity)
                .and_then(|r| r.object)
                .and_then(Handle::obstacle)
                .map(|id| id.0);
            frames.push(json!({"entity":entity.0,"status":status,"object":object,"state":scene_mines_oracle::snapshot(&world)}));
            controls.push(if branch == 20 {
                [0, class.0 as i32, 0, 0, limit as i32, 0]
            } else {
                [
                    14,
                    target.0 as i32,
                    i32::from(emitter.is_some()),
                    emitter.map_or(0, |v| v.0 as i32),
                    limit as i32,
                    0,
                ]
            });
        }
        if branch == 19 || branch == 21 {
            let slot = if branch == 19 { 3 } else { 8 };
            let result = rcc_lifecycle_probe::dispatch_weapon(&mut world, last, slot, 100000);
            let object = world
                .identity
                .entities
                .record(last)
                .and_then(|r| r.object)
                .and_then(Handle::obstacle)
                .map(|id| id.0);
            frames.push(json!({"entity":last.0,"status":result.map_or_else(|e|e,|()|0),"object":object,"state":scene_mines_oracle::snapshot(&world)}));
            controls.push([2, 0, slot as i32, last.0 as i32, 100000, 0]);
            if branch == 19 {
                let result = rcc_lifecycle_probe::spawn_ufo(
                    &mut world,
                    emitter,
                    target,
                    [12345, -23456, 34567],
                    100000,
                    true,
                );
                let (entity, status) = result.map_or_else(|e| (EntityId(0), e), |id| (id, 0));
                let object = world
                    .identity
                    .entities
                    .record(entity)
                    .and_then(|r| r.object)
                    .and_then(Handle::obstacle)
                    .map(|id| id.0);
                frames.push(json!({"entity":entity.0,"status":status,"object":object,"state":scene_mines_oracle::snapshot(&world)}));
                controls.push([14, target.0 as i32, 0, 0, 100000, 0]);
            }
        }
        if !matches!(branch, 9 | 10 | 11 | 12 | 13 | 16 | 17 | 18) {
            assert_eq!(frames[0]["status"], 0, "ufo branch {branch}");
        }
        let (scene, _) = scene_export::encode(&world.objects.world);
        let name = format!("scene-ufos-{branch}.bin");
        fs::write(
            out.join(&name),
            scene
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        cases.push(json!({"tag":format!("ufo-factory-{branch}"),"fixed_count":fixed,"generic":world.registry.generic.0,"obstacle":world.registry.obstacle.0,"world_class":world.registry.world.0,"projectile_class":world.objects.components.projectiles.class.unwrap().0,"resources_present":true,"homing_mesh":world.objects.components.projectiles.resources.unwrap().homing_mesh.index(),"launch":vec![0u16;93],"scene":name,"initial":initial,"controls":controls,"frames":frames,"ufo_cars":cars,"ufo_class":rcc_lifecycle_probe::ufo_config(&world).0.map_or(-1,|v|v.0 as i32),"ufo_resources_present":rcc_lifecycle_probe::ufo_config(&world).1.is_some(),"ufo_mesh":resources.mesh.index(),"ufo_material":resources.material,"vehicles_present":has_vehicles,"destination":destination}));
    }
    fs::write(out.join("scene-ufos.json"),serde_json::to_vec(&json!({"cases":cases,"scope":"Actual UFO class construction, live vehicle/obstacle target resolution, emitter and mesh publication, source memberships and default state, player flags, partial construction and shared retirement/reuse. Transfer Tick and attached projectile integration follow separately."})).unwrap()).unwrap();
}
