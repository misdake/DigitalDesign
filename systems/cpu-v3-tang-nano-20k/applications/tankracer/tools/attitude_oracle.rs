//! Oracle fixture producer. Build against the unmodified TankRacerCUP game crate.
use serde_json::{json, Value};
use std::{env, fs, path::PathBuf};
use tankracer_game::{
    attitude::{Config, Numerics, State},
    contact::Coefficient,
    math::{self, Matrix},
};
fn flatten(s: &State) -> Vec<i32> {
    s.reference
        .0
        .into_iter()
        .chain(s.matrix.0)
        .chain(s.position)
        .chain(s.angular)
        .chain([s.longitudinal_delta, s.lateral_delta])
        .chain(s.pivot)
        .collect()
}
fn shift(v: i32, s: u32) -> i32 {
    if v < 0 {
        v.wrapping_neg().wrapping_shr(s).wrapping_neg()
    } else {
        v.wrapping_shr(s)
    }
}
fn events(s: &State, c: &Config, frame: u32) -> u16 {
    let mut e = 0;
    let r = s.reference.0;
    let mut x = s.angular[0].wrapping_add(shift(
        s.longitudinal_delta
            .wrapping_mul(c.longitudinal.multiplier.into()),
        c.longitudinal.shift.into(),
    ));
    let mut z = s.angular[2].wrapping_sub(shift(
        s.lateral_delta.wrapping_mul(c.lateral.multiplier.into()),
        c.lateral.shift.into(),
    ));
    let Ok(first) = math::angle(
        r[1].wrapping_abs(),
        math::length2(r[0].wrapping_abs(), r[2].wrapping_abs()),
    ) else {
        return e;
    };
    if first > 12740 {
        e |= 1;
        if r[1] >= 0 {
            if z < 0 {
                z = z.wrapping_neg() >> 2;
                e |= 2
            }
            z = z.wrapping_add(5000)
        } else {
            if z > 0 {
                z = (z >> 2).wrapping_neg();
                e |= 2
            }
            z = z.wrapping_sub(5000)
        }
    }
    let Ok(second) = math::angle(
        r[7].wrapping_abs(),
        math::length2(r[6].wrapping_abs(), r[8].wrapping_abs()),
    ) else {
        return e;
    };
    if second > 12740 {
        e |= 4;
        if (r[7] <= 0 && x < 0) || (r[7] > 0 && x > 0) {
            x = shift(z, 2).wrapping_neg();
            e |= 8
        }
        let _ = x;
    }
    if frame % 10 == 0 {
        e |= 16;
        let mut check = s.clone();
        let mut original = *c;
        original.numerics = Numerics::Original;
        if check.step(&original, 0, frame).is_ok() && c.numerics == Numerics::OrthogonalV1 {
            e |= 32
        }
    }
    e
}
fn main() {
    let mut args = env::args_os().skip(1);
    let reference = PathBuf::from(args.next().unwrap());
    let output = PathBuf::from(args.next().unwrap());
    fs::create_dir_all(&output).unwrap();
    for name in ["sine.bin", "angle.bin"] {
        fs::copy(
            reference.join("crates/game/data").join(name),
            output.join(name),
        )
        .unwrap();
    }
    let mut cases: Vec<Value> = Vec::new();
    for (name, numerics, dt) in [
        ("orthogonal-16ms", Numerics::OrthogonalV1, 16),
        ("original-16ms", Numerics::Original, 16),
        ("orthogonal-33ms", Numerics::OrthogonalV1, 33),
    ] {
        let config = Config {
            longitudinal: Coefficient {
                multiplier: 7,
                shift: 8,
            },
            lateral: Coefficient {
                multiplier: 5,
                shift: 8,
            },
            numerics,
        };
        let mut state = State {
            matrix: Matrix::from_angles([1300, 17000, 700]),
            position: [i32::MIN + 4096, 77, -1923],
            angular: [11303, 27409, -9173],
            pivot: [-15000, 4300, 17000],
            ..State::default()
        };
        let initial = flatten(&state);
        let mut frames = Vec::new();
        let mut rng = 0x1267ab45u32;
        for frame in 1..=600 {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            state.longitudinal_delta = (rng as i32) % 11000;
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            state.lateral_delta = (rng as i32) % 9000;
            state.reference = state.matrix;
            // Explicit frozen contacts include old-reference rollover guards.
            if frame % 73 == 0 {
                state.reference = Matrix([0, 4096, 0, 4096, 0, 0, 0, -4096, 0]);
            }
            if frame % 109 == 0 {
                state.reference = Matrix([0, -4096, 0, 4096, 0, 0, 0, 4096, 0]);
            }
            let input = json!({"dt":dt,"frame":frame,"longitudinal":state.longitudinal_delta,"lateral":state.lateral_delta,"reference":state.reference.0});
            let branches = events(&state, &config, frame);
            let ok = state.step(&config, dt, frame).is_ok();
            frames.push(
                json!({"input":input,"state":flatten(&state),"ok":ok,"events":branches,"rng":rng}),
            );
            assert!(ok, "{name} failed at {frame}");
        }
        cases.push(json!({"name":name,"initial":initial,"config":[7,8,5,8,u16::from(numerics==Numerics::OrthogonalV1)],"frames":frames}));
    }
    for (name, matrix, reference, frame) in [
        (
            "normalize-partial",
            [4000, 1, 0, 65407, 339, 0, 0, 0, 4096],
            Matrix::IDENTITY.0,
            10,
        ),
        (
            "polar-reflected",
            [4096, 0, 0, 0, 4096, 0, 0, 0, -4096],
            Matrix::IDENTITY.0,
            10,
        ),
        (
            "angle-int-min",
            Matrix::IDENTITY.0,
            [4096, i32::MIN, 0, 0, 4096, 0, 0, 0, 4096],
            1,
        ),
    ] {
        let config = Config {
            numerics: Numerics::OrthogonalV1,
            ..Config::default()
        };
        let mut state = State {
            matrix: Matrix(matrix),
            reference: Matrix(reference),
            ..State::default()
        };
        let initial = flatten(&state);
        let branches = events(&state, &config, frame);
        let ok = state.step(&config, 0, frame).is_ok();
        assert!(!ok, "{name} must reach its intended error boundary");
        cases.push(json!({"name":name,"initial":initial,"config":[0,0,0,0,1],"frames":[{"input":{"dt":0,"frame":frame,"longitudinal":0,"lateral":0,"reference":reference},"state":flatten(&state),"ok":ok,"events":branches,"rng":0}]}));
    }
    for (name, angular, position, dt, deltas) in [
        ("negative-half", [-7, -9, -11], [0, 0, 0], 8192, [0, 0]),
        (
            "multiply-position-wrap",
            [i32::MAX, i32::MIN, i32::MIN + 3],
            [i32::MAX, i32::MIN, i32::MAX - 1],
            33,
            [i32::MIN, i32::MAX],
        ),
    ] {
        let config = Config {
            longitudinal: Coefficient {
                multiplier: 7,
                shift: 8,
            },
            lateral: Coefficient {
                multiplier: 5,
                shift: 8,
            },
            numerics: Numerics::OrthogonalV1,
        };
        let mut state = State {
            angular,
            position,
            pivot: [15000, -7000, 4300],
            longitudinal_delta: deltas[0],
            lateral_delta: deltas[1],
            ..State::default()
        };
        let initial = flatten(&state);
        let reference = state.reference.0;
        let branches = events(&state, &config, 1);
        let ok = state.step(&config, dt, 1).is_ok();
        assert!(ok, "{name}");
        if name == "negative-half" {
            assert_ne!(
                state.matrix,
                Matrix::IDENTITY,
                "negative-half must expose rotation quantization"
            );
        }
        cases.push(json!({"name":name,"initial":initial,"config":[7,8,5,8,1],"frames":[{"input":{"dt":dt,"frame":1,"longitudinal":deltas[0],"lateral":deltas[1],"reference":reference},"state":flatten(&state),"ok":ok,"events":branches,"rng":0}]}));
    }
    fs::write(
        output.join("attitude.json"),
        serde_json::to_vec_pretty(
            &json!({"policy":"TankRacerCUP unmodified Rust oracle", "cases":cases}),
        )
        .unwrap(),
    )
    .unwrap();
}
