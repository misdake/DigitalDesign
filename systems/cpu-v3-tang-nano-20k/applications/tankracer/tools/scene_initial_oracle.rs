//! Calls the original scene constructor helpers with independent input and RNG.
use serde_json::json;
use std::{fs, path::Path};
use tankracer_game::{math::Matrix, pose::Pose, scene_startup::rcc_initial_probe};
mod scene_actor_initial_oracle;

fn ints(out: &mut Vec<u16>, values: impl IntoIterator<Item = i32>) {
    for v in values {
        out.extend([v as u16, (v >> 16) as u16]);
    }
}
fn flock_fields(flock: &tankracer_game::actor_startup::flock::Flock) -> Vec<u16> {
    let mut out = Vec::new();
    ints(
        &mut out,
        [
            i32::from(flock.flight.phase),
            flock.flight.stage as i32,
            flock.flight.timer,
        ],
    );
    ints(&mut out, flock.flight.target);
    ints(&mut out, flock.flight.velocities.into_iter().flatten());
    ints(&mut out, flock.flight.wings_up.map(i32::from));
    for bird in flock.birds {
        ints(&mut out, bird.pose.position);
        ints(&mut out, bird.pose.matrix.0);
        ints(&mut out, [bird.speed, i32::from(bird.active)]);
    }
    assert_eq!(out.len(), 288);
    out
}

fn world_cases(
    root: &Path,
    level: &[u8],
    shared: &[u8],
    terrain: &tankracer_assets::terrain::TerrainData,
) -> Vec<serde_json::Value> {
    use tankracer_assets::{level::Level, parameters::Parameters};
    use tankracer_game::{
        actor_startup::Actor,
        vehicle_runtime::{Settings, Startup},
    };
    let data = fs::read(root.join("assets/game/DATA/FASTFILE.DAT")).unwrap();
    let archive = tankracer_assets::archive::Archive::parse(level).unwrap();
    let authored = Level::decode(archive.get("ENGB.DAT").unwrap()).unwrap();
    let mut cases = Vec::new();
    for seed in [0, 1, u32::MAX, 0x1234abcd] {
        let mut startup = Startup::new(
            std::sync::Arc::new(
                tankracer_assets::terrain::TerrainData::decode_bsm(
                    archive.get("ENGLAND.BSM").unwrap(),
                )
                .unwrap(),
            ),
            Settings {
                map: 0,
                mode: 0,
                special: false,
                environment_flag: 0,
                seed,
                spray_materials: tankracer_game::vehicle_scene::spray::Materials {
                    filth: 1,
                    crap: 2,
                    splash: 3,
                },
                light_material: 4,
                hardware: true,
            },
        )
        .unwrap();
        let noise = startup.shared.noise;
        let globals_seed = startup.shared.random;
        let mut shield = Vec::new();
        for bank in startup.shared.shield.banks {
            for orbit in bank.orbits {
                ints(
                    &mut shield,
                    [
                        orbit.pitch,
                        orbit.pitch_target,
                        orbit.yaw,
                        orbit.yaw_rate,
                        orbit.yaw_rate_target,
                        orbit.size,
                        i32::from(orbit.growing),
                        i32::from(orbit.spin),
                    ],
                );
                ints(&mut shield, orbit.position);
            }
            ints(&mut shield, [bank.tail as i32]);
        }
        let loaded = tankracer_game::scenery::construct_all(
            &mut startup,
            &data,
            level,
            shared,
            "ENGLAND",
            0,
        )
        .unwrap();
        assert_eq!(loaded.construction.len(), authored.objects.len());
        let mut records = Vec::new();
        for (record, trace) in authored.objects.iter().zip(&loaded.construction) {
            let class = record.class.to_ascii_lowercase();
            let kind = match class.as_str() {
                "crush" => 1,
                "flock" => 2,
                "bomber" => 3,
                "waypoint" => 4,
                "powerup" => 5,
                "obstacle" | "fall" | "bounce" | "closer" | "spin" | "mover" | "noisy"
                | "smoker" | "trackcam" | "train" => 0,
                _ => panic!("unmapped authored constructor {}", record.class),
            };
            let p = Parameters::new(record.parameters).unwrap();
            let id = startup
                .objects
                .world
                .colliders
                .iter()
                .position(|v| v.entity == trace.entity)
                .unwrap();
            let object = startup.objects.components.objects[id].as_ref().unwrap();
            let collider = &startup.objects.world.colliders[id];
            let visibility = object.data.visibility;
            let mut expected = Vec::new();
            if kind != 4 && kind != 5 {
                ints(
                    &mut expected,
                    [
                        i32::from(visibility.distance),
                        i32::from(visibility.add_lap),
                        i32::from(visibility.remove_lap),
                        i32::from(visibility.entered),
                    ],
                );
            }
            let debris = object.data.debris.as_ref();
            let sprite_mode = p.find(":shatter3d").is_none() && p.find(":shatter3dvfp").is_some();
            let piece_count = if let Some(mut values) = p.find(":shatter3d") {
                usize::try_from(values.integer().unwrap() as i16).unwrap()
            } else if let Some(mut values) = p.find(":shatter3dvfp") {
                values.word().unwrap();
                values.integer().unwrap();
                usize::try_from(values.integer().unwrap() as i16).unwrap()
            } else {
                0
            };
            assert_eq!(piece_count, debris.map_or(0, |d| d.pieces.len()));
            let mut authored_positions = Vec::new();
            if let Some(mut values) = p.find(":terrys") {
                for _ in 0..piece_count {
                    let position: [i32; 3] = std::array::from_fn(|_| {
                        tankracer_assets::parameters::fixed1024(values.real32().unwrap())
                    });
                    authored_positions.push(position);
                }
            }
            if let Some(debris) = debris {
                for piece in &debris.pieces {
                    ints(&mut expected, piece.position);
                    ints(&mut expected, piece.velocity);
                    ints(&mut expected, piece.matrix.0);
                    ints(&mut expected, piece.angular);
                    ints(
                        &mut expected,
                        [piece.previous_tilt, piece.flags as i32, piece.sprite_angle],
                    );
                    ints(&mut expected, piece.translation);
                }
            }
            let mut glows = Vec::new();
            let mut vertex_count = 0;
            if kind == 1 {
                let surface = startup.objects.components.surfaces.entries[id]
                    .as_ref()
                    .unwrap();
                vertex_count = startup
                    .objects
                    .components
                    .surfaces
                    .meshes
                    .get(surface.mesh)
                    .unwrap()
                    .vertices
                    .len();
                assert_eq!(vertex_count, surface.vertex_flags.len());
                if let Some(mut values) = p.find(":police_glow") {
                    let count = values.integer().unwrap();
                    for _ in 0..count {
                        let color = [
                            values.integer().unwrap() as u8,
                            values.integer().unwrap() as u8,
                            values.integer().unwrap() as u8,
                        ];
                        let position = [
                            values.integer().unwrap(),
                            values.integer().unwrap(),
                            values.integer().unwrap(),
                        ];
                        glows.push([
                            position[0],
                            position[1],
                            position[2],
                            i32::from(color[0]),
                            i32::from(color[1]),
                            i32::from(color[2]),
                        ]);
                    }
                }
                assert_eq!(glows.len(), surface.sampling.state.records.len());
                for glow in &surface.sampling.state.records {
                    ints(&mut expected, glow.position);
                    ints(&mut expected, glow.world_position);
                    ints(
                        &mut expected,
                        [
                            glow.timer,
                            i32::from(glow.state),
                            i32::from(glow.color[0]),
                            i32::from(glow.color[1]),
                            i32::from(glow.color[2]),
                        ],
                    );
                }
                expected.extend(surface.vertex_flags.iter().map(|v| u16::from(*v)));
            }
            if kind == 2 {
                let Some(Actor::Flock(flock)) = startup
                    .objects
                    .components
                    .actors
                    .get(tankracer_game::object_query::ObstacleId(id))
                else {
                    panic!("flock owner")
                };
                expected.extend(flock_fields(flock));
            }
            if kind == 3 {
                let Some(Actor::Emitter(emitter)) = startup
                    .objects
                    .components
                    .actors
                    .get(tankracer_game::object_query::ObstacleId(id))
                else {
                    panic!("emitter owner")
                };
                ints(&mut expected, [emitter.timer]);
            }
            let mut pose = Pose::from_level(record);
            if kind == 2 {
                let mut height = tankracer_game::terrain::MISS_HEIGHT;
                tankracer_game::terrain::height(
                    terrain,
                    pose.position[0],
                    pose.position[2],
                    &mut height,
                )
                .unwrap();
                pose.position[1] = height;
                assert_eq!(pose.position, collider.pose.position);
            }
            let distance = if matches!(
                class.as_str(),
                "obstacle" | "crush" | "fall" | "bounce" | "closer" | "spin" | "mover"
            ) {
                p.integer(":vis_dist").unwrap_or(150) as u16
            } else {
                150
            };
            let remove = p.integer(":lap_to_remove");
            let mut input = vec![
                kind,
                u16::try_from(vertex_count).unwrap(),
                u16::try_from(glows.len()).unwrap(),
                u16::from(remove.is_some()),
            ];
            ints(
                &mut input,
                [p.integer(":lap_to_add").unwrap_or(0), remove.unwrap_or(0)],
            );
            ints(&mut input, pose.matrix.0);
            ints(&mut input, pose.position);
            input.extend([p.integer(":manu").unwrap_or(68) as u16, distance]);
            input.extend([
                u16::try_from(piece_count).unwrap(),
                u16::from(sprite_mode),
                u16::from(p.find(":terrys").is_some()),
            ]);
            assert_eq!(input.len(), 37);
            for glow in &glows {
                ints(&mut input, *glow);
            }
            for position in authored_positions {
                ints(&mut input, position);
            }
            records.push(json!({"class":record.class,"name":record.name,"shape":record.shape,"has_shape":object.data.mesh.is_some(),"remove_rule":p.find(":lap_to_remove").is_some(),"entity":trace.entity.0,
                "input":input,"expected":expected,"random_before":trace.random_before,"random_after":trace.random_after}));
        }
        cases.push(
            json!({"seed":seed,"noise":noise,"shield":shield,"globals_seed":globals_seed,
            "records":records,"final_seed":startup.shared.random}),
        );
    }
    cases
}
fn main() {
    let root = std::env::args().nth(1).unwrap();
    let out = std::env::args().nth(2).unwrap();
    let level = fs::read(Path::new(&root).join("assets/game/LEVELS/ENGLAND/FASTFILE.DAT")).unwrap();
    let shared = fs::read(Path::new(&root).join("assets/game/INGAME/FASTFILE.DAT")).unwrap();
    let archive = tankracer_assets::archive::Archive::parse(&level).unwrap();
    let terrain =
        tankracer_assets::terrain::TerrainData::decode_bsm(archive.get("ENGLAND.BSM").unwrap())
            .unwrap();
    let seeds = [
        0,
        1,
        u32::MAX,
        0x80000000,
        0x12345678,
        0x1234abcd,
        0x00100001,
        0xdeadbeef,
    ];
    scene_actor_initial_oracle::export(
        Path::new(&root),
        Path::new(&out),
        &level,
        &shared,
        &terrain,
    );
    fs::write(Path::new(&out).join("scene-initial-world.json"),serde_json::to_vec_pretty(&json!({
        "cases":world_cases(Path::new(&root),&level,&shared,&terrain),
        "scope":"Real authored ENGLAND constructor order, initial noise/shield, every object RNG boundary and live randomized fields; immutable geometry/parameters are prepared from original resources. Membership publication and deterministic actor construction remain outside this qualifier."
    })).unwrap()).unwrap();
    let adds = [0, -99, 2, 3, i32::MIN, i32::MAX, -1, -99];
    let removes = [
        None,
        Some(0),
        Some(1),
        Some(65536),
        Some(i32::MIN),
        Some(i32::MAX),
        Some(-1),
        None,
    ];
    let sizes = [0, 1, 2, 3, 4, 17, 63, 129];
    let mut cases = Vec::new();
    for initial in seeds {
        for branch in 0..8 {
            let add = adds[branch];
            let remove = removes[branch];
            let mut parameters = format!(":lap_to_add {add} ");
            if let Some(value) = remove {
                parameters += &format!(":lap_to_remove {value} ");
            }
            let mut seed = initial;
            let visibility = rcc_initial_probe::visibility(&parameters, &mut seed);
            let count = branch % 5;
            let mut glow_parameters = format!(":police_glow {count} ");
            let mut input = Vec::new();
            for i in 0..count {
                let color = [128 + i as i32 * 83, -1 + i as i32 * 31, 300 - i as i32 * 53];
                let position = [1200 * (i as i32 + 1), -700 * branch as i32, -913 * i as i32];
                for value in color.into_iter().chain(position) {
                    glow_parameters += &format!("{value} ");
                }
                input.push([
                    position[0],
                    position[1],
                    position[2],
                    i32::from(color[0] as u8),
                    i32::from(color[1] as u8),
                    i32::from(color[2] as u8),
                ]);
            }
            let pose = Pose {
                matrix: Matrix::from_angles([
                    branch as u32 * 1301,
                    branch as u32 * 7919,
                    branch as u32 * 341,
                ]),
                position: [i32::MAX - 12345, -900000, 712345],
            };
            let records = rcc_initial_probe::glows(&glow_parameters, pose, &mut seed);
            let flags = rcc_initial_probe::vertex_flags(sizes[branch], &mut seed);
            let maneuver = [68, 0, -1, -32768, 32767, 65535, 65536, -99][branch];
            let mut flock_pose = Pose {
                matrix: Matrix::IDENTITY,
                position: [-235, 1000, 9072],
            };
            let flock = tankracer_game::scenery::rcc_actor_initial_probe::flock(
                &level,
                &shared,
                &terrain,
                &format!(":manu {maneuver} "),
                &mut flock_pose,
                &mut seed,
            );
            let mut flock_values = Vec::new();
            ints(
                &mut flock_values,
                [
                    i32::from(flock.flight.phase),
                    flock.flight.stage as i32,
                    flock.flight.timer,
                ],
            );
            ints(&mut flock_values, flock.flight.target);
            ints(
                &mut flock_values,
                flock.flight.velocities.into_iter().flatten(),
            );
            ints(&mut flock_values, flock.flight.wings_up.map(i32::from));
            for bird in flock.birds {
                ints(&mut flock_values, bird.pose.position);
                ints(&mut flock_values, bird.pose.matrix.0);
                ints(&mut flock_values, [bird.speed, i32::from(bird.active)]);
            }
            assert_eq!(flock_values.len(), 288);
            let emitter_timer = tankracer_game::scenery::rcc_actor_initial_probe::emitter(
                &level, &shared, &terrain, &mut seed,
            );
            let mut camera_pose = pose;
            if branch == 7 {
                camera_pose.matrix.0[7] = 4097;
            }
            let near = [i32::MAX - 12, i32::MIN + 13, branch as i32 * 137 - 9000];
            let camera =
                tankracer_game::world_startup::rcc_starting_camera(camera_pose, near, &mut seed);
            cases.push(json!({"seed":initial,"add":add,"remove":remove,
                "basis":pose.matrix.0,"origin":pose.position,"records":input,
                "flag_count":sizes[branch],"visibility":visibility,"glows":records,
                "flags":flags,"flock":flock_values,"flock_origin":flock_pose.position,"maneuver":maneuver as i16,"emitter_timer":emitter_timer,
                "camera_basis":camera_pose.matrix.0,"camera_near":near,"camera":camera,"final_seed":seed}));
        }
    }
    fs::write(Path::new(&out).join("scene-initial.json"),
        serde_json::to_vec_pretty(&json!({"cases":cases,"scope":"Original visibility, glows and vertex flags, resource-built flock state, emitter timer and starting camera on one shared RNG."})).unwrap()).unwrap();
}
