//! Exact fragment activation and updates from authored and boundary geometry.
#[allow(dead_code)]
mod reference_world;
use serde_json::{json, Value};
use std::{fs, path::Path};
use tankracer_assets::{
    config::Gain,
    mesh::{Geometry, Triangle},
};
use tankracer_game::{math::Matrix, mesh_break, obstacle_startup::Auxiliary};

mod fragment_snapshot;
use fragment_snapshot::Fixture;
fn triangle(indices: [u8; 3]) -> Triangle {
    Triangle {
        source_offset: 0,
        normal: [0; 3],
        color: 0,
        indices,
        sides: 0,
        uv: [[0; 2]; 3],
        flags: 0,
        material: None,
        material_source_offset: 0,
        billboard: None,
    }
}
fn synthetic() -> Fixture {
    Fixture {
        state: mesh_break::State::new(3),
        geometry: Geometry {
            vertices: vec![
                [0, 100, 0],
                [50, 100, 0],
                [0, 100, 50],
                [50, 100, 50],
                [100, 200, 0],
                [150, 200, 0],
                [100, 200, 50],
            ],
            triangles: vec![
                triangle([0, 1, 2]),
                triangle([2, 1, 3]),
                triangle([4, 5, 6]),
            ],
        },
        centers: [
            Some(vec![16, 33, 116]),
            Some(vec![100, 100, 200]),
            Some(vec![16, 33, 16]),
        ],
        radius: 128,
        config: Auxiliary {
            shape_name: "fixture".into(),
            gravity: 150,
            bounce: Gain {
                multiplier: 500,
                shift: 10,
            },
            friction: Gain {
                multiplier: 800,
                shift: 10,
            },
            push: Gain {
                multiplier: 150,
                shift: 10,
            },
            velocity: Gain {
                multiplier: 1,
                shift: 13,
            },
            spin: Gain {
                multiplier: 1,
                shift: 16,
            },
        },
        seed: 0x181293fa,
        velocity: Some([250000, -150000, 500000]),
        basis: Some(Matrix::IDENTITY),
        height: 800,
        position: [100, 200, -300],
        tail: None,
    }
}
fn record(cases: &mut Vec<Value>, f: &mut Fixture, op: i32, limit: u32, dt: i32, tag: &str) -> i32 {
    let input = f.input(op, limit, dt);
    let status = f.run(op, limit, dt);
    let expected = f.output(status);
    assert!(
        input.len() < 4096 && expected.len() < 2048,
        "fixture too large: {tag}"
    );
    cases.push(json!({"input":input,"expected":expected,"tag":tag}));
    status
}
fn main() {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let out = Path::new(&args[0]);
    let root = Path::new(&args[1]);
    let mut cases = Vec::<Value>::new();
    for variant in 0..20 {
        let mut base = synthetic();
        match variant {
            1 => base.velocity = None,
            2 => base.basis = None,
            3 => base.velocity = Some([i32::MIN, i32::MAX, -1]),
            4 => base.basis = Some(Matrix::from_angles([7000, 5000, 3000])),
            5 => base.seed = u32::MAX,
            6 => base.state.lifetime = i32::MAX,
            7 => base.geometry.triangles[1].indices = [0, 1, 2],
            8 => {
                base.geometry.triangles[1].indices = [0, 1, 2];
                base.tail = Some(3);
            }
            9 => base.geometry.triangles[1].indices = [4, 5, 6],
            10 => {
                base.config.spin.multiplier = 65535;
                base.config.spin.shift = 255;
                base.config.velocity.multiplier = 65535;
            }
            11 => {
                base.state.faces.pop();
            }
            12 => base.state.prepared = true,
            13 => base.state.retired = u32::MAX,
            14 => base.config.gravity = i32::MIN,
            15 => base.centers[1] = None,
            16 => base.centers[2] = Some(vec![]),
            17 => base.geometry.triangles[0].indices = [0, 1, 255],
            18 => {
                base.geometry.vertices[0] = [i16::MIN, i16::MAX, -1];
                base.config.bounce.multiplier = 65535;
                base.config.friction.shift = 63;
            }
            _ => (),
        }
        for limit in [0u32, 1, 2, 3, 4, 5, 100000, u32::MAX] {
            record(
                &mut cases,
                &mut base.clone(),
                0,
                limit,
                33,
                &format!("activate-{variant}-{limit}"),
            );
        }
        let mut live = base.clone();
        let status = live.run(0, 100000, 33);
        if status == 0 {
            for dt in [0, 1, 33, -33, i32::MAX, i32::MIN] {
                for limit in [0u32, 1, 2, 100000, u32::MAX] {
                    record(
                        &mut cases,
                        &mut live.clone(),
                        1,
                        limit,
                        dt,
                        &format!("tick-{variant}-{limit}-{dt}"),
                    );
                }
            }
            for _ in 0..12 {
                record(
                    &mut cases,
                    &mut live,
                    1,
                    100000,
                    256,
                    &format!("sequence-{variant}"),
                );
            }
        }
    }
    for variant in 0..12 {
        let mut f = synthetic();
        for (i, face) in f.state.faces.iter_mut().enumerate() {
            face.mode = 2;
            face.life = if variant == 0 {
                0
            } else if variant == 1 {
                1
            } else {
                2000
            };
            face.velocity = [i32::MAX, if variant == 2 { i32::MIN } else { -1000 }, -3000];
            face.angular = [-1000, 2000, 3000];
            face.extra_vertex = -1;
            if variant == 3 && i == 0 {
                face.mode = 0;
            }
        }
        match variant {
            4 => f.geometry.triangles.clear(),
            5 => f.centers[0] = None,
            6 => f.centers[1] = None,
            7 => f.centers[2] = None,
            8 => {
                f.state.faces[2].mode = 0;
                f.state.faces[2].life = 1;
            }
            9 => {
                f.state.faces[2].mode = 0;
                f.state.faces[2].life = 2000;
            }
            10 => f.geometry.vertices.clear(),
            11 => {
                f.state.retired = 0;
                for face in &mut f.state.faces {
                    face.life = 1;
                }
            }
            _ => (),
        }
        record(
            &mut cases,
            &mut f,
            1,
            100000,
            33,
            &format!("failure-{variant}"),
        );
    }
    let construction = reference_world::constructed(root);
    let mut inventory = Vec::new();
    for (index, object) in construction
        .world()
        .objects
        .components
        .objects
        .iter()
        .enumerate()
    {
        let Some(config) = object.as_ref().and_then(|o| o.data.auxiliary.as_ref()) else {
            continue;
        };
        let model = construction
            .scenery()
            .auxiliary_model(&config.shape_name)
            .unwrap_or_else(|| panic!("missing original auxiliary {}", config.shape_name));
        let count = model.geometry.triangles.len();
        inventory.push(json!({"object":index,"name":config.shape_name,"faces":count,"vertices":model.geometry.vertices.len()}));
        let base = Fixture {
            state: mesh_break::State::new(count),
            geometry: model.geometry.clone(),
            centers: model.face_centers.clone(),
            radius: model.radius,
            config: config.clone(),
            ..synthetic()
        };
        for limit in [0u32, 1, count as u32, count as u32 + 1, 100000] {
            record(
                &mut cases,
                &mut base.clone(),
                0,
                limit,
                33,
                &format!("authored-activate-{index}-{limit}"),
            );
        }
        let mut live = base;
        if record(
            &mut cases,
            &mut live,
            0,
            100000,
            33,
            &format!("authored-activate-{index}"),
        ) == 0
        {
            for frame in 0..16 {
                record(
                    &mut cases,
                    &mut live,
                    1,
                    100000,
                    256,
                    &format!("authored-tick-{index}-{frame}"),
                );
            }
        }
    }
    assert!(!inventory.is_empty());
    fs::write(out.join("mesh-break.json"),serde_json::to_vec(&json!({"cases":cases,"authored":inventory,
        "scope":"Original mesh_break activation/tick with exact RNG and committed failure prefixes, real ENGLAND auxiliary meshes and mutated boundary fixtures"})).unwrap()).unwrap();
    println!(
        "{} mesh-fragment cases, {} authored auxiliary owners",
        cases.len(),
        inventory.len()
    );
}
