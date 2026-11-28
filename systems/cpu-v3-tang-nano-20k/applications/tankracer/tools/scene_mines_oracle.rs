//! Real-map mine placement, shared memberships, limit retirement and reuse.
use super::{scene_creation_oracle, scene_export};
use serde_json::{Value, json};
use std::{fs, path::Path};
use tankracer_game::{
    mine::Owner, object_query::EntityId, scene_entity::Handle, scene_objects::rcc_lifecycle_probe,
    vehicle_runtime::SinglePlayer,
};
pub(super) fn snapshot(world: &SinglePlayer) -> Value {
    let mut value = scene_creation_oracle::snapshot(world);
    value["grid"] = json!(
        world
            .objects
            .world
            .grid
            .cells
            .iter()
            .map(|cell| cell.iter().map(|v| v.0 as i32).collect::<Vec<_>>())
            .collect::<Vec<_>>()
    );
    value["mine_order"] = json!(
        world
            .objects
            .components
            .mines
            .order
            .iter()
            .map(|v| v.0 as i32)
            .collect::<Vec<_>>()
    );
    value["mine_count"] = json!(world.objects.components.mines.count);
    value
}
pub(super) fn bindings(
    world: &SinglePlayer,
    owner: Owner,
    resources: tankracer_game::mine::Resources,
    mut value: Value,
) -> Value {
    let mut packed = Vec::new();
    super::ints(&mut packed, resources.mesh.map(|v| v.index() as i32));
    packed.extend(resources.radius);
    packed.extend(
        resources
            .bounds
            .into_iter()
            .flatten()
            .flatten()
            .map(|v| v as u16),
    );
    value["mine_resources"] = json!(packed);
    let mut packed = Vec::new();
    super::ints(&mut packed, [owner.entity.0 as i32]);
    super::ints(&mut packed, owner.position);
    value["mine_owner"] = json!(packed);
    value["mine_class"] = json!(
        world
            .objects
            .components
            .mines
            .class
            .map_or(-1, |v| v.0 as i32)
    );
    value["mine_resources_present"] = json!(world.objects.components.mines.resources.is_some());
    value["frame_group"] = json!(world.objects.components.frame_group);
    value["generic"] = json!(world.registry.generic.0);
    value["obstacle"] = json!(world.registry.obstacle.0);
    value["world_class"] = json!(world.registry.world.0);
    value["projectile_class"] = json!(world.objects.components.projectiles.class.unwrap().0);
    value["resources_present"] = json!(true);
    value["homing_mesh"] = json!(
        world
            .objects
            .components
            .projectiles
            .resources
            .unwrap()
            .homing_mesh
            .index()
    );
    value["launch"] = json!(vec![0u16; 93]);
    value
}
pub fn export(root: &Path, out: &Path) {
    let mut cases = Vec::new();
    for branch in 0..10 {
        let mut world = super::reference_world::race(root);
        let resources = world.objects.components.mines.resources.unwrap();
        let class = world.objects.components.mines.class.unwrap();
        if branch == 1 {
            let entity = rcc_lifecycle_probe::create_weapon(&mut world, class, 100000).unwrap();
            rcc_lifecycle_probe::release_transient(&mut world, entity).unwrap();
        }
        let car = world.vehicles.entries[0].as_ref().unwrap();
        let mut owner = Owner {
            entity: car.physics.entity,
            position: car.physics.motion.poses.base_position,
        };
        if branch == 1 {
            let data = world.terrain();
            let mut found = false;
            for cell in 0..64 {
                let x =
                    (data.origin[0] + ((cell % 8) << data.shifts[0]) + (1 << (data.shifts[0] - 1)))
                        * 8;
                let z =
                    (data.origin[1] + ((cell / 8) << data.shifts[1]) + (1 << (data.shifts[1] - 1)))
                        * 8;
                let mut height = 0;
                let mut normal = [0; 3];
                if tankracer_game::terrain::normal_height(data, x, z, &mut height, &mut normal)
                    .unwrap()
                    && (normal[0] != 0 || normal[2] != 0)
                {
                    owner.position = [x, height, z];
                    found = true;
                    break;
                }
            }
            assert!(found, "real ENGLAND slope");
        }
        if branch == 6 {
            owner.position = [i32::MAX, 100, i32::MAX];
        }
        if branch == 9 {
            world.objects.components.frame_group = u32::MAX - 2;
        }
        if branch == 2 {
            world.objects.components.mines.resources = None;
        }
        if branch == 4 {
            world.objects.components.mines.class = None;
        }
        if branch == 5 {
            rcc_lifecycle_probe::clear_terrain(&mut world.objects.components);
        }
        let initial = snapshot(&world);
        let fixed = world.objects.world.colliders.len();
        let mut operations = Vec::new();
        let count = if branch == 0 || branch == 7 {
            32
        } else if branch == 1 || branch == 8 {
            4
        } else {
            2
        };
        for i in 0..count {
            operations.push([
                4,
                if branch == 2 || branch == 3 { 2 } else { i % 2 },
                i % 2,
                0,
                if branch == 7 { 1 } else { 100000 },
            ]);
        }
        let mut frames = Vec::new();
        let mut controls = Vec::new();
        let mut entities = Vec::new();
        let mut expanded = false;
        let mut step = 0;
        while step < operations.len() {
            let c = operations[step];
            let (entity, result) = if c[0] == 4 {
                let result = rcc_lifecycle_probe::spawn_mine(
                    &mut world,
                    owner,
                    c[2] != 0,
                    c[1] as u32,
                    c[4] as u32,
                );
                let entity = result.as_ref().map_or(0, |v| v.0);
                if result.is_ok() {
                    entities.push(entity);
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
                .and_then(|v| v.object)
                .and_then(Handle::obstacle)
                .map(|v| v.0);
            frames.push(json!({"entity":entity.0,"status":result.map_or_else(|v|v,|_|0),"object":object,"state":snapshot(&world)}));
            controls.push(c);
            step += 1;
            if !expanded && step == operations.len() && !entities.is_empty() {
                expanded = true;
                // The first two mines already deactivated at the thirty-mine limit.
                let start = if branch == 0 { 2 } else { 0 };
                for &entity in &entities[start..] {
                    if branch == 8 {
                        operations.push([2, 0, 7, entity as i32, 100000]);
                        operations.push([2, 0, 6, entity as i32, 100000]);
                    }
                    operations.push([2, 0, 8, entity as i32, 100000]);
                    operations.push([2, 0, 2, entity as i32, 100000]);
                    operations.push([2, 0, 3, entity as i32, 100000]);
                }
                if branch == 1 {
                    operations.push([4, 0, 0, 0, 100000]);
                }
            }
        }
        if branch == 0 {
            assert_eq!(frames[31]["state"]["mine_count"], 30);
            assert_eq!(
                frames[31]["state"]["mine_order"].as_array().unwrap().len(),
                30
            );
        }
        if branch == 5 {
            assert!(frames.iter().all(|v| v["status"] == json!(-1)));
        }
        if branch == 6 {
            assert!(frames.iter().all(|v| v["status"] == json!(-4)));
        }
        if branch == 7 {
            assert_eq!(frames[30]["status"], -3);
            assert_eq!(frames[30]["state"]["mine_count"], 31);
            assert_eq!(frames[31]["status"], -3);
            assert_eq!(frames[31]["state"]["mine_count"], 32);
        }
        let (scene, _) = scene_export::encode(&world.objects.world);
        let name = format!("scene-mines-{branch}.bin");
        fs::write(
            out.join(&name),
            scene
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        cases.push(bindings(&world,owner,resources,json!({"tag":format!("mine-factory-{branch}"),"fixed_count":fixed,"scene":name,"initial":initial,"controls":controls,"frames":frames,"terrain_present":branch!=5})));
    }
    fs::write(out.join("scene-mines.json"),serde_json::to_vec(&json!({"cases":cases,"scope":"Original complete mine factory on ENGLAND terrain: constructors, height/normal, memberships, mesh/bounds, activation, thirty-mine limit, leave/reentry/deactivate/destroy/release, fixed/transient reuse and committed failure prefixes; repeat and hit/explosion scheduling follow separately"})).unwrap()).unwrap();
}
