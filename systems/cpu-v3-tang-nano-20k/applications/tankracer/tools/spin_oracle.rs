//! Direct original weapon-spin sequences and complete failure prefixes.
use serde_json::{json, Value};
use std::{fs, path::Path};
use tankracer_game::vehicle_prefix::spin::{Shared, State, Tuning};

fn values(s: &State) -> [i32; 9] {
    [
        i32::from(s.flags),
        i32::from(s.visible),
        s.hold,
        s.color_timer,
        s.color as i32,
        s.target_color as i32,
        s.angle,
        s.speed,
        i32::from(s.disabled),
    ]
}
fn case(
    tag: String,
    mut s: State,
    mut shared: Shared,
    mut seed: u32,
    controls: Vec<[i32; 11]>,
) -> Value {
    let mut input = values(&s).to_vec();
    input.extend([
        i32::from(shared.count),
        i32::from(shared.pulse),
        seed as i32,
        controls.len() as i32,
    ]);
    input.extend(controls.iter().flatten());
    let mut frames = Vec::new();
    for c in controls {
        let status = if c[0] == 0 {
            s.start(c[2] != 0, c[3], &mut shared, &mut seed) as i32
        } else {
            s.step(
                c[1],
                Tuning {
                    fast: c[5],
                    slow: c[6],
                    stop: c[7],
                    accelerate: c[8] as u32,
                    settle: c[9] as u32,
                    fade: c[10] as u32,
                },
                c[4] as u32,
                &mut shared,
                &mut seed,
            )
            .map_or_else(|e| e, |()| 0)
        };
        let mut out = vec![status, seed as i32];
        out.extend(values(&s));
        out.extend([i32::from(shared.count), i32::from(shared.pulse)]);
        frames.push(out);
    }
    json!({"tag":tag,"input":input,"frames":frames})
}
fn control(action: i32, dt: i32, player: bool, mode: i32, limit: u32) -> [i32; 11] {
    let t = Tuning::default();
    [
        action,
        dt,
        i32::from(player),
        mode,
        limit as i32,
        t.fast,
        t.slow,
        t.stop,
        t.accelerate as i32,
        t.settle as i32,
        t.fade as i32,
    ]
}
fn next(seed: &mut u32) -> u32 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 17;
    *seed ^= *seed << 5;
    *seed
}
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let out = Path::new(&args[0]);
    let mut cases = Vec::new();
    for mode in [0, 1, 4, 5, -1] {
        for player in [false, true] {
            let mut controls = vec![control(0, 0, player, mode, 65536)];
            controls.extend(
                (0..23).map(|i| control(1, if i % 3 == 0 { 61 } else { 33 }, player, mode, 65536)),
            );
            cases.push(case(
                format!("start-{player}-{mode}"),
                State::default(),
                Shared {
                    count: 255,
                    pulse: 255,
                },
                0x1234abcd,
                controls,
            ));
        }
    }
    for flags in 0..16 {
        for (speed, hold, timer, color) in [
            (16307, 0, 60, 0xdeadffff),
            (931, -1, 61, 0xface0000),
            (1, 0, 61, 0xa5a50001),
            (0, 4000, 0, 0),
        ] {
            let s = State {
                flags,
                visible: 1,
                hold,
                color_timer: timer,
                color,
                target_color: 0x12345678,
                angle: 32760,
                speed,
                disabled: 0,
            };
            cases.push(case(
                format!("phase-{flags}-{speed}"),
                s,
                Shared { count: 0, pulse: 0 },
                0xfedcba98,
                (0..8).map(|_| control(1, 61, true, 0, 65536)).collect(),
            ));
        }
    }
    let mut seed = 0x62b3a4cd;
    for i in 0..384 {
        let speed = next(&mut seed) as i32;
        let angle = next(&mut seed) as i32;
        let s = State {
            flags: next(&mut seed) as u8,
            visible: next(&mut seed) as u8,
            hold: next(&mut seed) as i32,
            color_timer: next(&mut seed) as i32,
            color: next(&mut seed),
            target_color: next(&mut seed),
            angle,
            speed,
            disabled: next(&mut seed) as u8,
        };
        let shared = Shared {
            count: next(&mut seed) as u8,
            pulse: next(&mut seed) as u8,
        };
        let mut controls = Vec::new();
        for k in 0..3 {
            let mut c = control(
                if k == 0 && i % 7 == 0 { 0 } else { 1 },
                [-33, 0, 1, 33, 60, 61, 255, 256, 1000, i32::MIN, i32::MAX][i % 11],
                i % 2 == 0,
                (i % 7) as i32,
                [0, 1, 4, 65536][i % 4],
            );
            if i % 3 == 0 {
                for v in &mut c[5..] {
                    *v = next(&mut seed) as i32;
                }
            }
            controls.push(c);
        }
        cases.push(case(
            format!("integer-{i}"),
            s,
            shared,
            next(&mut seed),
            controls,
        ));
    }
    // One-step edges around the exact asymmetric wrapping interval and fade end.
    for angle in [
        i32::MIN,
        -98321,
        -32760,
        -32759,
        32760,
        32761,
        98281,
        i32::MAX,
    ] {
        for limit in [0, 1, 65536] {
            cases.push(case(
                format!("angle-{angle}-{limit}"),
                State {
                    angle,
                    speed: 0,
                    ..Default::default()
                },
                Shared::default(),
                1,
                vec![control(1, 0, true, 0, limit)],
            ));
        }
    }
    fs::write(out.join("spin.json"),serde_json::to_vec(&json!({"scope":"Original spin start/step, packed approach/color arithmetic, narrow shared counts, RNG and committed error prefixes","cases":cases})).unwrap()).unwrap();
}
