//! Original pickup light banks and their shared quad/line allocation state.
use serde_json::json;
use std::{fs, path::Path};
use tankracer_game::{
    effect_pool::{self, Quad},
    math::Matrix,
    object_query::EntityId,
    vehicle_scene::pickup::{self, Camera, Materials, Start, Tuning},
};
mod pickup_snapshot;
use pickup_snapshot::values;
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let out = Path::new(&args[1]);
    let mut cases = Vec::new();
    for variant in 0..24 {
        let mut s = pickup::State::default();
        let mut pool = effect_pool::Pool::default();
        let camera = Camera {
            position: [-125, 300, 500],
            matrix: Matrix::from_angles([1200, 9000, 3400]),
        };
        let materials = Materials {
            rays: 0xf0000042,
            rings: 0x80000035,
        };
        let tuning = Tuning::new(937);
        if variant == 1 {
            pool.quad_count = 160;
        }
        if variant == 2 {
            for q in &mut pool.quads {
                q.points[0][1] = 0;
            }
            pool.quad_count = 0;
        }
        if variant == 3 {
            for _ in 0..155 {
                pool.allocate_quad(Quad::ZERO);
            }
        }
        if variant == 4 {
            s.banks[0].frame = u32::MAX;
            s.banks[1].frame = u32::MAX;
        }
        let target = EntityId(42);
        let origin = [10000, 4000, 15000];
        let velocity = [20000, -40000, 150000];
        if variant >= 5 {
            s.start(
                Start {
                    target,
                    origin,
                    velocity,
                    frame: 11,
                    follow: true,
                },
                materials,
                camera,
                &tuning,
                &mut pool,
            )
            .unwrap();
            match variant {
                5 => {}
                6 => {
                    s.banks[0].rays[3].handle = effect_pool::QuadHandle(999);
                }
                7 => {
                    s.banks[0].rings[4].handle = effect_pool::QuadHandle(999);
                }
                8 => {
                    s.banks[0].rays[2].palette_index = 10;
                }
                9 => {
                    s.banks[0].rays[9].palette_index = u32::MAX;
                }
                10 => {
                    s.banks[0].outer_radius = 25600;
                    s.banks[0].inner_radius = 25600;
                }
                11 => {
                    s.banks[0].life = 1;
                }
                12 => {
                    s.banks[0].frame = u32::MAX;
                    s.banks[1].frame = u32::MAX;
                }
                13 => {
                    s.banks[0].follow = [0, 1];
                    s.banks[0].speed = i32::MAX;
                }
                14 => {
                    s.banks[0].position = [i32::MIN, 0, i32::MIN];
                }
                15 => {
                    s.banks[0].rays[0].handle = effect_pool::QuadHandle::RELEASED;
                }
                _ => {}
            }
        }
        let mut input = values(&s, &pool);
        input.extend(camera.position);
        input.extend(camera.matrix.0);
        input.extend([
            materials.rays as i32,
            materials.rings as i32,
            tuning.outer_growth,
            tuning.inner_growth,
            tuning.flare_growth,
        ]);
        input.extend(tuning.palette.into_iter().flatten().map(i32::from));
        assert_eq!(input.len(), 4251);
        let mut controls = Vec::new();
        let mut frames = Vec::new();
        for step in 0..12 {
            let op = if variant < 5 {
                if step % 4 == 0 {
                    0
                } else if step % 4 == 3 {
                    2
                } else {
                    1
                }
            } else if variant == 6 || variant == 7 || variant == 15 {
                0
            } else if variant >= 16 {
                if step % 3 == 0 {
                    3
                } else if step % 3 == 1 {
                    4
                } else {
                    5
                }
            } else {
                1
            };
            let op = if variant == 23 && step == 6 { 6 } else { op };
            let mut dt = 33;
            if variant == 17 {
                dt = -33;
            }
            if variant == 18 {
                dt = 1000;
            }
            if variant == 19 {
                dt = i32::MAX;
            }
            let frame = if step == 0 { 11 } else { 12 + step };
            let target = if step == 6 { EntityId(99) } else { target };
            let position = if variant == 14 {
                [0, 4000, 0]
            } else {
                [10000 + step * 200, 4000 - step * 100, 15000]
            };
            let mut control = vec![op, dt, frame, target.0 as i32];
            control.extend(position);
            control.extend(velocity);
            control.push(if variant == 20 { 0 } else { 1 });
            let status = match op {
                0 => s.start(
                    Start {
                        target,
                        origin: position,
                        velocity,
                        frame: frame as u32,
                        follow: control[10] != 0,
                    },
                    materials,
                    camera,
                    &tuning,
                    &mut pool,
                ),
                1 => s.fade(target, position, frame as u32, dt, &tuning, &mut pool),
                2 => s.project(target, frame as u32, camera, &mut pool),
                3 => {
                    let colors = [[7, 8, 9, 10], [11, 12, 13, 14]];
                    let _ = pool.update(
                        effect_pool::LineHandle::EXHAUSTED,
                        [position, velocity],
                        colors,
                    );
                    Ok(())
                }
                4 => pool
                    .replace_quad(
                        effect_pool::QuadHandle(159),
                        Quad {
                            points: [position; 4],
                            material: 0xf000aaaa,
                            style: effect_pool::Style::new(
                                effect_pool::QuadBlend::Other(7),
                                [3, 8, 9],
                                [[4, 8]; 4],
                            ),
                        },
                    )
                    .map_err(|_| -1),
                5 => pool
                    .release_quad(effect_pool::QuadHandle(if variant == 21 {
                        160
                    } else if variant == 22 {
                        u32::MAX
                    } else {
                        999
                    }))
                    .map_err(|_| -1),
                _ => {
                    s = pickup::State::default();
                    pool.reset();
                    Ok(())
                }
            }
            .map_or_else(|e| e, |()| 0);
            let mut answer = vec![status];
            answer.extend(values(&s, &pool));
            frames.push(answer);
            controls.push(control);
        }
        cases.push(json!({"tag":format!("light-{variant}"),"input":input,"controls":controls,"frames":frames}));
    }
    fs::write(
        out.join("pickup-light.json"),
        serde_json::to_vec(&json!({"cases":cases})).unwrap(),
    )
    .unwrap();
}
