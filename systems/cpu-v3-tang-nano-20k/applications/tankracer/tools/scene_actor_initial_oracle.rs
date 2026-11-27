//! Actual Train and Pickup constructors over original first-map resources.
use serde_json::json;
use std::{fs, path::Path};
use tankracer_assets::{
    archive::Archive,
    level::{Level, Object},
    parameters::{Parameters, fixed1024},
    terrain::TerrainData,
};
use tankracer_game::{
    actor_startup::{
        pickup::Setup,
        train::{Motion, Train},
    },
    pose::Pose,
    scenery::rcc_actor_initial_probe,
};
fn ints(out: &mut Vec<u16>, values: impl IntoIterator<Item = i32>) {
    for value in values {
        out.extend([value as u16, (value >> 16) as u16]);
    }
}
fn point(p: Parameters<'_>, lower: &str, upper: &str) -> Option<[i32; 3]> {
    p.find(lower)
        .or_else(|| p.find(upper))
        .map(|mut v| std::array::from_fn(|_| fixed1024(v.real32().unwrap())))
}
fn travel(out: &mut Vec<u16>, t: &Train) {
    let v = &t.travel;
    ints(
        out,
        [
            i32::from(v.active),
            i32::from(v.running),
            i32::from(v.waiting),
            v.wait,
            v.angle,
        ],
    );
    ints(out, v.tilt);
    ints(out, v.angular_velocity);
    ints(out, [i32::from(v.sound_active), i32::from(v.lander_flying)]);
    ints(out, v.lander_direction);
    ints(out, v.lander_velocity);
}
fn setup_fields(setup: &Setup) -> Vec<u16> {
    let mut out = Vec::new();
    ints(
        &mut out,
        [
            setup.map as i32,
            setup.cup as i32,
            i32::from(setup.tilt),
            i32::from(setup.lap),
            setup.geometry.separation,
            setup.geometry.diameter as i32,
        ],
    );
    out
}
pub fn export(root: &Path, out: &Path, level: &[u8], shared: &[u8], terrain: &TerrainData) {
    let archive = Archive::parse(level).unwrap();
    let authored = Level::decode(archive.get("ENGB.DAT").unwrap()).unwrap();
    let mut parameters = Vec::new();
    for record in &authored.objects {
        if record.class.eq_ignore_ascii_case("Train") {
            parameters.push(record.parameters.to_owned());
        }
    }
    assert_eq!(parameters.len(), 2);
    for params in [
        "",
        ":start_pos 1 2 3 :end_pos 1 2 3 ",
        ":start_pos -10 15 20 :end_pos 100 -15 -200 :speed -3 ",
        ":Start_pos -1024 512 2000 :End_pos 2048 -512 -2000 :Speed 0 ",
        ":end_pos 1 2 3 :speed 3 ",
        ":start_pos 0 0 0 :end_pos 0.001 0.002 0.003 :speed 2147483647 ",
        ":start_pos -1000000 -1000000 -1000000 :end_pos 1000000 1000000 1000000 ",
        ":start_pos 0 0 0 :end_pos 0 300 0 :speed -2147483648 ",
    ] {
        parameters.push(params.to_owned());
    }
    let mut trains = Vec::new();
    for (index, params) in parameters.iter().enumerate() {
        let p = Parameters::new(params).unwrap();
        let mut original = Pose {
            matrix: tankracer_game::math::Matrix::from_angles([1301, 8911, 731]),
            position: [-123456, 345678, -567890],
        };
        if index < 2 {
            original = Pose::from_level(
                authored
                    .objects
                    .iter()
                    .filter(|r| r.class.eq_ignore_ascii_case("Train"))
                    .nth(index)
                    .unwrap(),
            );
        }
        let mut pose = original;
        let mut seed = 0x1234abcd;
        let train =
            rcc_actor_initial_probe::train(level, shared, terrain, params, &mut pose, &mut seed);
        assert_eq!(seed, 0x1234abcd);
        let start = point(p, ":start_pos", ":Start_pos");
        let end = point(p, ":end_pos", ":End_pos");
        let mut input = Vec::new();
        ints(&mut input, original.matrix.0);
        ints(&mut input, original.position);
        ints(&mut input, start.unwrap_or_default());
        ints(&mut input, end.unwrap_or_default());
        ints(
            &mut input,
            [
                p.integer(":Speed")
                    .or_else(|| p.integer(":speed"))
                    .unwrap_or(3),
                p.integer(":cue_range")
                    .or_else(|| p.integer(":Cue_range"))
                    .unwrap_or(160)
                    .wrapping_shl(10),
            ],
        );
        input.push(u16::from(start.is_some()));
        assert_eq!(input.len(), 41);
        let mut expected = Vec::new();
        travel(&mut expected, &train);
        let Motion::Path {
            start,
            end,
            velocity,
            ..
        } = train.motion
        else {
            panic!("first-map Train path")
        };
        ints(&mut expected, start);
        ints(&mut expected, end);
        ints(&mut expected, velocity);
        ints(&mut expected, pose.matrix.0);
        ints(&mut expected, pose.position);
        ints(&mut expected, train.origin);
        ints(&mut expected, [train.speed, train.range]);
        assert_eq!(expected.len(), 90);
        trains.push(json!({"index":index,"input":input,"expected":expected}));
    }
    let data = fs::read(root.join("assets/game/DATA/FASTFILE.DAT")).unwrap();
    let data = Archive::parse(&data).unwrap();
    let catalog = tankracer_assets::object_catalog::Catalog::decode_checked(
        data.get("OBPARAMS.DAT").unwrap(),
    )
    .unwrap();
    let records: Vec<_> = authored
        .objects
        .iter()
        .filter(|r| r.class.eq_ignore_ascii_case("Powerup"))
        .copied()
        .collect();
    assert_eq!(records.len(), 30);
    let variants = [
        ":fall_range 4 ",
        ":fall_range 4",
        ":aspect 0 90 180 ",
        ":aspect -12.5 720.25 180.5 :incr_lap_for_next ",
        ":incr_lap_for_next ",
    ];
    let mut groups = Vec::new();
    for (map, cup, tilt, lap, separation, diameter) in [
        (0, 0, 0, 1, 0, 0),
        (0, 1, 30, 255, 0, 0),
        (7, 2, 31, 2, 12345, 321),
    ] {
        let mut setup = Setup {
            map,
            cup,
            tilt,
            lap,
            config: catalog.pickup,
            geometry: tankracer_game::actor_startup::pickup::Geometry {
                separation,
                diameter,
            },
        };
        let initial = setup_fields(&setup);
        let mut rows = Vec::new();
        for index in 0..records.len() + variants.len() + 3 {
            let base = records[index % records.len()];
            let mut record = if index < records.len() {
                base
            } else {
                Object {
                    parameters: variants[(index - records.len()) % variants.len()],
                    ..base
                }
            };
            if index >= records.len() + variants.len() {
                record.shape =
                    ["cemine.sm", "cshell.sm", "cwarp.sm"][index - records.len() - variants.len()];
            }
            let p = Parameters::new(record.parameters).unwrap();
            let authored_pose = Pose::from_level(&record);
            let mut pose = authored_pose;
            let mut seed = 0x1234abcd;
            let pickup = rcc_actor_initial_probe::pickup(
                level, shared, terrain, record, &mut pose, &mut seed, &mut setup,
            );
            assert_eq!(seed, 0x1234abcd);
            let raw_kind = [
                "",
                "cmine.sm",
                "cemine.sm",
                "chomer.sm",
                "cufo.sm",
                "cconc.sm",
                "",
                "cturbo.sm",
                "cshield.sm",
                "cshell.sm",
                "cwarp.sm",
            ]
            .iter()
            .position(|name| name.eq_ignore_ascii_case(record.shape))
            .unwrap();
            let mut input = Vec::new();
            ints(&mut input, authored_pose.matrix.0);
            ints(&mut input, authored_pose.position);
            let mut ground = tankracer_game::terrain::MISS_HEIGHT;
            tankracer_game::terrain::height(
                terrain,
                record.position[0],
                record.position[2],
                &mut ground,
            )
            .unwrap();
            let mut aspect = [0; 3];
            if let Some(mut values) = p.find(":aspect") {
                for angle in &mut aspect {
                    *angle = (f64::from(values.real32().unwrap()) * 182.0).trunc() as i64 as i32;
                }
            }
            ints(
                &mut input,
                [
                    ground,
                    raw_kind as i32,
                    i32::from(p.find(":fall_range").is_some()),
                    p.integer(":fall_range").unwrap_or(0),
                    catalog.pickup.extra_range[cup],
                    catalog.pickup.circuit_extra,
                    i32::from(p.find(":incr_lap_for_next").is_some()),
                    i32::from(p.find(":aspect").is_some()),
                ],
            );
            ints(&mut input, aspect);
            let height = |model: &tankracer_assets::model::Model| {
                i32::from(model.bounds[1][1]) - i32::from(model.bounds[0][1])
            };
            ints(
                &mut input,
                [
                    height(&pickup.model),
                    height(&pickup.parachute),
                    i32::from(pickup.model.radius),
                    i32::from(p.integer(":fall_range").is_some()),
                ],
            );
            assert_eq!(input.len(), 54);
            let mut expected = Vec::new();
            ints(&mut expected, pose.matrix.0);
            ints(&mut expected, pose.position);
            let flight = &pickup.flight;
            ints(&mut expected, flight.angular);
            ints(&mut expected, flight.normal);
            ints(
                &mut expected,
                [
                    flight.blink_time,
                    flight.blink_count as i32,
                    flight.delay,
                    i32::from(flight.disabled),
                    i32::from(flight.parachute.is_some()),
                    pickup.ground,
                    pickup.fall_range,
                    pickup.regeneration,
                ],
            );
            assert!(flight.parachute.is_none());
            ints(&mut expected, [0; 12]);
            ints(
                &mut expected,
                [
                    pickup.kind as i32,
                    i32::from(pickup.phase),
                    i32::from(pickup.added_lap),
                    pickup.mass,
                    pickup.restitution[0] as i32,
                    pickup.restitution[1] as i32,
                ],
            );
            ints(&mut expected, pickup.auxiliary.map(i32::from));
            assert_eq!(expected.len(), 100);
            expected.extend(setup_fields(&setup));
            rows.push(json!({"input":input,"expected":expected}));
        }
        groups.push(json!({"initial":initial,"rows":rows}));
    }
    let spawn = authored
        .objects
        .iter()
        .find(|r| r.class == "Player")
        .map(Pose::from_level)
        .unwrap_or_default();
    let mut camera_input = Vec::new();
    ints(&mut camera_input, spawn.matrix.0);
    ints(&mut camera_input, spawn.position);
    ints(&mut camera_input, catalog.camera.near);
    fs::write(out.join("scene-actor-initial.json"),serde_json::to_vec_pretty(&json!({"trains":trains,"pickups":groups,"camera_input":camera_input,
        "scope":"Actual first-map Train path initialization and shared Pickup setup, tilt sequence, type availability, geometry defaults, lap bytes, fall-range token semantics and aspect over original resources. Resource allocation and membership publication remain caller-owned."})).unwrap()).unwrap();
}
