//! Mine method snapshots from actual vehicle construction and ENGLAND terrain.
use super::{ints, wide};
use serde_json::{Value, json};
use std::{fs, path::Path};
use tankracer_game::{
    math::Matrix,
    mine,
    pose::{ExternalBasis, Pose},
    vehicle,
    vehicle_runtime::SinglePlayer,
};

fn snapshot(s: mine::State, pose: Pose, v: &vehicle::State) -> Vec<u16> {
    let mut out = Vec::new();
    ints(
        &mut out,
        [
            i32::from(s.owner.is_some()),
            s.owner.map_or(0, |v| v.0 as i32),
            s.kind as i32,
            s.count.into(),
            s.repeat.into(),
            s.armed.into(),
            s.born as i32,
            s.last_contact as i32,
            s.elapsed,
        ],
    );
    ints(&mut out, pose.matrix.0);
    ints(&mut out, pose.position);
    ints(&mut out, v.motion.poses.base_position);
    ints(&mut out, v.motion.velocity);
    ints(&mut out, v.motion.angular);
    ints(&mut out, v.external_basis().map_or([0; 9], |m| m.0));
    out.push(u16::from(v.external_basis().is_some()));
    wide(&mut out, v.blast.health);
    wide(&mut out, v.collision.collision_size as i32);
    assert_eq!(out.len(), 83);
    out
}
fn run(
    tag: String,
    mut s: mine::State,
    mut pose: Pose,
    mut v: vehicle::State,
    controls: Vec<[i32; 6]>,
    world: &SinglePlayer,
) -> Value {
    let initial = snapshot(s, pose, &v);
    let mut frames = Vec::new();
    for c in &controls {
        let [op, dt, number, x, y, z] = *c;
        let mut action = 0u16;
        let status = match op {
            0 => {
                action = match s.tick(dt) {
                    mine::Action::None => 0,
                    mine::Action::Repeat => 1,
                    mine::Action::Retire => 2,
                };
                0
            }
            1 => {
                action = u16::from(s.contact(number as u32));
                0
            }
            2 => mine::align(&mut pose, world.terrain()).map_or_else(|v| v, |()| 0),
            3 => mine::point_hit(&mut v, [x, y, z]).map_or_else(|v| v, |()| 0),
            _ => panic!("mine operation"),
        };
        let mut out = Vec::new();
        wide(&mut out, status);
        out.push(action);
        out.extend(snapshot(s, pose, &v));
        frames.push(out);
    }
    json!({"tag":tag,"input":initial,"controls":controls,"frames":frames})
}
pub fn export(out: &Path, world: &SinglePlayer) {
    let car = world.vehicles.entries[0].as_ref().unwrap().physics.clone();
    let start = car.motion.poses.base_position;
    let base = Pose {
        matrix: Matrix::IDENTITY,
        position: start,
    };
    let mut cases = Vec::new();
    for repeat in [false, true] {
        for count in [0, 1, 2, 3, 127, 128, 255] {
            for elapsed in [0, 249, 250, 20000, i32::MAX] {
                let s = mine::State {
                    repeat,
                    count,
                    elapsed,
                    ..Default::default()
                };
                let controls = [0, 1, 250, 251, 20001, -33]
                    .map(|dt| [0, dt, 0, 0, 0, 0])
                    .to_vec();
                cases.push(run(
                    format!("tick-{repeat}-{count}-{elapsed}"),
                    s,
                    base,
                    car.clone(),
                    controls,
                    world,
                ));
            }
        }
    }
    for born in [0u32, 1, u32::MAX] {
        for last in [0, born, born.wrapping_sub(1)] {
            for armed in [false, true] {
                let s = mine::State {
                    born,
                    last_contact: last,
                    armed,
                    ..Default::default()
                };
                let controls = [
                    born,
                    born.wrapping_add(1),
                    born.wrapping_add(2),
                    born.wrapping_add(4),
                    born.wrapping_add(5),
                ]
                .map(|frame| [1, 0, frame as i32, 0, 0, 0])
                .to_vec();
                cases.push(run(
                    format!("contact-{born}-{last}-{armed}"),
                    s,
                    base,
                    car.clone(),
                    controls,
                    world,
                ));
            }
        }
    }
    for cell in 0..64 {
        for rotation in [0, 1] {
            let data = world.terrain();
            let position = [
                (data.origin[0] + ((cell % 8) << data.shifts[0]) + (1 << (data.shifts[0] - 1))) * 8,
                start[1],
                (data.origin[1] + ((cell / 8) << data.shifts[1]) + (1 << (data.shifts[1] - 1))) * 8,
            ];
            let pose = Pose {
                position,
                matrix: Matrix::from_angles([rotation * 3000, rotation * 14000, rotation * 2000]),
            };
            cases.push(run(
                format!("align-{cell}-{rotation}"),
                mine::State::default(),
                pose,
                car.clone(),
                vec![[2, 0, 0, 0, 0, 0]; 2],
                world,
            ));
        }
    }
    cases.push(run(
        "align-outside".into(),
        mine::State::default(),
        Pose {
            position: [i32::MAX / 2, 0, i32::MAX / 2],
            ..base
        },
        car.clone(),
        vec![[2, 0, 0, 0, 0, 0]],
        world,
    ));
    for external in 0..3 {
        for quadrant in 0..9 {
            for high in [false, true] {
                let mut v = car.clone();
                let matrix = Matrix::from_angles([1100, 13000, 2300]);
                v.pose_input.external = match external {
                    0 => ExternalBasis::None,
                    1 => {
                        v.motion.poses.reference = matrix;
                        ExternalBasis::Body
                    }
                    _ => ExternalBasis::Custom(matrix),
                };
                v.motion.velocity = if high {
                    [i32::MIN, i32::MAX, -1234567]
                } else {
                    [123456, -234567, 345678]
                };
                v.motion.angular = if high {
                    [i32::MAX, i32::MIN, 1234567]
                } else {
                    [0; 3]
                };
                v.blast.health = if high { -123 } else { 5000 };
                let point = [
                    start[0] + (quadrant % 3 - 1) * 500,
                    start[1] + 20,
                    start[2] + (quadrant / 3 - 1) * 700,
                ];
                let controls = vec![[3, 0, 0, point[0], point[1], point[2]]; 3];
                cases.push(run(
                    format!("impulse-{external}-{quadrant}-{high}"),
                    mine::State::default(),
                    base,
                    v,
                    controls,
                    world,
                ));
            }
        }
    }
    fs::write(out.join("mine.json"),serde_json::to_vec(&json!({"cases":cases,"scope":"Original mine lifetime, arming, real terrain alignment and complete point impulse affected state; spawning and explosion callbacks remain separate"})).unwrap()).unwrap();
}
