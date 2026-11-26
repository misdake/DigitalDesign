//! Complete original non-AI inactive Tank calls on real ENGLAND terrain.
use super::{
    ints, pickup_snapshot, temporary_oracle, track_effects_oracle, vehicle_effects_oracle,
    vehicle_startup_oracle,
};
#[allow(dead_code)]
#[path = "drive_oracle.rs"]
mod drive_fixture;
use serde_json::json;
use std::{fs, path::Path};
use tankracer_assets::config::Part;
use tankracer_game::{
    attitude,
    effect_pool::{LineHandle, Quad, QuadHandle},
    math::Matrix,
    object_query::TankId,
    pose::ExternalBasis,
    vehicle,
    vehicle_runtime::{SinglePlayer, rcc_car_probe},
    vehicle_scene::{pickup, temporary},
};
struct Device;
impl tankracer_game::scene_objects::collision::Services for Device {
    fn sound(
        &mut self,
        _: tankracer_game::scene_objects::collision::Sound,
        _: &mut tankracer_game::scene_objects::collision::Collision<'_>,
        _: tankracer_game::obstacle_collision::Invocation<'_>,
    ) -> Result<(), i32> {
        Ok(())
    }
    fn feedback(
        &mut self,
        _: i32,
        _: &mut tankracer_game::scene_objects::collision::Collision<'_>,
        _: tankracer_game::obstacle_collision::Invocation<'_>,
    ) -> Result<(), i32> {
        Ok(())
    }
}
impl tankracer_game::vehicle_runtime::Devices for Device {
    fn surface_feedback(&mut self) -> Result<(), i32> {
        Ok(())
    }
    fn impact_feedback(&mut self, _: i32) -> Result<(), i32> {
        Ok(())
    }
}
fn snapshot(world: &SinglePlayer) -> Vec<u16> {
    let c = world.vehicles.entries[0].as_ref().unwrap();
    let mut out = vehicle_startup_oracle::snapshot(c);
    let effects = vehicle_effects_oracle::world_snapshot(world);
    out.extend_from_slice(&effects[350..714]);
    out.extend_from_slice(&effects[744..1554]);
    ints(
        &mut out,
        pickup_snapshot::values(&world.shared.pickup, &world.shared.pool)[..816]
            .iter()
            .copied(),
    );
    out.extend_from_slice(
        &temporary_oracle::snapshot(
            &world.shared.temporary,
            &world.shared.pool,
            c.prefix.temporary_slot,
            world.shared.random,
        )[..274],
    );
    out.extend([
        u16::from(world.shared.spin.count),
        u16::from(world.shared.spin.pulse),
        0,
        0,
    ]);
    let tracks = track_effects_oracle::snapshot(&c.spray, &c.marks, &c.lights, &world.shared.pool);
    out.extend_from_slice(&tracks[384..]);
    out.extend_from_slice(&effects[1558..]);
    ints(&mut out, [world.shared.random as i32]);
    out.extend_from_slice(&effects[740..744]);
    out.extend_from_slice(&effects[714..738]);
    let v = &world.shared.view;
    ints(&mut out, [i32::from(v.enabled), i32::from(v.visible)]);
    ints(&mut out, v.position);
    ints(&mut out, [v.angular_rate]);
    ints(&mut out, v.matrix.0);
    let s = &world.shared;
    ints(
        &mut out,
        [s.camera.pitch, s.camera.height, s.camera.distance],
    );
    ints(
        &mut out,
        [
            s.boost.texture_frame as i32,
            i32::from(s.boost.texture_delta),
        ],
    );
    ints(&mut out, s.boost.source_uv.map(i32::from));
    ints(&mut out, s.boost.current_uv.map(i32::from));
    ints(&mut out, s.boost.feedback);
    assert_eq!(out.len(), 14280);
    ints(
        &mut out,
        drive_fixture::config(
            &c.model.config.tank,
            world.surfaces(),
            &rcc_car_probe::drive_tuning(world, TankId(0)),
        ),
    );
    assert_eq!(out.len(), 14796);
    out
}
fn fallback_position(world: &SinglePlayer) -> [i32; 3] {
    let data = world.terrain();
    for node in &data.fallback.nodes {
        let mesh = &data.meshes[node.mesh as usize];
        for center in &mesh.face_centers {
            let position = std::array::from_fn(|i| (mesh.position[i] + i32::from(center[i])) * 8);
            let mut hit = Default::default();
            let mut normal = [0; 3];
            if !tankracer_game::terrain::query(data, position[0], position[2], &mut hit).unwrap()
                && tankracer_game::geometry::fallback(data, position, 0, &mut normal)
            {
                return position;
            }
        }
    }
    panic!("ENGLAND inactive fallback start");
}
pub fn export(root: &Path, out: &Path) {
    let mut cases = Vec::new();
    for branch in 0..57 {
        let mut world = super::reference_world::full_race(root);
        let fallback = (branch == 36 || branch == 37).then(|| fallback_position(&world));
        let entity = world.vehicles.entries[0].as_ref().unwrap().physics.entity;
        let (_, _, pickup, offset, light_material, _) =
            rcc_car_probe::effect_config(&world, TankId(0));
        {
            let car = world.vehicles.entries[0].as_mut().unwrap();
            car.ai = None;
            car.visible = false;
            let v = &mut car.physics;
            v.player = branch & 1 == 0;
            v.motion.velocity = [5001, -9999, 45000];
            v.motion.angular = [500, -700, 300];
            v.motion.travel = [50000, -30000, 17000, -1001];
            v.motion.pivot = [1, 2, 3];
            v.motion.acceleration = [7000, -8000];
            v.motion.rates.penetrating_force = 123456;
            v.motion.rates.travel_sum = -3456;
            v.motion.ready = branch & 2 != 0;
            v.pose_input.timer = 213;
            v.pose_input.turret_yaw = 117;
            v.pose_input.external = if branch % 3 == 0 {
                ExternalBasis::None
            } else if branch % 3 == 1 {
                ExternalBasis::Custom(Matrix::from_angles([1234, 5678, 3456]))
            } else {
                ExternalBasis::Body
            };
            v.contact.sampling.primary.special_count = 7;
            v.contact.sampling.primary.flagged_count = 3;
            v.contact.sampling.primary.airborne_count = 2;
            v.contact.sampling.primary.has_extra = true;
            v.contact.sampling.materials = [2, 3];
            v.contact.response.vertical_delta = 137;
            car.input.drive.throttle = 12000;
            car.input.drive.steering = -4000;
            car.prefix.alternating_sign = -1;
            car.prefix.target = 39;
            car.prefix.boost.camera_velocity = 713;
            car.prefix.boost.camera_timer = 811;
            for (i, p) in car.spray.particles.iter_mut().enumerate() {
                p.life = i as i16 + 1;
                p.size = 123;
                p.position = [4, 5, 6];
            }
            for (i, p) in car.smoke.particles.iter_mut().enumerate() {
                p.life = i as i16 + 1;
                p.size = 117;
                p.position = [7, 8, 9];
            }
            car.spray.count = 8;
            car.smoke.count = 15;
            car.debris.active = 1;
            car.debris.timer = 417;
            match branch {
                2 | 3 => {
                    v.motion.velocity = [-700000, 2, 500100];
                    car.input.drive.throttle = -12000;
                }
                4 | 5 => {
                    v.powertrain.track_speed = -100;
                    car.input.drive.throttle = 0;
                    car.input.drive.brake = 20000;
                }
                6..=11 => {
                    car.prefix.boost.timer = [1, 39, 2500, 2501, 5000, i16::MAX][branch - 6];
                    car.prefix.boost.extra_speed = 170000;
                    car.prefix.boost.power = 34000;
                    car.prefix.boost.power_loss = 5;
                    car.prefix.boost.active = 1;
                    car.prefix.boost.phase = if branch & 1 == 0 { 8 } else { 2 };
                }
                12 | 13 => {
                    car.prefix.spin.speed = 500;
                    car.prefix.spin.hold = 1;
                    car.prefix.spin.color_timer = 1;
                }
                14 | 15 => {
                    car.prefix.spin.speed = 1;
                    car.prefix.spin.disabled = 1;
                }
                16..=19 => {
                    car.prefix.temporary_timer = if branch < 18 { 100 } else { 1 };
                    car.prefix.sidegrip.multiplier = 17;
                    car.model.config.tank.drive.sidegrip_fraction.multiplier = 19;
                }
                20 | 21 => {
                    car.spray.count = 0;
                    car.smoke.count = 0;
                }
                22 | 23 => {
                    v.contact.sampling.effect.active = 1;
                }
                24 => {
                    car.lights.active = [1, 1];
                    car.lights.handles = [QuadHandle(161), QuadHandle(0)];
                }
                25 => {
                    car.prefix.temporary_timer = 1;
                    car.prefix.temporary_slot = -1;
                }
                26 => {
                    car.prefix.temporary_timer = 1;
                    car.prefix.temporary_slot = 4;
                }
                27 => {
                    car.prefix.temporary_timer = 100;
                    car.prefix.temporary_slot = 4;
                }
                28 | 29 => {
                    car.model.parts[Part::LeftTrack.slot()] = None;
                }
                30 | 31 => {
                    v.motion.poses.reference.0[0] = i32::MIN;
                }
                32 | 33 => {
                    v.motion.poses.body.matrix = Matrix([0; 9]);
                    v.motion.poses.reference = Matrix([0; 9]);
                }
                34 | 35 => {
                    v.motion.poses.body.position = [i32::MIN / 2, 10000, i32::MIN / 2];
                    car.inactive_height = -1200;
                }
                36 | 37 => {
                    let pos = fallback.unwrap();
                    v.motion.poses.body.position = pos;
                    v.motion.poses.tracks[0].matrix = Matrix::IDENTITY;
                    v.motion.poses.tracks[0].position[1] =
                        pos[1] - v.contact.sampling.primary.points[0].local[1];
                }
                38 | 39 => {
                    v.motion.velocity = [i32::MAX, 1, i32::MIN + 1];
                    car.input.drive.steering = -32768;
                }
                40 | 41 => {
                    v.motion.rates.track_slip = -123456;
                }
                42 | 43 => {
                    car.prefix.boost.timer = 2501;
                    car.input.drive.throttle = -32768;
                    v.motion.velocity = [-500001, 5, -600000];
                }
                44 | 45 => {
                    v.contact.sampling.effect.active = 1;
                }
                46 | 47 => {
                    car.input.drive.brake = i32::MAX;
                    car.input.drive.steering = i32::MIN;
                }
                55 | 56 => {
                    v.contact.sampling.effect.active = 1;
                    car.prefix.spin.speed = 500;
                    car.prefix.spin.flags = 1;
                    car.prefix.temporary_timer = 100;
                }
                _ => {}
            }
        }
        let player = world.vehicles.entries[0].as_ref().unwrap().physics.player;
        let bank = usize::from(player);
        world.shared.debris.banks[bank].owner = Some(entity);
        for p in &mut world.shared.debris.banks[bank].particles {
            p.active = 1;
        }
        if (16..=19).contains(&branch) || branch >= 55 {
            let car = world.vehicles.entries[0].as_mut().unwrap();
            world
                .shared
                .temporary
                .start(
                    0,
                    &mut car.prefix.temporary_slot,
                    car.physics.motion.poses.base_position,
                    1,
                    temporary::Context {
                        pool: &mut world.shared.pool,
                        random: &mut world.shared.random,
                    },
                )
                .unwrap();
        }
        if branch == 44 || branch == 45 || branch >= 55 {
            world
                .shared
                .pickup
                .start(
                    pickup::Start {
                        target: entity,
                        origin: world.vehicles.entries[0]
                            .as_ref()
                            .unwrap()
                            .physics
                            .motion
                            .poses
                            .base_position,
                        velocity: [10000, 0, 10000],
                        frame: 1,
                        follow: true,
                    },
                    pickup::Materials {
                        rays: 11,
                        rings: 12,
                    },
                    pickup::Camera {
                        position: world.camera_position,
                        matrix: world.camera_basis,
                    },
                    &pickup,
                    &mut world.shared.pool,
                )
                .unwrap();
        }
        if branch == 22 || branch == 23 {
            world.shared.impacts.batches[0].active = true;
            world.shared.impacts.batches[0].owner = Some(entity);
            world.shared.impacts.batches[0].particles[0].line = LineHandle(101);
        }
        if branch == 46 || branch == 47 {
            let q = world.shared.pool.allocate_quad(Quad::ZERO);
            let car = world.vehicles.entries[0].as_mut().unwrap();
            car.lights.active = [1, 0];
            car.lights.handles[0] = q;
        }
        if branch >= 48 {
            let car = world.vehicles.entries[0].as_mut().unwrap();
            car.physics.impact_cooldown = 500;
            for (i, p) in car.physics.motion.contacts.iter_mut().enumerate() {
                p.world = [
                    10000 + (i as i32 / 2) * 12000,
                    4000,
                    20000 - (i as i32 % 2) * 2000,
                ];
            }
            let kind = if branch < 55 { branch as i32 - 48 } else { 0 };
            assert!(
                world
                    .shared
                    .impacts
                    .emit(
                        &mut car.physics,
                        0,
                        kind,
                        &mut tankracer_game::impact_particles::Context {
                            frame: 1,
                            random: &mut world.shared.random,
                            pool: &mut world.shared.pool,
                            tuning: Default::default(),
                        }
                    )
                    .unwrap()
            );
        }
        rcc_car_probe::numerics(
            &mut world,
            if branch & 2 == 0 {
                attitude::Numerics::Original
            } else {
                attitude::Numerics::OrthogonalV1
            },
        );
        let car = world.vehicles.entries[0].as_ref().unwrap();
        let bounds = car.model.geometry_bounds;
        let track = car.model.parts[Part::LeftTrack.slot()].as_ref();
        let hull = [
            i32::from(track.is_some()),
            track.map_or(0, |p| i32::from(p.source.bounds[1][2])),
            bounds.min[0],
            bounds.max[0],
            bounds.min[2],
            bounds.max[2],
        ];
        let initial = snapshot(&world);
        let mut controls = Vec::new();
        let mut frames = Vec::new();
        for tick in 0..3u32 {
            let dt = if tick == 1 { 33 } else { 40 };
            let number = if branch >= 48 {
                65538 + tick
            } else if branch >= 38 {
                65534 + tick
            } else {
                9 + tick
            };
            let control = [dt, number as i32, world.settings().mode, -100, 3000];
            controls.push(control);
            let frame = vehicle::Frame {
                dt,
                number,
                mode: world.settings().mode,
                input: tankracer_game::drive::Input {
                    throttle: control[3],
                    brake: control[4],
                    ..Default::default()
                },
                drive: Default::default(),
            };
            let status = rcc_car_probe::inactive(&mut world, TankId(0), &frame, &mut Device)
                .map_or_else(|e| e, |()| 0);
            let car = world.vehicles.entries[0].as_ref().unwrap();
            if branch == 36 || branch == 37 {
                assert_eq!(car.physics.contact.sampling.primary.points[0].kind, 4);
                assert_eq!(status, -1);
            }
            let mut answer = Vec::new();
            ints(&mut answer, [status]);
            answer.extend(snapshot(&world));
            frames.push(answer);
        }
        let mut tuning = Vec::new();
        ints(
            &mut tuning,
            [
                pickup.outer_growth,
                pickup.inner_growth,
                pickup.flare_growth,
            ],
        );
        tuning.extend(pickup.palette.into_iter().flatten().map(u16::from));
        assert_eq!(tuning.len(), 36);
        cases.push(json!({"tag":format!("inactive-{branch}"),"input":initial,"controls":controls,"frames":frames,"hull":hull,"light_offset":offset,"light_material":light_material,"pickup":tuning,"numerics":u16::from(world.numerics()==attitude::Numerics::OrthogonalV1)}));
    }
    fs::write(out.join("vehicle-inactive.json"),serde_json::to_vec(&json!({"scope":"Complete original non-AI inactive_car on real terrain, stored drive input and distinct frame controls; full vehicle, shared effects/pools/RNG, model grip, success and failure prefixes. AI recovery is a later scope.","cases":cases})).unwrap()).unwrap();
}
