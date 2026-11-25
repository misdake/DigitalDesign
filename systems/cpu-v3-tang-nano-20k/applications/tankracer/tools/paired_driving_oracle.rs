//! Persistent two-car trajectories through the original private collision entries.
mod deform_assets;
mod driving_capture;
mod impact_snapshot;
#[allow(dead_code)]
#[path = "terrain_assets.rs"]
mod pages;
#[allow(dead_code, clippy::needless_range_loop, clippy::manual_range_contains)]
#[path = "vehicle_oracle.rs"]
mod reference;
mod reference_world;
mod scene_export;
use serde_json::{json, Value};
use std::{fs, path::Path};
use tankracer_assets::{config::Part, terrain::TerrainData};
use tankracer_game::{
    attitude, body, contact, contact_pipeline as ground, drive, effect_pool, impact_particles,
    math::Matrix,
    motion, pose, powertrain,
    sampling::Transform,
    suspension, terrain,
    vehicle_runtime::{rcc_car_probe, rcc_probe, SinglePlayer},
    vehicle_startup::Player,
    velocity,
};

fn core(p: &Player, data: &TerrainData, stage: &[i32]) -> Vec<i32> {
    let v = &p.physics;
    let mut out =
        driving_capture::state(&v.motion, &v.powertrain, &p.prefix.boost, &v.contact, data);
    out[73..89].copy_from_slice(stage);
    let gun = &p.model.parts[Part::Gun.slot()]
        .as_ref()
        .unwrap()
        .geometry()
        .vertices;
    out.extend(driving_capture::body_state(&v.motion, &v.pose, gun));
    assert_eq!(out.len(), 295);
    out
}
fn mesh_state(out: &mut Vec<i32>, p: &Player) {
    out.extend(p.damage.body.map(i32::from));
    out.extend(p.damage.turret.map(i32::from));
    out.extend(p.damage.tracks.map(i32::from));
    let mesh = p.model.parts[Part::Body.slot()]
        .as_ref()
        .unwrap()
        .geometry();
    out.extend(mesh.vertices.iter().flatten().copied().map(i32::from));
    out.extend(mesh.triangles.iter().flat_map(|t| t.normal.map(i32::from)));
}
fn snapshot(world: &SinglePlayer, stages: &[Vec<i32>; 2]) -> Vec<i32> {
    let a = world.vehicles.entries[0].as_ref().unwrap();
    let b = world.vehicles.entries[1].as_ref().unwrap();
    let data = world.terrain();
    let mut out = core(a, data, &stages[0]);
    out.extend(impact_snapshot::snapshot(
        [0; 4],
        &world.shared.impacts,
        &a.physics,
        &world.shared.pool,
        world.shared.random,
    ));
    out.extend(a.damage.body.map(i32::from));
    out.extend(a.damage.turret.map(i32::from));
    out.extend(a.damage.tracks.map(i32::from));
    out.extend(a.physics.collision.ground_delta);
    let mesh = a.model.parts[Part::Body.slot()]
        .as_ref()
        .unwrap()
        .geometry();
    out.extend(mesh.vertices.iter().flatten().copied().map(i32::from));
    out.extend(mesh.triangles.iter().flat_map(|t| t.normal.map(i32::from)));
    out.push(a.physics.collision.timer);
    out.extend(a.physics.collision.tank_delta);
    out.extend(core(b, data, &stages[1]));
    out.extend([b.physics.impact_cooldown as i32, b.physics.collision.timer]);
    out.extend(b.physics.collision.tank_delta);
    out.extend(b.physics.collision.ground_delta);
    mesh_state(&mut out, b);
    assert_eq!(out.len(), 2279);
    for p in [a, b] {
        out.extend([
            p.physics.collision.material,
            p.physics.collision.deformable.map_or(-1, |e| e.0 as i32),
            p.physics.collision.impact_accumulator,
        ]);
    }
    out.push(world.identity.current_slot as i32);
    out
}
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let root = Path::new(&args[0]);
    let out = Path::new(&args[1]);
    let steps: usize = args[2].parse().unwrap();
    let template_world = reference_world::two_cars(root, 1);
    let originals = [
        template_world.vehicles.entries[0].as_ref().unwrap().clone(),
        template_world.vehicles.entries[1].as_ref().unwrap().clone(),
    ];
    let terrain = template_world.terrain().clone();
    let surfaces = template_world.surfaces().clone();
    let common: Value =
        serde_json::from_slice(&fs::read(out.join("driving.json")).unwrap()).unwrap();
    let env = &common["cases"][0]["environment"];
    let model = &originals[0].model;
    let tank = &model.config.tank;
    let tuning = drive::Tuning::default();
    let contact = contact::Config::try_from(&tank.suspension).unwrap();
    let bounds = model.geometry_bounds;
    let layout = [
        bounds.min[0],
        bounds.min[1],
        bounds.min[2],
        bounds.max[0],
        bounds.max[2],
    ];
    let mut exported = Vec::<Value>::new();
    let mut cells = std::collections::BTreeSet::new();
    for (name, velocities, separation, object) in [
        ("pair-chase", [100000, 0], 1600, None),
        ("pair-head-on", [300000, -300000], 1600, None),
        ("deform-wide", [300000, 0], 1600, Some(124)),
        ("deform-reverse", [-300000, 0], 1600, Some(173)),
    ] {
        let mut session = reference_world::complete(root);
        let mut empty = reference_world::two_cars(root, 1);
        let world = if object.is_some() {
            session.world_mut_at_load().unwrap()
        } else {
            &mut empty
        };
        let entity = world.vehicles.entries[0].as_ref().unwrap().physics.entity;
        let start = object.map(|index| {
            let c = &world.objects.world.colliders[index];
            let component = world.objects.components.surfaces.entries[index]
                .as_ref()
                .unwrap();
            let vertex = world
                .objects
                .components
                .surfaces
                .meshes
                .get(component.mesh)
                .unwrap()
                .vertices[0]
                .map(|v| i32::from(v) * 8);
            let mut point = c.pose.matrix.local_to_world(vertex);
            for i in 0..3 {
                point[i] += c.pose.position[i];
            }
            point
        });
        if object.is_some() {
            let (scene, _) = scene_export::encode(&world.objects.world);
            let config = deform_assets::encode(world);
            let bytes = |words: &[u16]| {
                words
                    .iter()
                    .flat_map(|v| v.to_le_bytes())
                    .collect::<Vec<_>>()
            };
            if out.join("scene.bin").exists() {
                assert_eq!(fs::read(out.join("scene.bin")).unwrap(), bytes(&scene));
            }
            fs::write(out.join("scene.bin"), bytes(&scene)).unwrap();
            fs::write(out.join("deform.bin"), bytes(&config)).unwrap();
        }
        for (i, original) in originals.iter().enumerate() {
            let mut p = original.clone();
            let v = &mut p.physics;
            v.motion = motion::State::default();
            v.powertrain = powertrain::State::default();
            v.contact = ground::Pipeline::default();
            v.collision = Default::default();
            v.impact_cooldown = if object.is_some() { 500 } else { 0 };
            if object.is_some() && i == 0 {
                v.entity = entity;
            }
            v.player = i == 0;
            p.slot = i as u8;
            p.visible = true;
            p.damage = Default::default();
            p.prefix.boost = Default::default();
            v.pose = p.model.pose_config([0; 3]);
            let z = start.map_or(9072, |p| p[2]) + i as i32 * separation;
            let x = start.map_or(-235, |p| p[0]);
            let mut height = 0;
            assert!(terrain::height(&terrain, x, z, &mut height).unwrap());
            v.motion.poses.physical_position = [
                x,
                height - p.model.primary_local[0][1] - v.pose.track_offsets[0][1],
                z,
            ];
            v.motion.poses.body.matrix = Matrix::IDENTITY;
            v.motion.poses.reference = Matrix::IDENTITY;
            v.motion
                .poses
                .update(
                    &v.pose,
                    [0; 4],
                    &pose::Environment {
                        timer: 0,
                        ambient_wobble: false,
                        session_active: true,
                        ambient_phase: 0,
                        recoil_mode: 0,
                        turret_yaw: 0,
                        recoil: 0,
                        gun_pitch: 0,
                        external: pose::ExternalBasis::Body,
                    },
                )
                .unwrap();
            v.motion.velocity = [0, 0, velocities[i]];
            v.pose_input.external = pose::ExternalBasis::Body;
            for j in 0..4 {
                v.contact.sampling.primary.points[j].local = p.model.primary_local[j];
                v.contact.response.points[j].corner = j as u32;
            }
            world.vehicles.entries[i] = Some(p);
        }
        world.vehicles.racers = [None; 9];
        world.vehicles.racers[0] = Some(tankracer_game::object_query::TankId(0));
        world.vehicles.racers[1] = Some(tankracer_game::object_query::TankId(1));
        if object.is_some() {
            world.vehicles.racers[1] = None;
        }
        world.shared.random = 0x1234abcd;
        world.shared.noise = ground::initialize_noise(&mut world.shared.random);
        world.shared.pool = effect_pool::Pool::default();
        world.shared.impacts = impact_particles::State::default();
        let noise = world.shared.noise;
        let initial_seed = world.shared.random;
        let mut stages = std::array::from_fn(|i| {
            let p = world.vehicles.entries[i].as_ref().unwrap();
            reference::drive_fixture::flatten(&p.physics.powertrain.drive_state(&p.physics.motion))
        });
        let initial = snapshot(&world, &stages);
        let entities: Vec<_> = world.vehicles.entries[..2]
            .iter()
            .map(|p| p.as_ref().unwrap().physics.entity.0)
            .collect();
        let mut frames = Vec::new();
        let mut hits = 0;
        let mut deform_hits = 0;
        let mut sampled_points = 0;
        let mut spark_events = 0;
        let before_mesh = deform_assets::capture(world);
        let actors = if object.is_some() { 1 } else { 2 };
        let mut visited = std::collections::BTreeSet::new();
        for frame in 0..steps {
            let number = frame as u32 + 1;
            world.frame = number;
            for own in 0..actors {
                let input = drive::Input {
                    throttle: if velocities[own] < 0 { -65535 } else { 65535 },
                    ..Default::default()
                };
                let p = world.vehicles.entries[own].as_mut().unwrap();
                let v = &mut p.physics;
                let mut deformable = rcc_probe::surface(
                    &mut world.objects.components.surfaces,
                    &world.objects.world.colliders,
                    v.collision.deformable,
                )
                .unwrap();
                let dt = 33;
                let se = suspension::Environment {
                    dt,
                    is_player: v.player,
                    session_active: 1,
                    player_multiplier: env[11].as_u64().unwrap() as u16,
                    player_shift: env[12].as_u64().unwrap() as u32,
                    other_multiplier: env[13].as_u64().unwrap() as u16,
                    other_shift: env[14].as_u64().unwrap() as u32,
                    penetration_shift: tank.suspension.susp_to_shape.shift as u8 as u32,
                    base: [
                        v.motion.poses.base_position[0],
                        v.motion.poses.base_position[2],
                    ],
                    ..Default::default()
                };
                let sampling = ground::Environment {
                    tracks: v.motion.poses.tracks.map(|t| Transform {
                        basis: t.matrix.0,
                        origin: t.position,
                    }),
                    auxiliary_local: p.model.auxiliary_local,
                    force_surface: false,
                    effect_gain: contact::Coefficient {
                        multiplier: 3,
                        shift: 9,
                    },
                    load: (&tank.suspension).into(),
                    dt,
                    frame: number,
                    player: u32::from(v.player),
                };
                v.contact
                    .step_vehicle(
                        ground::Vehicle {
                            motion: &mut v.motion,
                            powertrain: &mut v.powertrain,
                            pose: &mut v.pose,
                        },
                        &ground::World {
                            terrain: &terrain,
                            surface_bumps: &surfaces.entries.map(|s| s.bump.into()),
                            noise,
                        },
                        &ground::Config {
                            sampling: &sampling,
                            suspension: &se,
                            contact: &contact,
                            layout,
                        },
                        &mut world.shared.random,
                        deformable.as_mut(),
                        |_| true,
                    )
                    .unwrap();
                // Coverage only: sampling a cloned diagnostic state does not
                // alter the live record/dirty writes used by the next frame.
                if let Some(surface) = deformable.as_mut() {
                    let mut state = surface.state.clone();
                    for point in &v.contact.sampling.primary.points {
                        sampled_points +=
                            usize::from(state.sample(&surface.geometry, point.world).offset != 0);
                    }
                }
                for point in v
                    .contact
                    .sampling
                    .primary
                    .points
                    .iter()
                    .map(|p| p.world)
                    .chain(
                        v.contact
                            .sampling
                            .auxiliary
                            .iter()
                            .filter(|p| p.cached != 0)
                            .map(|p| p.world),
                    )
                {
                    let x = ((point[0] >> 3) - terrain.origin[0]) >> terrain.shifts[0];
                    let z = ((point[2] >> 3) - terrain.origin[1]) >> terrain.shifts[1];
                    if (0..8).contains(&x) && (0..8).contains(&z) {
                        visited.insert((x + z * 8) as usize);
                    }
                }
                let de = drive::Environment {
                    dt,
                    mode: 0,
                    player: v.player,
                    active: true,
                    launch_eligible: false,
                    materials: v.contact.sampling.materials.map(|m| m as usize),
                    kinds: v.contact.sampling.primary.points.each_ref().map(|p| p.kind),
                    contact_y: v
                        .contact
                        .sampling
                        .primary
                        .points
                        .each_ref()
                        .map(|p| p.world[1]),
                    airborne: v.contact.sampling.primary.airborne_count as i8,
                    boost_charge: 0,
                    boost_power: 0,
                    extra_drive: 0,
                };
                let mut control = reference::drive_fixture::frame(&input, &de);
                control.extend([number as i32, 0, own as i32, 0]);
                v.powertrain
                    .step(
                        &mut v.motion,
                        &powertrain::Config {
                            drive: &tank.drive,
                            surfaces: &surfaces,
                            tuning: &tuning,
                            yaw: velocity::YawConfig::from(&tank.drive),
                        },
                        &powertrain::Frame {
                            input: &input,
                            drive: &de,
                            vertical_delta: v.contact.response.vertical_delta,
                            yaw_lock: 0,
                        },
                    )
                    .unwrap();
                stages[own] =
                    reference::drive_fixture::flatten(&v.powertrain.drive_state(&v.motion));
                let before_timer = v.collision.timer;
                let id = tankracer_game::object_query::TankId(own);
                rcc_probe::tanks(world, id, dt, 0).unwrap();
                hits += usize::from(
                    world.vehicles.entries[own]
                        .as_ref()
                        .unwrap()
                        .physics
                        .collision
                        .timer
                        != before_timer,
                );
                if object.is_some() {
                    let before = world.vehicles.entries[own]
                        .as_ref()
                        .unwrap()
                        .physics
                        .collision
                        .impact_accumulator;
                    rcc_probe::deformables(world, id, dt, 0).unwrap();
                    deform_hits += usize::from(
                        world.vehicles.entries[own]
                            .as_ref()
                            .unwrap()
                            .physics
                            .collision
                            .deformable
                            .is_some(),
                    );
                    spark_events += ((world.vehicles.entries[own]
                        .as_ref()
                        .unwrap()
                        .physics
                        .collision
                        .impact_accumulator
                        - before)
                        / dt) as usize;
                }
                rcc_car_probe::ground(world, id, dt, number).unwrap();
                let p = world.vehicles.entries[own].as_mut().unwrap();
                let v = &mut p.physics;
                let indices = p.model.gun_vertices;
                let original_z = p.model.gun_original_z;
                p.model.parts[Part::Gun.slot()]
                    .as_mut()
                    .unwrap()
                    .edit(|mesh| {
                        v.motion.integrate_and_pose(
                            &motion::Config {
                                attitude: &attitude::Config::from(&tank.suspension),
                                body: &body::Tuning::default(),
                                pose: &v.pose,
                            },
                            &motion::Frame {
                                dt,
                                number,
                                player: v.player,
                                mode: 0,
                                pose: motion::PoseInput {
                                    session_active: true,
                                    external: pose::ExternalBasis::Body,
                                    ..Default::default()
                                },
                            },
                            Some(body::GunMesh {
                                vertices: &mut mesh.vertices,
                                indices,
                                original_z,
                            }),
                        )
                    })
                    .unwrap()
                    .unwrap();
                world
                    .shared
                    .impacts
                    .tick(
                        v,
                        dt,
                        &mut impact_particles::Context {
                            frame: number,
                            random: &mut world.shared.random,
                            pool: &mut world.shared.pool,
                            tuning: impact_particles::Tuning::default(),
                        },
                    )
                    .unwrap();
                v.motion.finish_frame(dt);
                let mut expected = snapshot(&world, &stages);
                expected.push(world.shared.random as i32);
                let surface = if object.is_some() {
                    deform_assets::capture(world)
                } else {
                    Vec::new()
                };
                frames.push(json!({"control":control,"expected":expected,"surface":surface}));
            }
        }
        let after_mesh = deform_assets::capture(world);
        let mut changed_vertices = 0;
        let mut cursor = 0;
        for id in &world.objects.world.deformables {
            let component = world.objects.components.surfaces.entries[id.0]
                .as_ref()
                .unwrap();
            cursor += 5;
            let count = world
                .objects
                .components
                .surfaces
                .meshes
                .get(component.mesh)
                .unwrap()
                .vertices
                .len();
            for _ in 0..count {
                changed_vertices +=
                    usize::from(before_mesh[cursor..cursor + 3] != after_mesh[cursor..cursor + 3]);
                cursor += 3;
            }
        }
        assert!(
            hits > 0 || (deform_hits > 0 && changed_vertices > 0),
            "paired trajectories must exercise real car collisions"
        );
        if object.is_some() && steps >= 2 {
            assert!(
                sampled_points > 0,
                "deformed height must reach next-frame contacts"
            );
        }
        cells.extend(visited.iter().copied());
        exported.push(json!({"name":name,"cars":actors,"entities":entities,"initial":initial,"seed_after_noise":initial_seed,"noise":noise,
            "frames":frames,"cells":visited,"fallback_contacts":0,"tank_contacts":hits,"deform_contacts":deform_hits,
            "deformed_vertices":changed_vertices,"surface_sample_points":sampled_points,"deform_spark_events":spark_events,"scene":object.is_some()}));
    }
    fs::write(
        out.join("paired.json"),
        serde_json::to_vec(&json!({"cases":exported,"cells":cells})).unwrap(),
    )
    .unwrap();
    let mut collision = Vec::new();
    for cell in &cells {
        collision.extend(pages::cell_bytes(&terrain, *cell));
    }
    fs::write(out.join("paired-cells.bin"), collision).unwrap();
}
