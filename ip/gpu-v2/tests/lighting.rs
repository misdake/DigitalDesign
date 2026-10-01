mod support;
use gpu_v2::lighting::{
    ports::*,
    sim::{counted, oracle},
};
use support::MAX_EVENTS;

fn compare(
    pixel: PixelInput,
    material: Material,
    light: Light,
    projection: Projection,
) -> counted::Report {
    let golden = oracle::evaluate(
        pixel,
        material,
        light,
        projection,
        oracle::Config::default(),
    )
    .unwrap();
    let counted = counted::evaluate(pixel, material, light, projection, MAX_EVENTS).unwrap();
    let observed: Vec<_> = counted
        .frame
        .outputs
        .iter()
        .map(|v| (v.name.clone(), v.raw))
        .collect();
    assert_eq!(
        observed, golden.stages,
        "pixel={pixel:?},material={material:?},light={light:?}"
    );
    assert_eq!(i128::from(counted.output.g), golden.g);
    assert_eq!(i128::from(counted.output.h), golden.h);
    assert!(counted.output.g <= 511 && counted.output.h <= 256);
    assert_eq!(counted.frame.scheduled_cycles(), None);
    counted.frame.audit().unwrap();
    counted
}

#[test]
fn oracle_known_directions_and_modes() {
    let p = PixelInput {
        normal: [0, 0, 16384],
        ndc: [0, 0],
    };
    let l = Light::default();
    let projection = Projection::default();
    let m = Material::default();
    assert_eq!(oracle::ideal(p, m, l, projection).unwrap(), [1.0, 0.875]);
    assert_eq!(
        compare(p, m, l, projection).output,
        LightingOutput { g: 256, h: 224 }
    );
    assert_eq!(
        compare(
            PixelInput {
                normal: [0; 3],
                ..p
            },
            m,
            l,
            projection
        )
        .output,
        LightingOutput { g: 32, h: 0 }
    );
    assert_eq!(
        compare(p, Material { unlit: true, ..m }, l, projection).output,
        LightingOutput { g: 256, h: 0 }
    );
    assert_eq!(
        compare(
            p,
            m,
            Light {
                directional: 0,
                ..l
            },
            projection
        )
        .output,
        LightingOutput { g: 32, h: 0 }
    );
    let diffuse = compare(
        p,
        Material {
            specular_color: [0; 3],
            ..m
        },
        l,
        projection,
    );
    assert_eq!(diffuse.output, LightingOutput { g: 256, h: 0 });
    assert!(diffuse
        .frame
        .outputs
        .iter()
        .all(|v| !v.name.starts_with("v.")));
    assert_eq!(
        compare(
            p,
            m,
            Light {
                ambient: 256,
                directional: 256,
                ..l
            },
            projection
        )
        .output
        .g,
        511
    );
}

#[test]
fn reciprocal_work_precision_experiment_preserves_external_formats() {
    // Reproduced input where retaining an interpolation bit reaches the output.
    let pixel = PixelInput {
        normal: [31689, 5502, -5932],
        ndc: [-56255, -29532],
    };
    let material = Material {
        shininess_code: 3,
        ..Material::default()
    };
    let light = Light {
        direction: [15050, -2994, -5741],
        ambient: 32,
        directional: 256,
    };
    let projection = Projection {
        ray_scale: [-12288, -8192],
        k: 10240,
    };
    let evaluate = |extra| {
        oracle::evaluate(
            pixel,
            material,
            light,
            projection,
            oracle::Config {
                reciprocal_work_extra: extra,
                ..oracle::Config::default()
            },
        )
    };
    assert_eq!(compare(pixel, material, light, projection).output.h, 143);
    assert_eq!(evaluate(0).unwrap().h, 143);
    for extra in [1, 3, 8] {
        let higher = evaluate(extra).unwrap();
        assert_eq!(higher.h, 142);
        assert_eq!(higher.intensity_fraction, 8);
    }
    assert!(matches!(evaluate(9), Err(InputError::Configuration)));
}

#[test]
fn representative_stage_goldens_match_counted() {
    for (pixel, material, light, projection) in support::representative() {
        compare(pixel, material, light, projection);
    }
}

#[test]
fn seeded_sphere_and_shininess_sweep_match_and_measure_ideal_error() {
    let mut random = support::Random(0x6ce4_231b_1978_02ab);
    let mut maximum = [0.0_f64; 2];
    for index in 0..1024 {
        let pixel = PixelInput {
            normal: random.direction(),
            ndc: std::array::from_fn(|_| (random.next() % 131073) as i32 - 65536),
        };
        let light = Light {
            direction: random.direction(),
            ambient: (random.next() % 257) as u16,
            directional: (random.next() % 257) as u16,
        };
        let material = Material {
            shininess_code: (index % 17) as u8,
            ..Material::default()
        };
        let projection = Projection {
            ray_scale: [-12288, -8192],
            k: 10240,
        };
        let report = compare(pixel, material, light, projection);
        let ideal = oracle::ideal(pixel, material, light, projection).unwrap();
        for (i, raw) in [report.output.g, report.output.h].iter().enumerate() {
            maximum[i] = maximum[i].max((f64::from(*raw) / 256.0 - ideal[i]).abs());
        }
    }
    println!(
        "same-quantized-input ideal errors, samples=1024, max_g={}, max_h={}",
        maximum[0], maximum[1]
    );
    // Includes the final U9.8 quantization; this is a corpus regression, not a bound.
    assert!(maximum[0] < 0.008 && maximum[1] < 0.02, "{maximum:?}");
}

#[test]
fn full_backlight_and_degenerate_half_keep_full_arithmetic() {
    let p = PixelInput {
        normal: [0, 0, -16384],
        ndc: [0, 0],
    };
    let r = compare(
        p,
        Material::default(),
        Light {
            direction: [0, 0, -16384],
            ..Light::default()
        },
        Projection::default(),
    );
    assert_eq!(r.output, LightingOutput { g: 256, h: 0 });
    for stage in ["v.q", "h.q", "nh", "power"] {
        assert!(r.frame.outputs.iter().any(|v| v.name == stage));
    }
    let b = compare(
        p,
        Material::default(),
        Light::default(),
        Projection::default(),
    );
    assert_eq!(b.output, LightingOutput { g: 32, h: 0 });
    assert!(b.frame.outputs.iter().any(|v| v.name == "power"));
}

#[test]
fn oracle_precision_and_approximation_controls_are_effective() {
    let p = PixelInput {
        normal: [7123, -519, 13567],
        ndc: [18319, -34891],
    };
    let evaluate = |c| {
        oracle::evaluate(
            p,
            Material::default(),
            Light::default(),
            Projection::default(),
            c,
        )
        .unwrap()
    };
    let baseline = evaluate(oracle::Config::default());
    let higher = evaluate(oracle::Config {
        direction_fraction: 18,
        reciprocal_fraction: 20,
        dot_fraction: 20,
        power_fraction: 20,
        intensity_fraction: 12,
        ..oracle::Config::default()
    });
    assert_ne!(baseline.stages, higher.stages);
    let exact = evaluate(oracle::Config {
        approximate_square: false,
        approximate_rsqrt: false,
        approximate_power: false,
        ..oracle::Config::default()
    });
    assert_ne!(baseline.stages, exact.stages);
    let ideal = oracle::ideal(
        p,
        Material::default(),
        Light::default(),
        Projection::default(),
    )
    .unwrap();
    for (actual, expected) in higher.intensities().iter().zip(ideal) {
        assert!((actual - expected).abs() < 0.005);
    }
}

#[test]
fn all_power_codes_are_monotone_and_within_half_a_color_code() {
    let exponents = [
        4, 5, 6, 7, 8, 10, 12, 14, 16, 20, 24, 28, 32, 40, 48, 56, 64,
    ];
    for (code, exponent) in exponents.iter().enumerate() {
        let mut previous = 0;
        for x in 0..=32768 {
            let p = oracle::power_table(x, code as u8).unwrap();
            assert!(p >= previous && p <= 32768);
            let expected = (f64::from(x) / 32768.0).powi(*exponent);
            assert!((f64::from(p) / 32768.0 - expected).abs() < 0.5 / 255.0);
            previous = p;
        }
        assert_eq!(previous, 32768);
    }
}

#[test]
fn invalid_inputs_and_event_limits_are_failures() {
    let p = PixelInput {
        normal: [0, 0, 16384],
        ndc: [0, 0],
    };
    let m = Material::default();
    let l = Light::default();
    let projection = Projection::default();
    assert!(counted::evaluate(
        p,
        Material {
            shininess_code: 17,
            ..m
        },
        l,
        projection,
        MAX_EVENTS
    )
    .is_err());
    assert!(counted::evaluate(p, m, Light { ambient: 257, ..l }, projection, MAX_EVENTS).is_err());
    assert!(counted::evaluate(
        p,
        m,
        Light {
            direction: [0; 3],
            ..l
        },
        projection,
        MAX_EVENTS
    )
    .is_err());
    assert!(counted::evaluate(
        PixelInput {
            ndc: [65537, 0],
            ..p
        },
        m,
        l,
        projection,
        MAX_EVENTS
    )
    .is_err());
    assert!(counted::evaluate(p, m, l, Projection { k: 0, ..projection }, MAX_EVENTS).is_err());
    assert!(matches!(
        counted::evaluate(p, m, l, projection, 2),
        Err(counted::Error::Audit(audited::Fault::EventLimit))
    ));
}
#[test]
fn half_threshold_is_applied_after_rne_and_normal_extreme_is_safe() {
    for x in [126, 127, 128] {
        let r = compare(
            PixelInput {
                normal: [i16::MIN, 0, 0],
                ndc: [0; 2],
            },
            Material::default(),
            Light {
                direction: [-x, 0, -16384],
                ..Light::default()
            },
            Projection::default(),
        );
        let h_x = r
            .frame
            .outputs
            .iter()
            .find(|v| v.name == "h.0")
            .unwrap()
            .raw;
        if x == 126 {
            assert_eq!(h_x, 0);
        } else {
            assert_eq!(h_x, -16384);
        }
    }
}

#[test]
fn optimized_counted_matches_independent_stages_and_limits_output_change() {
    let signed_only = counted::Config {
        signed_square: true,
        power_floor: false,
    };
    let optimized = counted::Config::optimized();
    for (pixel, material, light, projection) in support::representative() {
        let old = counted::evaluate(pixel, material, light, projection, MAX_EVENTS).unwrap();
        let exact = counted::evaluate_with_config(
            pixel,
            material,
            light,
            projection,
            MAX_EVENTS,
            signed_only,
        )
        .unwrap();
        let old_stages: Vec<_> = old.frame.outputs.iter().map(|v| (&v.name, v.raw)).collect();
        let exact_stages: Vec<_> = exact
            .frame
            .outputs
            .iter()
            .map(|v| (&v.name, v.raw))
            .collect();
        assert_eq!(old_stages, exact_stages);
        let new = counted::evaluate_with_config(
            pixel, material, light, projection, MAX_EVENTS, optimized,
        )
        .unwrap();
        let golden = oracle::evaluate(
            pixel,
            material,
            light,
            projection,
            oracle::Config {
                rounding: oracle::RoundingPolicy {
                    power: oracle::Rounding::Floor,
                    ..oracle::RoundingPolicy::default()
                },
                ..oracle::Config::default()
            },
        )
        .unwrap();
        let stages: Vec<_> = new
            .frame
            .outputs
            .iter()
            .map(|v| (v.name.clone(), v.raw))
            .collect();
        assert_eq!(stages, golden.stages);
        assert_eq!(new.output.g, old.output.g);
        assert!(old.output.h >= new.output.h && old.output.h - new.output.h <= 1);
        new.frame.audit().unwrap();
    }
}

#[test]
fn power_floor_is_monotone_and_at_most_one_q15_unit_below_rne() {
    for code in 0..17 {
        let mut previous = 0;
        for x in 0..=32768 {
            let old = oracle::power_table(x, code).unwrap();
            let new = oracle::power_table_with(x, code, oracle::Rounding::Floor).unwrap();
            assert!(
                new >= previous && new <= old && old - new <= 1,
                "code={code}, x={x}"
            );
            previous = new;
        }
        assert_eq!(previous, 32768);
    }
}

#[test]
fn flooring_half_moves_degeneracy_boundary_and_cannot_replace_rne() {
    let pixel = PixelInput {
        normal: [16384, 0, 0],
        ndc: [0; 2],
    };
    let light = Light {
        direction: [127, 0, -16384],
        ambient: 32,
        directional: 256,
    };
    let material = Material {
        shininess_code: 11,
        ..Material::default()
    };
    let rne = oracle::evaluate(
        pixel,
        material,
        light,
        Projection::default(),
        oracle::Config::default(),
    )
    .unwrap();
    let floor = oracle::evaluate(
        pixel,
        material,
        light,
        Projection::default(),
        oracle::Config {
            rounding: oracle::RoundingPolicy {
                half: oracle::Rounding::Floor,
                ..oracle::RoundingPolicy::default()
            },
            ..oracle::Config::default()
        },
    )
    .unwrap();
    assert_eq!((rne.h, floor.h), (256, 0));
}
