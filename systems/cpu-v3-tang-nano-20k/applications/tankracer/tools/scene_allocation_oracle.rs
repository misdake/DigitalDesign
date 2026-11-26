//! Stateful original transient allocation, ID growth, release and LIFO reuse.
use super::{identity_assets, scene_export, scene_lifecycle_assets};
use serde_json::json;
use std::{fs, path::Path};
use tankracer_game::{
    dispatch::ClassId, object_query::EntityId, scene_objects::rcc_lifecycle_probe,
    vehicle_runtime::SinglePlayer,
};

fn record(world: &SinglePlayer, index: usize) -> Vec<u16> {
    let mut row = scene_export::record(&world.objects.world, index);
    if row[58] == 0 && row[59] == 0 {
        row[26..44].fill(0);
    }
    row
}
pub(super) fn snapshot(world: &SinglePlayer) -> serde_json::Value {
    let lists = scene_lifecycle_assets::lists(world);
    json!({"identity":identity_assets::encode(&world.identity.entities),
        "flags":world.identity.ticks.flags,"slot":world.identity.current_slot,"random":world.shared.random,
        "next":world.identity.ticks.next.map_or(-1,|v|v.0 as i32),
        "colliders":(0..world.objects.world.colliders.len()).map(|i|record(world,i)).collect::<Vec<_>>(),
        "records":(0..world.objects.world.colliders.len()).map(|i|scene_lifecycle_assets::row(world,i)).collect::<Vec<_>>(),
        "component_count":world.objects.components.objects.len(),"lists":lists})
}
fn new(world: &mut SinglePlayer, class: Option<ClassId>) -> EntityId {
    let entity = world.identity.entities.create_record(100000).unwrap();
    world.identity.entities.record_mut(entity).unwrap().class = class;
    entity
}
fn allocate(world: &mut SinglePlayer) -> EntityId {
    let class = world.objects.components.projectiles.class;
    let entity = new(world, class);
    rcc_lifecycle_probe::allocate_transient(world, entity).unwrap();
    entity
}
pub fn export(root: &Path, out: &Path) {
    let mut cases = Vec::new();
    for branch in 0..8 {
        let mut world = super::reference_world::race(root);
        if branch == 1 || branch == 2 || branch == 6 {
            let a = allocate(&mut world);
            let b = allocate(&mut world);
            rcc_lifecycle_probe::release_transient(&mut world, a).unwrap();
            rcc_lifecycle_probe::release_transient(&mut world, b).unwrap();
        }
        if branch == 3 || branch == 6 {
            world.objects.components.objects.pop();
        }
        if branch == 5 {
            world.identity.entities.sequential = true;
            world.identity.entities.next_requested =
                EntityId(world.identity.entities.capacity() - 1);
        }
        let initial = snapshot(&world);
        let fixed = world.objects.world.colliders.len();
        assert!(fixed <= 273);
        let class = world.objects.components.projectiles.class.unwrap();
        let mut controls = Vec::new();
        let mut frames = Vec::new();
        let mut created = Vec::new();
        let count = if branch == 0 {
            140
        } else if branch == 7 {
            219
        } else {
            4
        };
        for step in 0..count {
            let release = (branch == 2 && step >= 2) || (branch == 7 && (215..217).contains(&step));
            let absent = branch == 4;
            let (entity, status, id) = if release {
                let entity = created[if branch == 7 { 429 - step } else { step - 2 }];
                let status = rcc_lifecycle_probe::release_transient(&mut world, entity)
                    .map_or_else(|v| v, |()| 0);
                (entity, status, None)
            } else {
                let entity = new(&mut world, if absent { None } else { Some(class) });
                let result = rcc_lifecycle_probe::allocate_transient(&mut world, entity);
                created.push(entity);
                (
                    entity,
                    result.as_ref().map_or_else(|&v| v, |_| 0),
                    result.ok().map(|id| id.0),
                )
            };
            controls.push([
                i32::from(release),
                class.0 as i32,
                i32::from(absent),
                entity.0 as i32,
            ]);
            frames.push(
                json!({"entity":entity.0,"status":status,"object":id,"state":snapshot(&world)}),
            );
        }
        let (scene, _) = scene_export::encode(&world.objects.world);
        let scene_file = format!("scene-allocation-{branch}.bin");
        fs::write(
            out.join(&scene_file),
            scene
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        cases.push(json!({"tag":format!("transient-{branch}"),"fixed_count":fixed,"generic":world.registry.generic.0,"obstacle":world.registry.obstacle.0,"scene":scene_file,"initial":initial,"controls":controls,"frames":frames}));
    }
    fs::write(out.join("scene-allocation.json"),serde_json::to_vec(&json!({"cases":cases,"scope":"Original allocation::allocate_object transient path, identity creation/growth, actual release methods, shared component rows and full collider/property/list snapshots; weapon component allocation and initialization follow separately"})).unwrap()).unwrap();
}
