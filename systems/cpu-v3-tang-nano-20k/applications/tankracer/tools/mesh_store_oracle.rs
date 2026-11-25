//! Original resource operations with complete current geometry and slot snapshots.
mod mesh_catalog_assets;
mod mesh_store_snapshot;
#[allow(dead_code)]
mod reference_world;
use mesh_store_snapshot::{payload, snapshot};
use serde_json::json;
use std::{fs, path::Path};
use tankracer_assets::{mesh::Geometry, model::Model};
use tankracer_game::scene_meshes::{rcc_probe as probe, Meshes};

fn execute(meshes: &mut Meshes, models: &[Model], op: [u16; 4]) -> i32 {
    let [kind, a, b, c] = op;
    let id = probe::id(a as usize);
    match kind {
        0 => meshes
            .bind(
                ["aux.sm", "other.sm", "AUX.SM"][b as usize],
                &models[a as usize],
            )
            .index() as i32,
        1 => meshes.private_copy(id).map_or(-1, |m| m.index() as i32),
        2 => meshes.release_private(id).map_or(-1, |()| 1),
        3 => meshes.get_mut(id).map_or(-1, |g| {
            if let Some(v) = g.vertices.first_mut() {
                v[0] = b as i16;
            }
            1
        }),
        4 => meshes.fragments(id).map_or(-1, |mesh| {
            *mesh.radius = b;
            if let Some(f) = mesh.geometry.triangles.first_mut() {
                f.flags ^= c;
            }
            if let Some(v) = mesh.centers[0].as_mut().and_then(|c| c.first_mut()) {
                *v = v.wrapping_sub(c as i16);
            }
            1
        }),
        5 => meshes.set_hint(id, b).map_or(-1, |()| 1),
        6 => meshes.insert(models[a as usize].geometry.clone()).index() as i32,
        7 => {
            probe::version(
                meshes,
                id,
                if b == 0 {
                    u64::MAX
                } else {
                    u64::from(u32::MAX)
                },
            );
            1
        }
        8 => meshes.radius(id).map_or(-1, i32::from),
        _ => unreachable!(),
    }
}
fn main() {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let out = Path::new(&args[0]);
    let root = Path::new(&args[1]);
    let (mut session, resources) = reference_world::constructed(root).into_session().unwrap();
    let world = session.world();
    let mut models = Vec::new();
    let mut names = Vec::new();
    for object in world.objects.components.objects.iter().flatten() {
        if let Some(aux) = &object.data.auxiliary {
            let key = aux.shape_name.to_ascii_lowercase();
            if !names.contains(&key) {
                models.push((*resources.auxiliary_model(&aux.shape_name).unwrap()).clone());
                names.push(key);
            }
        }
    }
    mesh_catalog_assets::export(session.world_mut_at_load().unwrap(), &models, &names, out);
    let authored = models.len();
    let mut empty = models[0].clone();
    empty.geometry = Geometry {
        vertices: vec![],
        triangles: vec![],
    };
    empty.face_centers = [None, None, None];
    empty.materials.clear();
    models.push(empty);
    let mut partial = models[0].clone();
    partial.face_centers[1] = None;
    partial.face_centers[2] = Some(vec![]);
    models.push(partial);
    let ops = vec![
        [0, 0, 0, 0],
        [0, 1, 2, 0],
        [0, 1, 1, 0],
        [3, 0, 60000, 0],
        [1, 0, 0, 0],
        [4, 2, 5120, 257],
        [3, 0, 12345, 0],
        [5, 0, 65535, 0],
        [1, 0, 0, 0],
        [2, 0, 0, 0],
        [2, 2, 0, 0],
        [2, 2, 0, 0],
        [3, 2, 55, 0],
        [4, 2, 64, 4],
        [7, 2, 0, 0],
        [6, 1, 0, 0],
        [8, 2, 0, 0],
        [2, 3, 0, 0],
        [2, 2, 0, 0],
        [1, 1, 0, 0],
        [1, 0, 0, 0],
        [2, 2, 0, 0],
        [6, 0, 0, 0],
        [1, 65535, 0, 0],
        [3, 65535, 55, 0],
        [2, 65535, 0, 0],
        [4, 65535, 16, 4],
        [5, 65535, 17, 0],
    ];
    let detached = vec![
        [0, 0, 0, 0],
        [1, 0, 0, 0],
        [4, 1, 5120, 257],
        [1, 1, 0, 0],
        [2, 1, 0, 0],
        [6, 1, 0, 0],
        [8, 2, 0, 0],
        [5, 1, 17, 0],
        [2, 2, 0, 0],
        [1, 1, 0, 0],
    ];
    let mut tables = vec![json!([])];
    let mut cases = Vec::new();
    for case in 0..models.len() + 2 {
        let mut i = case;
        if case == models.len() {
            i = 0;
        } else if case > models.len() {
            i = models.len() - 1;
        }
        let actions = if case < models.len() { &ops } else { &detached };
        let pair = [models[i].clone(), models[(i + 1) % models.len()].clone()];
        let packets = pair.each_ref().map(|m| payload(m, &mut tables));
        let slot_words = pair
            .iter()
            .map(|m| {
                m.geometry.vertices.len() * 3
                    + m.geometry.triangles.len()
                    + m.face_centers
                        .iter()
                        .flatten()
                        .map(Vec::len)
                        .max()
                        .unwrap_or(0)
                        * 3
            })
            .max()
            .unwrap() as u16;
        let mut input = vec![0, slot_words, 4, actions.len() as u16, 7, 0, 0];
        input.extend(&packets[0]);
        input[5] = input.len() as u16;
        input.extend(&packets[1]);
        input[6] = input.len() as u16;
        input.extend(actions.iter().flatten());
        let mut meshes = Meshes::default();
        let mut frames = Vec::new();
        for op in actions {
            let result = execute(&mut meshes, &pair, *op);
            frames.push(snapshot(
                &meshes,
                result,
                &mut tables,
                ["aux.sm", "other.sm"],
            ));
        }
        cases.push(json!({"input":input,"frames":frames,"model":i,"name":names.get(i)}));
    }
    fs::write(out.join("mesh-store.json"),serde_json::to_vec(&json!({"cases":cases,"authored_models":authored,"names":names,
        "scope":"Original bind/current private_copy/insert/release/get_mut/fragments/set_hint on complete ENGLAND auxiliary geometry, stable slots, name aliases, revisions and immutable material identities"})).unwrap()).unwrap();
    println!(
        "{} mesh-store sequences over {} distinct authored models",
        cases.len(),
        authored
    );
}
