mod support;
use audited::{Fixed, Model, Operation};
use gpu_v2::lighting::{
    emu::LightingEmu,
    ports::*,
    sim::{counted, oracle},
    LightingProfile, LightingQuantization,
};

#[test]
fn lit_queue_omits_unlit_gate_and_matches_independent_numeric_goldens() {
    for profile in [LightingProfile::Fast, LightingProfile::Compact] {
        for quantization in [
            LightingQuantization::NearestEven,
            LightingQuantization::CompensatedFloor,
        ] {
            let kernel = counted::Config::lit_queue_resource_profile(profile, quantization);
            let options = gpu_v2::lighting::rtl::LightingRtlOptions::lit_queue_resource_profile(
                profile,
                quantization,
            );
            let rtl = gpu_v2::lighting::rtl::generate_with_options(profile, options).unwrap();
            assert!(!rtl.source.contains("0: begin out_g=9'd256"));
            println!(
                "lit queue {profile:?}/{quantization:?}: full={} diffuse={} II={}/{}",
                rtl.latency, rtl.diffuse_latency, rtl.specular_ii, rtl.diffuse_ii
            );
            let mut emu =
                LightingEmu::lit_queue_resource_profile(profile, quantization, 100_000).unwrap();
            for (serial, (pixel, material, light, projection)) in
                support::representative().into_iter().enumerate()
            {
                let context = LightingContext {
                    material,
                    light,
                    projection,
                    epoch: serial as u16,
                };
                let tick = LightingTick {
                    reset: false,
                    ce: true,
                    context: Some(context),
                    input: None,
                    output_ready: true,
                };
                if material.unlit {
                    assert!(matches!(
                        counted::evaluate_with_config(
                            pixel,
                            material,
                            light,
                            projection,
                            support::MAX_EVENTS,
                            kernel
                        ),
                        Err(counted::Error::Input(InputError::UnlitQueue))
                    ));
                    assert!(emu.tick(tick).unwrap_err().contains("bypass"));
                    continue;
                }
                let counted = counted::evaluate_with_config(
                    pixel,
                    material,
                    light,
                    projection,
                    support::MAX_EVENTS,
                    kernel,
                )
                .unwrap();
                counted.frame.audit().unwrap();
                assert!(!counted
                    .frame
                    .values
                    .iter()
                    .any(|v| v.name.as_deref() == Some("mode.unlit")));
                let golden = oracle::evaluate(
                    pixel,
                    material,
                    light,
                    projection,
                    oracle::Config::from_counted(kernel),
                )
                .unwrap();
                assert_eq!(
                    [i128::from(counted.output.g), i128::from(counted.output.h)],
                    [golden.g, golden.h]
                );
                assert!(emu.tick(tick).unwrap().context_ready);
                let mut request = Some(LightingRequest {
                    id: serial as u32,
                    pixel,
                });
                let mut returned = false;
                for cycle in 0..1000 {
                    let signals = emu
                        .tick(LightingTick {
                            ce: cycle % 7 != 3,
                            context: None,
                            input: request,
                            ..tick
                        })
                        .unwrap();
                    if signals.input_ready {
                        request = None;
                    }
                    if let Some(result) = signals.output {
                        assert_eq!(result.output, counted.output);
                        assert_eq!(result.id, serial as u32);
                        assert_eq!(result.epoch, serial as u16);
                        returned = true;
                        break;
                    }
                }
                assert!(returned, "bounded queue result");
            }
        }
    }
}

#[test]
fn explicit_floor_and_guard_rounding_have_real_ledger_semantics() {
    for raw in -2048..=2047 {
        let mut model = Model::numerical();
        let input = model.input::<12, 4, true>("input", &[raw]).unwrap();
        let f = model.compute("quantization", 32).unwrap();
        let a = f.read(input.at::<0>()).unwrap();
        f.publish("floor", f.floor_to::<9, 0, true>(a).unwrap())
            .unwrap();
        f.publish("half-up", f.half_up_to::<9, 0, true>(a).unwrap())
            .unwrap();
        let report = f.finish();
        report.audit().unwrap();
        assert_eq!(report.outputs[0].raw, raw.div_euclid(16));
        assert_eq!(report.outputs[1].raw, (raw + 8).div_euclid(16));
        assert!(!report
            .events
            .iter()
            .any(|e| matches!(e.operation, Operation::RoundIncrement(_))));
    }
    let mut model = Model::numerical();
    let f = model.compute("checked overflow", 16).unwrap();
    assert!(f
        .half_up_to::<4, 0, true>(Fixed::<8, 4, true>::constant::<127>())
        .is_err());
    assert!(!f.finish().valid);
}

#[test]
fn compensated_counted_matches_independent_oracle_and_published_stages() {
    let mut inputs = support::representative();
    let mut random = support::Random(0x85ed427);
    for i in 0..1024 {
        let normal = if i == 0 {
            [32767; 3]
        } else {
            random.direction()
        };
        inputs.push((
            PixelInput {
                normal,
                ndc: [
                    ((random.next() % 131073) as i32) - 65536,
                    ((random.next() % 131073) as i32) - 65536,
                ],
            },
            Material {
                shininess_code: (i % 17) as u8,
                ..Material::default()
            },
            Light {
                direction: random.direction(),
                ambient: (random.next() % 257) as u16,
                directional: (random.next() % 257) as u16,
            },
            Projection::default(),
        ));
    }
    for profile in [LightingProfile::Fast, LightingProfile::Compact] {
        let kernel = counted::Config::compensated_resource_profile(profile);
        for &(pixel, material, light, projection) in &inputs {
            let actual = counted::evaluate_with_config(
                pixel,
                material,
                light,
                projection,
                support::MAX_EVENTS,
                kernel,
            )
            .unwrap();
            let golden = oracle::evaluate(
                pixel,
                material,
                light,
                projection,
                oracle::Config::from_counted(kernel),
            )
            .unwrap();
            assert_eq!(
                [i128::from(actual.output.g), i128::from(actual.output.h)],
                [golden.g, golden.h],
                "{profile:?} {pixel:?}"
            );
            for (name, raw) in golden.stages {
                assert_eq!(
                    actual
                        .frame
                        .outputs
                        .iter()
                        .find(|o| o.name == name)
                        .unwrap()
                        .raw,
                    raw,
                    "{profile:?} {name} {pixel:?}"
                );
            }
            assert!(actual.frame.memories.iter().all(|m| m.name != "POWER"));
        }
    }
}

#[test]
fn compensated_timed_plan_and_configuration_guards() {
    use gpu_v2::lighting::sim::timed;
    let pixels = [
        PixelInput {
            normal: [9459; 3],
            ndc: [12345, -54321],
        },
        PixelInput {
            normal: [0, 0, 16384],
            ndc: [0; 2],
        },
    ];
    for profile in [LightingProfile::Fast, LightingProfile::Compact] {
        let kernel = counted::Config::compensated_resource_profile(profile);
        let mut hardware = timed::Hardware::lighting_architecture_ii2();
        hardware.kernel = kernel;
        hardware.max_cycles = 20000;
        let plan = timed::plan(
            &pixels,
            Material::default(),
            Light::default(),
            Projection::default(),
            hardware,
            timed::Storage::Registers,
            timed::Strategy::Interleaved,
        )
        .unwrap();
        plan.audit().unwrap();
        plan.compare_oracle(
            &pixels,
            Material::default(),
            Light::default(),
            Projection::default(),
        )
        .unwrap();
        assert!(counted::evaluate_with_config(
            pixels[0],
            Material::default(),
            Light::default(),
            Projection::default(),
            support::MAX_EVENTS,
            counted::Config {
                shared_half: true,
                ..kernel
            }
        )
        .is_err());
        let mut wrong = oracle::Config::from_counted(kernel);
        wrong.rounding = Default::default();
        assert!(oracle::evaluate(
            pixels[0],
            Material::default(),
            Light::default(),
            Projection::default(),
            wrong
        )
        .is_err());
    }
}
