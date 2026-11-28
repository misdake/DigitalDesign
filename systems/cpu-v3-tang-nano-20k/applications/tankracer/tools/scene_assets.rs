//! Initial ENGLAND collision assets and independent ordered reference queries.
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fs,
    path::Path,
};
use tankracer_game::{
    object_query::{self as query, EntityId, Hit, ObstacleId},
    session::loading::{Files, Selection},
    world_startup::Constructed,
};
mod scene_bounce;
mod scene_export;
mod scene_hits;
#[allow(dead_code)]
#[path = "terrain_assets.rs"]
mod terrain_pages;
use scene_export::{append_i32, record};
fn binary(path: &Path, words: &[u16]) -> Result<(), Box<dyn Error>> {
    fs::write(
        path,
        words
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
    )?;
    Ok(())
}
fn main() -> Result<(), Box<dyn Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let root = Path::new(&args[0]);
    let out = Path::new(&args[1]);
    fs::create_dir_all(out)?;
    let names = [
        "DATA/FASTFILE.DAT",
        "TANKS/BRONZE/FASTFILE.DAT",
        "TANKS/SILVER/FASTFILE.DAT",
        "TANKS/GOLD/FASTFILE.DAT",
        "INGAME/FASTFILE.DAT",
        "LEVELS/ENGLAND/FASTFILE.DAT",
    ];
    let archives = names
        .iter()
        .map(|name| {
            let p = root.join("assets/game").join(name);
            if !p.is_file() {
                return Err(format!("Missing source {}", p.display()).into());
            }
            Ok(fs::read(p)?)
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    let vehicles = [
        archives[1].as_slice(),
        archives[2].as_slice(),
        archives[3].as_slice(),
    ];
    let constructed = Constructed::load(
        Files {
            data: &archives[0],
            vehicles: &vehicles,
            shared: &archives[4],
            level: &archives[5],
        },
        Selection::default_cup(0, 1)?,
    )?;
    let world = &constructed.world().objects.world;
    assert_eq!(world.colliders.len(), world.objects.len());
    assert!(world.objects.len() < 65535 && world.classes.len() < 65535);
    let (words, records) = scene_export::encode(world);
    binary(&out.join("scene.bin"), &words)?;
    binary(&out.join("records.bin"), &records)?;
    let mut controls = Vec::new();
    for index in [world.objects.len() - 1, 0]
        .into_iter()
        .chain(0..world.objects.len())
    {
        let c = &world.colliders[index];
        let pos = c.pose.position;
        for variant in 0..4 {
            let mut a = pos;
            let mut b = pos;
            if variant == 0 {
                a[0] = a[0].wrapping_add(10000);
                b[0] = b[0].wrapping_sub(10000);
            }
            if variant == 1 {
                a[2] = a[2].wrapping_add(10000);
                b[2] = b[2].wrapping_sub(10000);
            }
            if variant == 3 {
                a[1] = a[1].wrapping_add(4000);
                b[1] = b[1].wrapping_sub(4000);
            }
            controls.push((
                a,
                b,
                if variant == 3 {
                    c.entity
                } else {
                    EntityId(u32::MAX)
                },
                false,
                24u16,
            ));
        }
        if world.deformables.contains(&ObstacleId(index)) {
            controls.push((pos, pos, EntityId(u32::MAX), true, 21));
            let mut a = pos;
            let mut b = pos;
            a[0] = a[0].wrapping_add(20000);
            b[0] = b[0].wrapping_sub(20000);
            controls.push((a, b, EntityId(u32::MAX), true, 21));
        }
    }
    let mut inputs = Vec::new();
    let mut cases = Vec::<Value>::new();
    let mut hit_classes = BTreeMap::<usize, usize>::new();
    let mut fall_segments = Vec::new();
    let mut bounce_segments = Vec::new();
    let mut bounce_profiles = BTreeSet::new();
    let grid = query::Grid {
        origin: world.grid.origin,
        shifts: world.grid.shifts,
        cells: world.grid.cells.each_ref().map(|v| v.as_slice()),
    };
    for (a, b, entity, deform, slot) in controls {
        let mut hit = Hit {
            entity: None,
            object: None,
            code: 0,
            point: [11, 22, 33],
            normal: [-44, 55, -66],
        };
        let status = if deform {
            query::deformables(&world.deformables, &world.colliders, a, b, &mut hit)
        } else {
            query::obstacles(&grid, &world.colliders, entity, a, b, &mut hit)
        };
        let status = status.map_or_else(
            |e| if e == query::Error::Arithmetic { 3 } else { 4 },
            u16::from,
        );
        let mut expected = vec![
            hit.entity.map_or(-1, |e| e.0 as i32),
            hit.object.map_or(-1, |o| o.0 as i32),
            hit.code,
        ];
        expected.extend(hit.point);
        expected.extend(hit.normal);
        let mut extra = vec![0; 14];
        if let Some(id) = hit.object {
            let o = &world.objects[id.0];
            let class = &world.classes[o.class.0];
            let components = &constructed.world().objects.components;
            match components.bounces.get(id.0).and_then(Option::as_ref) {
                Some(c) if bounce_profiles.insert(format!("{:?}", c.config)) => {
                    bounce_segments.push((id.0, a, b));
                }
                _ => {}
            }
            if fall_segments.len() < 4
                && !fall_segments.iter().any(|(index, _, _)| *index == id.0)
                && components.falls[id.0]
                    .as_ref()
                    .is_some_and(|s| s.attached.is_none())
                && components.objects[id.0]
                    .as_ref()
                    .is_some_and(|s| s.data.auxiliary.is_none() && s.data.debris.is_none())
                && o.activity == 0
                && o.flags & 1 == 0
            {
                fall_segments.push((id.0, a, b));
            }
            *hit_classes.entry(o.class.0).or_default() += 1;
            extra[..5].copy_from_slice(&[
                o.class.0 as i32,
                o.weight,
                o.threshold,
                o.activity,
                o.flags as i32,
            ]);
            for (dst, v) in extra[5..11]
                .iter_mut()
                .zip(o.mesh_bounds.into_iter().flatten())
            {
                *dst = i32::from(v);
            }
            let (state, method) = if i32::from(slot) > class.maximum_slot {
                (2, 0)
            } else {
                match class.methods.get(slot as usize) {
                    Some(Some(id)) => (1, i32::from(id.0)),
                    Some(None) => (0, 0),
                    None => (4, 0),
                }
            };
            extra[11..].copy_from_slice(&[state, method, class.maximum_slot]);
        }
        expected.extend(extra);
        let mut control = Vec::new();
        for v in a
            .into_iter()
            .chain(b)
            .chain([entity.0 as i32, i32::from(deform)])
        {
            append_i32(&mut control, v);
        }
        control.push(slot);
        assert_eq!(control.len(), 17);
        assert_eq!(expected.len(), 23);
        inputs.extend(&control);
        inputs.push(status);
        for &v in &expected {
            append_i32(&mut inputs, v);
        }
        cases.push(json!({"control":control,"status":status,"expected":expected}));
    }
    binary(&out.join("queries.bin"), &inputs)?;
    let mut report = json!({"format":1,"object_count":world.objects.len(),"class_count":world.classes.len(),"deformable_count":world.deformables.len(),"scene_bytes":words.len()*2,"query_count":cases.len(),"query_words":64,"record_words":76,"hit_classes":hit_classes,"initialization_seed":constructed.world().shared.random,"scenery_classes":constructed.scenery().classes,"scope":"initial ENGLAND geometry, ordered queries/class slots and connected Fall/Bounce contact, motion and stop requests; full world scheduling, other methods and vehicles remain separate"});
    let (hit_count, tick_count, bounce_count) =
        scene_hits::export(constructed, &fall_segments, &bounce_segments, out)?;
    report["hit_count"] = json!(hit_count);
    report["fall_tick_count"] = json!(tick_count);
    report["bounce_update_count"] = json!(bounce_count);
    fs::write(
        out.join("scene.json"),
        serde_json::to_vec_pretty(&json!({"metadata":report,"cases":cases}))?,
    )?;
    println!("{report}");
    Ok(())
}
