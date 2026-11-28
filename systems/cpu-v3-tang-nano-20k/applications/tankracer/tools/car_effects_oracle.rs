//! Full original car::effects on initialized ENGLAND owners and actual profiles.
use super::{
    ints, pickup_snapshot, temporary_oracle, track_effects_oracle, vehicle_effects_oracle,
};
use serde_json::json;
use std::{fs, path::Path};
use tankracer_game::{
    effect_pool::{LineHandle, QuadHandle},
    math::Matrix,
    object_query::EntityId,
    pose::{ExternalBasis, Pose},
    vehicle,
    vehicle_runtime::{SinglePlayer, rcc_player_probe},
    vehicle_scene::{debris, lights, marks, pickup, shield, smoke, spray},
};
fn snapshot(world: &SinglePlayer) -> Vec<u16> {
    let car = world.vehicles.entries[0].as_ref().unwrap();
    let mut out = vehicle_effects_oracle::world_snapshot(world);
    out.extend(track_effects_oracle::snapshot(
        &car.spray,
        &car.marks,
        &car.lights,
        &world.shared.pool,
    ));
    ints(
        &mut out,
        pickup_snapshot::values(&world.shared.pickup, &world.shared.pool)[..816]
            .iter()
            .copied(),
    );
    out.extend(
        &temporary_oracle::snapshot(
            &world.shared.temporary,
            &world.shared.pool,
            car.prefix.temporary_slot,
            world.shared.random,
        )[..274],
    );
    assert_eq!(out.len(), 13200);
    out
}
fn inputs(world: &SinglePlayer, dt: i32, number: u32) -> Vec<i32> {
    let car = world.vehicles.entries[0].as_ref().unwrap();
    let v = &car.physics;
    let (_, _, _, offset, material, special) = rcc_player_probe::effect_tuning(world);
    let frame = vehicle::Frame {
        dt,
        number,
        mode: 0,
        input: car.input.drive,
        drive: Default::default(),
    };
    let s = smoke::Input::vehicle(v, &frame, special);
    let d = debris::Input::vehicle(v, &frame, (world.camera_position, world.camera_forward));
    let h = shield::Input::vehicle(v, &frame, None);
    let t = spray::Input::vehicle(v, &frame, i32::from(car.visible));
    let m = marks::Input::vehicle(v, &frame);
    let l = lights::Input::vehicle(
        v,
        &frame,
        lights::Settings {
            active: true,
            visible: i32::from(car.visible),
            queued: false,
            reverse_braking: car.brake.reverse_braking,
            material,
        },
        offset,
    );
    let mut f = vec![
        6,
        dt,
        i32::from(s.player),
        i32::from(s.special),
        s.lateral_velocity,
        s.track_slip,
        s.penetrating_force,
        i32::from(d.has_plane),
        d.plane_distance,
        0,
        i32::from(h.enabled),
        h.bank as i32,
        i32::from(l.basis.is_some()),
        l.forward_velocity,
        v.entity.0 as i32,
        number as i32,
    ];
    f.extend(s.velocity);
    f.extend(d.position);
    f.extend(s.materials);
    f.extend(s.contacts.into_iter().flatten());
    f.extend(s.corners.map(|v| v as i32));
    for pose in s.poses {
        f.extend(pose.matrix.0);
        f.extend(pose.position);
    }
    f.extend(d.camera_position);
    f.extend(d.camera_forward);
    f.extend(l.basis.unwrap_or(Matrix::IDENTITY).0);
    assert_eq!(f.len(), 79);
    f.extend([t.track_speed, m.mode, i32::from(m.deformable)]);
    f.extend(m.travel);
    f.extend(m.kinds);
    f.extend(m.tracks.into_iter().flat_map(|m| m.0));
    f.extend(l.raw_heights);
    f.extend([
        t.visible,
        material as i32,
        l.throttle,
        l.brake,
        i32::from(car.brake.reverse_braking),
        i32::from(car.prefix.temporary_timer),
        i32::from(car.prefix.temporary_slot),
        world.vehicles.entries.len() as i32,
    ]);
    f.extend(l.offset);
    f.extend(l.position);
    f.extend(t.basis.0);
    f.push(i32::from(l.disabled));
    assert_eq!(f.len(), 136);
    f
}
pub fn export(root: &Path, out: &Path) {
    let mut cases = Vec::new();
    for branch in 0..38 {
        let mut world = super::reference_world::full_race(root);
        let entity = world.vehicles.entries[0].as_ref().unwrap().physics.entity;
        let (spray, smoke, pickup, _, _, _) = rcc_player_probe::effect_tuning(&world);
        {
            let car = world.vehicles.entries[0].as_mut().unwrap();
            let v = &mut car.physics;
            v.motion.velocity = [200000, -130000, 350000];
            v.motion.rates.lateral_velocity = 200000;
            v.motion.rates.track_slip = 60000;
            v.motion.rates.penetrating_force = 5000000;
            v.powertrain.track_speed = 300000;
            v.powertrain.local_velocity[2] = 300000;
            v.impact_cooldown = 500;
            v.contact.sampling.materials = [1, 1];
            v.contact.sampling.primary.has_extra = true;
            v.contact.sampling.primary.extra[3] = -100;
            // Deliberately different anchors and reference/external bases.
            v.motion.poses.base_position = [15000, 1000, 25000];
            v.motion.poses.physical_position = [15500, 1200, 24700];
            v.motion.poses.reference = Matrix::from_angles([100, 12000, 200]);
            v.pose_input.external = ExternalBasis::Custom(Matrix::from_angles([1234, 5678, 3456]));
            let points = [
                [10000, 4000, 20000],
                [12000, 4200, 18000],
                [20000, 4500, 20000],
                [22000, 4300, 18000],
            ];
            for i in 0..4 {
                v.motion.contacts[i].world = points[i];
                v.contact.sampling.primary.points[i].kind = 0;
                v.contact.sampling.primary.points[i].raw_height = 0;
                v.contact.response.points[i].corner = i as u32;
                v.motion.travel[i] = 50000;
            }
            v.motion.poses.tracks[0].matrix = Matrix::from_angles([200, 5678, 0]);
            v.motion.poses.tracks[1].matrix = Matrix::from_angles([400, 5678, 0]);
            v.motion.poses.body = Pose {
                matrix: Matrix::from_angles([1000, 2000, 3000]),
                position: [15000, 1000, 25000],
            };
            v.motion.poses.turret = Pose {
                matrix: Matrix::from_angles([2000, 3000, 4000]),
                position: [15000, 2000, 25000],
            };
            car.spray.timer = 40;
            car.smoke.timer = 100;
            car.smoke.damage = [9, 9];
            car.smoke.damage_timers = [300, 300];
            car.input.drive.throttle = 0;
            car.input.drive.brake = 1;
        }
        // Activate actual shared pickup and temporary effects before the first call.
        if matches!(
            branch,
            0 | 1
                | 2
                | 4
                | 7
                | 10
                | 12
                | 14
                | 15
                | 17
                | 20
                | 23
                | 24
                | 25
                | 26
                | 27
                | 28
                | 29
                | 30
                | 31
                | 32
                | 33
                | 34
                | 35
                | 36
                | 37
        ) {
            let camera = pickup::Camera {
                position: world.camera_position,
                matrix: world.camera_basis,
            };
            world
                .shared
                .pickup
                .start(
                    pickup::Start {
                        target: entity,
                        origin: [15000, 1000, 25000],
                        velocity: [10000, 0, 10000],
                        frame: 1,
                        follow: true,
                    },
                    pickup::Materials {
                        rays: 11,
                        rings: 12,
                    },
                    camera,
                    &pickup,
                    &mut world.shared.pool,
                )
                .unwrap();
        }
        {
            let car = world.vehicles.entries[0].as_mut().unwrap();
            match branch {
                0 => {}
                1 => {
                    car.prefix.temporary_timer = 100;
                    car.prefix.temporary_slot = 0;
                }
                2 => {
                    car.physics.contact.sampling.effect.active = 1;
                }
                3 => {
                    car.spray.particles[0].life = 1;
                    car.spray.particles[0].kind = -1;
                }
                4 => {
                    car.marks.count = 68;
                    car.marks.head = 68;
                    car.marks.active = [1, 1];
                }
                5 => {
                    car.smoke.sockets[0] = 6;
                }
                6 => {
                    car.smoke.sockets[1] = 255;
                }
                7 => {
                    car.lights.active[0] = 1;
                    car.lights.handles[0] = QuadHandle(161);
                }
                8 => {
                    car.prefix.temporary_timer = 1;
                    car.prefix.temporary_slot = -1;
                }
                9 => {
                    car.visible = false;
                }
                10 => {
                    car.physics.player = false;
                }
                11 => {
                    car.physics.pose_input.external = ExternalBasis::None;
                }
                12 => {
                    car.brake.reverse_braking = true;
                    car.physics.powertrain.local_velocity[2] = -100001;
                }
                13 => {
                    car.input.drive.throttle = -1000;
                    car.input.drive.brake = 0;
                }
                14 => {
                    car.physics.contact.sampling.primary.has_extra = false;
                }
                15 => {
                    car.physics.contact.sampling.materials = [2, 2];
                }
                16 => {
                    car.smoke.side = 3;
                    car.smoke.damage = [0, 0];
                    car.physics.motion.rates.lateral_velocity = 0;
                }
                17 => {
                    car.physics.contact.sampling.effect.active = 1;
                    car.physics.player = false;
                }
                18 => {
                    car.prefix.temporary_timer = 1;
                    car.prefix.temporary_slot = 4;
                }
                19 => {
                    car.physics.motion.rates.lateral_velocity = 0;
                    car.physics.motion.rates.track_slip = 0;
                }
                20 => {
                    car.physics.motion.poses.physical_position[1] = 50000;
                }
                21 => {
                    car.spray.timer = i32::MAX;
                }
                22 => {
                    car.physics.contact.sampling.materials = [-1, 1];
                }
                23 => {
                    car.physics.motion.travel = [0; 4];
                }
                24 => {
                    car.smoke.damage = [0, 0];
                    car.smoke.particles.fill(smoke::Particle {
                        life: 1,
                        velocity: [100, -100, 200],
                        ..Default::default()
                    });
                }
                25 => {
                    car.debris.timer = 0;
                    car.debris.previous_plane = 1;
                }
                26 => {
                    car.physics.contact.sampling.effect.active = 1;
                }
                27 => {
                    car.smoke.damage = [0, 0];
                    car.physics.impact_cooldown = 0;
                }
                28 => {
                    car.marks.active = [1, 1];
                    car.marks.count = 68;
                    car.marks.tail = 0;
                    car.marks.head = 0;
                }
                29 => {
                    car.physics.motion.rates.lateral_velocity = -200000;
                    car.physics.powertrain.local_velocity[2] = -1;
                }
                30 => {
                    car.physics.contact.sampling.effect.active = 1;
                    car.physics.pose_input.external = ExternalBasis::None;
                }
                31 => {
                    car.prefix.temporary_timer = 100;
                    car.prefix.temporary_slot = 0;
                }
                32 => {}
                33 => {}
                34 => {}
                35 => {}
                36 => {}
                37 => {}
                _ => unreachable!(),
            }
        }
        if branch == 1 || branch == 31 {
            let mut slot = 0;
            world
                .shared
                .temporary
                .start(
                    0,
                    &mut slot,
                    [15000, 1000, 25000],
                    1,
                    tankracer_game::vehicle_scene::temporary::Context {
                        pool: &mut world.shared.pool,
                        random: &mut world.shared.random,
                    },
                )
                .unwrap();
            world.vehicles.entries[0]
                .as_mut()
                .unwrap()
                .prefix
                .temporary_slot = slot;
        }
        if branch == 2 || branch == 26 {
            rcc_player_probe::special_effects(&mut world, true);
        }
        if branch == 4 {
            world.vehicles.entries[0].as_mut().unwrap().marks.endpoints =
                [[[1, 2, 3], [4, 5, 6]]; 2];
        }
        if branch == 12 {
            world.shared.debris.banks[1].owner = Some(EntityId(999));
        }
        if branch == 27 {
            for l in &mut world.shared.pool.lines {
                l.points[0][1] = 0;
            }
            world.shared.pool.line_count = 100;
        }
        if branch == 28 {
            world.vehicles.entries[0]
                .as_mut()
                .unwrap()
                .marks
                .handles
                .fill(QuadHandle(0));
        }
        if branch == 32 {
            world.vehicles.entries.resize_with(10, || None);
        }
        if branch == 33 {
            world.shared.impacts.batches[0].active = true;
            world.shared.impacts.batches[0].owner = Some(entity);
            world.shared.impacts.batches[0].particles[0].line = LineHandle(101);
        }
        if branch == 34 {
            world.shared.pickup.banks[0].frame = 17;
        }
        if branch == 35 {
            world.shared.random = u32::MAX;
        }
        if branch == 36 {
            world.shared.pickup.banks[0].rays[3].palette_index = 10;
        }
        if branch == 37 {
            world.shared.pickup.banks[0].life = 1;
            world.shared.pickup.banks[0].rings[4].handle = QuadHandle(999);
        }
        let input = snapshot(&world);
        let mut controls = Vec::new();
        let mut frames = Vec::new();
        for frame in 0..3 {
            let dt = if frame == 1 { 33 } else { 40 };
            let number = 17 + frame;
            controls.push(inputs(&world, dt, number));
            let status =
                rcc_player_probe::effects(&mut world, dt, number).map_or_else(|e| e, |()| 0);
            let mut answer = Vec::new();
            ints(&mut answer, [status]);
            answer.extend(snapshot(&world));
            frames.push(answer);
        }
        let mut styles = Vec::new();
        for style in spray.styles {
            ints(
                &mut styles,
                [
                    style.spin as i32,
                    i32::from(style.initial_size),
                    i32::from(style.gain.multiplier),
                    i32::from(style.gain.shift),
                ],
            );
        }
        let mut pickup_tuning = Vec::new();
        ints(
            &mut pickup_tuning,
            [
                pickup.outer_growth,
                pickup.inner_growth,
                pickup.flare_growth,
            ],
        );
        pickup_tuning.extend(pickup.palette.into_iter().flatten().map(u16::from));
        cases.push(json!({"tag":format!("car-effects-{branch}"),"input":input,"controls":controls,"frames":frames,"kinds":spray.kinds,"styles":styles,"sockets":smoke.sockets.into_iter().flatten().flatten().collect::<Vec<_>>(),"pickup":pickup_tuning}));
    }
    fs::write(out.join("car-effects.json"),serde_json::to_vec(&json!({"cases":cases,"scope":"Original complete car::effects on FullRace-loaded ENGLAND car/profile/shared owners: spray, marks, pickup fade, impacts, smoke, debris, lights, temporary and shield in actual source order. Full enclosing Tank physics/init/mixed scheduler remain separate."})).unwrap()).unwrap();
}
