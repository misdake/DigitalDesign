use gpu_v2::lighting::{
    ports::*,
    sim::{counted, timed::*},
};

#[test]
fn compact_row_calendar_and_narrow_magnitude_match_aligned_working_values() {
    let compact = [
        CompactPixelInput {
            normal: [-2048, 1, 2047],
            ndc: [-16384, 16384],
        },
        CompactPixelInput {
            normal: [377, -286, 939],
            ndc: [3086, -7864],
        },
        CompactPixelInput {
            normal: [0; 3],
            ndc: [0; 2],
        },
        CompactPixelInput {
            normal: [1, -1, 0],
            ndc: [16384, -16384],
        },
    ];
    let wide: Vec<_> = compact.iter().map(|p| p.expanded().unwrap()).collect();
    for full in [false, true] {
        let material = Material {
            specular_color: if full { [255; 3] } else { [0; 3] },
            ..Default::default()
        };
        for storage in [
            Storage::Registers,
            Storage::Rows {
                read_lanes: 1,
                latency: 2,
            },
        ] {
            let hardware = Hardware {
                kernel: counted::Config::optimized(),
                ..Hardware::lighting_optimized_ii2()
            };
            let aligned = plan(
                &wide,
                material,
                Light::default(),
                Projection::default(),
                hardware,
                storage,
                Strategy::Interleaved,
            )
            .unwrap();
            let mut candidate = plan_compact(
                &compact,
                material,
                Light::default(),
                Projection::default(),
                hardware,
                storage,
                Strategy::Interleaved,
            )
            .unwrap();
            candidate.audit().unwrap();
            candidate
                .compare_oracle(&wide, material, Light::default(), Projection::default())
                .unwrap();
            assert_eq!(candidate.outputs, aligned.outputs);
            assert_eq!(aligned.pixel_payload_bits, 4 * 80);
            assert_eq!(candidate.pixel_payload_bits, 4 * 68);
            if matches!(storage, Storage::Rows { .. }) {
                assert_eq!(aligned.source_reads, 4 + 3 * 4);
                assert_eq!(candidate.source_reads, 4 + 2 * 4);
                assert!(candidate
                    .reads
                    .iter()
                    .filter(|r| r.pixel.is_some())
                    .all(|r| r.row < 2));
            }
            println!("compact full={full} storage={storage:?} cycles={} aligned={} payload={} aligned_payload={} reads={} aligned_reads={}",
                candidate.cycles, aligned.cycles, candidate.pixel_payload_bits, aligned.pixel_payload_bits,
                candidate.source_reads, aligned.source_reads);
            if matches!(storage, Storage::Rows { .. }) {
                let read = candidate
                    .events
                    .iter()
                    .position(|r| {
                        matches!(candidate.template.events[r.event].operation,
                        audited::Operation::Read { memory, .. }
                        if candidate.template.memories[memory].name == "pixel.compact-rows")
                    })
                    .unwrap();
                candidate.events[read].issue = 0;
                assert_eq!(candidate.audit().unwrap_err(), "physical input not ready");
            }
        }
    }
}

#[test]
fn compact_block_prescale_preserves_stage_values_without_extreme_guard() {
    for normal in [[-2048, 2047, 0], [2047, -1024, 1], [1, 0, -1], [0; 3]] {
        let pixel = CompactPixelInput {
            normal,
            ndc: [-4332, 7864],
        }
        .expanded()
        .unwrap();
        for scalar in [false, true] {
            let config = counted::Config {
                block_prescale: true,
                scalar_norm: scalar,
                scalar_normal: scalar,
                ..counted::Config::architecture()
            };
            let baseline = counted::evaluate_with_config(
                pixel,
                Material::default(),
                Light::default(),
                Projection::default(),
                4096,
                config,
            )
            .unwrap();
            let compact = counted::evaluate_with_config(
                pixel,
                Material::default(),
                Light::default(),
                Projection::default(),
                4096,
                counted::Config {
                    compact_normal: true,
                    ..config
                },
            )
            .unwrap();
            compact.frame.audit().unwrap();
            let stages = |r: &counted::Report| {
                r.frame
                    .outputs
                    .iter()
                    .map(|o| (o.name.clone(), o.raw))
                    .collect::<Vec<_>>()
            };
            assert_eq!(stages(&baseline), stages(&compact));
        }
    }
}

#[test]
fn compact_plans_reject_implicit_quantization_and_preserve_context_restriction() {
    let pixel = PixelInput {
        normal: [16383, 0, 1],
        ndc: [0; 2],
    };
    let compact = CompactPixelInput::from_q14(pixel).unwrap();
    assert_eq!(compact.normal, [1024, 0, 0]);
    let hardware = Hardware {
        kernel: counted::Config {
            compact_normal: true,
            ..counted::Config::architecture()
        },
        ..Hardware::lighting_optimized_ii2()
    };
    assert!(plan(
        &[pixel],
        Material::default(),
        Light::default(),
        Projection::default(),
        hardware,
        Storage::Registers,
        Strategy::Interleaved
    )
    .is_err());
    assert!(plan_compact(
        &[compact],
        Material::default(),
        Light::default(),
        Projection::default(),
        hardware,
        Storage::Rows {
            read_lanes: 1,
            latency: 1
        },
        Strategy::Interleaved
    )
    .err()
    .unwrap()
    .contains("register inputs"));
    let candidate = plan_compact(
        &[compact],
        Material::default(),
        Light::default(),
        Projection::default(),
        hardware,
        Storage::Registers,
        Strategy::Interleaved,
    )
    .unwrap();
    assert_eq!(candidate.pixel_payload_bits, 68);
}
