//! Original terrain-mark queue, full shared pool and allocation aliases.
use super::{explosion_blasts_oracle, ints};
use serde_json::json;
use std::{fs, path::Path};
use tankracer_assets::terrain::TerrainData;
use tankracer_game::{
    effect_pool::{Pool, Quad, QuadHandle},
    explosion::blasts::Blasts,
    projectile::marks::Marks,
    terrain,
};
fn snapshot(marks: &Marks, pool: &Pool) -> Vec<u16> {
    let mut words = Vec::new();
    ints(
        &mut words,
        [marks.count as i32, marks.head as i32, marks.next as i32],
    );
    ints(&mut words, marks.handles.map(|v| v.0 as i32));
    assert_eq!(words.len(), 54);
    words.extend(&explosion_blasts_oracle::snapshot(&Blasts::default(), pool, 0)[374..7150]);
    assert_eq!(words.len(), 6830);
    words
}
pub(super) fn category(data: &TerrainData, origin: [i32; 3]) -> u8 {
    let mut a = terrain::Hit::default();
    let mut b = terrain::Hit::default();
    let first = match terrain::query(
        data,
        origin[0].wrapping_sub(1024),
        origin[2].wrapping_sub(1024),
        &mut a,
    ) {
        Ok(v) => v,
        Err(_) => return 5,
    };
    if !first {
        return 0;
    }
    if a.flags & 16 != 0 {
        return 1;
    }
    let second = match terrain::query(
        data,
        origin[0].wrapping_add(1024),
        origin[2].wrapping_add(1024),
        &mut b,
    ) {
        Ok(v) => v,
        Err(_) => return 6,
    };
    if !second {
        return 2;
    }
    if b.flags & 16 != 0 {
        return 3;
    }
    4
}
pub fn export(root: &Path, out: &Path) {
    let world = super::reference_world::race(root);
    let data = world.terrain();
    let mut positions = [None; 7];
    let base = world.vehicles.entries[0]
        .as_ref()
        .unwrap()
        .physics
        .motion
        .poses
        .base_position;
    for dx in -10..=10 {
        for dz in -10..=10 {
            let p = [base[0] + dx * 512, 0, base[2] + dz * 512];
            positions[category(data, p) as usize].get_or_insert(p);
        }
    }
    for mesh in &data.meshes {
        for face in &mesh.triangles {
            let vertices = face.indices.map(|i| mesh.vertices[usize::from(i)]);
            let x = vertices.iter().map(|v| i32::from(v[0])).sum::<i32>() / 3;
            let z = vertices.iter().map(|v| i32::from(v[2])).sum::<i32>() / 3;
            for dx in [-1024, 0, 1024] {
                for dz in [-1024, 0, 1024] {
                    let p = [
                        (mesh.position[0] + x) * 8 + dx,
                        0,
                        (mesh.position[2] + z) * 8 + dz,
                    ];
                    positions[category(data, p) as usize].get_or_insert(p);
                }
            }
        }
    }
    let origin = positions[4].expect("real ENGLAND mark surface");
    let material = world
        .objects
        .components
        .projectiles
        .resources
        .unwrap()
        .crater;
    let mut cases = Vec::new();
    for branch in 0..14 {
        let mut marks = Marks::default();
        let mut pool = Pool::default();
        if matches!(branch, 3 | 4 | 5 | 6 | 11) {
            for _ in 0..if branch == 3 { 23 } else { 24 } {
                marks.add(origin, material, data, &mut pool).unwrap();
            }
        }
        if matches!(branch, 2 | 3 | 4) {
            pool.quad_count = 160;
        }
        if branch == 5 {
            marks.handles[marks.head] = QuadHandle(161);
        }
        if branch == 6 {
            marks.handles[marks.head] = QuadHandle::EXHAUSTED;
        }
        if branch == 7 {
            for _ in 0..158 {
                pool.allocate_quad(Quad::ZERO);
            }
        }
        if branch == 8 {
            for quad in &mut pool.quads {
                quad.points[0][1] = 0;
            }
            pool.quad_count = 0;
        }
        if branch == 11 {
            marks.head = 23;
            marks.next = 23;
        }
        if branch == 13 {
            marks.count = u32::MAX;
        }
        let input = snapshot(&marks, &pool);
        let mut controls = Vec::new();
        let mut frames = Vec::new();
        for step in 0..if matches!(branch, 0 | 1 | 7 | 8 | 11) {
            32
        } else {
            2
        } {
            let p = if branch == 9 {
                [i32::MAX, 0, i32::MIN]
            } else if branch == 10 {
                positions[0].unwrap_or([i32::MAX, 0, i32::MIN])
            } else if branch == 1 {
                [origin[0] + 3, step - 111, origin[2] - 5]
            } else {
                origin
            };
            let material = if branch == 12 { u32::MAX } else { material };
            let status = marks
                .add(p, material, data, &mut pool)
                .map_or_else(|e| e, |()| 0);
            let mut answer = Vec::new();
            ints(&mut answer, [status]);
            answer.extend(snapshot(&marks, &pool));
            frames.push(answer);
            controls.push([p[0], p[1], p[2], material as i32]);
        }
        if branch == 0 || branch == 11 {
            assert_eq!(marks.count, 24);
        }
        if branch == 2 {
            assert_eq!(marks.count, 0);
        }
        if branch == 5 {
            assert!(frames.iter().all(|f| f[..2] == [65535, 65535]));
        }
        cases.push(json!({"tag":format!("marks-{branch}"),"input":input,"controls":controls,"frames":frames}));
    }
    for (class, p) in positions.into_iter().enumerate() {
        if class == 4 {
            continue;
        }
        let Some(p) = p else {
            continue;
        };
        let mut marks = Marks::default();
        let mut pool = Pool::default();
        let input = snapshot(&marks, &pool);
        let status = marks
            .add(p, material, data, &mut pool)
            .map_or_else(|e| e, |()| 0);
        assert_eq!(marks.count, 0);
        let mut answer = Vec::new();
        ints(&mut answer, [status]);
        answer.extend(snapshot(&marks, &pool));
        cases.push(json!({"tag":format!("marks-surface-{class}"),"input":input,"controls":[[p[0],p[1],p[2],material as i32]],"frames":[answer]}));
    }
    fs::write(out.join("projectile-marks.json"),serde_json::to_vec(&json!({"cases":cases,"scope":"Original 24-entry crater queue on real ENGLAND triangles: both corner queries and exclusion flags, signed geometry, material blend/UV, ring wrap, exhaustion, shared aliases and release failure. No frame scheduling or projectile impact dispatch is included."})).unwrap()).unwrap();
}
