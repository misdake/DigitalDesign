//! Complete original part-pose snapshots, including arithmetic failure prefixes.
use super::ints;
use serde_json::json;
use std::{fs, path::Path};
use tankracer_game::{
    math::Matrix,
    pose::{self, ExternalBasis},
    vehicle_runtime::SinglePlayer,
};

pub(super) fn snapshot(state: &pose::State) -> Vec<u16> {
    let mut out = Vec::new();
    ints(&mut out, state.physical_position);
    ints(&mut out, state.reference.0);
    ints(&mut out, state.base_position);
    for p in [
        state.body,
        state.tracks[0],
        state.tracks[1],
        state.turret,
        state.gun,
        state.attachments[0],
        state.attachments[1],
    ] {
        ints(&mut out, p.matrix.0);
        ints(&mut out, p.position);
    }
    ints(&mut out, state.bottom_position);
    ints(
        &mut out,
        [
            i32::from(state.recoil_factor),
            i32::from(state.recoil_shift),
        ],
    );
    assert_eq!(out.len(), 208);
    out
}
pub(super) fn config(c: &pose::Config) -> Vec<u16> {
    let mut out = Vec::new();
    ints(
        &mut out,
        [
            i32::from(c.travel_gain.multiplier),
            i32::from(c.travel_gain.shift),
            c.track_length,
        ],
    );
    ints(&mut out, c.track_offsets.into_iter().flatten());
    ints(&mut out, c.turret_offset);
    ints(&mut out, c.gun_offset);
    ints(&mut out, c.attachment_offsets.into_iter().flatten());
    ints(&mut out, c.bottom_offset);
    ints(&mut out, c.gun_pivot);
    assert_eq!(out.len(), 54);
    out
}
pub(super) fn environment(e: &pose::Environment) -> Vec<i32> {
    let (kind, m) = match e.external {
        ExternalBasis::None => (0, [0; 9]),
        ExternalBasis::Body => (1, [0; 9]),
        ExternalBasis::Custom(m) => (2, m.0),
    };
    let mut out = vec![
        i32::from(e.timer),
        i32::from(e.ambient_wobble),
        i32::from(e.session_active),
        e.ambient_phase as i32,
        e.recoil_mode,
        i32::from(e.turret_yaw),
        e.recoil,
        e.gun_pitch,
        kind,
    ];
    out.extend(m);
    out
}
pub fn export(out: &Path, world: &SinglePlayer) {
    let car = world.vehicles.entries[0].as_ref().unwrap();
    let mut cases = Vec::new();
    for branch in 0..64 {
        let mut c = car.physics.pose.clone();
        let mut state = car.physics.motion.poses.clone();
        state.body.matrix = Matrix::from_angles([
            700 + (branch * 613) as u32,
            13000 + (branch * 997) as u32,
            1900,
        ]);
        state.physical_position = [12345, -23456, 34567];
        state.body.position = [-1001, 2002, -3003];
        state.reference = Matrix::from_angles([600, 500, 400]);
        let mut e = pose::Environment {
            timer: if branch % 2 == 0 { 0 } else { 310 },
            ambient_wobble: branch % 4 >= 2,
            session_active: branch % 4 == 3,
            ambient_phase: 197,
            recoil_mode: match branch % 3 {
                0 => 0,
                1 => 3,
                _ => 5,
            },
            turret_yaw: -5234,
            recoil: -173,
            gun_pitch: 321,
            external: match branch % 3 {
                0 => ExternalBasis::None,
                1 => ExternalBasis::Body,
                _ => ExternalBasis::Custom(Matrix::from_angles([1700, 30000, 1900])),
            },
        };
        let mut travel = [-1333, 2444, -3555, 4666];
        match branch {
            0 => {
                state = car.physics.motion.poses.clone();
                travel = car.physics.motion.travel;
                let input = car.physics.pose_input;
                e = pose::Environment {
                    timer: input.timer,
                    ambient_wobble: input.ambient_wobble,
                    session_active: input.session_active,
                    ambient_phase: input.ambient_phase,
                    recoil_mode: input.recoil_mode,
                    turret_yaw: input.turret_yaw,
                    recoil: car.physics.motion.articulation.recoil,
                    gun_pitch: car.physics.motion.articulation.pitch,
                    external: input.external,
                };
            }
            1 => e.timer = 127,
            4 => e.timer = u16::MAX,
            5 => c.travel_gain.shift = 0,
            6 => c.travel_gain.shift = 31,
            7 => c.travel_gain.shift = 32,
            8 => c.travel_gain.shift = u16::MAX,
            9 => c.track_length = 0,
            10 => c.track_length = -1,
            11 => c.track_length = i32::MIN,
            12 => {
                c.travel_gain.shift = 0;
                travel[2] = i32::MIN;
                travel[3] = 0;
            }
            13 => {
                c.travel_gain.shift = 0;
                travel[0] = i32::MIN;
                travel[1] = 0;
            }
            14 => {
                c.travel_gain.shift = 0;
                travel[0] = i32::MIN;
                travel[1] = i32::MAX;
            }
            15 => {
                state.body.matrix = Matrix([
                    1000000, -987654, 876543, -765432, 654321, -543210, 432109, -321098, 210987,
                ])
            }
            16 => e.turret_yaw = i16::MIN,
            17 => e.turret_yaw = i16::MAX,
            18 => e.gun_pitch = i32::MIN,
            19 => e.recoil = i32::MIN,
            20 => {
                c.gun_pivot = [i32::MIN, i32::MAX, 34567890];
                c.bottom_offset = [i32::MAX, i32::MIN, -76543210];
            }
            21 => state.body.matrix = Matrix([0; 9]),
            22 => {
                e.timer = 0;
                e.ambient_phase = 0;
            }
            24 => state.physical_position = [i32::MAX; 3],
            25 => {
                travel = [i32::MIN, i32::MAX, -2147483000, 2147483000];
                c.travel_gain.shift = 16;
            }
            _ => {}
        }
        let initial = snapshot(&state);
        let cfg = config(&c);
        let mut controls = Vec::new();
        let mut frames = Vec::new();
        for step in 0..3 {
            let mut control = travel.to_vec();
            control.extend(environment(&e));
            assert_eq!(control.len(), 22);
            controls.push(control);
            let status = state.update(&c, travel, &e).map_or(-2, |()| 0);
            let mut frame = Vec::new();
            ints(&mut frame, [status]);
            frame.extend(snapshot(&state));
            frames.push(frame);
            if branch != 0 {
                e.timer = e.timer.wrapping_sub(17);
                e.ambient_phase = e.ambient_phase.wrapping_add(31);
            }
            if branch >= 26 {
                e.turret_yaw = e.turret_yaw.wrapping_add(3210);
                e.gun_pitch = e.gun_pitch.wrapping_sub(1500);
                travel[step as usize] = travel[step as usize].wrapping_add(719);
            }
        }
        if matches!(branch, 11 | 12 | 13) {
            assert_eq!(frames[0][0], 65534);
        } else {
            assert_eq!(frames[0][0], 0, "pose branch {branch}");
        }
        cases.push(json!({"tag":format!("pose-{branch}"),"state":initial,"config":cfg,"controls":controls,"frames":frames}));
    }
    fs::write(out.join("vehicle-pose.json"),serde_json::to_vec(&json!({"cases":cases,"scope":"Complete original pose::State::update on real vehicle configuration: temporary and ambient wobble, tracks, external recoil, turret, pivoted gun, attachments, base/physical/bottom publication and early/second-track failure prefixes. Exact original integer arithmetic; caller-owned views."})).unwrap()).unwrap();
}
