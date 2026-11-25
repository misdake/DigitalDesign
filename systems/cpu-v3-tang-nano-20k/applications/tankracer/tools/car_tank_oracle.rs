//! Call the original live car collision entry through a visibility-only bridge.
use serde_json::{json, Value};
use std::{fs, path::Path};
use tankracer_assets::{
    config::Part,
    mesh::{Geometry, Triangle},
};
use tankracer_game::{
    effect_pool,
    geometry::Bounds,
    impact_particles,
    math::Matrix,
    object_query::{Tank, TankId},
    pose::{ExternalBasis, Pose},
    vehicle_runtime::rcc_probe,
    vehicle_startup::Player,
};
#[path = "impact_snapshot.rs"]
mod impact_snapshot;
#[allow(dead_code)]
mod reference_world;
use reference_world::two_cars as world;

fn triangle() -> Triangle {
    Triangle {
        source_offset: 0,
        normal: [11, 22, 33],
        color: 0,
        indices: [0, 1, 2],
        sides: 0,
        uv: [[0; 2]; 3],
        flags: 0,
        material: None,
        material_source_offset: 0,
        billboard: None,
    }
}
fn mesh() -> Geometry {
    Geometry {
        vertices: vec![[4096, -17, -4096], [0, 300, 20], [300, 5, 0]],
        triangles: vec![triangle()],
    }
}
fn geometry_row(t: &Tank) -> Vec<i32> {
    let c = &t.collider;
    let mut out = vec![c.entity.0 as i32];
    out.extend(c.pose.position);
    out.extend(c.pose.matrix.0);
    out.extend(c.external_basis.unwrap_or(Matrix::IDENTITY).0);
    out.extend(c.bounds.min);
    out.extend(c.bounds.max);
    out.extend([
        c.radius as i32,
        i32::from(c.external_basis.is_some()),
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
fn state_row(p: &Player) -> Vec<i32> {
    let v = &p.physics;
    let mut out = v.motion.velocity.to_vec();
    out.extend(v.powertrain.local_velocity);
    out.extend(v.motion.previous_position);
    out.extend([v.collision.timer, i32::from(v.player)]);
    out
}
fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let out = Path::new(&args[0]);
    let mut world = world(Path::new(&args[1]), 2);
    let templates = [
        world.vehicles.entries[0].as_ref().unwrap().clone(),
        world.vehicles.entries[1].as_ref().unwrap().clone(),
    ];
    let mut cases = Vec::<Value>::new();
    for variant in 0..21 {
        for speed in [0, 250001, 450000, 900001] {
            for mode in [0, 4] {
                for own in 0..2 {
                    for (i, template) in templates.iter().enumerate() {
                        let mut p = template.clone();
                        let position = if i == 0 {
                            [0; 3]
                        } else if variant == 1 || variant == 18 {
                            [0, 0, 1000]
                        } else if variant == 2 {
                            [50, 0, 0]
                        } else {
                            [0, 0, 50]
                        };
                        let basis = if variant == 3 {
                            Matrix::from_angles([3000, 13000, 8000])
                        } else {
                            Matrix::IDENTITY
                        };
                        let bounds = Bounds {
                            min: [-100; 3],
                            max: [100; 3],
                        };
                        p.slot = i as u8;
                        p.visible = !((variant == 4 && i == own)
                            || (variant == 13 && i != own)
                            || (variant == 19 && i == own));
                        p.physics.motion.poses.base_position = position;
                        p.physics.motion.poses.reference = basis;
                        p.physics.motion.poses.body = Pose {
                            position,
                            matrix: basis,
                        };
                        p.physics.pose_input.external = ExternalBasis::Custom(Matrix::IDENTITY);
                        p.physics.motion.velocity = if i == 0 {
                            [0, 777, speed]
                        } else {
                            [0, -999, 0]
                        };
                        p.physics.powertrain.local_velocity =
                            [0, 0, if i == 0 { speed } else { 0 }];
                        p.physics.motion.previous_position = if variant == 1 {
                            [0, 0, -1600]
                        } else {
                            position
                        };
                        p.physics.motion.ready = !(variant == 6 && i != own);
                        p.physics.collision.disabled = (variant == 5 && i != own)
                            || ((variant == 16 || variant == 20) && i == own);
                        p.physics.collision.timer = if i == 0 { 77 } else { -55 };
                        p.physics.collision.tank_delta = [99; 3];
                        p.physics.player = i == 0;
                        p.physics.impact_cooldown = 500;
                        p.damage = Default::default();
                        p.model.geometry_bounds = bounds;
                        p.model.collision_bounds = bounds;
                        p.model.damage_vertices[0] = [0; 4];
                        let body = p.model.parts[Part::Body.slot()].as_mut().unwrap();
                        body.radius = 20;
                        body.edit(|g| *g = mesh()).expect("private player mesh");
                        if variant == 11 && i == 0 {
                            p.model.parts[Part::Body.slot()] = None;
                        }
                        let contacts = if variant == 1 || variant == 18 {
                            [[100000; 3]; 4]
                        } else {
                            [[0, 0, 10], [0, 0, -10], [10, 0, 10], [10, 0, -10]]
                        };
                        for (j, point) in contacts.into_iter().enumerate() {
                            p.physics.contact.sampling.primary.points[j].world = point;
                            p.physics.contact.response.points[j].corner = j as u32;
                            p.physics.motion.contacts[j].world = point;
                        }
                        world.vehicles.entries[i] = Some(p);
                    }
                    world.vehicles.racers = [None; 9];
                    world.vehicles.racers[0] = Some(TankId(0));
                    world.vehicles.racers[1] = Some(TankId(1));
                    if variant == 7 || variant == 18 {
                        world.vehicles.racers[1 - own] = Some(TankId(own));
                    }
                    if variant == 8 || variant == 19 || variant == 20 {
                        world.vehicles.racers[1 - own] = Some(TankId(7));
                    }
                    if variant == 12 {
                        world.vehicles.racers = [None; 9];
                    }
                    let initial = [
                        world.vehicles.entries[0].as_ref().unwrap().clone(),
                        world.vehicles.entries[1].as_ref().unwrap().clone(),
                    ];
                    let mut input = vec![
                        33,
                        mode,
                        0,
                        own as i32,
                        variant,
                        2,
                        2,
                        if variant == 15 { 2 } else { 1 },
                    ];
                    for i in 0..2 {
                        input.extend(geometry_row(&rcc_probe::geometry(&world, i)));
                    }
                    for p in &initial {
                        input.extend(state_row(p));
                    }
                    input.extend(
                        world
                            .vehicles
                            .racers
                            .map(|id| id.map_or(65535, |id| id.0 as i32)),
                    );
                    input.extend([i32::from(variant != 11), 1]);
                    let mut present = [1, 1];
                    if variant == 14 {
                        present[1 - own] = 0;
                        world.vehicles.entries[1 - own] = None;
                    }
                    if variant == 17 {
                        present[own] = 0;
                        world.vehicles.entries[own] = None;
                    }
                    input.extend(present);
                    assert_eq!(input.len(), 147);
                    world.shared.impacts = impact_particles::State::default();
                    if variant == 10 {
                        world.shared.impacts.next = 4;
                    }
                    world.shared.pool = effect_pool::Pool::default();
                    world.shared.random = 7;
                    world.frame = 17;
                    let mut selected = own;
                    let mut status = 0;
                    for _ in 0..input[7] {
                        status = rcc_probe::tanks(&mut world, TankId(selected), 33, mode)
                            .err()
                            .unwrap_or(0);
                        if status != 0 {
                            break;
                        }
                        selected = 1 - selected;
                    }
                    let players = std::array::from_fn::<_, 2, _>(|i| {
                        world.vehicles.entries[i].as_ref().unwrap_or(&initial[i])
                    });
                    let mut expected = impact_snapshot::snapshot(
                        [status, -99, -99, -99],
                        &world.shared.impacts,
                        &players[0].physics,
                        &world.shared.pool,
                        world.shared.random,
                    );
                    expected.extend([-1, -1, 0, 0]);
                    for p in players {
                        expected.extend(state_row(p));
                    }
                    expected.push(players[1].physics.impact_cooldown as i32);
                    for p in players {
                        expected.extend(p.damage.body.map(i32::from));
                        expected.extend(p.damage.turret.map(i32::from));
                        expected.extend(p.damage.tracks.map(i32::from));
                        let fallback = mesh();
                        let g = p.model.parts[Part::Body.slot()]
                            .as_ref()
                            .map_or(&fallback, |m| m.geometry());
                        expected.extend(g.vertices.iter().flatten().copied().map(i32::from));
                        expected.extend(g.triangles[0].normal.map(i32::from));
                    }
                    for p in players {
                        expected.extend(p.physics.collision.tank_delta);
                    }
                    assert_eq!(expected.len(), 1365);
                    cases.push(json!({"kind":29,"input":input,"status":0,"expected":expected}));
                }
            }
        }
    }
    fs::write(
        out.join("car-tanks.json"),
        serde_json::to_vec(&cases).unwrap(),
    )
    .unwrap();
}
