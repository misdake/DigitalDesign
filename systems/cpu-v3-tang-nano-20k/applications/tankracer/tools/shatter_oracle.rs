//! Direct original debris calls, complete physical fields and shared RNG.
#[allow(dead_code)]
mod reference_world;
use serde_json::{json, Value};
use std::{fs, path::Path};
use tankracer_game::{
    math::Matrix,
    object_query::EntityId,
    obstacle_startup::{Animation, Debris, Piece, Sprite},
    random, shatter,
};

fn animation() -> Animation {
    Animation {
        material: None,
        position: [0; 3],
        blend: 0,
        sheet_size: 64,
        cell_size: 16,
        initial_size: 2048,
        cycles: 2,
        growth: 7,
        timer: 0,
        rotation: 0,
        cycle: 0,
        size: [2048; 2],
        uv: [0, 0, 15, 15],
        color: [255; 3],
    }
}
fn sprite() -> Sprite {
    Sprite {
        material: None,
        position: [0; 3],
        flags: 0,
        timer: 0,
        size: [1024; 2],
        uv: [0, 0, 31, 31],
        color: [255; 3],
    }
}
fn piece() -> Piece {
    Piece {
        source: None,
        position: [0; 3],
        velocity: [0; 3],
        matrix: Matrix::IDENTITY,
        translation: [0; 3],
        angular: [0; 3],
        previous_tilt: -1,
        velocity_gain: 128,
        spin_gain: 64,
        flags: 1,
        sprite_angle: 0,
    }
}
fn animation_values(a: &Animation) -> Vec<i32> {
    let mut out = a.position.to_vec();
    out.extend([
        a.initial_size,
        i32::from(a.cycles),
        i32::from(a.growth),
        i32::from(a.timer),
        i32::from(a.rotation),
        i32::from(a.cycle),
    ]);
    out.extend(a.size);
    out.extend(a.uv.map(i32::from));
    out.extend([i32::from(a.sheet_size), i32::from(a.cell_size)]);
    out
}
fn state_values(s: &Debris) -> Vec<i32> {
    let mut out = vec![
        s.flags as i32,
        i32::from(s.retired),
        s.ground_height,
        s.gravity,
        s.friction,
        s.direction,
    ];
    out.extend(s.offset);
    out.extend(
        s.animation
            .as_ref()
            .map_or_else(|| vec![0; 17], animation_values),
    );
    for p in &s.pieces {
        out.extend(p.position);
        out.extend(p.velocity);
        out.extend(p.matrix.0);
        out.extend(p.translation);
        out.extend(p.angular);
        out.extend([
            p.previous_tilt,
            i32::from(p.velocity_gain),
            i32::from(p.spin_gain),
            p.flags as i32,
            p.sprite_angle,
            p.source.as_ref().map_or(0, |m| i32::from(m.radius) * 8),
        ]);
    }
    out
}
fn capture(s: &Debris, status: i32, prefix: i32, seed: u32) -> Vec<i32> {
    let mut out = vec![status, prefix, seed as i32];
    out.extend(state_values(s));
    out
}
fn case(tag: String, mut state: Debris, seed: u32, controls: Vec<[i32; 14]>) -> Value {
    let mut input = vec![
        state.pieces.len() as i32,
        i32::from(state.animation.is_some()),
        i32::from(state.sprite.is_some()),
        state.sprite.as_ref().map_or(0, |s| s.size[0]),
        seed as i32,
        controls.len() as i32,
    ];
    input.extend(state_values(&state));
    input.extend(controls.iter().flatten());
    let mut frames = Vec::new();
    let mut seed = seed;
    for c in controls {
        let [action, dt, entity, group, limit, activation_limit, xor, draws, x, y, z, vx, vy, vz] =
            c;
        let mut prefix = 0;
        let status = match action {
            0 => match shatter::begin(&mut state, [vx, vy, vz], limit as u32) {
                Err(code) => {
                    prefix = code;
                    code
                }
                Ok(None) => 0,
                Ok(Some(a)) => {
                    prefix = 1;
                    state.flags ^= xor as u32;
                    for _ in 0..draws {
                        random::next(&mut seed);
                    }
                    shatter::activate(&mut state, [x, y, z], a, &mut seed, activation_limit as u32)
                        .map_or_else(|code| code, |()| 0)
                }
            },
            1 => shatter::tick(
                &mut state,
                EntityId(entity as u32),
                dt,
                group as u32,
                &mut seed,
                limit as u32,
            )
            .map_or_else(|code| code, i32::from),
            2 => {
                shatter::animate(state.animation.as_mut().unwrap(), dt);
                0
            }
            3 => i32::from(
                shatter::BlastTarget {
                    entity: EntityId(entity as u32),
                    position: [vx, vy, vz],
                    protection_timer: dt,
                }
                .affected([x, y, z]),
            ),
            _ => panic!("unknown action"),
        };
        frames.push(capture(&state, status, prefix, seed));
    }
    json!({"tag":tag,"input":input,"frames":frames})
}
fn control(action: i32, dt: i32, limit: u32) -> [i32; 14] {
    [
        action,
        dt,
        5,
        1,
        limit as i32,
        limit as i32,
        0,
        0,
        203960,
        -10782,
        440258,
        250000,
        -150000,
        500000,
    ]
}
fn next(seed: &mut u32) -> i32 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 17;
    *seed ^= *seed << 5;
    *seed as i32
}
fn main() {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let root = Path::new(&args[0]);
    let output = Path::new(&args[1]);
    fs::create_dir_all(output).unwrap();
    let mut session = reference_world::complete(root);
    let world = session.world_mut_at_load().unwrap();
    let mut cases = Vec::new();
    let mut authored = Vec::new();
    let mut templates = Vec::new();
    for (id, object) in world.objects.components.objects.iter().enumerate() {
        let Some(s) = object.as_ref().and_then(|o| o.data.debris.as_ref()) else {
            continue;
        };
        authored.push(json!({"object":id,"pieces":s.pieces.len(),"flags":s.flags,
            "animation":s.animation.is_some(),"sprite":s.sprite.is_some()}));
        templates.push(s.clone());
        for variant in 0..12 {
            let mut state = s.clone();
            state.flags &= !1;
            if variant == 1 {
                state.flags |= 2;
            }
            if variant == 2 {
                state.flags |= 64;
            }
            if variant == 3 {
                state.flags |= 256;
            }
            if variant == 4 {
                state.flags |= 2050;
            }
            if variant == 5 {
                state.flags |= 128;
                state.sprite = Some(sprite());
            }
            if variant == 6 {
                state.animation = Some(animation());
            }
            let limit = if variant == 7 {
                0
            } else if variant == 8 {
                1
            } else {
                100000
            };
            let mut first = control(0, 33, limit);
            if variant == 9 {
                first[6] = 2;
                first[7] = 3;
            }
            if variant == 10 {
                first[5] = 0;
            }
            if variant == 11 {
                first[5] = 1;
            }
            cases.push(case(
                format!("authored-{id}-{variant}"),
                state,
                0x1234abcd,
                vec![
                    first,
                    control(0, 33, 100000),
                    control(1, 33, 100000),
                    control(1, 128, 100000),
                    control(1, 33, 100000),
                ],
            ));
        }
    }
    assert_eq!(templates.len(), 6);
    let mut random_seed = 0x7d19b023;
    for index in 0..640 {
        let mut state = templates[index % templates.len()].clone();
        let count = index % 6;
        state.pieces = (0..count)
            .map(|_| {
                let mut p = piece();
                for i in 0..3 {
                    p.position[i] = next(&mut random_seed) % 32768;
                    p.velocity[i] = next(&mut random_seed) % 50000;
                    p.angular[i] = next(&mut random_seed) % 65536;
                }
                p.matrix =
                    Matrix::from_angles(std::array::from_fn(|_| next(&mut random_seed) as u32));
                p.previous_tilt = if index % 3 == 0 {
                    -1
                } else {
                    next(&mut random_seed) % 32768
                };
                p.velocity_gain = next(&mut random_seed) as i16;
                p.spin_gain = next(&mut random_seed) as i16;
                p.flags = next(&mut random_seed) as u32 & 15;
                p
            })
            .collect();
        state.flags = next(&mut random_seed) as u32 & 4095;
        state.retired = if index % 29 == 0 { 65535 } else { 0 };
        state.gravity = next(&mut random_seed) % 1000;
        state.friction = next(&mut random_seed) % 1000;
        state.ground_height = next(&mut random_seed) % 2048;
        state.direction = next(&mut random_seed);
        state.offset = std::array::from_fn(|_| next(&mut random_seed) % 32768);
        state.animation = if index % 4 == 0 {
            None
        } else {
            Some(animation())
        };
        state.sprite = if index % 3 == 0 { None } else { Some(sprite()) };
        let mut controls = Vec::new();
        for step in 0..4 {
            let limit = [0, 1, 3, u32::MAX, 0x80000000, 100000][(index + step) % 6];
            let mut c = control(
                if step == 0 && index % 2 == 0 { 0 } else { 1 },
                [-33, 0, 1, 33, 128, 1000][(index + step) % 6],
                limit,
            );
            c[2] = next(&mut random_seed);
            c[3] = index as i32 % 5;
            c[6] = (next(&mut random_seed) as u32 & 4095) as i32;
            c[7] = index as i32 % 4;
            controls.push(c);
        }
        cases.push(case(format!("mixed-{index}"), state, random_seed, controls));
    }
    for edge in 0..9 {
        let mut state = templates[0].clone();
        state.flags = 0;
        state.retired = 65535;
        state.gravity = 0;
        state.friction = 0;
        state.ground_height = 0;
        state.animation = None;
        state.sprite = None;
        state.pieces = vec![piece()];
        let p = &mut state.pieces[0];
        p.flags = 0;
        p.position = [0, -1, 0];
        match edge {
            0 => {}
            1 => {
                p.flags = 4;
                p.matrix.0[7] = i32::MIN;
            }
            2 => {
                p.flags = 4;
                p.matrix.0[6] = i32::MIN;
                p.matrix.0[8] = i32::MIN;
                p.previous_tilt = 1;
            }
            3 => {
                p.flags = 1;
                p.velocity[1] = i32::MIN;
            }
            4 => {
                p.flags = 0x80000009;
                p.velocity = [i32::MIN, i32::MAX, i32::MIN];
            }
            5 => {
                state.flags = 128;
                p.flags = 1;
                p.angular[1] = i32::MAX;
            }
            6 => {
                state.flags = 8;
            }
            7 => {
                state.flags = 0x80000802;
            }
            _ => {
                p.flags = 4;
                p.previous_tilt = i32::MAX;
            }
        }
        let controls = if edge == 7 {
            vec![control(0, 0, u32::MAX), control(1, 0, u32::MAX)]
        } else {
            vec![control(1, 0, 100000), control(1, 33, 100000)]
        };
        cases.push(case(
            format!("integer-prefix-{edge}"),
            state,
            0x7abc0321,
            controls,
        ));
    }
    for dt in [-33, 0, 1, 61, 128, i32::MIN, i32::MAX] {
        let mut state = templates[0].clone();
        state.pieces.clear();
        state.flags = 8;
        let mut a = animation();
        a.growth = -32768;
        a.timer = 61;
        state.animation = Some(a);
        cases.push(case(
            format!("signed-growth-{dt}"),
            state,
            0,
            vec![control(1, dt, 100000)],
        ));
    }
    for sheet in [0, 1, 64, 255, 256, 32768, 65535] {
        for cell in [0, 1, 16, 255, 256, 32768, 65535] {
            for timer in [-32768, -1, 0, 60, 61, 32767] {
                let mut state = templates[0].clone();
                state.pieces.clear();
                let mut a = animation();
                a.sheet_size = sheet;
                a.cell_size = cell;
                a.timer = timer;
                a.cycle = 65535;
                a.uv = [255, 255, 255, 255];
                a.growth = -32768;
                state.animation = Some(a);
                cases.push(case(
                    format!("animation-{sheet}-{cell}-{timer}"),
                    state,
                    0,
                    [0, 1, 33, 128, -1, i32::MAX]
                        .into_iter()
                        .map(|dt| control(2, dt, 100000))
                        .collect(),
                ));
            }
        }
    }
    for x in [
        i32::MIN,
        i32::MIN + 1,
        -20480,
        -20479,
        0,
        20479,
        20480,
        i32::MAX,
    ] {
        for timer in [i32::MIN, -1, 0, 1, i32::MAX] {
            let mut state = templates[0].clone();
            state.pieces.clear();
            state.animation = None;
            let mut c = control(3, timer, 100000);
            c[8] = 0;
            c[9] = 0;
            c[10] = 0;
            c[11] = x;
            c[12] = 0;
            c[13] = x;
            cases.push(case(format!("blast-{x}-{timer}"), state, 0, vec![c]));
        }
    }
    let checked: usize = cases
        .iter()
        .flat_map(|c| c["frames"].as_array().unwrap())
        .map(|f| f.as_array().unwrap().len())
        .sum();
    fs::write(output.join("shatter.json"),serde_json::to_vec(&json!({"cases":cases,
        "authored":authored,"checked_i32_values":checked,
        "scope":"Original shatter begin/activate/tick/animate and blast gates; complete mutable physical fields, direct authored resources, narrow counters, RNG and committed failure prefixes"})).unwrap()).unwrap();
    fs::copy(
        root.join("crates/game/data/sine.bin"),
        output.join("sine.i16.bin"),
    )
    .unwrap();
    fs::copy(
        root.join("crates/game/data/angle.bin"),
        output.join("angle.i16.bin"),
    )
    .unwrap();
    println!("{} debris cases, {checked} integers", cases.len());
}
