mod support;
use gpu_v2::lighting::{
    ports::*,
    sim::{counted, oracle},
};
use std::fmt::Write;

#[test]
fn system_expressions_match_independent_stages_and_report_error_classes() {
    let mut cases: Vec<_> = support::representative()
        .into_iter()
        .map(|c| ("boundary", c))
        .collect();
    let mut random = support::Random(0x45a69207);
    for i in 0..3000 {
        let light = Light {
            direction: random.direction(),
            ambient: 0,
            directional: 256,
        };
        let mut pixel = PixelInput {
            normal: std::array::from_fn(|_| random.next() as i16),
            ndc: std::array::from_fn(|_| (random.next() % 32769) as i32 - 16384),
        };
        let material = Material {
            shininess_code: (i % 17) as u8,
            ..Default::default()
        };
        let projection = Projection::default();
        if i & 1 == 0 {
            let old =
                oracle::evaluate(pixel, material, light, projection, Default::default()).unwrap();
            pixel.normal = std::array::from_fn(|axis| {
                let h = old
                    .stages
                    .iter()
                    .find(|(n, _)| n == &format!("h.{axis}"))
                    .unwrap()
                    .1;
                (h + i128::from((random.next() % 513) as i32 - 256)).clamp(-32768, 32767) as i16
            });
        }
        cases.push(("ordinary", (pixel, material, light, projection)));
    }
    for lx in 64_i16..=512 {
        let lz = -((268435456_f64 - f64::from(lx).powi(2))
            .sqrt()
            .round_ties_even() as i16);
        for magnitude in [4, 63, 64, 127, 128, 1023, 8191, 16383, 16384, 32767] {
            for delta in -2..=2 {
                let z = (f64::from(magnitude) * f64::from(lx) / f64::from(-lz)).round_ties_even()
                    as i16
                    + delta;
                cases.push((
                    "NL cancellation",
                    (
                        PixelInput {
                            normal: [magnitude, 0, z],
                            ndc: [0, 0],
                        },
                        Material {
                            shininess_code: 16,
                            ..Default::default()
                        },
                        Light {
                            direction: [lx, 0, lz],
                            ambient: 0,
                            directional: 256,
                        },
                        Projection::default(),
                    ),
                ));
            }
        }
    }
    for x in -64..=64 {
        for code in 0..17 {
            cases.push((
                "H degeneration",
                (
                    PixelInput {
                        normal: [16384, 0, 0],
                        ndc: [(x * 32) / 4, 0],
                    },
                    Material {
                        shininess_code: code,
                        ..Default::default()
                    },
                    Light {
                        direction: [0, 0, -16384],
                        ambient: 0,
                        directional: 256,
                    },
                    Projection::default(),
                ),
            ));
        }
    }
    let policy = oracle::RoundingPolicy {
        power: oracle::Rounding::Floor,
        ..Default::default()
    };
    let mut report=String::from("direct_all,square9,exact_gate,class,count,max_change_g,max_change_h,max_ideal_g,max_ideal_h,mean_abs_ideal_h,large_h_changes,old_max_ideal_h\n");
    for (direct_all, square9, exact_gate) in [
        (false, false, false),
        (true, false, false),
        (true, true, false),
        (true, false, true),
    ] {
        let mut classes =
            std::collections::BTreeMap::<&str, (usize, [i128; 2], [f64; 2], f64, usize, f64)>::new(
            );
        let mut gates = std::collections::BTreeMap::<&str, (usize, f64, f64)>::new();
        for (class, (p, m, l, pr)) in &cases {
            let golden = oracle::evaluate(
                *p,
                *m,
                *l,
                *pr,
                oracle::Config {
                    scalar_norm: true,
                    scalar_normal: true,
                    direct_all_squares: direct_all,
                    square9,
                    rounding: policy,
                    exact_normal_gate: exact_gate,
                    ..Default::default()
                },
            )
            .unwrap();
            let actual = counted::evaluate_with_config(
                *p,
                *m,
                *l,
                *pr,
                support::MAX_EVENTS,
                counted::Config {
                    square9,
                    exact_normal_gate: exact_gate,
                    ..counted::Config::system_candidate(direct_all)
                },
            )
            .unwrap();
            actual.frame.audit().unwrap();
            for o in actual.frame.outputs {
                assert_eq!(
                    o.raw,
                    golden.stages.iter().find(|(n, _)| n == &o.name).unwrap().1,
                    "direct_all={direct_all},stage={}, {p:?} {m:?} {l:?}",
                    o.name
                );
            }
            assert_eq!(
                (i128::from(actual.output.g), i128::from(actual.output.h)),
                (golden.g, golden.h)
            );
            let old = oracle::evaluate(
                *p,
                *m,
                *l,
                *pr,
                oracle::Config {
                    rounding: policy,
                    ..Default::default()
                },
            )
            .unwrap();
            let ideal = oracle::ideal(*p, *m, *l, *pr).unwrap();
            if *class == "NL cancellation" {
                let raw_dot: i64 = p
                    .normal
                    .iter()
                    .zip(l.direction)
                    .map(|(&a, b)| i64::from(a) * i64::from(b))
                    .sum();
                let truth = p.normal.iter().any(|&a| i32::from(a).abs() >= 4) && raw_dot > 0;
                let old_gate = old.stages.iter().find(|(n, _)| n == "nl").unwrap().1 > 0;
                let new_gate = golden
                    .stages
                    .iter()
                    .find(|(n, _)| n == "nl.gate")
                    .unwrap()
                    .1
                    != 0;
                let group = match (old_gate == truth, new_gate == truth) {
                    (false, true) => "repaired",
                    (true, false) => "introduced",
                    (false, false) => "both wrong",
                    (true, true) => "both correct",
                };
                let entry = gates.entry(group).or_default();
                entry.0 += 1;
                entry.1 += (old.h as f64 - 256.0 * ideal[1]).abs();
                entry.2 += (golden.h as f64 - 256.0 * ideal[1]).abs();
            }
            let c = classes.entry(class).or_default();
            c.0 += 1;
            for (axis, (new, old_value)) in [(golden.g, old.g), (golden.h, old.h)]
                .into_iter()
                .enumerate()
            {
                c.1[axis] = c.1[axis].max((new - old_value).abs());
                c.2[axis] = c.2[axis].max((new as f64 - 256.0 * ideal[axis]).abs());
            }
            c.3 += (golden.h as f64 - 256.0 * ideal[1]).abs();
            c.4 += usize::from((golden.h - old.h).abs() > 4);
            c.5 = c.5.max((old.h as f64 - 256.0 * ideal[1]).abs());
        }
        for (class, (n, change, error, sum, large, old_error)) in classes {
            writeln!(
                report,
                "{direct_all},{square9},{exact_gate},{class},{n},{},{},{:.6},{:.6},{:.6},{large},{old_error:.6}",
                change[0],
                change[1],
                error[0],
                error[1],
                sum / n as f64
            )
            .unwrap();
        }
        let mut gate_report =
            String::from("class,count,mean_abs_old_h_error,mean_abs_new_h_error\n");
        for (class, (count, old_error, new_error)) in gates {
            writeln!(
                gate_report,
                "{class},{count},{:.6},{:.6}",
                old_error / count as f64,
                new_error / count as f64
            )
            .unwrap();
        }
        std::fs::create_dir_all("../../target/gpu-v2-lighting/system-study").unwrap();
        std::fs::write(
            format!("../../target/gpu-v2-lighting/system-study/gates-{direct_all}-{square9}-{exact_gate}.csv"),
            gate_report,
        )
        .unwrap();
        // Preserve the previous scalar-N rejection fixture explicitly. The
        // candidate gates on the raw dot sign, before narrowing or output RNE.
        let p = PixelInput {
            normal: [1023, 0, 8],
            ndc: [0, 0],
        };
        let l = Light {
            direction: [128, 0, -16383],
            ambient: 0,
            directional: 256,
        };
        let m = Material {
            shininess_code: 16,
            ..Default::default()
        };
        let g = oracle::evaluate(
            p,
            m,
            l,
            Projection::default(),
            oracle::Config {
                scalar_norm: true,
                scalar_normal: true,
                direct_all_squares: direct_all,
                square9,
                rounding: policy,
                exact_normal_gate: exact_gate,
                ..Default::default()
            },
        )
        .unwrap();
        // Its exact input dot is -120: the original rounded unit normal
        // incorrectly enables the highlight. Retain the 256-code change,
        // and independently establish which sign the ideal reference uses.
        assert_eq!(
            oracle::ideal(p, m, l, Projection::default()).unwrap()[1],
            0.0
        );
        assert_eq!(g.stages.iter().find(|(n, _)| n == "nl.gate").unwrap().1, 0);
        assert_eq!(g.h, 0, "preserved NL cancellation regression: {g:?}");
        let old = oracle::evaluate(
            p,
            m,
            l,
            Projection::default(),
            oracle::Config {
                rounding: policy,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(old.h, 256);
    }
    std::fs::create_dir_all("../../target/gpu-v2-lighting/system-study").unwrap();
    std::fs::write(
        "../../target/gpu-v2-lighting/system-study/numerical.csv",
        &report,
    )
    .unwrap();
    println!("{report}");
}
