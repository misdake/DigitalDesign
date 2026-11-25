//! Pure collision answers obtained from the unmodified reference Rust functions.
#[path = "impact_snapshot.rs"]
mod impact_snapshot;
use impact_snapshot::snapshot as impact_snapshot;
use serde_json::{json, Value};
use std::{fs, path::PathBuf};
use tankracer_assets::mesh::{Geometry, Triangle};
use tankracer_game::{
    body,
    collision::{self, Scale, Trajectory},
    damage, deformable_hit as deform_hit, deformation as deform, effect_pool, geometry,
    impact_particles as impact,
    math::Matrix,
    object_query as query,
    pose::Pose,
    vector,
};
fn constructed_world(root: &std::path::Path) -> tankracer_game::world_startup::Constructed {
    use tankracer_game::session::loading::{Files, Selection};
    let names = [
        "DATA",
        "TANKS/BRONZE",
        "TANKS/SILVER",
        "TANKS/GOLD",
        "INGAME",
        "LEVELS/ENGLAND",
    ];
    let a = names.map(|n| fs::read(root.join("assets/game").join(n).join("FASTFILE.DAT")).unwrap());
    let vehicles = [a[1].as_slice(), a[2].as_slice(), a[3].as_slice()];
    tankracer_game::world_startup::Constructed::load(
        Files {
            data: &a[0],
            vehicles: &vehicles,
            shared: &a[4],
            level: &a[5],
        },
        Selection::default_cup(0, 1).unwrap(),
    )
    .unwrap()
}
fn collider_row(c: &query::Collider) -> Vec<i32> {
    let mut out = vec![c.entity.0 as i32];
    out.extend(c.pose.position);
    out.extend(c.pose.matrix.0);
    out.extend(c.external_basis.unwrap_or(Matrix::IDENTITY).0);
    out.extend(c.bounds.min);
    out.extend(c.bounds.max);
    out.extend([c.radius as i32, i32::from(c.external_basis.is_some())]);
    assert_eq!(out.len(), 30);
    out
}
fn tank_row(t: &query::Tank) -> Vec<i32> {
    let mut out = collider_row(&t.collider);
    out.extend([
        t.slot as i32,
        i32::from(t.active),
        i32::from(t.disabled),
        i32::from(t.ready),
    ]);
    out.extend(t.overlap_bounds.min);
    out.extend(t.overlap_bounds.max);
    out.extend(t.contacts.into_iter().flatten());
    assert_eq!(out.len(), 52);
    out
}
fn obstacle_hit(h: &query::Hit<query::ObstacleId>) -> Vec<i32> {
    vec![
        h.entity.map_or(-1, |e| e.0 as i32),
        h.object.map_or(-1, |o| o.0 as i32),
        h.code,
    ]
    .into_iter()
    .chain(h.point)
    .chain(h.normal)
    .collect()
}
fn tank_hit(h: &query::Hit<query::TankId>) -> Vec<i32> {
    vec![
        h.entity.map_or(-1, |e| e.0 as i32),
        h.object.map_or(-1, |o| o.0 as i32),
        h.code,
    ]
    .into_iter()
    .chain(h.point)
    .chain(h.normal)
    .collect()
}
fn sample_row(sample: &deform::Sample) -> Vec<i32> {
    sample
        .local_raw
        .into_iter()
        .chain(sample.local_clamped)
        .chain([
            sample.vertex_height,
            sample.edge_weight,
            sample.offset,
            sample.activated_records as i32,
        ])
        .collect()
}
fn record_row(record: &deform::Record) -> Vec<i32> {
    record
        .position
        .into_iter()
        .chain(record.world_position)
        .chain([record.timer, record.state as i32])
        .chain(record.color.map(i32::from))
        .collect()
}
#[path = "blast_oracle.rs"]
mod blast_cases;
#[path = "bounce_hit_oracle.rs"]
mod bounce_hit_cases;
#[path = "bounce_tick_oracle.rs"]
mod bounce_tick_cases;
#[path = "crush_oracle.rs"]
mod crush_cases;
#[path = "fall_hit_oracle.rs"]
mod fall_hit_cases;
#[path = "falling_oracle.rs"]
mod falling_cases;
#[path = "ground_response_oracle.rs"]
mod ground_cases;
#[path = "lifecycle_oracle.rs"]
mod lifecycle_cases;
#[path = "scene_contact_oracle.rs"]
mod scene_contact_cases;
#[path = "scene_world_oracle.rs"]
mod scene_world_cases;
#[path = "spatial_oracle.rs"]
mod spatial_cases;
#[path = "tank_response_oracle.rs"]
mod tank_cases;
fn main() {
    let out = PathBuf::from(std::env::args_os().nth(1).expect("output"));
    fs::create_dir_all(&out).unwrap();
    let mut cases = Vec::<Value>::new();
    for normal in [
        [4096, 0, 0],
        [-4096, 0, 0],
        [0, 0, 4096],
        [0, 0, -4096],
        [123, 77, -4321],
        [0, 0, 0],
        [i32::MIN, 0, i32::MAX],
    ] {
        for velocity in [
            [20000, 123, 0],
            [-20000, -456, 0],
            [500001, 1, -300000],
            [-600001, -777, 350000],
            [0, 0, 0],
            [i32::MIN, 0, 0],
        ] {
            for (local, final_scale, mode, player) in [
                (None, None, 0, true),
                (None, None, 4, true),
                (None, None, 0, false),
                (
                    Some(Scale {
                        multiplier: 1,
                        shift: 1,
                    }),
                    Some(Scale {
                        multiplier: 1024,
                        shift: 10,
                    }),
                    0,
                    true,
                ),
            ] {
                let mut input = normal.to_vec();
                input.extend(velocity);
                input.extend([
                    i32::from(local.is_some()),
                    local.map_or(0, |s| s.multiplier as i32),
                    local.map_or(0, |s| s.shift as i32),
                    i32::from(final_scale.is_some()),
                    final_scale.map_or(0, |s| s.multiplier as i32),
                    final_scale.map_or(0, |s| s.shift as i32),
                    mode,
                    i32::from(player),
                ]);
                let (status, expected) =
                    match collision::respond(normal, velocity, local, final_scale, mode, player) {
                        Ok(r) => (
                            0,
                            r.normal
                                .into_iter()
                                .chain(r.velocity)
                                .chain([r.approach, r.speed_units])
                                .collect::<Vec<_>>(),
                        ),
                        Err(_) => (2, vec![0x12345678; 8]),
                    };
                cases.push(json!({"kind":0,"input":input,"status":status,"expected":expected}));
            }
        }
    }
    for angles in [[0, 0, 0], [4000, 2000, 8000], [100, 1300, 200]] {
        let basis = Matrix::from_angles(angles);
        for (local, mut mine, mut other, normal, incoming) in [
            ([0; 3], [0; 3], [0; 3], [4096, 0, 0], [0; 3]),
            (
                [0; 3],
                [10000, 77, 0],
                [-10000, -33, 0],
                [4096, 0, 0],
                [300000, 0, 0],
            ),
            (
                [-5000, 100, 7000],
                [-450000, 12, 90000],
                [600000, -9000, 180000],
                [-2345, 99, 4123],
                [600000, -9000, 180000],
            ),
            (
                [0; 3],
                [100000, 33, 100000],
                [300000, 45, 300000],
                [4096, 0, 4096],
                [600000, 0, 600000],
            ),
        ] {
            let mut input = basis.0.to_vec();
            input.extend(local);
            input.extend(mine);
            input.extend(other);
            input.extend(normal);
            input.extend(incoming);
            let approach =
                collision::exchange(basis, local, &mut mine, &mut other, normal, incoming);
            cases.push(json!({"kind":1,"input":input,"status":0,"expected":mine.into_iter().chain(other).chain([approach]).collect::<Vec<_>>()}));
        }
    }
    for v in [
        [0, 0, 0],
        [1, 2, 3],
        [-1, -2, -3],
        [262143, 1, 0],
        [262144, 0, 0],
        [262145, 0, 0],
        [i32::MIN, 0, 0],
        [i32::MIN, i32::MAX, i32::MIN],
        [i32::MAX, i32::MAX, i32::MAX],
        [123, -456, 789],
    ] {
        let mut normalized = v;
        vector::normalize(&mut normalized);
        let mut expected = normalized.to_vec();
        expected.push(geometry::approximate_distance(v));
        expected.push(collision::direction(v[0], v[2]) as i32);
        cases.push(json!({"kind":2,"input":v,"status":0,"expected":expected}));
    }
    for value in [
        i32::MIN,
        i32::MIN + 1,
        -1234567890,
        -65536,
        -4096,
        -11,
        -10,
        -1,
        0,
        1,
        9,
        10,
        11,
        4096,
        65536,
        1234567890,
        i32::MAX,
    ] {
        cases.push(json!({"kind":3,"input":[value],"status":0,"expected":[collision::binary_one_point_seven(value)]}));
    }
    for dt in [0, 1, 33, 65535, -1] {
        let a = Trajectory {
            position: [0, 0, 1000],
            velocity: [123, -456, 200000],
        };
        let b = Trajectory {
            position: [0, 2, 2000],
            velocity: [0, 0, -100000],
        };
        let mut input = a.position.to_vec();
        input.extend(a.velocity);
        input.extend(b.position);
        input.extend(b.velocity);
        input.push(dt);
        cases.push(json!({"kind":4,"input":input,"status":0,"expected":[i32::from(collision::approaching(a,b,dt))]}));
    }
    for dt in [0, 16, 33, 65535] {
        for initial in [
            [70, 13, -99, 123],
            [-600, 1000, 400, 700],
            [i32::MIN, i32::MAX, 0, i32::MIN],
        ] {
            for present in [false, true] {
                for indices in [[0, 1, 0, 1], [0, 3, 0, 1]] {
                    let mut state = body::Articulation {
                        recoil: initial[0],
                        recoil_rate: initial[1],
                        pitch: initial[2],
                        pitch_rate: initial[3],
                    };
                    let mut mesh = [[1i16, 2, 3]; 2];
                    let original = [80, 160, 240, 320];
                    let mut input = initial.to_vec();
                    input.extend([15, 1, 3, 1, 14, 6, dt]);
                    input.extend([i32::from(present), 2]);
                    input.extend(indices.map(i32::from));
                    input.extend(original.map(i32::from));
                    input.extend(mesh.into_iter().flatten().map(i32::from));
                    let binding = if present {
                        Some(body::GunMesh {
                            vertices: &mut mesh,
                            indices,
                            original_z: original,
                        })
                    } else {
                        None
                    };
                    let status = if state.step(dt, &body::Tuning::default(), binding).is_ok() {
                        0
                    } else {
                        2
                    };
                    let mut expected = vec![
                        state.recoil,
                        state.recoil_rate,
                        state.pitch,
                        state.pitch_rate,
                    ];
                    expected.extend(mesh.into_iter().flatten().map(i32::from));
                    cases.push(json!({"kind":5,"input":input,"status":status,"expected":expected}));
                }
            }
        }
        for initial in [
            [10000, 10000, 0, 0],
            [-5000, -12345, 6000, 4321],
            [0, 0, 0, 0],
        ] {
            let acceleration = [-12345, 67890];
            let mut state = body::Lean {
                lateral: body::Spring {
                    position: initial[0],
                    rate: initial[1],
                },
                longitudinal: body::Spring {
                    position: initial[2],
                    rate: initial[3],
                },
            };
            let mut input = initial.to_vec();
            input.extend(acceleration);
            input.extend([15, 1, 3, 1, 14, 6, dt]);
            state.step(acceleration, dt, &body::Tuning::default());
            cases.push(json!({"kind":6,"input":input,"status":0,"expected":[state.lateral.position,state.lateral.rate,state.longitudinal.position,state.longitudinal.rate]}));
        }
    }
    for current in [[0; 3], [i32::MIN; 3], [i32::MAX; 3]] {
        for previous in [[0; 3], [i32::MAX, 70000, i32::MIN]] {
            let contacts = std::array::from_fn(|i| body::Contact {
                world: [80 + i as i32 * 16, 40 - i as i32 * 8, -160 + i as i32 * 32],
                gap: 24 + i as i32,
            });
            let mut input = current.to_vec();
            input.extend(previous);
            for c in contacts {
                input.extend(c.world);
                input.push(c.gap);
            }
            let mut hull = body::Hull::default();
            hull.refresh(current, previous, contacts);
            let expected = hull
                .position
                .into_iter()
                .chain(hull.matrix.0)
                .chain(hull.vertices.into_iter().flatten().map(i32::from))
                .chain([i32::from(hull.radius)])
                .collect::<Vec<_>>();
            cases.push(json!({"kind":7,"input":input,"status":0,"expected":expected}));
        }
    }
    let bounds = query::Bounds {
        min: [-10; 3],
        max: [10; 3],
    };
    for (a, b, limits) in [
        ([0; 3], [0; 3], bounds),
        ([-20, 0, 0], [-15, 0, 0], bounds),
        ([-20, 0, 0], [20, 0, 0], bounds),
        ([20; 3], [0; 3], bounds),
        ([20, 0, 0], [20, 0, 0], bounds),
        (
            [524288, 0, 0],
            [523264, 4194304, 0],
            query::Bounds {
                min: [-10; 3],
                max: [0, 10, 10],
            },
        ),
    ] {
        let mut hit = query::SegmentHit {
            point: [99; 3],
            normal: [123; 3],
        };
        let mut input = a.to_vec();
        input.extend(b);
        input.extend(limits.min);
        input.extend(limits.max);
        input.extend(hit.point);
        input.extend(hit.normal);
        let status = query::segment_box(a, b, limits, &mut hit).map_or(3, |c| c as i32);
        cases.push(json!({"kind":8,"input":input,"status":status,"expected":hit.point.into_iter().chain(hit.normal).collect::<Vec<_>>()}));
    }
    for rotated in [false, true] {
        let basis = if rotated {
            Matrix::from_angles([0, 16384, 0])
        } else {
            Matrix::IDENTITY
        };
        let object = query::Collider {
            entity: query::EntityId(1),
            pose: Pose {
                position: [100, 0, 100],
                matrix: basis,
            },
            external_basis: rotated.then_some(basis),
            radius: 100,
            bounds,
        };
        let objects = [
            object,
            query::Collider {
                entity: query::EntityId(2),
                ..object
            },
        ];
        for (a, b) in [
            ([80, 0, 100], [120, 0, 100]),
            ([100, 0, 100], [100, 0, 100]),
            ([100, 0, 100], [120, 0, 100]),
            ([1000; 3], [1001; 3]),
        ] {
            for deformable in [false, true] {
                for ids in [
                    vec![query::ObstacleId(1), query::ObstacleId(0)],
                    vec![],
                    vec![query::ObstacleId(3)],
                ] {
                    let mut hit = query::Hit {
                        entity: Some(query::EntityId(7)),
                        object: Some(query::ObstacleId(7)),
                        code: 9,
                        point: [99; 3],
                        normal: [123; 3],
                    };
                    let mut input = a.to_vec();
                    input.extend(b);
                    input.extend([2, i32::from(deformable), ids.len() as i32, 2]);
                    for c in &objects {
                        input.extend(collider_row(c));
                    }
                    input.extend([
                        ids.first().map_or(65535, |i| i.0 as i32),
                        ids.get(1).map_or(65535, |i| i.0 as i32),
                    ]);
                    input.extend(obstacle_hit(&hit));
                    assert_eq!(input.len(), 81);
                    let result = if deformable {
                        query::deformables(&ids, &objects, a, b, &mut hit)
                    } else {
                        let grid = query::Grid {
                            origin: [0; 2],
                            shifts: [10; 2],
                            cells: [ids.as_slice(); 64],
                        };
                        query::obstacles(&grid, &objects, query::EntityId(2), a, b, &mut hit)
                    };
                    let status = match result {
                        Ok(found) => i32::from(found),
                        Err(query::Error::Arithmetic) => 3,
                        Err(_) => 4,
                    };
                    cases.push(json!({"kind":9,"input":input,"status":status,"expected":obstacle_hit(&hit)}));
                }
            }
        }
        let own = query::Tank {
            collider: query::Collider {
                entity: query::EntityId(0),
                pose: Pose {
                    position: [0; 3],
                    matrix: Matrix::IDENTITY,
                },
                external_basis: None,
                radius: 20,
                bounds,
            },
            slot: 0,
            active: true,
            disabled: false,
            ready: true,
            overlap_bounds: bounds,
            contacts: [[0; 3]; 4],
        };
        let peer = query::Tank {
            collider: object,
            slot: 1,
            active: false,
            contacts: [[100, 0, 100]; 4],
            ..own
        };
        for own_active in [false, true] {
            for ready in [false, true] {
                for disabled in [false, true] {
                    for (a, b) in [
                        ([80, 0, 100], [120, 0, 100]),
                        ([100, 0, 100], [100, 0, 100]),
                        ([1000; 3], [1001; 3]),
                    ] {
                        let own = query::Tank {
                            active: own_active,
                            ..own
                        };
                        let peer = query::Tank {
                            ready,
                            disabled,
                            ..peer
                        };
                        let objects = [
                            own,
                            peer,
                            query::Tank {
                                collider: query::Collider {
                                    entity: query::EntityId(2),
                                    ..object
                                },
                                slot: 2,
                                ..peer
                            },
                        ];
                        let mut slots = [None; 9];
                        slots[0] = Some(query::TankId(0));
                        slots[1] = Some(query::TankId(2));
                        slots[2] = Some(query::TankId(1));
                        let mut hit = query::Hit {
                            entity: Some(query::EntityId(7)),
                            object: Some(query::TankId(7)),
                            code: 9,
                            point: [99; 3],
                            normal: [123; 3],
                        };
                        let mut input = a.to_vec();
                        input.extend(b);
                        input.extend([0, 0, 9, 3]);
                        for t in &objects {
                            input.extend(tank_row(t));
                        }
                        input.extend(slots.map(|s| s.map_or(65535, |id| id.0 as i32)));
                        input.extend(tank_hit(&hit));
                        assert_eq!(input.len(), 184);
                        let status = query::tanks(&own, &objects, &slots, a, b, &mut hit)
                            .map_or(3, |c| c as i32);
                        cases.push(json!({"kind":10,"input":input,"status":status,"expected":tank_hit(&hit)}));
                    }
                }
            }
        }
    }
    let lists: Vec<Vec<query::ObstacleId>> = (0..64).map(|i| vec![query::ObstacleId(i)]).collect();
    for start in [
        [0; 3],
        [-1, 0, -1],
        [i32::MIN, 0, i32::MAX],
        [7000, 123, 4000],
    ] {
        for shift in [0, 10, 32, 63] {
            let origin = [-100, 200];
            let grid = query::Grid {
                origin,
                shifts: [shift, shift],
                cells: std::array::from_fn(|i| lists[i].as_slice()),
            };
            let mut input = start.to_vec();
            input.extend(origin);
            input.extend([shift as i32, shift as i32]);
            cases.push(json!({"kind":11,"input":input,"status":0,"expected":[grid.candidates(start)[0].0 as i32]}));
        }
    }
    for rotated in [false, true] {
        for dt in [0, 1, 33] {
            for compression in [1, 64] {
                for timer in [0, 33, 1000] {
                    for flag_count in [0, 3] {
                        for sound in [false, true] {
                            let basis = Matrix::from_angles([0, 16384, 0]);
                            let collider = query::Collider {
                                entity: query::EntityId(1),
                                pose: Pose {
                                    position: [0; 3],
                                    matrix: Matrix::IDENTITY,
                                },
                                external_basis: rotated.then_some(basis),
                                radius: 1,
                                bounds,
                            };
                            let config = deform_hit::Config {
                                floor: 0,
                                compression,
                                drift_shift: 33,
                                material: -7,
                                has_sound: sound,
                            };
                            let mut state = deform_hit::State {
                                dirty: 0,
                                last_vehicle: None,
                                sound_timer: timer,
                            };
                            let mut vertices = [[256i16, 200, 0], [255, 200, 0], [0, 16, 0]];
                            let flags = [7, 5, 4];
                            let (mut material, mut seed) = (99, 0x1234abcdu32);
                            let mut input = collider_row(&collider);
                            input.extend([0, compression, 33, -7, i32::from(sound)]);
                            input.extend([0, -1, timer, 2, 0, 0, 0, material, seed as i32, dt]);
                            input.extend(vertices.into_iter().flatten().map(i32::from));
                            input.extend(flags.map(i32::from));
                            input.push(flag_count as i32);
                            assert_eq!(input.len(), 58);
                            let status = state
                                .hit(
                                    config,
                                    &collider,
                                    deform_hit::Mesh {
                                        vertices: &mut vertices,
                                        flags: &flags[..flag_count],
                                    },
                                    deform_hit::Vehicle {
                                        entity: query::EntityId(2),
                                        position: [0; 3],
                                        material: &mut material,
                                        random: &mut seed,
                                        dt,
                                    },
                                )
                                .map_or(2, i32::from);
                            let expected = vec![
                                state.dirty as i32,
                                state.last_vehicle.map_or(-1, |id| id.0 as i32),
                                state.sound_timer,
                                material,
                                seed as i32,
                            ]
                            .into_iter()
                            .chain(vertices.into_iter().flatten().map(i32::from))
                            .collect::<Vec<_>>();
                            cases.push(json!({"kind":12,"input":input,"status":status,"expected":expected}));
                        }
                    }
                }
            }
        }
    }
    for rotated in [false, true] {
        for sample_bounds in [[[-10, 10]; 2], [[10, -10], [20, -20]]] {
            for world in [[800, 0, 0], [0; 3], [i32::MIN, i32::MAX, i32::MIN]] {
                for attached in [false, true] {
                    let basis = if rotated {
                        Matrix::from_angles([0, 16384, 0]).0
                    } else {
                        Matrix::IDENTITY.0
                    };
                    let vertices = [[138i16, 300, 0], [137, 40, 0], [0, -100, 0]];
                    let geometry = deform::Geometry {
                        sample_bounds,
                        edges: [-10000, 10000, -10000, 10000],
                        edge_cap: 65536,
                        vertices: &vertices,
                    };
                    let mut state = deform::State {
                        pose: deform::Pose {
                            origin: [0; 3],
                            inverse_basis: basis,
                        },
                        records: vec![
                            deform::Record {
                                position: [800, 0, 0],
                                world_position: [11, 22, 33],
                                timer: -77,
                                state: 1,
                                color: [9; 3],
                            },
                            deform::Record {
                                position: [50, 0, 0],
                                world_position: [11, 22, 33],
                                timer: 31,
                                state: 1,
                                color: [9; 3],
                            },
                            deform::Record {
                                position: [i32::MAX; 3],
                                world_position: [11, 22, 33],
                                timer: 0,
                                state: 0,
                                color: [9; 3],
                            },
                        ],
                        records_attached: attached,
                        dirty: 7,
                    };
                    let mut input = state.pose.origin.to_vec();
                    input.extend(basis);
                    input.extend(sample_bounds.into_iter().flatten());
                    input.extend(geometry.edges);
                    input.extend([geometry.edge_cap, i32::from(attached), 7]);
                    input.extend(world);
                    input.extend(vertices.into_iter().flatten().map(i32::from));
                    for r in &state.records {
                        input.extend(record_row(r));
                    }
                    assert_eq!(input.len(), 68);
                    let first = state.sample(&geometry, world);
                    let second = state.sample(&geometry, world);
                    let mut expected = sample_row(&first);
                    expected.extend(sample_row(&second));
                    expected.push(state.dirty as i32);
                    for r in &state.records {
                        expected.extend(record_row(r));
                    }
                    assert_eq!(expected.len(), 54);
                    cases.push(json!({"kind":13,"input":input,"status":0,"expected":expected}));
                }
            }
        }
    }
    damage_cases(&mut cases);
    impact_cases(&mut cases);
    ground_cases::append(&mut cases);
    tank_cases::append(&mut cases);
    lifecycle_cases::append(&mut cases);
    crush_cases::append(&mut cases);
    spatial_cases::append(&mut cases);
    let root = PathBuf::from(std::env::args_os().nth(2).expect("reference root"));
    falling_cases::append(&mut cases, &root);
    blast_cases::append(&mut cases);
    scene_world_cases::append(&mut cases);
    scene_contact_cases::append(&mut cases);
    scene_contact_cases::append_live(&mut cases);
    fall_hit_cases::append(&mut cases, &root);
    bounce_hit_cases::append(&mut cases, &root);
    bounce_tick_cases::append(&mut cases, &root, &out);
    fs::write(out.join("collision.json"),serde_json::to_vec_pretty(&json!({"cases":cases,"scope":"unmodified collision/body/queries, deformation, damage, impacts, blast impulse, ordered ground/tank response, live spatial/lifecycle and real Fall class motion"})).unwrap()).unwrap();
}

fn impact_error(e: impact::Error) -> i32 {
    match e {
        impact::Error::Arithmetic(_) => 2,
        impact::Error::Pool(_) => 3,
        _ => 4,
    }
}
fn impact_cases(cases: &mut Vec<Value>) {
    for kind in 0..=6 {
        for variant in 0..9 {
            for reverse in [false, true] {
                let mut state = impact::State::default();
                let mut v = tankracer_game::vehicle::State {
                    entity: query::EntityId(1),
                    impact_cooldown: if variant == 7 { 0 } else { 500 },
                    ..Default::default()
                };
                let points = [[0, 0, 100], [0, 0, -100], [100, 0, 100], [100, 0, -100]];
                for (c, p) in v.motion.contacts.iter_mut().zip(points) {
                    c.world = p;
                }
                if variant == 6 {
                    v.motion.contacts[1].world[0] = i32::MIN;
                }
                v.powertrain.local_velocity[2] = if reverse { -1 } else { 1 };
                let basis = Matrix::from_angles([3000, 13000, (-1000i32) as u32]);
                v.pose_input.external = if variant == 5 || variant == 8 {
                    tankracer_game::pose::ExternalBasis::None
                } else {
                    tankracer_game::pose::ExternalBasis::Custom(basis)
                };
                let mut pool = effect_pool::Pool::default();
                if variant == 1 || variant == 2 {
                    for _ in 0..100 {
                        pool.allocate([[0; 3]; 2], [[0; 4]; 2]);
                    }
                    if variant == 2 {
                        pool.line_count = 99;
                    }
                }
                if variant == 3 {
                    state.batches[0].active = true;
                    state.batches[0].particles[3].line = effect_pool::LineHandle(101);
                }
                if variant == 4 {
                    state.next = 4;
                }
                let dt = 33;
                let mut input = vec![
                    kind,
                    if variant == 5 { 4 } else { 2 },
                    v.impact_cooldown as i32,
                    variant,
                    i32::from(variant != 5 && variant != 8),
                    v.powertrain.local_velocity[2],
                    dt,
                    state.next as i32,
                    if variant == 0 { 5 } else { 1 },
                ];
                input.extend(v.motion.contacts.iter().flat_map(|c| c.world));
                input.extend(basis.0);
                let tuning = impact::Tuning::default();
                input.extend(tuning.forward_offset);
                input.extend(tuning.reverse_offset);
                assert_eq!(input.len(), 36);
                let mut seed = 7u32;
                let mut phases = [-99; 4];
                let mut context = impact::Context {
                    frame: 17,
                    random: &mut seed,
                    pool: &mut pool,
                    tuning,
                };
                let mut failed = false;
                for _ in 0..input[8] {
                    match state.emit(&mut v, input[1] as u32, kind, &mut context) {
                        Ok(r) => phases[0] = i32::from(r),
                        Err(e) => {
                            phases[0] = impact_error(e);
                            failed = true;
                            break;
                        }
                    }
                }
                if !failed {
                    phases[1] = state
                        .tick(&mut v, dt, &mut context)
                        .map_or_else(impact_error, |()| 0);
                    failed = phases[1] != 0;
                }
                if !failed {
                    context.frame += 1;
                    phases[2] = state
                        .tick(&mut v, dt, &mut context)
                        .map_or_else(impact_error, |()| 0);
                    failed = phases[2] != 0;
                }
                if !failed {
                    context.frame += 1;
                    phases[3] = state
                        .tick(&mut v, dt, &mut context)
                        .map_or_else(impact_error, |()| 0);
                }
                let expected = impact_snapshot(phases, &state, &v, &pool, seed);
                assert_eq!(expected.len(), 1288);
                cases.push(json!({"kind":15,"input":input,"status":0,"expected":expected}));
            }
        }
    }
}

fn triangle(indices: [u8; 3]) -> Triangle {
    Triangle {
        source_offset: 0,
        normal: [11, 22, 33],
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
fn damage_cases(cases: &mut Vec<Value>) {
    for rotated in [false, true] {
        for world in [[1, 0, 1], [-1, 0, 1], [0; 3], [1, 0, -1]] {
            for operation in 0..4 {
                for variant in 0..5 {
                    let pose = Pose {
                        position: [0; 3],
                        matrix: if rotated {
                            Matrix::from_angles([0, 16384, 0])
                        } else {
                            Matrix::IDENTITY
                        },
                    };
                    let mut state = damage::State {
                        tracks: [9, 9],
                        ..Default::default()
                    };
                    if variant == 4 {
                        state.body = [1; 4];
                        state.tracks = [10; 2];
                    }
                    let selected = if variant == 2 { [7; 4] } else { [0; 4] };
                    let present = variant != 1;
                    let mut mesh = Geometry {
                        vertices: vec![[4096, -17, -4096], [0, 300, 20], [300, 5, 0], [7, -300, 9]],
                        triangles: vec![
                            triangle([0, 1, 2]),
                            triangle([1, 2, 3]),
                            triangle(if variant == 3 { [0, 1, 8] } else { [0, 2, 3] }),
                        ],
                    };
                    let mut input = pose.matrix.0.to_vec();
                    input.extend(pose.position);
                    input.extend(world);
                    input.extend(state.body.map(i32::from));
                    input.extend(state.turret.map(i32::from));
                    input.extend(state.tracks.map(i32::from));
                    input.extend(selected.map(i32::from));
                    input.extend(mesh.vertices.iter().flatten().copied().map(i32::from));
                    input.extend(mesh.triangles.iter().flat_map(|f| f.indices.map(i32::from)));
                    input.extend(mesh.triangles.iter().flat_map(|f| f.normal.map(i32::from)));
                    input.extend([i32::from(rotated), i32::from(present), operation, 3, 4, 3]);
                    let mut status = 0;
                    for _ in 0..3 {
                        let before = (state.body, state.turret);
                        let result = if operation >= 2 {
                            damage::Target {
                                state: &mut state,
                                player: rotated,
                                body_pose: pose,
                                body_vertices: selected,
                                body_mesh: present.then_some(&mut mesh),
                            }
                            .track(if operation == 2 { 2 } else { 3 }, world)
                        } else if present {
                            state
                                .hit(
                                    if operation == 0 {
                                        damage::Part::Body
                                    } else {
                                        damage::Part::Turret
                                    },
                                    pose,
                                    world,
                                    selected,
                                    &mut mesh,
                                )
                                .map(|_| ())
                                .map_err(damage::TargetError::Mesh)
                        } else {
                            let q = damage::quadrant(pose, world);
                            let flags = if operation == 0 {
                                &mut state.body
                            } else {
                                &mut state.turret
                            };
                            if flags[q] != 0 {
                                Ok(())
                            } else {
                                flags[q] = 1;
                                Err(damage::TargetError::MissingBody)
                            }
                        };
                        match result {
                            Ok(()) => {
                                status = i32::from(before != (state.body, state.turret));
                            }
                            Err(damage::TargetError::MissingBody) => {
                                status = 3;
                                break;
                            }
                            Err(damage::TargetError::Mesh(_)) => {
                                status = 2;
                                break;
                            }
                        }
                    }
                    let mut expected = state.body.map(i32::from).to_vec();
                    expected.extend(state.turret.map(i32::from));
                    expected.extend(state.tracks.map(i32::from));
                    expected.extend(mesh.vertices.iter().flatten().copied().map(i32::from));
                    expected.extend(mesh.triangles.iter().flat_map(|f| f.normal.map(i32::from)));
                    assert_eq!(input.len(), 65);
                    assert_eq!(expected.len(), 31);
                    cases
                        .push(json!({"kind":14,"input":input,"status":status,"expected":expected}));
                }
            }
        }
    }
}
