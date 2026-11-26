//! Actual contact response, including both private part geometries and revisions.
use super::{ints, wide};
use serde_json::json;
use std::{fs, path::Path};
use tankracer_assets::{
    config::{Gain, Part},
    object_catalog::{HomerHit, ShellHit},
};
use tankracer_game::{
    math::Matrix,
    object_query::{SegmentHit, TankId},
    pose::ExternalBasis,
    projectile::{Kind, sweep::Hit},
    tank_hit::{self, Coefficients},
    vehicle_runtime::{SinglePlayer, rcc_car_probe},
    vehicle_startup::Player,
};

fn gains(out: &mut Vec<u16>, gain: Gain) {
    out.extend([gain.multiplier as u16, gain.shift as u16]);
}
fn coefficients(out: &mut Vec<u16>, c: Coefficients) {
    for gain in [c.tilt, c.yaw, c.push, c.slow] {
        gains(out, gain);
    }
    out.push(c.slow_after);
}
fn snapshot(
    car: &Player,
    c: Option<Coefficients>,
    sizes: [[usize; 2]; 2],
    shared: &tankracer_game::vehicle_scene::temporary::State,
    pool: &tankracer_game::effect_pool::Pool,
    seed: u32,
) -> Vec<u16> {
    let mut out = Vec::new();
    if let Some(c) = c {
        coefficients(&mut out, c);
    } else {
        out.extend([0; 9]);
    }
    let v = &car.physics;
    ints(&mut out, v.motion.velocity);
    ints(&mut out, v.motion.angular);
    ints(
        &mut out,
        [
            v.contact.response.timer,
            v.collision.collision_size as i32,
            car.inventory.weapon as i32,
            i32::from(car.inventory.ammo),
        ],
    );
    out.extend(car.hit_counts.iter().flatten().map(|&v| u16::from(v)));
    out.extend(car.smoke.damage.map(u16::from));
    out.extend(car.smoke.sockets.map(u16::from));
    out.extend(car.damage.body.map(u16::from));
    out.extend(car.damage.turret.map(u16::from));
    out.extend(car.damage.tracks.map(u16::from));
    for part in [Part::Body, Part::Turret] {
        let revision = car.model.parts[part.slot()]
            .as_ref()
            .map_or(0, |mesh| mesh.revision());
        for shift in [0, 16, 32, 48] {
            out.push((revision >> shift) as u16);
        }
    }
    assert_eq!(out.len(), 63);
    for (index, part) in [Part::Body, Part::Turret].into_iter().enumerate() {
        if let Some(mesh) = &car.model.parts[part.slot()] {
            let geometry = mesh.geometry();
            assert_eq!(
                [geometry.vertices.len(), geometry.triangles.len()],
                sizes[index]
            );
            out.extend(geometry.vertices.iter().flatten().map(|&v| v as u16));
            out.extend(
                geometry
                    .triangles
                    .iter()
                    .flat_map(|t| t.normal)
                    .map(|v| v as u16),
            );
        } else {
            out.resize(out.len() + (sizes[index][0] + sizes[index][1]) * 3, 0);
        }
    }
    ints(
        &mut out,
        [
            i32::from(car.prefix.sidegrip.multiplier),
            i32::from(car.prefix.sidegrip.shift),
            i32::from(car.model.config.tank.drive.sidegrip_fraction.multiplier),
            i32::from(car.model.config.tank.drive.sidegrip_fraction.shift),
            i32::from(car.prefix.temporary_timer),
            i32::from(car.prefix.temporary_slot),
        ],
    );
    let mut temporary =
        super::temporary_oracle::snapshot(shared, pool, car.prefix.temporary_slot, seed);
    temporary.drain(274..276);
    out.extend(temporary);
    out
}
pub(super) fn input(
    car: &Player,
    shell: ShellHit,
    homer: HomerHit,
    sizes: [[usize; 2]; 2],
    original: &Player,
    shared: &tankracer_game::vehicle_scene::temporary::State,
    pool: &tankracer_game::effect_pool::Pool,
    seed: u32,
    apply: bool,
) -> Vec<u16> {
    let mut out = vec![0u16; 16];
    out.extend(snapshot(car, None, sizes, shared, pool, seed));
    out[0] = (out.len() - 16) as u16;
    out[1] = 16 + 63;
    out[2] = out[1] + (sizes[0][0] * 3) as u16;
    out[3] = out[2] + (sizes[0][1] * 3) as u16;
    out[4] = out[3] + (sizes[1][0] * 3) as u16;
    out[5] = out.len() as u16;
    out[12] = out[4] + (sizes[1][1] * 3) as u16;
    out[13] = out[12] + 12;
    out[14] = u16::from(apply);
    out[6] = sizes[0][0] as u16;
    out[7] = sizes[0][1] as u16;
    out[8] = sizes[1][0] as u16;
    out[9] = sizes[1][1] as u16;
    out.extend([
        u16::from(car.physics.player),
        u16::from(car.visible),
        u16::from(car.physics.external_basis().is_some()),
    ]);
    ints(
        &mut out,
        car.physics.external_basis().map_or([0; 9], |m| m.0),
    );
    for (upper, part) in [Part::Body, Part::Turret].into_iter().enumerate() {
        let mesh = car.model.parts[part.slot()].as_ref();
        out.extend([
            u16::from(mesh.is_some()),
            u16::from(mesh.is_some_and(|m| m.is_private())),
        ]);
        out.extend(car.model.damage_vertices[upper].map(u16::from));
    }
    for pose in [
        car.physics.motion.poses.body,
        car.physics.motion.poses.turret,
    ] {
        ints(&mut out, pose.matrix.0);
        ints(&mut out, pose.position);
    }
    gains(&mut out, shell.player_turn);
    for gain in shell.ai_turn {
        gains(&mut out, gain);
    }
    for gain in [
        shell.player_speed,
        shell.ai_speed,
        shell.player_push,
        shell.ai_push,
    ] {
        gains(&mut out, gain);
    }
    out.push(shell.slow_after);
    for gain in [
        homer.player_turn,
        homer.ai_turn,
        homer.player_speed,
        homer.ai_speed,
    ] {
        gains(&mut out, gain);
    }
    assert_eq!(out.len(), usize::from(out[5]) + 106);
    ints(&mut out, car.physics.motion.poses.base_position);
    for (i, part) in [Part::Body, Part::Turret].into_iter().enumerate() {
        out[10 + i] = out.len() as u16;
        out.extend(
            original.model.parts[part.slot()]
                .as_ref()
                .unwrap()
                .geometry()
                .triangles
                .iter()
                .flat_map(|t| t.indices)
                .map(u16::from),
        );
    }
    out
}
fn kind(index: u32) -> Kind {
    match index {
        0 => Kind::Shell,
        1 => Kind::Concussion,
        2 => Kind::Homing,
        3 => Kind::Ufo,
        4 => Kind::Debris,
        _ => Kind::Warp,
    }
}
pub fn export(out: &Path, world: &SinglePlayer) {
    let original = world.vehicles.entries[0].as_ref().unwrap();
    let sizes = [Part::Body, Part::Turret].map(|p| {
        let g = original.model.parts[p.slot()].as_ref().unwrap().geometry();
        [g.vertices.len(), g.triangles.len()]
    });
    let shell = world.objects.components.projectiles.shell_hit;
    let homer = world.objects.components.projectiles.homer_hit;
    let mut cases = Vec::new();
    for apply in [false, true] {
        for kind_index in 0..6 {
            for player in [false, true] {
                for cup in 0..3 {
                    for branch in 0..19 {
                        if !apply && branch >= 12 {
                            continue;
                        }
                        if apply
                            && kind_index != 5
                            && branch > 0
                            && !((kind_index == 0 || kind_index == 2)
                                && (branch == 14 || branch == 15))
                        {
                            continue;
                        }
                        let mut car = original.clone();
                        car.physics.player = player;
                        car.visible = true;
                        let mut shell = shell;
                        let mut homer = homer;
                        let mut shared = tankracer_game::vehicle_scene::temporary::State::default();
                        let mut pool = tankracer_game::effect_pool::Pool::default();
                        let mut seed = 0x19876543;
                        car.physics.motion.velocity = [123456, -234567, 345678];
                        car.physics.motion.angular = [1000, -2000, 3000];
                        car.physics.pose_input.external =
                            ExternalBasis::Custom(Matrix::from_angles([700, 13000, 1900]));
                        car.physics.contact.response.timer = 0;
                        car.hit_counts = [[0; 6]; 2];
                        car.smoke.damage = [0; 2];
                        car.smoke.sockets = [0; 2];
                        car.damage = Default::default();
                        car.inventory.weapon = 0;
                        car.inventory.ammo = 3;
                        match branch {
                            0 => {}
                            1 => car.physics.pose_input.external = ExternalBasis::None,
                            2 => car.visible = false,
                            3 => car.hit_counts = [[6; 6]; 2],
                            4 => car.hit_counts = [[10; 6]; 2],
                            5 => {
                                car.model.parts[Part::Body.slot()] = None;
                                car.model.parts[Part::Turret.slot()] = None;
                                car.hit_counts = [[1; 6]; 2];
                            }
                            6 => {
                                rcc_car_probe::tank_hit_unown(&mut car, Part::Body.slot());
                                rcc_car_probe::tank_hit_unown(&mut car, Part::Turret.slot());
                                car.hit_counts = [[1; 6]; 2];
                            }
                            7 => {
                                car.model.damage_vertices = [[255; 4]; 2];
                                car.hit_counts = [[1; 6]; 2];
                            }
                            8 => {
                                car.hit_counts = [[1; 6]; 2];
                                car.damage.body = [1; 4];
                                car.damage.turret = [1; 4];
                            }
                            9 => {
                                car.inventory.weapon = 1;
                                car.physics.contact.response.timer = 950;
                            }
                            10 => {
                                car.physics.motion.velocity = [i32::MIN, i32::MAX, -7654321];
                            }
                            12 => {
                                car.prefix.temporary_timer = 1;
                                car.prefix.temporary_slot = 4;
                            }
                            13 => car.prefix.sidegrip.shift = u16::MAX,
                            14 | 15 => {
                                let gain = Gain {
                                    multiplier: if branch == 14 { u16::MAX } else { 32768 },
                                    shift: if branch == 14 { u16::MAX } else { 0 },
                                };
                                shell.player_turn = gain;
                                shell.ai_turn = [gain; 3];
                                shell.player_speed = gain;
                                shell.ai_speed = gain;
                                shell.player_push = gain;
                                shell.ai_push = gain;
                                homer.player_turn = gain;
                                homer.ai_turn = gain;
                                homer.player_speed = gain;
                                homer.ai_speed = gain;
                            }
                            16 => car.prefix.sidegrip.shift = 32767,
                            17 => pool.line_count = 100,
                            18 => {
                                car.prefix.temporary_timer = -1;
                                car.prefix.temporary_slot = -1;
                            }
                            _ => {}
                        }
                        let initial = input(
                            &car, shell, homer, sizes, original, &shared, &pool, seed, apply,
                        );
                        let upper = (branch + kind_index as usize) % 2;
                        let face = (branch + cup) % 7;
                        let mut local_normal = [0; 3];
                        if face < 6 {
                            local_normal[face / 2] = if face % 2 == 0 { 4096 } else { -4096 };
                        }
                        let pose = if upper == 0 {
                            car.physics.motion.poses.body
                        } else {
                            car.physics.motion.poses.turret
                        };
                        let local_point = [-500, if upper == 0 { 1024 } else { 1025 }, 700];
                        let mut world_point = pose.matrix.local_to_world(local_point);
                        for i in 0..3 {
                            world_point[i] = world_point[i].wrapping_add(pose.position[i]);
                        }
                        let hit = Hit {
                            vehicle: TankId(0),
                            world: SegmentHit {
                                point: world_point,
                                normal: pose.matrix.local_to_world(local_normal),
                            },
                            local: SegmentHit {
                                point: local_point,
                                normal: local_normal,
                            },
                        };
                        let velocity = if branch == 10 {
                            [i32::MIN, i32::MAX, 0]
                        } else {
                            [654321, -123456, 765432]
                        };
                        let mut controls = Vec::new();
                        let mut frames = Vec::new();
                        for age in [
                            0,
                            i32::from(shell.slow_after),
                            i32::from(shell.slow_after) + 1,
                        ] {
                            let mut control = vec![kind_index as i32, age, cup as i32];
                            control.extend(hit.world.point);
                            control.extend(hit.world.normal);
                            control.extend(hit.local.point);
                            control.extend(hit.local.normal);
                            control.extend(velocity);
                            let c =
                                tank_hit::coefficients(kind(kind_index), player, cup, shell, homer);
                            let status = if apply {
                                tank_hit::apply(
                                    &mut car,
                                    None,
                                    hit,
                                    velocity,
                                    kind(kind_index),
                                    age,
                                    c,
                                    &mut shared,
                                    &mut pool,
                                    age as u32,
                                    &mut seed,
                                )
                            } else {
                                tank_hit::contact(&mut car, hit, velocity, age, c)
                            }
                            .map_or_else(|v| v, |()| 0);
                            let mut answer = Vec::new();
                            wide(&mut answer, status);
                            answer.extend(snapshot(&car, Some(c), sizes, &shared, &pool, seed));
                            controls.push(control);
                            frames.push(answer);
                        }
                        cases.push(json!({"tag":format!("contact-{kind_index}-{player}-{cup}-{branch}-{apply}"),"input":initial,"controls":controls,"frames":frames}));
                    }
                }
            }
        }
    }
    fs::write(out.join("tank-hit.json"),serde_json::to_vec(&json!({"cases":cases,"scope":"Original tank_hit::coefficients/contact/apply on authored body/turret meshes, affected vehicle fields, dents/normals/revisions and Warp sidegrip/temporary effects with shared pool/RNG; AI notifications remain separate"})).unwrap()).unwrap();
}
