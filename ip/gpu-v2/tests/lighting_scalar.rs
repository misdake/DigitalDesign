mod support;
use gpu_v2::lighting::{
    ports::*,
    sim::{counted, oracle},
};

#[test]
fn bounded_block_prescale_preserves_all_published_stage_values() {
    let mut cases = support::representative();
    let mut random = support::Random(0xb04532619);
    for i in 0..3000 {
        cases.push((
            PixelInput {
                normal: std::array::from_fn(|_| random.next() as i16),
                ndc: std::array::from_fn(|_| (random.next() % 131073) as i32 - 65536),
            },
            Material {
                shininess_code: (i % 17) as u8,
                ..Default::default()
            },
            Light {
                direction: random.direction(),
                ..Default::default()
            },
            Projection::default(),
        ));
    }
    for (pixel, material, light, projection) in cases {
        for scalar_norm in [false, true] {
            let kernel = counted::Config {
                scalar_norm,
                block_prescale: true,
                ..counted::Config::architecture()
            };
            let actual =
                counted::evaluate_with_config(pixel, material, light, projection, 2048, kernel)
                    .unwrap();
            actual.frame.audit().unwrap();
            let golden = oracle::evaluate(
                pixel,
                material,
                light,
                projection,
                oracle::Config {
                    scalar_norm,
                    rounding: oracle::RoundingPolicy {
                        power: oracle::Rounding::Floor,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .unwrap();
            for observation in actual.frame.outputs {
                assert_eq!(
                    observation.raw,
                    golden
                        .stages
                        .iter()
                        .find(|(n, _)| n == &observation.name)
                        .unwrap()
                        .1,
                    "{} {pixel:?}",
                    observation.name
                );
            }
        }
    }
}

#[test]
fn scalar_expression_matches_independent_oracle_and_measures_output_error() {
    let mut cases = support::representative();
    let mut random = support::Random(0x86a47f231);
    for i in 0..10000 {
        let projection = Projection {
            ray_scale: [-12288, 12288],
            k: 8192 + (random.next() % 4097) as i16,
        };
        let light = Light {
            direction: random.direction(),
            ambient: 0,
            directional: 256,
        };
        let pixel = PixelInput {
            normal: std::array::from_fn(|_| random.next() as i16),
            ndc: std::array::from_fn(|_| (random.next() % 131073) as i32 - 65536),
        };
        let material = Material {
            shininess_code: (i % 17) as u8,
            ..Default::default()
        };
        // Concentrate half the samples near the highlight, where x^64 amplifies
        // dot rounding. The independent reference constructs the half vector.
        let mut pixel = pixel;
        if i & 1 == 0 {
            let probe =
                oracle::evaluate(pixel, material, light, projection, Default::default()).unwrap();
            pixel.normal = std::array::from_fn(|axis| {
                let name = format!("h.{axis}");
                let value = probe.stages.iter().find(|(n, _)| n == &name).unwrap().1;
                (value + i128::from((random.next() % 1025) as i32 - 512)).clamp(-32768, 32767)
                    as i16
            });
        }
        cases.push((pixel, material, light, projection));
    }
    // H threshold and all normal pre-scale boundaries with signed endpoints.
    for code in 0..17 {
        for coordinate in -64..=64 {
            for magnitude in [3, 4, 7, 8, 15, 16, 8191, 8192, 16383, 16384, 32767] {
                cases.push((
                    PixelInput {
                        normal: [-magnitude, 1, -1],
                        ndc: [coordinate * 32, 0],
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
                ));
            }
        }
    }
    // A tiny NL sign change can turn a full highlight on/off. Sweep the
    // cancellation surface independently of random/highlight samples.
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
                ));
            }
        }
    }
    for direct in [false, true] {
        let mut max_change = [0_i128; 2];
        let mut changed = [0; 2];
        let mut ideal_error = [0_f64; 2];
        let mut old_ideal_error = [0_f64; 2];
        for (pixel, material, light, projection) in &cases {
            let policy = oracle::RoundingPolicy {
                power: oracle::Rounding::Floor,
                ..Default::default()
            };
            let golden = oracle::evaluate(
                *pixel,
                *material,
                *light,
                *projection,
                oracle::Config {
                    scalar_norm: true,
                    direct_square: direct,
                    rounding: policy,
                    ..Default::default()
                },
            )
            .unwrap();
            let actual = counted::evaluate_with_config(
                *pixel,
                *material,
                *light,
                *projection,
                support::MAX_EVENTS,
                counted::Config {
                    direct_square: direct,
                    block_prescale: true,
                    ..counted::Config::scalar_pipeline()
                },
            )
            .unwrap();
            actual.frame.audit().unwrap();
            for observation in &actual.frame.outputs {
                assert_eq!(
                    observation.raw,
                    golden
                        .stages
                        .iter()
                        .find(|(n, _)| n == &observation.name)
                        .unwrap()
                        .1,
                    "{} {pixel:?} {material:?} {light:?}",
                    observation.name
                );
            }
            assert_eq!(
                (i128::from(actual.output.g), i128::from(actual.output.h)),
                (golden.g, golden.h)
            );
            let old = oracle::evaluate(
                *pixel,
                *material,
                *light,
                *projection,
                oracle::Config {
                    rounding: policy,
                    ..Default::default()
                },
            )
            .unwrap();
            for observation in &actual.frame.outputs {
                if observation.name.starts_with("n.")
                    || observation.name.starts_with("v.")
                    || observation.name.starts_with("ray.")
                    || ["nl", "d", "g", "h.shift"].contains(&observation.name.as_str())
                {
                    assert_eq!(
                        observation.raw,
                        old.stages
                            .iter()
                            .find(|(n, _)| n == &observation.name)
                            .unwrap()
                            .1,
                        "frozen stage {} {pixel:?} {light:?}",
                        observation.name
                    );
                }
            }
            let ideal = oracle::ideal(*pixel, *material, *light, *projection).unwrap();
            for (axis, (new, old)) in [(golden.g, old.g), (golden.h, old.h)]
                .into_iter()
                .enumerate()
            {
                let change = (new - old).abs();
                max_change[axis] = max_change[axis].max(change);
                changed[axis] += usize::from(change != 0);
                ideal_error[axis] = ideal_error[axis].max((new as f64 - ideal[axis] * 256.0).abs());
                old_ideal_error[axis] =
                    old_ideal_error[axis].max((old as f64 - ideal[axis] * 256.0).abs());
                assert!(
                    change <= if direct { 2 } else { 1 },
                    "axis={axis}, new={new},old={old}, {pixel:?} {material:?} {light:?}"
                );
            }
        }
        let report = format!("direct={direct},cases={},max_change={max_change:?},changed={changed:?},ideal_error={ideal_error:?},old_ideal_error={old_ideal_error:?}\n",cases.len());
        std::fs::create_dir_all("../../target/gpu-v2-lighting/scalar-study").unwrap();
        std::fs::write(
            format!("../../target/gpu-v2-lighting/scalar-study/numerical-direct-{direct}.txt"),
            &report,
        )
        .unwrap();
        println!("{report}");
    }
}
