//! Original ground pipeline and physical calls on persistent real-map trajectories.
mod driving_ground;
#[allow(dead_code)]
#[path = "terrain_assets.rs"]
mod pages;
#[allow(dead_code, clippy::needless_range_loop, clippy::manual_range_contains)]
#[path = "vehicle_oracle.rs"]
mod reference;
use serde_json::{Value, json};
use std::{fs, path::Path};
use tankracer_assets::{
    archive::Archive,
    config::{Part, ResolveContext, Surfaces, Tank},
    model::ShapeArchive,
    object_catalog::Catalog,
    terrain::TerrainData,
};
use tankracer_game::{
    attitude, body, contact,
    contact::Coefficient,
    contact_pipeline as ground, drive,
    math::Matrix,
    motion, pose, powertrain,
    sampling::Transform,
    suspension, terrain,
    vehicle_model::{BuildContext, Bundles, ModelBank, VehicleModel},
    vehicle_prefix::boost,
    velocity,
};
mod driving_capture;
use driving_capture::{body_state, state};
fn gain(out: &mut Vec<i32>, g: tankracer_assets::config::Gain) {
    out.extend([g.multiplier as i32, g.shift as u8 as i32]);
}
fn fallback_start(data: &TerrainData, local: [i32; 3], track: [i32; 3]) -> [i32; 3] {
    // Place the first contact at a real fallback face whose primary query misses.
    for node in &data.fallback.nodes {
        let mesh = &data.meshes[node.mesh as usize];
        if mesh.flags & 4 != 0 {
            continue;
        }
        for center in &mesh.face_centers {
            let world = std::array::from_fn(|i| (mesh.position[i] + i32::from(center[i])) * 8);
            let mut hit = terrain::Hit::default();
            let mut normal = [0; 3];
            if matches!(
                terrain::query(data, world[0], world[2], &mut hit),
                Ok(false)
            ) && tankracer_game::geometry::fallback(data, world, 1, &mut normal)
                && (normal[0] != 0 || normal[2] != 0)
            {
                return std::array::from_fn(|i| world[i] - local[i] - track[i]);
            }
        }
    }
    panic!("ENGLAND has no qualified fallback-face driving start");
}
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let root = Path::new(&args[0]);
    let out = Path::new(&args[1]);
    let steps: usize = args[2].parse().unwrap();
    fs::create_dir_all(out).unwrap();
    let bytes = fs::read(root.join("assets/game/DATA/FASTFILE.DAT")).unwrap();
    let a = Archive::parse(&bytes).unwrap();
    let tank = Tank::decode(a.get("TANK1.DAT").unwrap()).unwrap();
    let surfaces = Surfaces::decode(a.get("SURFS.DAT").unwrap()).unwrap();
    let catalog = Catalog::decode_checked(a.get("OBPARAMS.DAT").unwrap()).unwrap();
    let bytes = fs::read(root.join("assets/game/LEVELS/ENGLAND/FASTFILE.DAT")).unwrap();
    let a = Archive::parse(&bytes).unwrap();
    let data = TerrainData::decode_bsm(a.get("ENGLAND.BSM").unwrap()).unwrap();
    let bytes = fs::read(root.join("assets/game/TANKS/BRONZE/FASTFILE.DAT")).unwrap();
    let a = Archive::parse(&bytes).unwrap();
    let shared = fs::read(root.join("assets/game/INGAME/FASTFILE.DAT")).unwrap();
    let sa = Archive::parse(&shared).unwrap();
    let bundles = Bundles {
        vehicle: vec![ShapeArchive::parse(a.get("TANKB.SS").unwrap()).unwrap()],
        shared: vec![ShapeArchive::parse(sa.get("INGAME.SS").unwrap()).unwrap()],
        level: vec![],
    };
    let model = VehicleModel::build(
        &tank,
        &bundles,
        &mut ModelBank::default(),
        &surfaces,
        BuildContext {
            resolve: ResolveContext {
                player: true,
                ..ResolveContext::default()
            },
            ambient_wobble: false,
            session_active: false,
        },
    )
    .unwrap();
    let pose = model.pose_config([0; 3]);
    let original_gun = model.parts[Part::Gun.slot()]
        .as_ref()
        .unwrap()
        .geometry()
        .vertices
        .clone();
    let body = model.parts[Part::Body.slot()].as_ref().unwrap();
    assert!(
        body.is_private(),
        "player body damage requires instance-owned geometry"
    );
    let original_body = body.geometry().clone();
    let selected = model.damage_vertices[0];
    let mut body_data: Vec<i32> = selected.map(i32::from).to_vec();
    body_data.extend(
        original_body
            .vertices
            .iter()
            .flatten()
            .copied()
            .map(i32::from),
    );
    body_data.extend(
        original_body
            .triangles
            .iter()
            .flat_map(|t| t.indices.map(i32::from)),
    );
    body_data.extend(
        original_body
            .triangles
            .iter()
            .flat_map(|t| t.normal.map(i32::from)),
    );
    assert!(original_gun.len() <= 256);
    let mut binding = vec![1, original_gun.len() as i32];
    binding.extend(model.gun_vertices.map(i32::from));
    binding.extend(model.gun_original_z.map(i32::from));
    let bounds = model.geometry_bounds;
    let mut collision_binding =
        vec![model.parts[Part::Body.slot()].as_ref().unwrap().radius as i32];
    collision_binding.extend(model.collision_bounds.min);
    collision_binding.extend(model.collision_bounds.max);
    collision_binding.extend(bounds.min);
    collision_binding.extend(bounds.max);
    let layout = [
        bounds.min[0],
        bounds.min[1],
        bounds.min[2],
        bounds.max[0],
        bounds.max[2],
    ];
    let sc = contact::Config::try_from(&tank.suspension).unwrap();
    let s = &tank.suspension;
    let mut spring = vec![sc.lower, sc.upper];
    for g in [
        sc.force_positive,
        sc.force_negative,
        sc.output_gain,
        sc.tilt_gain,
    ] {
        spring.extend([g.multiplier as i32, g.shift as i32]);
    }
    spring.push(sc.shape_shift as i32);
    for g in [sc.restore_positive, sc.restore_negative] {
        spring.extend([g.multiplier as i32, g.shift as i32]);
    }
    spring.extend([
        sc.damping_shift as i32,
        sc.travel_dead_step as i32,
        s.initial_weight,
        s.initial_pressure,
        s.pressure_fraction.multiplier as i32,
        s.pressure_fraction.shift as u8 as i32,
    ]);
    spring.extend(layout);
    assert_eq!(spring.len(), 26);
    let tuning = drive::Tuning::default();
    let mut drive = reference::drive_fixture::config(&tank, &surfaces, &tuning);
    drive.extend([catalog.turbo.player_power, catalog.turbo.player_loss]);
    let mut pc = vec![
        pose.travel_gain.multiplier as i32,
        pose.travel_gain.shift as i32,
        pose.track_length,
    ];
    for t in pose.track_offsets {
        pc.extend(t);
    }
    let mut bumps = Vec::new();
    for surface in surfaces.entries {
        gain(&mut bumps, surface.bump);
    }
    bumps.extend([3, 9]);
    // Bump/effect coefficients are u16 pairs; load coefficients are raw i32 pairs.
    let load = vec![
        s.initial_weight,
        s.initial_pressure,
        s.pressure_fraction.multiplier as i32,
        s.pressure_fraction.shift as u8 as i32,
    ];
    let mut cases = Vec::<Value>::new();
    let mut starts = Vec::new();
    for (name, z, velocity, force, boosted, effect) in [
        ("start-drive", 9072, 45000, false, false, false),
        ("cell-boundary", 72600, 60000, false, false, false),
        ("reverse-effect", 9072, -15000, true, true, true),
    ] {
        let mut height = 0;
        assert!(terrain::height(&data, -235, z, &mut height).unwrap());
        starts.push((
            name,
            [
                -235,
                height - model.primary_local[0][1] - pose.track_offsets[0][1],
                z,
            ],
            velocity,
            force,
            boosted,
            effect,
        ));
    }
    let wall = fallback_start(&data, model.primary_local[0], pose.track_offsets[0]);
    starts.push(("fallback-forward", wall, 600000, false, false, false));
    starts.push(("fallback-reverse", wall, -600000, false, false, false));
    let mut cells = std::collections::BTreeSet::new();
    for (name, position, velocity, force, boosted, effect) in starts {
        let dt = 33;
        let mut live_pose = pose.clone();
        let mut gun = original_gun.clone();
        let mut contacts = driving_ground::State::new(original_body.clone(), selected);
        let mut seed = 0x1234abcdu32;
        let noise = ground::initialize_noise(&mut seed);
        let world = ground::World {
            terrain: &data,
            surface_bumps: &surfaces.entries.map(|s| s.bump.into()),
            noise,
        };
        let mut m = motion::State::default();
        m.poses.body.matrix = Matrix::IDENTITY;
        m.poses.reference = Matrix::IDENTITY;
        m.poses.physical_position = position;
        let pe = pose::Environment {
            timer: 0,
            ambient_wobble: false,
            session_active: true,
            ambient_phase: 0,
            recoil_mode: 0,
            turret_yaw: 0,
            recoil: 0,
            gun_pitch: 0,
            external: pose::ExternalBasis::Body,
        };
        m.poses.update(&live_pose, [0; 4], &pe).unwrap();
        m.velocity = [0, 0, velocity];
        let mut p = powertrain::State::default();
        let mut b = boost::State::default();
        let mut g = ground::Pipeline::default();
        for i in 0..4 {
            g.sampling.primary.points[i].local = model.primary_local[i];
            g.response.points[i].corner = i as u32;
        }
        if effect {
            g.sampling.effect = ground::SurfaceEffect {
                timer: 70,
                restore: 20,
                active: 1,
                impulse: [300, 1800],
                pose: 20,
                ..Default::default()
            };
        }
        if effect {
            m.articulation = body::Articulation {
                recoil: 70,
                recoil_rate: 13,
                pitch: -99,
                pitch_rate: 123,
            };
        }
        let mut initial = state(&m, &p, &b, &g, &data);
        initial.extend(body_state(&m, &live_pose, &gun));
        initial.extend(contacts.snapshot(seed));
        let car_bindings = contacts.bindings();
        let initial_seed = seed;
        let mut frames = Vec::new();
        let mut visited = Vec::new();
        let mut fallback_contacts = 0;
        let env = suspension::Environment {
            dt,
            is_player: true,
            session_active: 1,
            player_multiplier: catalog.moon.player.multiplier,
            player_shift: catalog.moon.player.shift as u8 as u32,
            other_multiplier: catalog.moon.other.multiplier,
            other_shift: catalog.moon.other.shift as u8 as u32,
            penetration_shift: s.susp_to_shape.shift as u8 as u32,
            ..Default::default()
        };
        let flat_env = vec![
            dt,
            0,
            1,
            0,
            1,
            0,
            0,
            0x4a77f,
            0xfffb4856u32 as i32,
            0x813d2,
            0xffff80ecu32 as i32,
            env.player_multiplier as i32,
            env.player_shift as i32,
            env.other_multiplier as i32,
            env.other_shift as i32,
            env.penetration_shift as i32,
            0,
            0,
        ];
        for index in 0..steps {
            let number = index as u32 + 1;
            let applied = boosted && index == 0;
            if applied {
                b.apply(
                    tank.drive.turbo_amount,
                    catalog.turbo.player_power,
                    catalog.turbo.player_loss,
                );
            }
            b.decay(dt, true);
            let input = drive::Input {
                throttle: if index < steps * 2 / 3 {
                    if velocity < 0 { -65535 } else { 65535 }
                } else {
                    0
                },
                brake: if index >= steps * 2 / 3 { 65535 } else { 0 },
                steering: if index % 16 < 8 { 1000 } else { -1000 },
                ..Default::default()
            };
            let sample_env = ground::Environment {
                tracks: m.poses.tracks.map(|t| Transform {
                    basis: t.matrix.0,
                    origin: t.position,
                }),
                auxiliary_local: model.auxiliary_local,
                force_surface: force,
                effect_gain: Coefficient {
                    multiplier: 3,
                    shift: 9,
                },
                load: (&tank.suspension).into(),
                dt,
                frame: number,
                player: 1,
            };
            let se = suspension::Environment {
                base: [m.poses.base_position[0], m.poses.base_position[2]],
                ..env
            };
            g.step_vehicle(
                ground::Vehicle {
                    motion: &mut m,
                    powertrain: &mut p,
                    pose: &mut live_pose,
                },
                &world,
                &ground::Config {
                    sampling: &sample_env,
                    suspension: &se,
                    contact: &sc,
                    layout,
                },
                &mut seed,
                None,
                |_| true,
            )
            .unwrap();
            fallback_contacts += g
                .sampling
                .primary
                .points
                .iter()
                .filter(|p| p.kind == 4)
                .count();
            for world in g.sampling.primary.points.iter().map(|p| p.world).chain(
                g.sampling
                    .auxiliary
                    .iter()
                    .filter(|p| p.cached != 0)
                    .map(|p| p.world),
            ) {
                let x = ((world[0] >> 3) - data.origin[0]) >> data.shifts[0];
                let z = ((world[2] >> 3) - data.origin[1]) >> data.shifts[1];
                let cell = x + z * 8;
                if (0..8).contains(&x) && (0..8).contains(&z) {
                    cells.insert(cell as usize);
                    if !visited.contains(&cell) {
                        visited.push(cell);
                    }
                }
            }
            let de = drive::Environment {
                dt,
                mode: 0,
                player: true,
                active: true,
                launch_eligible: false,
                materials: g.sampling.materials.map(|m| m as usize),
                kinds: g.sampling.primary.points.each_ref().map(|p| p.kind),
                contact_y: g.sampling.primary.points.each_ref().map(|p| p.world[1]),
                airborne: g.sampling.primary.airborne_count as i8,
                boost_charge: b.timer,
                boost_power: b.extra_speed,
                extra_drive: b.power,
            };
            let mut f = reference::drive_fixture::frame(&input, &de);
            f.extend([number as i32, i32::from(applied), 0, i32::from(force)]);
            assert_eq!(f.len(), 30);
            p.step(
                &mut m,
                &powertrain::Config {
                    drive: &tank.drive,
                    surfaces: &surfaces,
                    tuning: &tuning,
                    yaw: velocity::YawConfig::from(&tank.drive),
                },
                &powertrain::Frame {
                    input: &input,
                    drive: &de,
                    vertical_delta: g.response.vertical_delta,
                    yaw_lock: 0,
                },
            )
            .unwrap();
            let stage = reference::drive_fixture::flatten(&p.drive_state(&m));
            contacts.respond(&mut m, &p, &mut g, number, &mut seed);
            m.integrate_and_pose(
                &motion::Config {
                    attitude: &attitude::Config::from(s),
                    body: &body::Tuning::default(),
                    pose: &live_pose,
                },
                &motion::Frame {
                    dt,
                    number,
                    player: true,
                    mode: 0,
                    pose: motion::PoseInput {
                        session_active: true,
                        external: pose::ExternalBasis::Body,
                        ..Default::default()
                    },
                },
                Some(body::GunMesh {
                    vertices: &mut gun,
                    indices: model.gun_vertices,
                    original_z: model.gun_original_z,
                }),
            )
            .unwrap();
            contacts.tick(&m, &p, number, dt, &mut seed);
            m.finish_frame(dt);
            let mut expected = state(&m, &p, &b, &g, &data);
            expected[73..89].copy_from_slice(&stage);
            expected.extend(body_state(&m, &live_pose, &gun));
            expected.extend(contacts.snapshot(seed));
            expected.push(seed as i32);
            frames.push(json!({"control":f,"expected":expected}));
        }
        if name.starts_with("fallback-") {
            assert!(
                fallback_contacts > 0,
                "fallback trajectory must reach ground response"
            );
            assert!(
                contacts.damage.tracks.iter().any(|v| *v > 0),
                "high-speed fallback must cause live track/body damage"
            );
        }
        cases.push(json!({"name":name,"car_bindings":car_bindings,"initial":initial,"seed_after_noise":initial_seed,"noise":noise,"environment":flat_env,"frames":frames,"cells":visited,"fallback_contacts":fallback_contacts}));
    }
    let mut collision = Vec::new();
    let mut directory = [65535u16; 64];
    for (page, cell) in cells.into_iter().enumerate() {
        directory[cell] = page as u16;
        collision.extend(pages::cell_bytes(&data, cell));
    }
    fs::write(out.join("cells.bin"), collision).unwrap();
    fs::write(
        out.join("directory.bin"),
        directory
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    fs::write(out.join("driving.json"),serde_json::to_vec_pretty(&json!({"cases":cases,"spring":spring,"drive":drive,"pose":pc,"attitude":[s.accel_to_rot.multiplier,s.accel_to_rot.shift as u8 as u16,s.steer_to_rot.multiplier,s.steer_to_rot.shift as u8 as u16,0],"bumps":bumps,"load":load,"auxiliary":model.auxiliary_local,"body":[15,1,3,1,14,6],"gun_binding":binding,"gun_words":original_gun.len()*3,"body_data":body_data,"collision_binding":collision_binding,"body_vertices":original_body.vertices.len(),"body_faces":original_body.triangles.len(),"steps":steps,"scope":"persistent real contact adapter, suspension/power, ground reflection/impact/damage, motion, articulation, lean, live gun/body vertices, contact hull, shared impact animation/pool and RNG; excludes scene/tank response dispatch and other rendered part poses"})).unwrap()).unwrap();
}
