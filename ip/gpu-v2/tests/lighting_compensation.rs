mod support;
use audited::{Fixed, Model, Operation};
use gpu_v2::lighting::{
    ports::*,
    sim::{counted, oracle},
    LightingProfile,
};

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
