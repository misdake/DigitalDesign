//! Original complete Motion::integrate_and_pose and successful-tick publication.
use super::{ints, vehicle_pose_oracle};
use serde_json::json;
use std::{fs, path::Path};
use tankracer_game::{
    attitude, body, math::Matrix, motion, pose::ExternalBasis, vehicle_runtime::SinglePlayer,
};
pub(super) fn snapshot(m: &motion::State, mesh: &[[i16; 3]; 6]) -> Vec<u16> {
    let mut out = vehicle_pose_oracle::snapshot(&m.poses);
    ints(&mut out, m.previous_position);
    ints(&mut out, m.velocity);
    ints(&mut out, m.angular);
    ints(&mut out, m.acceleration);
    ints(&mut out, m.pivot);
    ints(
        &mut out,
        [
            m.articulation.recoil,
            m.articulation.recoil_rate,
            m.articulation.pitch,
            m.articulation.pitch_rate,
            m.lean.lateral.position,
            m.lean.lateral.rate,
            m.lean.longitudinal.position,
            m.lean.longitudinal.rate,
        ],
    );
    for c in m.contacts {
        ints(&mut out, c.world);
        ints(&mut out, [c.gap]);
    }
    ints(&mut out, m.travel);
    ints(&mut out, m.hull.position);
    ints(&mut out, m.hull.matrix.0);
    out.extend(m.hull.vertices.into_iter().flatten().map(|v| v as u16));
    out.push(m.hull.radius);
    ints(
        &mut out,
        [
            i32::from(m.ready),
            m.rates.travel_sum,
            m.rates.penetrating_force,
            m.rates.lateral_velocity,
            m.rates.track_slip,
        ],
    );
    assert_eq!(out.len(), 339);
    out.extend(mesh.iter().flatten().map(|v| *v as u16));
    assert_eq!(out.len(), 357);
    out
}
pub fn export(out: &Path, world: &SinglePlayer) {
    let car = world.vehicles.entries[0].as_ref().unwrap();
    let mut cases = Vec::new();
    for branch in 0..56 {
        let mut m = car.physics.motion.clone();
        let mut pose = car.physics.pose.clone();
        let mut ac = attitude::Config::from(&car.model.config.tank.suspension);
        let body = body::Tuning::default();
        m.poses.physical_position = [10000, 20000, 30000];
        m.poses.body.position = [-100, 200, -300];
        m.poses.reference = Matrix::from_angles([100, 200, 300]);
        m.poses.body.matrix = Matrix::from_angles([700, 13000, 1900]);
        m.velocity = [123456, -234567, 345678];
        m.angular = [1000, -2000, 3000];
        m.acceleration = [-1000, 2000];
        m.pivot = [123, -234, 345];
        m.travel = [1000, 2000, -3000, 4000];
        m.articulation = body::Articulation {
            recoil: -173,
            recoil_rate: 25,
            pitch: 321,
            pitch_rate: -15,
        };
        m.lean.lateral.position = 3000;
        m.lean.longitudinal.position = -3000;
        m.rates = motion::Rates {
            travel_sum: 1234,
            penetrating_force: 5000000,
            lateral_velocity: -200000,
            track_slip: 300000,
        };
        for i in 0..4 {
            m.contacts[i] = body::Contact {
                world: [1000 + i as i32 * 100, -5000, 3000 - i as i32 * 200],
                gap: 200 + i as i32 * 11,
            };
        }
        let mut mesh = [
            [11i16, 12, 13],
            [21, 22, 23],
            [31, 32, 33],
            [41, 42, 43],
            [51, 52, 53],
            [61, 62, 63],
        ];
        let mut binding = [1u16, 6, 0, 1, 2, 3, 13, 23, 33, 43];
        let mut f = vec![0, 40, 17, 1, 0, 310, 1, 0, 197, 3, -5234, 2];
        f.extend(Matrix::from_angles([1700, 30000, 1900]).0);
        assert_eq!(f.len(), 21);
        match branch {
            0 => {}
            1 => {
                f[1] = 0;
            }
            2 => {
                f[1] = -1;
            }
            3 => {
                f[3] = 0;
            }
            4 => {
                f[4] = 2;
            }
            5 => {
                f[4] = 3;
            }
            6 => {
                binding[0] = 0;
            }
            7 => {
                binding[5] = 6;
            }
            8 => {
                binding[2] = 6;
            }
            9 => {
                binding[2..6].fill(1);
            }
            10 => {
                f[11] = 0;
            }
            11 => {
                f[11] = 1;
            }
            12 => {
                f[5] = 0;
                f[6] = 0;
            }
            13 => {
                f[7] = 1;
            }
            14 => {
                f[9] = 5;
            }
            15 => {
                m.poses.body.matrix = Matrix([4000, 1, 0, 65407, 339, 0, 0, 0, 4096]);
                m.angular = [0; 3];
                m.acceleration = [0; 2];
                f[1] = 16;
                f[2] = 10;
            }
            16 => {
                pose.travel_gain.multiplier = 1;
                pose.travel_gain.shift = 0;
                m.travel = [i32::MIN, 0, 0, 0];
            }
            17 => {
                m.velocity = [i32::MIN, i32::MAX, -1];
            }
            18 => {
                m.articulation.recoil = i32::MIN;
                m.articulation.pitch = i32::MAX;
            }
            19 => {
                m.angular = [i32::MIN, i32::MAX, -1];
            }
            20 => {
                f[1] = 100;
            }
            21 => {
                f[1] = 1;
            }
            22 => {
                f[1] = i32::MAX;
            }
            23 => {
                f[2] = -1;
            }
            24 => {
                f[2] = 1024;
            }
            25 => {
                f[0] = 1;
            }
            26 => {
                m.ready = true;
                f[0] = 1;
            }
            27 => {
                m.rates.travel_sum = i32::MAX;
                m.rates.track_slip = i32::MIN;
                f[0] = 1;
            }
            28 => {
                m.contacts[0].world = [i32::MIN, i32::MAX, 0];
                m.contacts[0].gap = i32::MIN;
                f[0] = 1;
            }
            29 => {
                f[0] = 2;
            }
            30 => {
                f[0] = 2;
                f[3] = 0;
            }
            31 => {
                f[0] = 2;
                binding[5] = 255;
            }
            32 => {
                m.lean.lateral.rate = i32::MAX;
                m.lean.longitudinal.rate = i32::MIN;
            }
            33 => {
                m.pivot = [i32::MAX, i32::MIN, -1];
            }
            34 => {
                f[5] = 65535;
            }
            35 => {
                f[8] = -1;
            }
            36 => {
                f[10] = -32768;
            }
            37 => {
                f[10] = 32767;
            }
            38 => {
                f[9] = 0;
            }
            39 => {
                m.travel = [0; 4];
                pose.track_length = 0;
            }
            40 => {
                pose.gun_pivot = [i32::MAX, i32::MIN, -1];
            }
            41 => {
                m.poses.recoil_factor = 255;
                m.poses.recoil_shift = 31;
            }
            42 => {
                f[6] = 0;
                f[7] = 0;
            }
            43 => {
                ac.numerics = attitude::Numerics::OrthogonalV1;
                f[2] = 8;
            }
            44 => {
                ac.numerics = attitude::Numerics::OrthogonalV1;
                f[2] = 1024;
            }
            45 => {
                binding[0] = 0;
                f[0] = 2;
            }
            46 => {
                m.travel = [4000, -3000, 2000, -1000];
            }
            47 => {
                m.rates.travel_sum = -1;
                f[0] = 2;
                f[1] = 33;
            }
            48..=55 => {
                f[2] = [65534, 131069, i32::MAX - 1, -2][(branch - 48) % 4];
                if branch >= 52 {
                    ac.numerics = attitude::Numerics::OrthogonalV1;
                }
            }
            _ => unreachable!(),
        }
        let initial = snapshot(&m, &mesh);
        let config = motion::Config {
            attitude: &ac,
            body: &body,
            pose: &pose,
        };
        let mut controls = Vec::new();
        let mut frames = Vec::new();
        for step in 0..4 {
            let mut c = f.clone();
            c[2] = c[2].wrapping_add(step);
            if step == 1 && branch != 1 && branch != 2 && branch != 22 {
                c[1] = 33;
            }
            let ext = match c[11] {
                0 => ExternalBasis::None,
                1 => ExternalBasis::Body,
                _ => ExternalBasis::Custom(Matrix(c[12..21].try_into().unwrap())),
            };
            let frame = motion::Frame {
                dt: c[1],
                number: c[2] as u32,
                player: c[3] != 0,
                mode: c[4],
                pose: motion::PoseInput {
                    timer: c[5] as u16,
                    ambient_wobble: c[6] != 0,
                    session_active: c[7] != 0,
                    ambient_phase: c[8] as u32,
                    recoil_mode: c[9],
                    turret_yaw: c[10] as i16,
                    external: ext,
                },
            };
            let gun = (binding[0] != 0).then_some(body::GunMesh {
                vertices: &mut mesh,
                indices: std::array::from_fn(|i| binding[2 + i] as u8),
                original_z: std::array::from_fn(|i| binding[6 + i] as i16),
            });
            let mut status = 0;
            if c[0] != 1 {
                status = m
                    .integrate_and_pose(&config, &frame, gun)
                    .map_or_else(|e| e.status(), |()| 0);
            }
            if c[0] != 0 && status == 0 {
                m.finish_frame(c[1]);
            }
            let mut answer = Vec::new();
            ints(&mut answer, [status]);
            answer.extend(snapshot(&m, &mesh));
            frames.push(answer);
            controls.push(c);
        }
        let mut body_config = Vec::new();
        ints(
            &mut body_config,
            [
                i32::from(body.recoil_decay),
                i32::from(body.recoil_step),
                i32::from(body.pitch_damping.step),
                i32::from(body.pitch_damping.fraction),
                i32::from(body.pitch_damping.shift),
                i32::from(body.lean_decay),
            ],
        );
        cases.push(json!({"tag":format!("car-motion-{branch}"),"state":initial,"binding":binding,"attitude":[ac.longitudinal.multiplier,ac.longitudinal.shift,ac.lateral.multiplier,ac.lateral.shift,u16::from(ac.numerics==attitude::Numerics::OrthogonalV1)],"body":body_config,"pose":vehicle_pose_oracle::config(&pose),"controls":controls,"frames":frames}));
    }
    fs::write(out.join("car-motion.json"),serde_json::to_vec(&json!({"cases":cases,"scope":"Original complete Motion::integrate_and_pose and explicit finish_frame on actual ENGLAND configuration. Includes physical/previous position, transactional attitude, gun articulation, player lean, every part pose, contact hull and final rate scaling. Contacts/collisions and intervening effects are separate stages."})).unwrap()).unwrap();
}
