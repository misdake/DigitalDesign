//! Original spray/marks/lights and every shared quad/line alias after each call.
use super::ints;
use serde_json::json;
use std::{fs, path::Path};
use tankracer_game::{
    effect_pool::{Pool, Quad, QuadHandle},
    math::Matrix,
    vehicle_scene::{lights, marks, pickup, spray},
};
#[path = "pickup_snapshot.rs"]
mod pool_snapshot;
pub(super) fn snapshot(
    s: &spray::State,
    m: &marks::State,
    l: &lights::State,
    p: &Pool,
) -> Vec<u16> {
    let mut out = Vec::new();
    ints(&mut out, [s.timer, s.side as i32, s.count as i32]);
    for p in s.particles {
        ints(&mut out, p.position);
        ints(&mut out, p.velocity);
        ints(
            &mut out,
            [
                i32::from(p.life),
                i32::from(p.size),
                i32::from(p.floor),
                i32::from(p.rotation),
                i32::from(p.kind),
                i32::from(p.side),
            ],
        );
    }
    ints(&mut out, [i32::from(m.active[0]), i32::from(m.active[1])]);
    ints(&mut out, m.movement);
    ints(&mut out, [m.count as i32, m.head as i32, m.tail as i32]);
    ints(&mut out, m.endpoints.into_iter().flatten().flatten());
    ints(&mut out, m.handles.map(|h| h.0 as i32));
    ints(
        &mut out,
        [
            i32::from(l.brake_mode),
            i32::from(l.active[0]),
            i32::from(l.active[1]),
            l.handles[0].0 as i32,
            l.handles[1].0 as i32,
        ],
    );
    let values = pool_snapshot::values(&pickup::State::default(), p);
    ints(&mut out, values[816..].iter().copied());
    assert_eq!(out.len(), 7160);
    out
}
fn frame(v: &spray::Input, m: &marks::Input, l: &lights::Input) -> Vec<i32> {
    let mut out = vec![
        v.dt,
        v.track_speed,
        m.track_slip,
        m.lateral_velocity,
        m.mode,
        i32::from(v.visible == 1),
        i32::from(m.deformable),
        v.visible,
        i32::from(l.disabled),
        i32::from(l.settings.active),
        i32::from(l.settings.queued),
        i32::from(l.settings.reverse_braking),
        l.throttle,
        l.brake,
        l.forward_velocity,
        l.settings.material as i32,
        i32::from(l.basis.is_some()),
    ];
    out[5] = i32::from(m.player);
    out.extend(v.velocity);
    out.extend(l.position);
    out.extend(v.basis.0);
    out.extend(v.materials.map(|v| v as i32));
    out.extend(m.travel);
    out.extend(v.kinds);
    out.extend(v.contacts.into_iter().flatten());
    out.extend(m.tracks.into_iter().flat_map(|m| m.0));
    out.extend(l.raw_heights);
    out.extend(l.offset);
    assert_eq!(out.len(), 79);
    out
}
pub fn export(out: &Path) {
    let mut cases = Vec::new();
    for branch in 0..73 {
        let mut s = spray::State::default();
        let mut m = marks::State::default();
        let mut l = lights::State::default();
        let mut pool = Pool::default();
        let mut tuning = spray::Tuning::environment(
            spray::Materials {
                filth: 11,
                crap: 12,
                splash: 13,
            },
            false,
            true,
        );
        let basis = Matrix::from_angles([1234, 5678, 3456]);
        let contacts = [
            [10000, 4000, 20000],
            [12000, 4200, 18000],
            [20000, 4500, 20000],
            [22000, 4300, 18000],
        ];
        let mut v = spray::Input {
            dt: 40,
            track_speed: 300000,
            velocity: [1000, -2000, 30000],
            basis,
            materials: [1, 1],
            kinds: [0; 4],
            contacts,
            visible: 1,
        };
        let mut mi = marks::Input {
            player: true,
            deformable: false,
            mode: 0,
            dt: 40,
            lateral_velocity: 80000,
            track_slip: 0,
            velocity: v.velocity,
            materials: [1, 1],
            travel: [50000; 4],
            kinds: v.kinds,
            contacts,
            tracks: [basis, basis],
        };
        let mut li = lights::Input {
            player: true,
            disabled: false,
            settings: lights::Settings {
                active: true,
                visible: 1,
                queued: false,
                reverse_braking: false,
                material: 0xf0000042,
            },
            throttle: -100,
            brake: 0,
            forward_velocity: -3000,
            position: [15000, 5000, 19000],
            basis: Some(basis),
            materials: [1, 1],
            raw_heights: [4000; 4],
            offset: [1200, 0, -1800],
        };
        let op = if branch < 24 {
            0
        } else if branch < 48 {
            1
        } else if branch < 72 {
            2
        } else {
            3
        };
        let variant = branch % 24;
        if op == 0 {
            if matches!(variant, 2 | 3) {
                for p in &mut s.particles {
                    p.life = if variant == 2 { 1 } else { 1000 };
                    p.kind = 1;
                    p.velocity = [-1000, 2000, -3000];
                }
                if variant == 3 {
                    s.particles[2].life = 0;
                    s.particles[7].life = 0;
                }
            }
            if variant == 1 {
                s.timer = 39;
                v.dt = 1;
            }
            if variant == 4 || variant == 5 {
                s.particles[0].life = 1;
                s.particles[0].kind = if variant == 4 { -1 } else { 6 };
            }
            if variant == 6 {
                tuning.styles[5].spin = 65536;
            }
            if variant == 7 {
                tuning.styles[5].spin = 65535;
            }
            if variant == 8 {
                s.side = 2;
            }
            if variant == 9 {
                v.materials[1] = usize::MAX;
            }
            if variant == 10 {
                tuning.kinds[1] = -1;
            }
            if variant == 11 {
                v.visible = 0;
            }
            if variant == 13 {
                v.track_speed = 50000;
            }
            if variant == 14 {
                v.track_speed = 50001;
            }
            if variant == 15 {
                v.kinds = [65536; 4];
            }
            if variant == 16 {
                v.kinds = [2; 4];
            }
            if variant == 17 {
                v.contacts[2][1] = 40000;
            }
            if variant == 18 {
                v.dt = i32::MAX;
                v.track_speed = i32::MIN;
            }
            if variant == 19 {
                v.dt = i32::MIN;
            }
            if variant == 20 {
                tuning.styles[5].gain.multiplier = 65535;
                tuning.styles[5].gain.shift = 255;
            }
            if variant == 21 {
                s.particles[0] = spray::Particle {
                    position: [i32::MAX, i32::MIN, 30],
                    velocity: [i32::MIN, 1000, i32::MAX],
                    life: 32767,
                    size: 65535,
                    floor: -32768,
                    rotation: 65535,
                    kind: 5,
                    side: 255,
                };
            }
            if variant == 22 {
                s.timer = i32::MAX;
            }
            if variant == 23 {
                s.side = u32::MAX;
            }
        }
        if op == 1 {
            if variant == 1 {
                mi.lateral_velocity = 79999;
                m.active = [1; 2];
            }
            if variant == 2 {
                mi.player = false;
            }
            if variant == 3 {
                mi.deformable = true;
            }
            if variant == 4 {
                mi.mode = 2;
            }
            if variant == 5 {
                mi.mode = 3;
            }
            if variant == 6 {
                mi.materials[0] = 0;
                m.active = [1; 2];
            }
            if variant == 7 {
                mi.travel = [40000; 4];
                m.active = [1; 2];
            }
            if variant == 8 {
                mi.travel = [40001; 4];
            }
            if variant == 9 {
                mi.kinds = [65536; 4];
            }
            if matches!(variant, 10 | 11 | 13 | 20 | 21 | 22) {
                m.count = 68;
                m.active = [1; 2];
                for h in &mut m.handles {
                    *h = pool.allocate_quad(Quad::ZERO);
                }
            }
            if variant == 11 {
                m.head = 68;
            }
            if variant == 12 {
                m.tail = 68;
                m.active = [1; 2];
            }
            if variant == 13 {
                m.handles[0] = QuadHandle(999);
            }
            if variant == 14 {
                pool.quad_count = 160;
                m.active = [1; 2];
            }
            if variant == 15 {
                for q in &mut pool.quads {
                    q.points[0][1] = 0;
                }
                pool.quad_count = 0;
                m.active = [1; 2];
            }
            if variant == 16 {
                m.count = 68;
                m.active = [1; 2];
                m.handles[0] = QuadHandle::EXHAUSTED;
            }
            if variant == 17 {
                mi.velocity = [i32::MIN, i32::MAX, -1];
                mi.dt = 100;
            }
            if variant == 18 {
                mi.tracks = [Matrix([i32::MIN; 9]); 2];
            }
            if variant == 19 {
                m.count = u32::MAX;
                m.active = [1; 2];
            }
            if variant == 20 {
                m.head = 67;
                m.tail = 67;
            }
            if variant == 21 {
                m.handles[1] = QuadHandle(999);
            }
            if variant == 22 {
                m.tail = 68;
            }
            if variant == 23 {
                mi.materials = [-1; 2];
                m.active = [255; 2];
            }
            m.endpoints = [[[1000, 2000, 3000], [4000, 5000, 6000]]; 2];
        }
        if op == 2 {
            if matches!(variant, 3 | 6 | 7 | 17 | 18 | 19 | 20) {
                l.step(&li, &mut pool).unwrap();
            }
            if variant == 1 {
                li.throttle = 0;
                li.brake = 100;
                li.forward_velocity = 3000;
            }
            if variant == 2 {
                li.throttle = 100;
            }
            if variant == 3 {
                li.settings.queued = true;
            }
            if variant == 4 {
                li.player = false;
                li.settings.active = false;
            }
            if variant == 5 {
                li.settings.active = false;
                li.disabled = true;
            }
            if variant == 6 {
                li.settings.visible = 0;
            }
            if variant == 7 {
                li.settings.visible = 2;
            }
            if variant == 8 {
                li.player = false;
                li.disabled = true;
            }
            if variant == 9 {
                li.materials[0] = 0;
            }
            if variant == 10 {
                li.position[1] = 6999;
            }
            if variant == 11 {
                li.position[1] = 7000;
            }
            if variant == 12 {
                li.raw_heights = [i32::MAX; 4];
            }
            if variant == 13 {
                li.basis = None;
            }
            if variant == 14 {
                li.basis = Some(Matrix([i32::MIN; 9]));
            }
            if variant == 15 {
                pool.quad_count = 160;
            }
            if variant == 16 {
                for q in &mut pool.quads {
                    q.points[0][1] = 0;
                }
                pool.quad_count = 0;
            }
            if variant == 17 {
                l.handles[0] = QuadHandle(999);
            }
            if variant == 18 {
                l.handles[1] = QuadHandle(999);
                li.settings.visible = 0;
            }
            if variant == 19 {
                l.handles[0] = QuadHandle::EXHAUSTED;
            }
            if variant == 20 {
                li.brake = 100;
                li.settings.reverse_braking = true;
                l.brake_mode = 1;
            }
            if variant == 21 {
                li.throttle = 0;
                li.brake = 100;
                li.forward_velocity = 1000;
            }
            if variant == 22 {
                li.player = false;
            }
            if variant == 23 {
                li.raw_heights = [-32768, 32767, i32::MAX, i32::MIN];
                li.position[1] = -1000;
            }
        }
        let initial = snapshot(&s, &m, &l, &pool);
        let kinds = tuning.kinds;
        let mut styles = Vec::new();
        for style in tuning.styles {
            ints(
                &mut styles,
                [
                    style.spin as i32,
                    i32::from(style.initial_size),
                    i32::from(style.gain.multiplier),
                    style.gain.shift as i32,
                ],
            );
        }
        let mut controls = Vec::new();
        let mut frames = Vec::new();
        for step in 0..4 {
            if step == 3 && (op == 1 || op == 3) {
                mi.travel = [0; 4];
            }
            // A common matrix/velocity block carries the selected operation's input.
            if op == 1 {
                v.dt = mi.dt;
                v.velocity = mi.velocity;
                v.kinds = mi.kinds;
                v.contacts = mi.contacts;
                v.materials = mi.materials.map(|x| x as usize);
            }
            if op == 2 {
                v.basis = li.basis.unwrap_or(Matrix::IDENTITY);
                v.materials = li.materials.map(|x| x as usize);
                v.visible = li.settings.visible;
                mi.player = li.player;
            }
            let mut control = vec![op];
            control.extend(frame(&v, &mi, &li));
            controls.push(control);
            let status = if op == 0 {
                s.step(&v, &tuning)
            } else if op == 1 {
                m.step(&mi, &mut pool)
            } else if op == 2 {
                l.step(&li, &mut pool)
            } else {
                s.step(&v, &tuning)
                    .and_then(|_| m.step(&mi, &mut pool))
                    .and_then(|_| l.step(&li, &mut pool))
            }
            .err()
            .unwrap_or(0);
            let mut answer = Vec::new();
            ints(&mut answer, [status]);
            answer.extend(snapshot(&s, &m, &l, &pool));
            frames.push(answer);
        }
        cases.push(json!({"tag":format!("tracks-{branch}"),"input":initial,"kinds":kinds,"styles":styles,"controls":controls,"frames":frames}));
    }
    fs::write(out.join("track-effects.json"),serde_json::to_vec(&json!({"cases":cases,"scope":"Original track spray, joining mark FIFO and brake/reverse lights, including all shared quad/line/exhaustion state, narrow arithmetic, allocation/update/release and late failures. Sampled strided vehicle inputs; smoke/debris/shield and enclosing effects order remain separate."})).unwrap()).unwrap();
}
