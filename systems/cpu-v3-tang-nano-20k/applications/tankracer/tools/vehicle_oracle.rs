//! Whole physical chain using unmodified suspension, powertrain and motion calls.
#[allow(dead_code)]
#[path = "drive_oracle.rs"]
pub mod drive_fixture;
use serde_json::{json, Value};
use std::{fs, path::PathBuf};
use tankracer_assets::{
    archive::Archive,
    config::{ResolveContext, Surfaces, Tank},
    model::ShapeArchive,
    object_catalog::Catalog,
};
use tankracer_game::{
    attitude, body, contact, drive,
    math::Matrix,
    motion, pose, powertrain, suspension,
    vehicle_model::{BuildContext, Bundles, ModelBank, VehicleModel},
    vehicle_prefix::boost,
    velocity,
};
pub fn spring(s: &suspension::State, load: i32, pivot: [i32; 3]) -> Vec<i32> {
    let mut v = Vec::new();
    for p in s.points {
        v.extend([p.kind, p.gap, p.corner as i32, p.travel, p.rate, p.output]);
    }
    v.extend(s.angular);
    v.extend(s.tilt);
    v.extend([
        s.penetrating_input_sum,
        s.travel_sum,
        s.vertical_delta,
        s.world_vy,
        s.position_y,
        s.timer,
    ]);
    v.extend(s.auxiliary_type);
    v.extend(s.auxiliary_cached);
    v.extend([s.max_penetration, s.last_penetration, load]);
    v.extend(pivot);
    v
}
pub fn attitude_state(m: &motion::State) -> Vec<i32> {
    m.poses
        .reference
        .0
        .into_iter()
        .chain(m.poses.body.matrix.0)
        .chain(m.poses.body.position)
        .chain(m.angular)
        .chain([m.acceleration[1], m.acceleration[0]])
        .chain(m.pivot)
        .collect()
}
pub fn motion_state(m: &motion::State) -> Vec<i32> {
    m.velocity
        .into_iter()
        .chain(m.poses.physical_position)
        .chain(m.previous_position)
        .chain(m.poses.base_position)
        .chain([
            m.rates.travel_sum,
            m.rates.penetrating_force,
            m.rates.lateral_velocity,
            m.rates.track_slip,
        ])
        .collect()
}
pub fn boost_state(b: &boost::State) -> Vec<i32> {
    vec![b.extra_speed, b.power, b.power_loss, b.timer.into()]
}
fn main() {
    let mut args = std::env::args_os().skip(1);
    let reference = PathBuf::from(args.next().unwrap());
    let output = PathBuf::from(args.next().unwrap());
    let count = args
        .next()
        .map(|v| v.to_str().unwrap().parse::<usize>().unwrap())
        .unwrap_or(600);
    fs::create_dir_all(&output).unwrap();
    let data = fs::read(reference.join("assets/game/DATA/FASTFILE.DAT")).unwrap();
    let archive = Archive::parse(&data).unwrap();
    let tank = Tank::decode(archive.get("TANK1.DAT").unwrap()).unwrap();
    let surfaces = Surfaces::decode(archive.get("SURFS.DAT").unwrap()).unwrap();
    let catalog = Catalog::decode_checked(archive.get("OBPARAMS.DAT").unwrap()).unwrap();
    let bronze = fs::read(reference.join("assets/game/TANKS/BRONZE/FASTFILE.DAT")).unwrap();
    let ba = Archive::parse(&bronze).unwrap();
    let shared = fs::read(reference.join("assets/game/INGAME/FASTFILE.DAT")).unwrap();
    let sa = Archive::parse(&shared).unwrap();
    let bundles = Bundles {
        vehicle: vec![ShapeArchive::parse(ba.get("TANKB.SS").unwrap()).unwrap()],
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
    let b = model.geometry_bounds;
    let layout = [b.min[0], b.min[1], b.min[2], b.max[0], b.max[2]];
    let pose = model.pose_config([0; 3]);
    let c = contact::Config::try_from(&tank.suspension).unwrap();
    let s = &tank.suspension;
    let mut sc = vec![
        c.lower,
        c.upper,
        c.force_positive.multiplier as i32,
        c.force_positive.shift as i32,
        c.force_negative.multiplier as i32,
        c.force_negative.shift as i32,
        c.output_gain.multiplier as i32,
        c.output_gain.shift as i32,
        c.tilt_gain.multiplier as i32,
        c.tilt_gain.shift as i32,
        c.shape_shift as i32,
        c.restore_positive.multiplier as i32,
        c.restore_positive.shift as i32,
        c.restore_negative.multiplier as i32,
        c.restore_negative.shift as i32,
        c.damping_shift as i32,
        c.travel_dead_step as i32,
        s.initial_weight,
        s.initial_pressure,
        s.pressure_fraction.multiplier as i32,
        s.pressure_fraction.shift as u8 as i32,
    ];
    sc.extend(layout);
    let tuning = drive::Tuning::default();
    let mut dc = drive_fixture::config(&tank, &surfaces, &tuning);
    dc.extend([catalog.turbo.player_power, catalog.turbo.player_loss]);
    let body = body::Tuning::default();
    let mut cases = Vec::<Value>::new();
    for (name, dt, initial_velocity, launch, boosted, numerics) in [
        (
            "throttle-brake",
            16,
            [7001, -5001, 45000],
            false,
            false,
            attitude::Numerics::Original,
        ),
        (
            "reverse-coast",
            33,
            [-7001, -3200, -60000],
            false,
            false,
            attitude::Numerics::Original,
        ),
        (
            "airborne-boost",
            16,
            [17001, 0, 130000],
            false,
            true,
            attitude::Numerics::OrthogonalV1,
        ),
        (
            "launch-mixed",
            33,
            [-17001, 1000, -17000],
            true,
            false,
            attitude::Numerics::OrthogonalV1,
        ),
        (
            "precision-large-negative",
            16,
            [-600001, -17, -100001],
            false,
            false,
            attitude::Numerics::Original,
        ),
        (
            "precision-large-positive",
            16,
            [600001, -17, 100001],
            false,
            false,
            attitude::Numerics::Original,
        ),
    ] {
        let ac = attitude::Config {
            numerics,
            ..attitude::Config::from(s)
        };
        let mut motion = motion::State::default();
        motion.poses.physical_position = [1000, 1500, -2700];
        motion.poses.body.matrix = Matrix::from_angles([100, 1300, 200]);
        motion.poses.reference = motion.poses.body.matrix;
        motion
            .poses
            .update(
                &pose,
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
        motion.velocity = initial_velocity;
        motion.angular = [-100, 700, 77];
        let mut suspension = suspension::State {
            points: std::array::from_fn(|i| suspension::Point {
                corner: i as u32,
                ..suspension::Point::default()
            }),
            timer: 1,
            ..suspension::State::default()
        };
        let mut power = powertrain::State::default();
        let mut boost = boost::State::default();
        let mut stage = drive_fixture::flatten(&power.drive_state(&motion));
        let initial = json!({"attitude":attitude_state(&motion),"suspension":spring(&suspension,0,[0;3]),"drive":stage,"motion":motion_state(&motion),"boost":boost_state(&boost)});
        let mut frames = Vec::new();
        let updates = if name.starts_with("precision-") {
            count.min(3)
        } else {
            count
        };
        for index in 0..updates {
            let apply_boost = boosted && (index == 0 || index == 350);
            if apply_boost {
                boost.apply(
                    tank.drive.turbo_amount,
                    catalog.turbo.player_power,
                    catalog.turbo.player_loss,
                );
            }
            boost.decay(dt, true);
            let kinds = match index % 8 {
                0 => [1, 1, 1, 1],
                1 => [0, 1, 2, 2],
                2 => [2, 2, 2, 2],
                3 => [0, 2, 0, 2],
                4 => [1, 2, 1, 2],
                5 => [3, 3, 3, 3],
                6 => [4, 4, 1, 1],
                _ => [1, 0, 1, 0],
            };
            let gaps: [i32; 4] = std::array::from_fn(|i| {
                if kinds[i] == 0 {
                    -155 - i as i32 * 45
                } else if kinds[i] == 2 {
                    300
                } else {
                    0
                }
            });
            let env = suspension::Environment {
                dt,
                is_player: true,
                session_active: 1,
                player_multiplier: catalog.moon.player.multiplier,
                player_shift: catalog.moon.player.shift as u8 as u32,
                other_multiplier: catalog.moon.other.multiplier,
                other_shift: catalog.moon.other.shift as u8 as u32,
                penetration_shift: s.susp_to_shape.shift as u8 as u32,
                auxiliary_query_type: [2, 1],
                ..suspension::Environment::default()
            };
            let flat_env = vec![
                dt,
                0,
                1,
                0,
                1,
                motion.poses.base_position[0],
                motion.poses.base_position[2],
                0x4a77f,
                0xfffb4856u32 as i32,
                0x813d2,
                0xffff80ecu32 as i32,
                env.player_multiplier as i32,
                env.player_shift as i32,
                env.other_multiplier as i32,
                env.other_shift as i32,
                env.penetration_shift as i32,
                2,
                1,
            ];
            for i in 0..4 {
                suspension.points[i].kind = kinds[i];
                suspension.points[i].gap = gaps[i];
                suspension.points[i].travel = motion.travel[i];
            }
            suspension.auxiliary_cached = [0; 2];
            suspension.angular = [motion.angular[0], motion.angular[2]];
            suspension.world_vy = motion.velocity[1];
            suspension.position_y = motion.poses.physical_position[1];
            let load = suspension::load(
                kinds,
                motion.travel,
                s.initial_weight,
                s.initial_pressure,
                s.pressure_fraction.multiplier,
                s.pressure_fraction.shift as u8 as u32,
            );
            let pivot = suspension::layout(kinds, layout);
            let had_timer = suspension.timer > 0;
            let mut queries = 0u16;
            suspension
                .step_resolved(&c, &env, |side, kind| {
                    queries |= 1 << side;
                    *kind = env.auxiliary_query_type[side];
                    Ok::<(), ()>(())
                })
                .unwrap();
            motion.angular[0] = suspension.angular[0];
            motion.angular[2] = suspension.angular[1];
            motion.velocity[1] = suspension.world_vy;
            motion.poses.physical_position[1] = suspension.position_y;
            motion.travel = suspension.points.map(|p| p.travel);
            motion.pivot = pivot;
            motion.rates.travel_sum = suspension.travel_sum;
            motion.rates.penetrating_force = suspension.penetrating_input_sum;
            power.load = load;
            let spring_state = spring(&suspension, load, pivot);
            let mut kind_events = 0u16;
            for i in 0..4 {
                if kinds[i] == 2 {
                    kind_events |= 1 << i;
                }
                if kinds[i] == 0 {
                    kind_events |= 16 << i;
                }
            }
            let spring_events = [
                queries,
                u16::from(suspension.max_penetration > 0) | if had_timer { 2 } else { 0 },
                kind_events,
            ];
            let phase = index % 10;
            let input = drive::Input {
                throttle: if phase < 3 || phase >= 8 {
                    65535
                } else if phase == 6 || phase == 7 {
                    -65535
                } else {
                    0
                },
                brake: if phase == 4 || phase == 5 { 65535 } else { 0 },
                steering: if index % 3 == 0 { -7000 } else { 9000 },
                ..drive::Input::default()
            };
            let contact_y = std::array::from_fn(|i| {
                model.primary_local[i][1] + model.offsets[if i < 2 { 3 } else { 4 }][1] + 1500
            });
            let de = drive::Environment {
                dt,
                mode: 0,
                player: true,
                active: true,
                launch_eligible: launch,
                materials: if index & 1 == 0 { [1, 2] } else { [7, 5] },
                kinds,
                contact_y,
                airborne: kinds.into_iter().filter(|&k| k == 2).count() as i8,
                boost_charge: boost.timer,
                boost_power: boost.extra_speed,
                extra_drive: boost.power,
            };
            let mut drive_input = drive_fixture::frame(&input, &de);
            drive_input.extend([(index + 1) as i32, i32::from(apply_boost), 0]);
            power
                .step(
                    &mut motion,
                    &powertrain::Config {
                        drive: &tank.drive,
                        surfaces: &surfaces,
                        tuning: &tuning,
                        yaw: velocity::YawConfig::from(&tank.drive),
                    },
                    &powertrain::Frame {
                        input: &input,
                        drive: &de,
                        vertical_delta: suspension.vertical_delta,
                        yaw_lock: 0,
                    },
                )
                .unwrap();
            stage = drive_fixture::flatten(&power.drive_state(&motion));
            let after_power =
                json!({"drive":stage,"velocity":motion.velocity,"angular":motion.angular});
            motion
                .integrate_and_pose(
                    &motion::Config {
                        attitude: &ac,
                        body: &body,
                        pose: &pose,
                    },
                    &motion::Frame {
                        dt,
                        number: (index + 1) as u32,
                        player: true,
                        mode: 0,
                        pose: motion::PoseInput {
                            session_active: true,
                            ..motion::PoseInput::default()
                        },
                    },
                    None,
                )
                .unwrap();
            motion.finish_frame(dt);
            frames.push(json!({"input":{"environment":flat_env,"frame":drive_input,"kinds":kinds,"gaps":gaps},"after_suspension":spring_state,"suspension_events":spring_events,"after_powertrain":after_power,"state":{"attitude":attitude_state(&motion),"suspension":spring_state,"drive":stage,"motion":motion_state(&motion),"boost":boost_state(&boost)},"rng_before":305441741u32,"rng_after":305441741u32}));
        }
        cases.push(json!({"name":name,"initial":initial,"suspension_config":sc,"drive_config":dc,"attitude_config":[s.accel_to_rot.multiplier,s.accel_to_rot.shift as u8 as u16,s.steer_to_rot.multiplier,s.steer_to_rot.shift as u8 as u16,u16::from(numerics==attitude::Numerics::OrthogonalV1)],"pose_config":[pose.travel_gain.multiplier as i32,pose.travel_gain.shift as i32],"frames":frames}));
    }
    for name in ["sine.bin", "angle.bin"] {
        fs::copy(
            reference.join("crates/game/data").join(name),
            output.join(name),
        )
        .unwrap();
    }
    fs::write(output.join("vehicle.json"),serde_json::to_vec_pretty(&json!({"scope":"complete single-vehicle PHYSICAL suspension->powertrain->integration->attitude chain at frozen contact classifications/gaps/world contact Y/auxiliary query results; export checks physical state only; excludes collision actors, terrain traversal, sampling effects/RNG, articulation/render-part outputs and whole game tick","config_source":"TANK1.DAT/SURFS.DAT/OBPARAMS.DAT plus actual TANKB.SS/INGAME.SS geometry","layout":layout,"turbo":format!("{:?}",catalog.turbo),"cases":cases})).unwrap()).unwrap();
}
