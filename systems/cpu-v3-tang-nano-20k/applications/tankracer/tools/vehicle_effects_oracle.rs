//! Original vehicle smoke, shared debris ownership and shield updates.
use super::ints;
use serde_json::json;
use std::{fs, path::Path};
use tankracer_game::{
    effect_pool::{LineHandle, Pool},
    impact_particles as impact,
    math::Matrix,
    object_query::EntityId,
    pose::{ExternalBasis, Pose},
    vehicle,
    vehicle_scene::{debris, shield, smoke},
};
#[path = "impact_snapshot.rs"]
mod impact_snapshot;
struct State {
    smoke: smoke::State,
    debris: debris::State,
    shield: shield::State,
    owners: [debris::Vehicle; 3],
    ids: [EntityId; 3],
    seed: u32,
    accumulator: i32,
    sound: Option<u32>,
    impacts: impact::State,
    vehicle: vehicle::State,
    pool: Pool,
}
fn snapshot(s: &State) -> Vec<u16> {
    let mut out = Vec::new();
    let m = &s.smoke;
    ints(&mut out, [m.timer, m.side as i32, m.force, m.count as i32]);
    ints(&mut out, m.damage.map(i32::from));
    ints(&mut out, m.sockets.map(i32::from));
    ints(&mut out, m.damage_timers);
    for p in m.particles {
        ints(&mut out, p.position);
        ints(&mut out, p.velocity);
        ints(
            &mut out,
            [
                i32::from(p.life),
                i32::from(p.size),
                i32::from(p.rotation),
                i32::from(p.direction),
                i32::from(p.expanding),
            ],
        );
    }
    for b in &s.debris.banks {
        for p in b.particles {
            ints(&mut out, p.velocity);
            ints(&mut out, p.position);
            ints(&mut out, p.color.map(i32::from));
            ints(&mut out, [i32::from(p.active), p.size as i32]);
        }
        ints(
            &mut out,
            [
                b.floor,
                i32::from(b.owner.is_some()),
                b.owner.map_or(0, |e| e.0 as i32),
            ],
        );
    }
    for (id, v) in s.ids.iter().zip(s.owners) {
        ints(
            &mut out,
            [
                id.0 as i32,
                v.active as i32,
                v.timer,
                i32::from(v.previous_plane),
            ],
        );
    }
    ints(
        &mut out,
        [
            s.accumulator,
            i32::from(s.sound.is_some()),
            s.sound.unwrap_or(0) as i32,
        ],
    );
    for b in &s.shield.banks {
        for o in b.orbits {
            ints(
                &mut out,
                [
                    o.pitch,
                    o.pitch_target,
                    o.yaw,
                    o.yaw_rate,
                    o.yaw_rate_target,
                    o.size,
                    i32::from(o.growing),
                    i32::from(o.spin),
                ],
            );
            ints(&mut out, o.position);
        }
        ints(&mut out, [b.tail as i32]);
    }
    ints(&mut out, [s.seed as i32, s.vehicle.impact_cooldown as i32]);
    ints(
        &mut out,
        impact_snapshot::snapshot([0; 4], &s.impacts, &s.vehicle, &s.pool, s.seed),
    );
    assert_eq!(out.len(), 4134);
    out
}
pub(super) fn world_snapshot(world: &tankracer_game::vehicle_runtime::SinglePlayer) -> Vec<u16> {
    let car = world.vehicles.entries[world.player.0].as_ref().unwrap();
    snapshot(&State {
        smoke: car.smoke.clone(),
        debris: world.shared.debris.clone(),
        shield: world.shared.shield.clone(),
        owners: std::array::from_fn(|i| {
            world
                .vehicles
                .entries
                .get(i)
                .and_then(Option::as_ref)
                .map_or_else(Default::default, |c| c.debris)
        }),
        ids: std::array::from_fn(|i| {
            world
                .vehicles
                .entries
                .get(i)
                .and_then(Option::as_ref)
                .map_or(EntityId(0), |c| c.physics.entity)
        }),
        seed: world.shared.random,
        accumulator: car.physics.collision.impact_accumulator,
        sound: world.shared.debris_sound,
        impacts: world.shared.impacts.clone(),
        vehicle: car.physics.clone(),
        pool: Pool {
            lines: world.shared.pool.lines,
            line_count: world.shared.pool.line_count,
            quads: world.shared.pool.quads,
            quad_count: world.shared.pool.quad_count,
            overflow: world.shared.pool.overflow,
            exhausted_colors: world.shared.pool.exhausted_colors,
            exhausted_quad: world.shared.pool.exhausted_quad,
        },
    })
}
fn frame(op: i32, dt: i32, basis: Matrix) -> Vec<i32> {
    let mut f = vec![op, dt, 1, 0, 0, 0, 0, 1, -100, 0, 1, 1, 1, 1, 11, 17];
    f.extend([200000, -130000, 350000]);
    f.extend([15000, 1000, 25000]);
    f.extend([1, 1]);
    f.extend(
        [
            [10000, 4000, 20000],
            [12000, 4200, 18000],
            [20000, 4500, 20000],
            [22000, 4300, 18000],
        ]
        .into_iter()
        .flatten(),
    );
    f.extend([0, 1, 2, 3]);
    for position in [[14000, 2000, 24000], [15000, 1000, 25000]] {
        f.extend(basis.0);
        f.extend(position);
    }
    f.extend([1000, 100, 1000]);
    f.extend([0, 0, 4096]);
    f.extend(basis.0);
    assert_eq!(f.len(), 79);
    f
}
fn call(s: &mut State, f: &[i32], tuning: &smoke::Tuning, kinds: &[i8; 16]) -> i32 {
    let op = f[0];
    let dt = f[1];
    let player = f[2] != 0;
    let position = f[19..22].try_into().unwrap();
    let velocity = f[16..19].try_into().unwrap();
    let basis = Matrix(f[70..79].try_into().unwrap());
    let contacts: [[i32; 3]; 4] =
        std::array::from_fn(|i| f[24 + i * 3..27 + i * 3].try_into().unwrap());
    s.vehicle.entity = EntityId(f[14] as u32);
    s.vehicle.powertrain.local_velocity[2] = f[13];
    s.vehicle.pose_input.external = if f[12] != 0 {
        ExternalBasis::Custom(basis)
    } else {
        ExternalBasis::None
    };
    for (c, p) in s.vehicle.motion.contacts.iter_mut().zip(contacts) {
        c.world = p;
    }
    if op == 0 || op == 5 {
        let input = smoke::Input {
            dt,
            player,
            special: f[3] != 0,
            velocity,
            lateral_velocity: f[4],
            track_slip: f[5],
            penetrating_force: f[6],
            materials: f[22..24].try_into().unwrap(),
            contacts,
            corners: std::array::from_fn(|i| f[36 + i] as u32),
            poses: std::array::from_fn(|i| Pose {
                matrix: Matrix(f[40 + i * 12..49 + i * 12].try_into().unwrap()),
                position: f[49 + i * 12..52 + i * 12].try_into().unwrap(),
            }),
        };
        let mut context = impact::Context {
            frame: f[15] as u32,
            random: &mut s.seed,
            pool: &mut s.pool,
            tuning: impact::Tuning::default(),
        };
        if let Err(e) = s
            .smoke
            .step(&input, tuning, &mut s.impacts, &mut s.vehicle, &mut context)
        {
            return e;
        }
    }
    if op == 1 || op == 5 {
        let input = debris::Input {
            dt,
            player,
            position,
            velocity,
            has_plane: f[7] as u8,
            plane_distance: f[8],
            materials: [f[22] as usize, f[23] as usize],
            contacts: [contacts[0], contacts[2]],
            camera_position: f[64..67].try_into().unwrap(),
            camera_forward: f[67..70].try_into().unwrap(),
        };
        let mut owners: Vec<_> = s
            .owners
            .iter_mut()
            .zip(s.ids)
            .map(|(state, entity)| debris::Owner { entity, state })
            .collect();
        let mut context = debris::Context {
            kinds,
            random: &mut s.seed,
            impact_accumulator: &mut s.accumulator,
            sound_draw: &mut s.sound,
        };
        if let Err(e) = s
            .debris
            .step(&mut owners, f[9] as usize, &input, &mut context)
        {
            return e;
        }
    }
    if op == 2 || op == 5 {
        return s
            .shield
            .tick(
                shield::Input {
                    enabled: f[10] != 0,
                    bank: f[11] as usize,
                    dt,
                    position,
                    basis: (f[12] != 0).then_some(basis),
                },
                &mut s.seed,
            )
            .map_or_else(|e| e, |()| 0);
    }
    if op == 3 {
        s.shield.initialize(&mut s.seed);
    }
    if op == 4 {
        s.debris.clear(player, &mut s.owners[f[9] as usize]);
    }
    0
}
pub fn export(out: &Path) {
    let mut cases = Vec::new();
    let tuning = smoke::Tuning::default();
    for branch in 0..104 {
        let op = if branch < 40 {
            0
        } else if branch < 76 {
            1
        } else if branch < 100 {
            2
        } else {
            5
        };
        let basis = Matrix::from_angles([1234, 5678, 3456]);
        let mut f = frame(op, 40, basis);
        let mut s = State {
            smoke: smoke::State::default(),
            debris: debris::State::default(),
            shield: shield::State::default(),
            owners: [debris::Vehicle::default(); 3],
            ids: [EntityId(11), EntityId(12), EntityId(13)],
            seed: 0x91234567,
            accumulator: 19,
            sound: None,
            impacts: impact::State::default(),
            vehicle: vehicle::State::default(),
            pool: Pool::default(),
        };
        s.vehicle.impact_cooldown = 500;
        let mut kinds = [0i8; 16];
        kinds[1] = 1;
        // Nonzero dormant payload catches selective initialization/clear stores.
        for (i, b) in s.shield.banks.iter_mut().enumerate() {
            b.tail = (i * 7 + 3) as u32;
            for (j, o) in b.orbits.iter_mut().enumerate() {
                o.position = [i as i32 + 123, j as i32 - 234, 345];
                o.size = 200 + j as i32 * 100;
                o.spin = 65000;
            }
        }
        if branch < 40 {
            s.smoke.timer = 69;
            match branch {
                0 => {}
                1 => {
                    f[5] = 50000;
                }
                2 => {
                    f[6] = 5000000;
                }
                3 => {
                    f[4] = 80000;
                }
                4 => {
                    f[4] = -80000;
                }
                5 => {
                    f[4] = 130001;
                }
                6 => {
                    f[4] = -130001;
                    f[13] = -1;
                }
                7 => {
                    f[3] = 1;
                    f[4] = 200000;
                }
                8 => {
                    f[2] = 0;
                    f[3] = 1;
                    f[4] = 200000;
                }
                9 => {
                    s.smoke.damage = [9, 8];
                    s.smoke.damage_timers = [149, 299];
                }
                10 => {
                    s.smoke.damage = [1, 9];
                    s.smoke.sockets = [5, 5];
                    s.smoke.damage_timers = [260, 110];
                }
                11 => {
                    s.smoke.damage = [1, 9];
                    s.smoke.sockets = [6, 0];
                    s.smoke.damage_timers = [300, 300];
                }
                12 => {
                    s.smoke.damage = [1, 9];
                    s.smoke.sockets = [0, 255];
                    s.smoke.damage_timers = [300, 300];
                }
                13 => {
                    s.smoke.damage = [9, 9];
                    s.smoke.damage_timers = [0, 0];
                    f[5] = 50000;
                }
                14 => {
                    s.smoke.side = 3;
                    f[5] = 50000;
                }
                15 => {
                    f[22] = 2;
                    f[23] = 3;
                    f[5] = 50000;
                }
                16 => {
                    s.smoke.particles.fill(smoke::Particle {
                        position: [i32::MAX, -10, i32::MIN],
                        velocity: [-1000, i32::MAX, 2000],
                        life: 1,
                        size: 65530,
                        rotation: 65000,
                        direction: 1,
                        expanding: 1,
                    });
                    f[1] = 100;
                    f[5] = 90000;
                }
                17 => {
                    s.smoke.force = -5000000;
                    f[6] = i32::MIN;
                    f[1] = 100;
                }
                18 => {
                    f[4] = 130000;
                }
                19 => {
                    f[4] = -130000;
                }
                20 => {
                    f[4] = 200000;
                    f[16] = 0;
                    f[17] = 0;
                    f[18] = 250000;
                }
                21 => {
                    f[4] = 200000;
                    s.vehicle.impact_cooldown = 499;
                }
                22 => {
                    f[4] = 200000;
                    f[39] = 4;
                }
                23 => {
                    f[4] = -200000;
                    f[13] = -1;
                    f[36] = 4;
                }
                24 => {
                    f[4] = 200000;
                    s.impacts.next = 4;
                }
                25 => {
                    f[4] = 200000;
                    s.impacts.batches[0].active = true;
                    s.impacts.batches[0].particles[3].line = LineHandle(101);
                }
                26 => {
                    f[4] = 200000;
                    for _ in 0..100 {
                        s.pool.allocate([[0; 3]; 2], [[1, 2, 3, 4]; 2]);
                    }
                }
                27 => {
                    f[4] = 200000;
                    s.seed = u32::MAX;
                    f[12] = 0;
                }
                28 => {
                    f[5] = i32::MIN;
                }
                29 => {
                    f[4] = i32::MIN;
                }
                30 => {
                    f[1] = 32768;
                    f[5] = 500000;
                }
                31 => {
                    s.smoke.timer = i32::MAX;
                    s.smoke.damage = [9, 9];
                    s.smoke.damage_timers = [i32::MAX, i32::MAX];
                }
                32 => {
                    s.smoke.damage = [9, 9];
                    s.smoke.damage_timers = [300, 300];
                    s.smoke.particles[0].direction = 37;
                }
                33 => {
                    s.smoke.particles[0] = smoke::Particle {
                        life: 2000,
                        velocity: [-1001, -23, 1001],
                        size: 65000,
                        rotation: 5,
                        direction: 0,
                        expanding: 0,
                        ..Default::default()
                    };
                }
                34 => {
                    s.smoke.side = u32::MAX;
                    f[5] = 40000;
                }
                35 => {
                    s.smoke.timer = 0;
                    f[1] = 0;
                    f[5] = 50000;
                }
                36 => {
                    s.smoke.force = 4000000;
                    f[1] = 0;
                }
                37 => {
                    s.smoke.force = 4000001;
                    f[1] = 0;
                }
                38 => {
                    f[4] = 200000;
                    f[30] = i32::MIN;
                    f[33] = 0;
                }
                39 => {
                    f[2] = 0;
                    f[4] = -200000;
                }
                _ => unreachable!(),
            }
        } else if branch < 76 {
            match branch - 40 {
                0 => {}
                1 => {
                    f[2] = 0;
                }
                2 => {
                    f[9] = 1;
                }
                3 => {
                    f[9] = 3;
                }
                4 => {
                    f[22] = 16;
                }
                5 => {
                    f[23] = -1;
                }
                6 => {
                    s.debris.banks[1].owner = Some(EntityId(12));
                    s.owners[1].active = 1;
                }
                7 => {
                    s.debris.banks[1].owner = Some(EntityId(999));
                }
                8 => {
                    f[2] = 0;
                    f[69] = -4096;
                }
                9 => {
                    s.owners[0].timer = 41;
                }
                10 => {
                    s.owners[0].previous_plane = 1;
                    f[17] = 102400;
                }
                11 => {
                    f[16] = 0;
                    f[17] = 0;
                    f[18] = 102400;
                }
                12 => {
                    f[16] = 0;
                    f[17] = 0;
                    f[18] = 102401;
                }
                13 => {
                    f[17] = i32::MIN;
                }
                14 => {
                    f[16] = i32::MIN;
                }
                15 => {
                    f[17] = 129;
                }
                16 => {
                    f[17] = -129;
                }
                17 => {
                    s.owners[0].active = 1;
                    s.debris.banks[1].owner = Some(EntityId(11));
                    s.debris.banks[1].floor = 100;
                    s.debris.banks[1].particles[0] = debris::Particle {
                        position: [1, 200, 3],
                        velocity: [-1000, 0, 1000],
                        active: 1,
                        ..Default::default()
                    };
                    f[7] = 0;
                }
                18 => {
                    s.owners[0].active = 1;
                    s.owners[0].timer = 99;
                    f[7] = 0;
                }
                19 => {
                    f[22] = 0;
                    f[23] = 1;
                }
                20 => {
                    f[1] = i32::MIN;
                    s.owners[0].timer = i32::MAX;
                }
                21 => {
                    f[0] = 4;
                    s.owners[0].active = 1;
                    s.owners[0].timer = 100;
                    s.owners[0].previous_plane = 1;
                    s.debris.banks[1].owner = Some(EntityId(11));
                    s.debris.banks[1].particles[2].active = 1;
                    s.debris.banks[1].particles[2].position = [11, 22, 33];
                }
                22 => {
                    f[0] = 4;
                    s.debris.banks[1].owner = Some(EntityId(12));
                }
                23 => {
                    f[7] = 0;
                }
                24 => {
                    s.owners[0].active = 1;
                    s.owners[0].previous_plane = 1;
                }
                25 => {
                    s.seed = u32::MAX;
                    s.accumulator = i32::MAX;
                }
                26 => {
                    f[2] = 0;
                    f[67] = 0;
                    f[68] = 0;
                    f[69] = 0;
                }
                27 => {
                    f[2] = 0;
                    s.debris.banks[0].owner = Some(EntityId(12));
                }
                28 => {
                    f[9] = 2;
                    s.ids[2] = EntityId(u32::MAX);
                }
                29 => {
                    s.sound = Some(123);
                    f[7] = 0;
                }
                30 => {
                    s.debris.banks[1].owner = Some(EntityId(11));
                }
                31 => {
                    f[7] = 255;
                }
                32 => {
                    s.owners[0].previous_plane = 1;
                    f[17] = -102401;
                }
                33 => {
                    f[19] = i32::MAX;
                    f[64] = i32::MIN;
                    f[69] = i32::MAX;
                }
                34 => {
                    f[1] = 0;
                }
                35 => {
                    f[1] = 100;
                }
                _ => unreachable!(),
            }
        } else if branch < 100 {
            let k = branch - 76;
            if k < 9 {
                f[0] = 3;
                f[11] = k;
            } else {
                match k {
                    9 => {
                        f[11] = 9;
                    }
                    10 => {
                        f[11] = -1;
                    }
                    11 => {
                        f[10] = 0;
                        f[11] = -1;
                    }
                    12 => {
                        f[12] = 0;
                    }
                    13 => {
                        f[1] = 0;
                    }
                    14 => {
                        f[1] = 100;
                    }
                    15 => {
                        f[1] = -40;
                    }
                    16 => {
                        f[1] = i32::MAX;
                    }
                    17 => {
                        f[11] = 8;
                    }
                    18 => {
                        s.seed = u32::MAX;
                    }
                    19 => {
                        s.shield.banks[1].orbits[0].growing = 255;
                    }
                    20 => {
                        s.shield.banks[1].orbits[0].size = i32::MIN;
                    }
                    21 => {
                        s.shield.banks[1].orbits[0].yaw_rate = i32::MAX;
                    }
                    22 => {
                        s.shield.banks[1].orbits[0].pitch = 65535;
                    }
                    23 => {
                        f[19] = i32::MAX;
                    }
                    _ => unreachable!(),
                }
            }
        } else {
            s.smoke.timer = 100;
            s.smoke.damage = [9, 9];
            s.smoke.damage_timers = [300, 300];
            f[4] = 200000;
            f[9] = (branch - 100) % 3;
        }
        let input = snapshot(&s);
        let mut controls = Vec::new();
        let mut frames = Vec::new();
        for step in 0..4 {
            let mut c = f.clone();
            c[15] += step;
            if branch >= 76 && branch < 85 && step > 0 {
                c[0] = 2;
            }
            if branch == 42 || branch >= 100 {
                c[9] = (c[9] + step) % 3;
            }
            if branch == 46 && step == 2 {
                c[0] = 4;
                c[9] = 1;
            }
            if branch == 38 && step > 0 {
                c[30] = 20000;
                c[33] = 22000;
            }
            let status = call(&mut s, &c, &tuning, &kinds);
            let mut answer = Vec::new();
            ints(&mut answer, [status]);
            answer.extend(snapshot(&s));
            frames.push(answer);
            controls.push(c);
        }
        cases.push(json!({"tag":format!("vehicle-effects-{branch}"),"input":input,"controls":controls,"frames":frames,"kinds":kinds,"sockets":tuning.sockets.into_iter().flatten().flatten().collect::<Vec<_>>() }));
    }
    fs::write(out.join("vehicle-effects.json"),serde_json::to_vec(&json!({"cases":cases,"scope":"Original damage/traction smoke with synchronous live impact generation, shared debris bank theft/clear and shield initialization/tick. Full state and shared RNG after each call; enclosing complete Tank effects order remains separate."})).unwrap()).unwrap();
}
