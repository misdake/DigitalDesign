mod support;

use gpu_v2::lighting::{
    ports::*,
    rtl::{self, LightingRtlOptions},
    sim::{counted, oracle},
    LightingProfile, LightingQuantization,
};

fn candidate(policy: LightingQuantization) -> counted::Config {
    counted::Config {
        weighted_view: true,
        ..counted::Config::lit_queue_resource_profile(LightingProfile::Fast, policy)
    }
}

#[test]
fn floor_weighted_add_can_narrow_before_adding_without_changing_a_bit() {
    for product in -131072_i64..=131071 {
        for ray in [-16384_i64, -1, 0, 1, 16384] {
            assert_eq!((product + (ray << 15)) >> 16, ((product >> 15) + ray) >> 1);
        }
    }
}

#[test]
fn length_scaled_degeneracy_can_use_a_narrow_ceil_threshold() {
    for length in 0_u32..1 << 17 {
        let threshold = (length + 511) >> 9;
        for magnitude in [
            0,
            threshold.saturating_sub(1),
            threshold,
            threshold + 1,
            32768,
        ] {
            assert_eq!(magnitude * 512 < length, magnitude < threshold);
        }
    }
}

#[test]
fn weighted_view_matches_independent_integer_stages_and_bounds() {
    let mut cases = support::representative();
    let mut random = support::Random(0x7284_5532_9238_4781);
    for i in 0..2048 {
        cases.push((
            PixelInput {
                normal: random.direction(),
                ndc: std::array::from_fn(|_| (random.next() % 32769) as i32 - 16384),
            },
            Material {
                shininess_code: (i % 17) as u8,
                ..Default::default()
            },
            Light {
                direction: random.direction(),
                ..Default::default()
            },
            Projection {
                ray_scale: [-12288; 2],
                k: (8192 + random.next() % 4097) as i16,
            },
        ));
    }
    // Include the scaled degeneracy boundary for several allowed ray lengths.
    for k in [8192, 10000, 12288] {
        for x in -180_i32..=180 {
            let z = -((16384_f64.powi(2) - f64::from(x).powi(2))
                .sqrt()
                .round_ties_even() as i16);
            cases.push((
                PixelInput {
                    normal: [0, 0, 16384],
                    ndc: [0, 0],
                },
                Material::default(),
                Light {
                    direction: [x as i16, 0, z],
                    ..Default::default()
                },
                Projection {
                    k,
                    ..Default::default()
                },
            ));
        }
    }
    for policy in [
        LightingQuantization::NearestEven,
        LightingQuantization::CompensatedFloor,
    ] {
        let kernel = candidate(policy);
        let baseline = counted::Config {
            weighted_view: false,
            ..kernel
        };
        let mut changed = 0;
        let mut maximum_delta = 0;
        let mut checks = 0;
        for &(pixel, material, light, projection) in &cases {
            let actual = counted::evaluate_with_config(
                pixel,
                material,
                light,
                projection,
                support::MAX_EVENTS,
                kernel,
            )
            .unwrap();
            actual.frame.audit().unwrap();
            let golden = oracle::evaluate(
                pixel,
                material,
                light,
                projection,
                oracle::Config::from_counted(kernel),
            )
            .unwrap();
            assert_eq!(
                [actual.output.g as i128, actual.output.h as i128],
                [golden.g, golden.h]
            );
            for publication in &actual.frame.outputs {
                let expected = golden
                    .stages
                    .iter()
                    .find(|(name, _)| name == &publication.name)
                    .unwrap_or_else(|| panic!("missing independent stage {}", publication.name));
                assert_eq!(
                    publication.raw, expected.1,
                    "{} {policy:?} {pixel:?} {light:?}",
                    publication.name
                );
                checks += 1;
            }
            if let Some((_, length)) = golden.stages.iter().find(|(name, _)| name == "v.length") {
                assert!(
                    (16384..=42570).contains(length),
                    "bounded Q15 length {length}"
                );
                assert!(!golden
                    .stages
                    .iter()
                    .any(|(name, _)| name == "v.r" || name == "v.0"));
            }
            let old = oracle::evaluate(
                pixel,
                material,
                light,
                projection,
                oracle::Config::from_counted(baseline),
            )
            .unwrap();
            assert_eq!(golden.g, old.g, "diffuse arithmetic must be unchanged");
            let delta = (golden.h - old.h).abs();
            changed += usize::from(delta != 0);
            maximum_delta = maximum_delta.max(delta);
        }
        println!("{policy:?}: cases={} independent_stage_checks={checks} h_changed={changed} max_h_delta_q8={maximum_delta}", cases.len());
    }
}

#[test]
fn weighted_view_has_an_audited_resource_schedule_and_explicit_scope() {
    for policy in [
        LightingQuantization::NearestEven,
        LightingQuantization::CompensatedFloor,
    ] {
        let options = LightingRtlOptions {
            weighted_view: true,
            ..LightingRtlOptions::lit_queue_resource_profile(LightingProfile::Fast, policy)
        };
        let rtl = rtl::generate_with_options(LightingProfile::Fast, options).unwrap();
        assert_eq!((rtl.specular_ii, rtl.diffuse_ii), (2, 1));
        println!(
            "weighted/{policy:?}: full={} diffuse={}",
            rtl.latency, rtl.diffuse_latency
        );
    }
    let (pixel, material, light, projection) = support::representative()[0];
    for invalid in [
        counted::Config {
            weighted_view: true,
            ..Default::default()
        },
        counted::Config {
            prepared_ray: true,
            ..candidate(Default::default())
        },
        counted::Config {
            compact_normal: true,
            ..candidate(Default::default())
        },
    ] {
        assert!(matches!(
            counted::evaluate_with_config(
                pixel,
                material,
                light,
                projection,
                support::MAX_EVENTS,
                invalid
            ),
            Err(counted::Error::Input(InputError::Configuration))
        ));
    }
}

#[test]
#[ignore = "bounded larger numerical comparison; writes an experiment receipt"]
fn weighted_view_error_distribution_against_fast_and_ideal() {
    use std::fmt::Write;
    let mut random = support::Random(0x6759_3146_7893_1567);
    let mut csv = String::from("group,policy,cases,changed,max_delta_q8,baseline_mean_abs_ideal_q8,candidate_mean_abs_ideal_q8,baseline_max_abs_ideal_q8,candidate_max_abs_ideal_q8\n");
    let mut witnesses = String::from("group,policy,index,normal_x,normal_y,normal_z,ndc_x,ndc_y,light_x,light_y,light_z,ray_k,shininess_code,baseline_h,candidate_h,ideal_h_q8\n");
    for near_reverse in [false, true] {
        for policy in [
            LightingQuantization::NearestEven,
            LightingQuantization::CompensatedFloor,
        ] {
            let kernel = candidate(policy);
            let mut old_cfg = oracle::Config::from_counted(kernel);
            old_cfg.weighted_view = false;
            let new_cfg = oracle::Config::from_counted(kernel);
            let mut delta_max = 0;
            let mut changed = 0;
            let mut sums = [0.0_f64; 2];
            let mut maxima = [0.0_f64; 2];
            let count = if near_reverse { 8192 } else { 32768 };
            for i in 0..count {
                let mut pixel = PixelInput {
                    normal: random.direction(),
                    ndc: std::array::from_fn(|_| (random.next() % 32769) as i32 - 16384),
                };
                let projection = Projection {
                    ray_scale: [-12288; 2],
                    k: (8192 + random.next() % 4097) as i16,
                };
                let material = Material {
                    shininess_code: (i % 17) as u8,
                    ..Default::default()
                };
                let mut light = Light {
                    direction: random.direction(),
                    ..Default::default()
                };
                if near_reverse {
                    let ray = [
                        pixel.ndc[0] as f64 / 16384.0 * projection.ray_scale[0] as f64,
                        pixel.ndc[1] as f64 / 16384.0 * projection.ray_scale[1] as f64,
                        projection.k as f64,
                    ];
                    let norm = |v: [f64; 3]| {
                        let s = v.iter().map(|x| x * x).sum::<f64>().sqrt();
                        v.map(|x| x / s)
                    };
                    let v = norm(ray);
                    let perturbation = (i % 512) as f64 / 16384.0;
                    let l = norm([-v[0] + perturbation, -v[1], -v[2]]);
                    light.direction = l.map(|x| (x * 16384.0).round_ties_even() as i16);
                    if perturbation > 0.0 {
                        // Deliberately maximize sensitivity: normal follows H,
                        // while L and V approach the legitimate antiparallel case.
                        pixel.normal = norm(std::array::from_fn(|axis| v[axis] + l[axis]))
                            .map(|x| (x * 16384.0).round_ties_even() as i16);
                    }
                }
                let old = oracle::evaluate(pixel, material, light, projection, old_cfg).unwrap();
                let new = oracle::evaluate(pixel, material, light, projection, new_cfg).unwrap();
                assert_eq!(new.g, old.g);
                let delta = (old.h - new.h).abs();
                changed += usize::from(delta != 0);
                let ideal = oracle::ideal(pixel, material, light, projection).unwrap()[1] * 256.0;
                if delta > delta_max {
                    writeln!(
                        witnesses,
                        "{},{policy:?},{i},{},{},{},{},{},{},{},{},{},{},{},{},{ideal:.8}",
                        if near_reverse {
                            "near-antiparallel-sensitive"
                        } else {
                            "ordinary"
                        },
                        pixel.normal[0],
                        pixel.normal[1],
                        pixel.normal[2],
                        pixel.ndc[0],
                        pixel.ndc[1],
                        light.direction[0],
                        light.direction[1],
                        light.direction[2],
                        projection.k,
                        material.shininess_code,
                        old.h,
                        new.h
                    )
                    .unwrap();
                }
                delta_max = delta_max.max(delta);
                for (j, value) in [old.h, new.h].into_iter().enumerate() {
                    let error = (value as f64 - ideal).abs();
                    sums[j] += error;
                    maxima[j] = maxima[j].max(error);
                }
            }
            writeln!(
                csv,
                "{},{policy:?},{count},{changed},{delta_max},{:.8},{:.8},{:.8},{:.8}",
                if near_reverse {
                    "near-antiparallel-sensitive"
                } else {
                    "ordinary"
                },
                sums[0] / count as f64,
                sums[1] / count as f64,
                maxima[0],
                maxima[1]
            )
            .unwrap();
        }
    }
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/lighting-weighted-half-20261006");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("numerical-errors.csv"), &csv).unwrap();
    std::fs::write(directory.join("numerical-witnesses.csv"), &witnesses).unwrap();
    println!("{csv}");
}
