//! Synchronous effect-slot writes before successful or failing auxiliary queries.
use super::ints;
#[path = "sampling_snapshot.rs"]
mod sampling_snapshot;
use serde_json::json;
use std::{fs, path::Path};
use tankracer_assets::terrain::{
    CollisionMesh, CollisionTriangle, NodeId, Span, TerrainData, TerrainNode,
};
use tankracer_game::{contact::Coefficient, contact_pipeline as ground, sampling::Transform};

fn terrain(variant: usize) -> TerrainData {
    TerrainData {
        source_root: 1,
        origin: [-32768; 2],
        shifts: [13; 2],
        cells: [Span {
            start: 0,
            length: 3,
        }; 64],
        node_refs: vec![NodeId(0), NodeId(1), NodeId(2)],
        nodes: (0..3)
            .map(|i| TerrainNode {
                source_offset: 256 + i * 32,
                mesh: i,
                shifts: [0; 2],
                bins: [0; 64],
                candidates: Span {
                    start: 0,
                    length: 2,
                },
                half_bounds: [1000; 2],
                surface: i as u16 + 1,
                extra: [0; 4],
                has_owner: false,
            })
            .collect(),
        meshes: (0..3)
            .map(|i| CollisionMesh {
                source_offset: 1024 + i * 256,
                position: [i as i32 * 5000, 100, 0],
                flags: 0,
                radius: 1500,
                vertices: vec![[-1000, 0, -1000], [1000, 0, -1000], [-1000, 0, 1000]],
                triangles: vec![CollisionTriangle {
                    source_offset: 4096 + i * 24,
                    normal: [
                        0,
                        if (variant == 1 && i == 2) || (variant == 2 && i == 0) {
                            0
                        } else {
                            4096
                        },
                        0,
                    ],
                    indices: [0, 1, 2],
                    flags: 0,
                }],
                face_centers: vec![],
            })
            .collect(),
        triangle_refs: vec![0, 255],
        fallback: Default::default(),
    }
}
fn snapshot(s: &ground::Sampling, data: &TerrainData, seed: u32, tails: [u32; 2]) -> Vec<u16> {
    let mut out = Vec::new();
    ints(&mut out, sampling_snapshot::values(s, data));
    ints(&mut out, [seed as i32, tails[0] as i32, tails[1] as i32]);
    out
}
pub fn export(out: &Path) {
    for variant in 0..3 {
        let data = terrain(variant);
        let mut cells = super::pages::cell_bytes(&data, 28);
        cells.extend(super::pages::cell_bytes(&data, 29));
        fs::write(
            out.join(format!("contact-effect-cells-{variant}.bin")),
            cells,
        )
        .unwrap();
    }
    let mut cases = Vec::new();
    for branch in 0..9 {
        let variant = if branch == 2 || branch == 8 {
            1
        } else if branch == 3 {
            2
        } else {
            0
        };
        let data = terrain(variant);
        let gains = [Coefficient::default(); 10];
        let world = ground::World {
            terrain: &data,
            surface_bumps: &gains,
            noise: [128; 16],
        };
        let mut env = ground::Environment {
            tracks: [
                Transform {
                    basis: [4096, 0, 0, 0, 4096, 0, 0, 0, 4096],
                    origin: [0, 1600, -4000],
                },
                Transform {
                    basis: [4096, 0, 0, 0, 4096, 0, 0, 0, 4096],
                    origin: [40000, 1600, -4000],
                },
            ],
            auxiliary_local: [[80000, 0, 0], [40000, 0, 0]],
            force_surface: branch == 8,
            effect_gain: Coefficient::default(),
            load: Default::default(),
            dt: 40,
            frame: 0,
            player: if branch == 1 {
                0
            } else if branch == 4 || branch == 7 {
                2
            } else {
                1
            },
        };
        let mut s = ground::Sampling {
            effect: ground::SurfaceEffect {
                timer: if branch == 5 {
                    0
                } else if branch == 6 {
                    80
                } else {
                    1
                },
                restore: 314,
                active: 1,
                phase: 201,
                impulse: [73, -49],
                pose: 22,
                clear_player_slot: true,
            },
            materials: [7, 8],
            fallback_mode: 9,
            feedback_required: true,
            clear_player_slot: Some(1),
            ..Default::default()
        };
        for (i, p) in s.primary.points.iter_mut().enumerate() {
            p.gap = 50 + i as i32;
            p.previous_gap = -40;
            p.travel = 100 + i as i32;
            p.normal = [3, 4, 5];
        }
        let mut seed = 0x12345678u32;
        let mut tails = [0x12345678, 0x87654321];
        let enabled = branch != 7;
        let input = snapshot(&s, &data, seed, tails);
        let tracks: Vec<_> = env
            .tracks
            .iter()
            .flat_map(|t| t.basis.into_iter().chain(t.origin))
            .collect();
        let mut controls = Vec::new();
        let mut frames = Vec::new();
        for tick in 0..3 {
            env.frame = 4 + tick;
            controls.push([
                env.dt,
                env.frame as i32,
                env.player as i32,
                i32::from(env.force_surface),
            ]);
            let status = match ground::rcc_probe::sample(
                &mut s, &world, &env, &mut seed, &mut tails, enabled,
            ) {
                Ok(()) => 0,
                Err(ground::Error::Terrain(tankracer_game::terrain::Error::Arithmetic)) => 2,
                Err(ground::Error::Terrain(tankracer_game::terrain::Error::InvalidBucket)) => 3,
                Err(ground::Error::MissingSurface(_)) => 5,
                Err(ground::Error::EffectSlotWrite(_)) => 6,
            };
            if tick == 0 {
                if branch == 2 {
                    assert_eq!(status, 2);
                    assert_eq!(tails[1], 0);
                    assert_ne!(seed, 0x12345678);
                }
                if branch == 3 {
                    assert_eq!(status, 2);
                    assert_eq!(tails, [0x12345678, 0x87654321]);
                    assert_eq!(seed, 0x12345678);
                }
                if branch == 4 {
                    assert_eq!(status, 6);
                }
                if branch == 8 {
                    assert_eq!(status, 0);
                    assert_eq!(tails[1], 0);
                }
            }
            let expected_page = 32 + u16::from(status != 6 && branch != 3 && branch != 8);
            let mut frame = vec![status as u16, expected_page];
            frame.extend(snapshot(&s, &data, seed, tails));
            frames.push(frame);
        }
        cases.push(json!({"tag":format!("contact-effect-{branch}"),"variant":variant,"input":input,"controls":controls,"frames":frames,"tracks":tracks,"auxiliary":env.auxiliary_local,"enabled":enabled}));
    }
    fs::write(out.join("contact-effect.json"),serde_json::to_vec(&json!({"cases":cases,"scope":"Original Sampling::sample_with_effects on diagnostic terrain planes, including an auxiliary arithmetic failure after slot clearing and a primary failure before it. Complete partial sampling state, seed and shared tail writes after each call."})).unwrap()).unwrap();
}
