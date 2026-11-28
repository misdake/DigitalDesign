//! Shared blast allocation, pool aliases, time edges and derived geometry.
use super::ints;
use serde_json::{Value, json};
use std::{fs, path::Path};
use tankracer_game::{
    effect_pool::{self, Pool, Quad, QuadHandle},
    explosion::blasts::{self, Blasts, Released, Start},
    math::Matrix,
    object_query::EntityId,
    vehicle_runtime::SinglePlayer,
};
fn quad(out: &mut Vec<u16>, q: Quad) {
    ints(out, q.points.into_iter().flatten());
    ints(out, [q.material as i32]);
    for bytes in q.style.compat_bytes().chunks_exact(4) {
        ints(out, [i32::from_le_bytes(bytes.try_into().unwrap())]);
    }
}
fn line(out: &mut Vec<u16>, l: effect_pool::Line) {
    ints(out, l.points.into_iter().flatten());
    ints(out, l.colors.map(i32::from_le_bytes));
}
pub(super) fn snapshot(b: &Blasts, pool: &Pool, seed: u32) -> Vec<u16> {
    let mut out = Vec::new();
    ints(&mut out, b.times.into_iter().flatten());
    ints(&mut out, b.next.map(|v| v as i32));
    ints(&mut out, b.count.map(|v| v as i32));
    ints(&mut out, [b.total as i32]);
    for row in b.bands {
        ints(&mut out, [row.inner, row.outer]);
        ints(&mut out, row.origin);
        ints(
            &mut out,
            [
                i32::from(row.owner.is_some()),
                row.owner.map_or(0, |v| v.0 as i32),
                i32::from(row.flipped),
            ],
        );
        ints(&mut out, row.handles.map(|v| v.0 as i32));
    }
    for row in b.rings {
        ints(&mut out, [row.life]);
        ints(&mut out, row.origin);
        ints(
            &mut out,
            [
                i32::from(row.owner.is_some()),
                row.owner.map_or(0, |v| v.0 as i32),
                i32::from(row.kind),
                i32::from(row.alpha),
                i32::from(row.rotation),
            ],
        );
        for r in row.rays {
            ints(
                &mut out,
                [r.handle.0 as i32, r.radius, r.width, r.zero, r.angle as i32],
            );
        }
    }
    assert_eq!(out.len(), 374);
    for q in &pool.quads[..159] {
        quad(&mut out, *q);
    }
    let mut last = Vec::new();
    quad(&mut last, pool.quads[159]);
    out.extend(&last[..20]);
    quad(&mut out, pool.exhausted_quad);
    ints(&mut out, [pool.quad_count as i32, pool.line_count as i32]);
    for l in pool.lines {
        line(&mut out, l);
    }
    line(&mut out, pool.overflow);
    out.extend(&last[20..]);
    for c in pool.exhausted_colors {
        ints(&mut out, [i32::from_le_bytes(c)]);
    }
    ints(&mut out, [seed as i32]);
    assert_eq!(out.len(), 7152);
    out
}
fn run(
    tag: String,
    mut b: Blasts,
    mut pool: Pool,
    mut seed: u32,
    camera: Matrix,
    origin: [i32; 3],
    controls: Vec<[i32; 6]>,
) -> Value {
    let mut input = snapshot(&b, &pool, seed);
    let materials = b.materials.unwrap_or(blasts::Materials {
        band: 0,
        ring: [0; 3],
    });
    ints(
        &mut input,
        [i32::from(b.materials.is_some()), materials.band as i32],
    );
    ints(&mut input, materials.ring.map(|v| v as i32));
    ints(&mut input, camera.0);
    ints(&mut input, origin);
    assert_eq!(input.len(), 7186);
    let mut frames = Vec::new();
    for c in &controls {
        let start = Start {
            origin,
            owner: EntityId(c[5] as u32),
            time: c[4],
            camera,
        };
        let r: Result<(u32, Option<Released>), i32> = match c[0] {
            0 => b.band(start, c[2], c[3] != 0, &mut pool),
            1 => b.ring(start, c[1] as u8, &mut seed, &mut pool),
            2 => b
                .tick_band(c[1] as u32, c[2], &mut pool)
                .map(|r| (u32::MAX, r)),
            3 => b
                .tick_ring(c[1] as u32, c[2], &mut seed, &mut pool)
                .map(|r| (u32::MAX, r)),
            4 => b
                .release(c[3] != 0, c[1] as u32 as usize, &mut pool)
                .map(|r| (u32::MAX, Some(r))),
            _ => Ok((u32::MAX, None)),
        };
        let (status, slot, released) = match r {
            Ok((s, r)) => (0, s, r),
            Err(e) => (e, u32::MAX, None),
        };
        let mut frame = Vec::new();
        ints(&mut frame, [status, slot as i32]);
        frame.push(u16::from(released.is_some()));
        ints(&mut frame, [released.map_or(0, |r| r.owner.0 as i32)]);
        frame.push(u16::from(released.is_some_and(|r| r.ring)));
        assert_eq!(frame.len(), 8);
        frame.extend(snapshot(&b, &pool, seed));
        match b.project(camera) {
            Ok(geometry) => {
                ints(&mut frame, [geometry.len() as i32]);
                for q in geometry {
                    quad(&mut frame, q);
                }
            }
            Err(e) => ints(&mut frame, [e]),
        }
        frames.push(frame);
    }
    json!({"tag":tag,"input":input,"controls":controls,"frames":frames})
}
pub fn export(out: &Path, world: &SinglePlayer) {
    let materials = world.objects.components.blasts.materials;
    assert!(materials.is_some());
    let origin = world.vehicles.entries[0]
        .as_ref()
        .unwrap()
        .physics
        .motion
        .poses
        .base_position;
    let camera = Matrix::from_angles([1300, 19000, 2300]);
    let mut cases = Vec::new();
    for variant in 0..28 {
        let mut b = Blasts {
            materials,
            ..Default::default()
        };
        let mut pool = Pool::default();
        let mut seed = world.shared.random ^ variant;
        let mut camera = camera;
        let mut origin = origin;
        let mut controls = vec![
            [0, 0, 20480, 0, 1, 101],
            [0, 0, 32000, 1, 2, 102],
            [1, 1, 0, 0, 3, 103],
            [1, 2, 0, 0, 4, 104],
            [1, 3, 0, 0, 5, 105],
            [0, 0, 2000, 1, 6, 106],
            [2, 0, 16, 0, 0, 0],
            [3, 0, 16, 0, 0, 0],
            [3, 1, 250, 0, 0, 0],
        ];
        let start = Start {
            origin,
            owner: EntityId(77),
            time: 1,
            camera,
        };
        match variant {
            0 => {}
            1 => {
                controls = vec![
                    [1, 1, 0, 0, 1, 11],
                    [1, 2, 0, 0, 2, 12],
                    [1, 3, 0, 0, 3, 13],
                    [3, 0, 16, 0, 0, 0],
                    [3, 1, 249, 0, 0, 0],
                    [3, 1, 1, 0, 0, 0],
                ]
            }
            2 => pool.quad_count = 160,
            3 | 20 => {
                for q in &mut pool.quads {
                    q.points[0][1] = 0;
                }
                pool.quad_count = if variant == 3 { 0 } else { 161 };
            }
            4 | 21 => {
                for _ in 0..if variant == 4 { 155 } else { 159 } {
                    pool.allocate_quad(Quad::ZERO);
                }
            }
            5 => {
                b.materials = None;
                controls = vec![
                    [0, 0, 5000, 0, 1, 1],
                    [1, 1, 0, 0, 1, 1],
                    [1, 0, 0, 0, 1, 1],
                    [1, 4, 0, 0, 1, 1],
                ];
            }
            6 => controls = vec![[1, 0, 0, 0, 1, 1], [1, 4, 0, 0, 1, 1], [1, 255, 0, 0, 1, 1]],
            7 | 27 => {
                controls = vec![
                    [0, 0, 20480, 0, 0, 1],
                    [0, 0, 20480, 1, 0, 2],
                    [0, 0, 20480, 0, 0, 3],
                    [1, 2, 0, 0, 0, 4],
                ]
            }
            8 => {
                controls = vec![
                    [0, 0, 5000, 0, i32::MIN, 1],
                    [1, 2, 0, 0, i32::MAX, 2],
                    [1, 3, 0, 0, -1, 3],
                    [0, 0, 5000, 0, 0, 4],
                ]
            }
            9 => {
                controls = vec![
                    [4, -1, 0, 0, 0, 0],
                    [4, 2, 0, 1, 0, 0],
                    [4, 0, 0, 0, 0, 0],
                    [2, 2, 16, 0, 0, 0],
                    [3, -1, 16, 0, 0, 0],
                ]
            }
            10 | 12 | 19 => {
                b.band(start, 20480, false, &mut pool).unwrap();
                if variant == 10 {
                    b.bands[0].handles[3] = QuadHandle(999);
                }
                if variant == 12 {
                    b.bands[0].owner = None;
                }
                controls = vec![
                    [4, 0, 0, 0, 0, 0],
                    [4, 0, 0, 0, 0, 0],
                    [0, 0, 10000, 1, 2, 22],
                ];
            }
            11 | 16 | 17 | 26 => {
                b.ring(start, 2, &mut seed, &mut pool).unwrap();
                if variant == 11 {
                    b.rings[0].rays[6].handle = QuadHandle(0xfeb4024d);
                }
                if variant == 16 {
                    b.rings[0].life = i32::MIN;
                }
                controls = vec![
                    [3, 0, if variant == 17 { i32::MIN } else { 1 }, 0, 0, 0],
                    [3, 0, 250, 0, 0, 0],
                ];
                if variant == 26 {
                    b.rings[0].alpha = 129;
                    b.rings[0].rotation = 65535;
                    pool.quad_count = 160;
                    b.next[1] = 0;
                    controls = vec![[1, 1, 0, 0, 2, 2], [3, 0, 0, 0, 0, 0]];
                }
            }
            13 => {
                b.band(Start { time: 2, ..start }, 20480, false, &mut pool)
                    .unwrap();
                b.ring(start, 1, &mut seed, &mut pool).unwrap();
                b.next[0] = u32::MAX;
                controls = vec![[0, 0, 10000, 0, 3, 3], [1, 2, 0, 0, 4, 4]];
            }
            14 => {
                b.total = u32::MAX;
                b.count = [u32::MAX; 2];
                controls = vec![
                    [0, 0, 5000, 0, 1, 1],
                    [2, 0, 250, 0, 0, 0],
                    [4, 0, 0, 0, 0, 0],
                ];
            }
            15 => {
                b.bands[0].inner = i32::MAX;
                b.bands[0].outer = i32::MIN;
                controls = vec![[2, 0, 1, 0, 0, 0], [2, 0, i32::MIN, 0, 0, 0]];
            }
            18 => {
                controls = vec![
                    [1, 1, 0, 0, -7, 1],
                    [0, 0, 5000, 1, -7, 2],
                    [1, 3, 0, 0, 1, 3],
                    [0, 0, 5000, 0, 2, 4],
                ]
            }
            22 => {
                b.times = [[1, 2], [3, 4]];
                b.total = 4;
                b.count = [2, 2];
                for row in &mut b.bands {
                    row.inner = 12345;
                    row.outer = 30000;
                    row.origin = origin;
                }
                for row in &mut b.rings {
                    row.life = 249;
                    row.kind = 2;
                    row.origin = origin;
                }
                controls = vec![[5, 0, 0, 0, 0, 0]];
            }
            23 | 24 | 25 => {
                b.band(start, 20480, variant == 24, &mut pool).unwrap();
                b.ring(Start { time: 2, ..start }, 3, &mut seed, &mut pool)
                    .unwrap();
                b.bands[0].inner = i32::MIN;
                b.bands[0].outer = i32::MAX;
                b.rings[0].life = -12345;
                for ray in &mut b.rings[0].rays {
                    ray.width = i32::MIN;
                    ray.radius = i32::MAX;
                    ray.angle = 65535;
                }
                if variant == 25 {
                    camera = Matrix([i32::MAX, i32::MIN, -1, 0, 4096, 1, i32::MIN, 0, i32::MAX]);
                    origin = [i32::MAX, i32::MIN, -1];
                }
                controls = vec![
                    [5, 0, 0, 0, 0, 0],
                    [3, 0, -12346, 0, 0, 0],
                    [2, 0, -1, 0, 0, 0],
                ];
            }
            _ => unreachable!(),
        }
        cases.push(run(
            format!("blasts-{variant}"),
            b,
            pool,
            seed,
            camera,
            origin,
            controls,
        ));
    }
    fs::write(out.join("explosion-blasts.json"),serde_json::to_vec(&json!({"cases":cases,"scope":"Original shared blast allocation/eviction/release/tick with complete pools, RNG, failure prefixes and read-only additive projection. Actual explosion owner callbacks and mixed native world scheduling follow separately."})).unwrap()).unwrap();
}
