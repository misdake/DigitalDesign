//! Real ENGLAND ordinary destruction through the original shared factory.
use serde_json::{json, Value};
use std::{fs, path::Path};
mod identity_assets;
#[allow(dead_code)]
mod reference_world;
mod scene_export;
mod scene_lifecycle_assets;
fn main() {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let out = Path::new(&args[0]);
    let root = Path::new(&args[1]);
    let mut session = reference_world::complete(root);
    let world = session.world_mut_at_load().unwrap();
    let (scene, _) = scene_export::encode(&world.objects.world);
    let bytes = scene
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect::<Vec<_>>();
    if out.join("car-deform-scene.bin").is_file() {
        assert_eq!(fs::read(out.join("car-deform-scene.bin")).unwrap(), bytes);
    }
    fs::write(out.join("scene-lifecycle.bin"), bytes).unwrap();
    let ordinary = (0..world.objects.world.objects.len())
        .filter_map(|id| {
            let r = scene_lifecycle_assets::row(world, id);
            if (r[0] == 1 || r[0] == 2) && r[7] & 65532 == 0 {
                Some((id, r[0]))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    let mut counts = [0; 3];
    let mut cases = Vec::<Value>::new();
    for (id, kind) in ordinary {
        counts[kind as usize] += 1;
        let variants = if counts[kind as usize] == 1 { 11 } else { 1 };
        for variant in 0..variants {
            let limits: &[u32] = if variants == 1 {
                &[100000]
            } else {
                &[0, 1, 2, 4, 100000]
            };
            for &limit in limits {
                for action in 0..3 {
                    let mut session = reference_world::complete(root);
                    let world = session.world_mut_at_load().unwrap();
                    scene_lifecycle_assets::prepare(world, id, action, variant);
                    let input =
                        scene_lifecycle_assets::input(world, id, action, limit, scene.len());
                    let status = scene_lifecycle_assets::run(world, id, action, limit);
                    let mut expected = vec![status];
                    expected.extend(scene_lifecycle_assets::state(world, id));
                    assert!(input.len() * 2 < 4096 && expected.len() * 2 < 4094);
                    cases.push(
                        json!({"kind":33,"input":input,"expected":expected,"status":0,
                        "object":id,"tag":kind,"variant":variant,"action":action,"limit":limit}),
                    );
                }
            }
        }
    }
    assert_eq!(counts[1], 84);
    assert_eq!(counts[2], 29);
    let supported = (0..world.objects.world.objects.len())
        .filter(|id| {
            let class = world.objects.world.objects[*id].class;
            class == world.registry.fall
                || class == world.registry.bounce
                || class == world.registry.closer
        })
        .collect::<Vec<_>>();
    let mut kinds = std::collections::BTreeSet::new();
    let mut completions = 0;
    for id in supported {
        let r = scene_lifecycle_assets::row(world, id);
        let class = world.objects.world.objects[id].class;
        let first = kinds.insert((r[0], r[7] & 12, class == world.registry.closer));
        let variants = if first { 12 } else { 1 };
        for variant in 0..variants {
            let limits: &[u32] = if first {
                &[0, 1, 2, 4, 100000]
            } else {
                &[100000]
            };
            for &limit in limits {
                for action in [3, 4] {
                    let mut session = reference_world::complete(root);
                    let current = session.world_mut_at_load().unwrap();
                    scene_lifecycle_assets::prepare(current, id, action, variant);
                    let input =
                        scene_lifecycle_assets::input(current, id, action, limit, scene.len());
                    let status = scene_lifecycle_assets::run(current, id, action, limit);
                    let mut expected = vec![status];
                    expected.extend(scene_lifecycle_assets::state(current, id));
                    assert!(input.len() * 2 < 4096 && expected.len() * 2 < 4094);
                    cases.push(json!({"kind":33,"input":input,"expected":expected,"status":0,
                        "object":id,"tag":r[0],"variant":variant,"action":action,"limit":limit,"completion":true}));
                    completions += 1;
                }
            }
        }
    }
    fs::write(
        out.join("scene-lifecycle.json"),
        serde_json::to_vec(&cases).unwrap(),
    )
    .unwrap();
    println!(
        "ordinary lifecycle: {counts:?}, {} cases ({completions} completion dispatches)",
        cases.len()
    );
}
