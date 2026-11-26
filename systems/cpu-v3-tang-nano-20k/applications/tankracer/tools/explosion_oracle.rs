//! Original explosion kernel and private contact fragments on the authored car.
use super::{ints, sprite};
use serde_json::{Value, json};
use std::{fs, path::Path};
use tankracer_game::{
    explosion::{self, State},
    math::Matrix,
    object_query::{EntityId, SegmentHit, TankId},
    pose::Pose,
    projectile,
    vehicle_runtime::{SinglePlayer, rcc_car_probe},
    vehicle_startup::Player,
};
pub(super) fn snapshot(s: &State, seed: u32) -> Vec<u16> {
    let mut out = Vec::new();
    ints(
        &mut out,
        [
            i32::from(s.slot),
            s.timer,
            s.angular_velocity,
            i32::from(s.visible),
            s.smoke_timer,
            i32::from(s.smoke_count),
            s.band as i32,
            s.ring as i32,
            i32::from(s.owner.is_some()),
            s.owner.map_or(0, |v| v.0 as i32),
        ],
    );
    ints(&mut out, s.velocity);
    ints(&mut out, s.attachment);
    ints(
        &mut out,
        [
            i32::from(s.fragments_enabled),
            i32::from(s.fragment_count),
            s.fragment_time,
            i32::from(s.fragment_vehicle.is_some()),
            s.fragment_vehicle.map_or(0, |v| v.0 as i32),
        ],
    );
    out.extend([
        u16::from(s.fragment_uv[0]) | (u16::from(s.fragment_uv[1]) << 8),
        u16::from(s.fragment_uv[2]) | (u16::from(s.fragment_uv[3]) << 8),
    ]);
    sprite(&mut out, s.sprite);
    for puff in s.smoke {
        sprite(&mut out, puff.sprite);
        ints(&mut out, puff.velocity);
        ints(&mut out, [puff.lift, puff.angular_velocity]);
        out.extend([puff.active, 0]);
    }
    for f in s.fragments {
        ints(&mut out, f.position);
        ints(&mut out, f.velocity);
        out.extend(f.size.map(|v| v as u16));
        ints(&mut out, [f.angular]);
        out.extend([f.rotation, 0]);
    }
    assert_eq!(out.len(), 676);
    ints(&mut out, [seed as i32]);
    out
}
fn run(
    tag: String,
    mut state: State,
    resources: explosion::Resources,
    car: &Player,
    mut seed: u32,
    owner: Option<Pose>,
    controls: Vec<[i32; 3]>,
) -> Value {
    let origin = car
        .physics
        .motion
        .poses
        .base_position
        .map(|v| v.wrapping_add(333));
    let local = [13456, -2345, 45321];
    let world = origin.map(|v| v.wrapping_sub(1234));
    let velocity = [123456, -654321, -987654];
    let mut input = snapshot(&state, seed);
    ints(
        &mut input,
        [
            resources.main as i32,
            resources.smoke as i32,
            i32::from(resources.small),
            resources.damping as i32,
            i32::from(resources.map_fog),
        ],
    );
    ints(&mut input, origin);
    input.push(u16::from(owner.is_some()));
    ints(&mut input, owner.map_or(Matrix::IDENTITY, |p| p.matrix).0);
    ints(&mut input, owner.map_or([0; 3], |p| p.position));
    ints(&mut input, [car.physics.entity.0 as i32]);
    ints(&mut input, car.physics.motion.velocity);
    ints(&mut input, local);
    ints(&mut input, world);
    ints(&mut input, velocity);
    ints(&mut input, [car.prefix.spin.speed]);
    let body = car.model.parts[tankracer_assets::config::Part::Body.slot()].as_ref();
    input.push(u16::from(body.is_some()));
    input.push(body.map_or(0, |m| m.source.bounds[1][2]) as u16);
    assert_eq!(input.len(), 749);
    let mut frames = Vec::new();
    for c in &controls {
        let status = match c[0] {
            0 => {
                explosion::initialize(&mut state, origin, c[1] as u32, &mut seed, resources);
                0
            }
            1 => {
                explosion::finish_spawn(&mut state);
                0
            }
            2 => {
                rcc_car_probe::explosion_tick(&mut state, origin, c[1], &mut seed, resources, owner)
                    .map_or_else(|v| v, i32::from)
            }
            3 => rcc_car_probe::explosion_contact(
                &mut state,
                car,
                explosion::Contact {
                    hit: projectile::sweep::Hit {
                        vehicle: TankId(0),
                        world: SegmentHit {
                            point: world,
                            normal: [0; 3],
                        },
                        local: SegmentHit {
                            point: local,
                            normal: [0; 3],
                        },
                    },
                    velocity,
                },
                super::kind_of(c[2]),
                &mut seed,
            )
            .map_or_else(|v| v, |()| 0),
            _ => panic!("explosion operation"),
        };
        let mut frame = Vec::new();
        ints(&mut frame, [status]);
        frame.extend(snapshot(&state, seed));
        frames.push(frame);
    }
    json!({"tag":tag,"input":input,"controls":controls,"frames":frames})
}
pub fn export(out: &Path, world: &SinglePlayer) {
    let original = world.vehicles.entries[0].as_ref().unwrap();
    let base = world.objects.components.explosion_resources;
    let mut cases = Vec::new();
    for kind in 0..6 {
        for enabled in [false, true] {
            for spin in [0, 1] {
                for has_body in [false, true] {
                    let mut car = original.clone();
                    car.prefix.spin.speed = spin;
                    car.physics.motion.velocity = [11111, -22222, 33333];
                    if !has_body {
                        car.model.parts[tankracer_assets::config::Part::Body.slot()] = None;
                    }
                    let mut state = State {
                        fragment_count: 2,
                        fragment_time: 123,
                        band: u32::MAX,
                        ring: u32::MAX,
                        ..Default::default()
                    };
                    for (i, f) in state.fragments.iter_mut().enumerate() {
                        f.rotation = 65000u16.wrapping_add(i as u16);
                        f.size = [175, 175];
                    }
                    cases.push(run(
                        format!("contact-{kind}-{enabled}-{spin}-{has_body}"),
                        state,
                        base,
                        &car,
                        world.shared.random,
                        None,
                        vec![
                            [0, i32::from(enabled), 0],
                            [1, 0, 0],
                            [3, 0, kind],
                            [2, 33, 0],
                            [3, 0, kind],
                            [2, 257, 0],
                        ],
                    ));
                }
            }
        }
    }
    for variant in 0..12 {
        for attached in [false, true] {
            let mut state = State {
                band: u32::MAX,
                ring: u32::MAX,
                owner: attached.then_some(EntityId(997)),
                velocity: [-123456, 234567, -345678],
                attachment: [2048, -1024, 512],
                ..Default::default()
            };
            let mut resources = base;
            let mut controls = vec![
                [2, 0, 0],
                [2, 1, 0],
                [2, 31, 0],
                [2, 65, 0],
                [2, 97, 0],
                [2, 257, 0],
            ];
            if variant == 0 {
                controls = vec![[0, 0x1234567, 0], [1, 0, 0], [2, 31, 0], [2, 97, 0]];
            }
            if variant == 1 || variant == 2 {
                state.sprite.uv = [96, 96, 127, 127];
                controls = vec![[2, 31, 0], [2, 31, 0]];
                if variant == 2 {
                    state.smoke[1].active = 1;
                    state.smoke[1].sprite.color = [1; 3];
                }
            }
            if variant == 3 {
                state.smoke_count = 9;
                state.smoke_timer = if attached { 64 } else { 96 };
                state.smoke[9].lift = 1234;
                state.smoke[1].active = u16::MAX;
                state.smoke[1].sprite.color = [128, 19, 0];
                state.smoke[1].angular_velocity = -25;
            }
            if variant == 4 {
                state.timer = i32::MAX;
                state.angular_velocity = i32::MIN + 1;
                state.velocity = [i32::MIN, -1, i32::MAX];
                controls = vec![
                    [2, 1, 0],
                    [2, -1, 0],
                    [2, i32::MAX, 0],
                    [2, i32::MIN, 0],
                    [2, 33, 0],
                ];
            }
            if variant == 5 {
                state.fragment_count = 15;
                state.fragment_time = i32::MAX;
                for (i, f) in state.fragments.iter_mut().enumerate() {
                    f.size = [i16::MIN, i16::MAX];
                    f.velocity = [i32::MAX, i32::MIN, -1];
                    f.angular = if i % 2 == 0 { -1 } else { 0 };
                    f.rotation = u16::MAX;
                }
            }
            if variant == 6 {
                state.band = 0;
                state.smoke_timer = 96;
                controls = vec![[2, 1, 0], [1, 0, 0], [2, 97, 0]];
            }
            if variant == 7 {
                state.ring = 1;
                state.sprite.uv = [96, 96, 127, 127];
                controls = vec![[2, 31, 0]];
            }
            if variant == 8 {
                resources.damping = 0xff00_ffff;
                state.smoke_count = 255;
                state.smoke_timer = i32::MAX;
                controls = vec![[2, -33, 0], [2, i32::MAX, 0], [2, i32::MIN, 0]];
            }
            if variant == 9 {
                resources.small = !resources.small;
                resources.main = 0xfedc_ba98;
                controls = vec![[0, -1, 0], [1, 0, 0], [2, 97, 0]];
            }
            if variant == 10 {
                state.smoke_timer = i32::MAX;
                state.smoke[0].active = 17;
                state.smoke[0].sprite.color = [5, 1, 0];
                state.smoke[0].lift = i32::MIN;
                state.smoke[0].angular_velocity = i32::MAX;
                controls = vec![[2, 1, 0], [2, -257, 0], [2, 128, 0]];
            }
            let owner = if attached && variant != 11 {
                Some(Pose {
                    position: original.physics.motion.poses.base_position,
                    matrix: Matrix::from_angles([1234, 23456, 3456]),
                })
            } else {
                None
            };
            cases.push(run(
                format!("animation-{variant}-{attached}"),
                state,
                resources,
                original,
                world.shared.random ^ (variant as u32),
                owner,
                controls,
            ));
        }
    }
    fs::write(out.join("explosion.json"),serde_json::to_vec(&json!({"cases":cases,"scope":"Original explosion initialize/finish/tick and private contact fragments on the authored model; animation/smoke/fragment state, exact RNG and middle-stage failure prefixes. Shared band/ring ownership and full scene callbacks follow separately."})).unwrap()).unwrap();
}
