//! Stored-domain error and mixed-pixel tests for the selected Q13 calendar.
mod support;
use gpu_v2::lighting::{
    calendars::UnifiedCalendar,
    emu::LightingEmu,
    ports::*,
    rsqrt::*,
    rtl,
    sim::{counted, oracle, workbench::Workbench},
    LightingProfile, LightingQuantization,
};
use std::collections::VecDeque;

fn config() -> counted::Config {
    counted::Config {
        rsqrt_q13: true,
        ..counted::Config::lit_queue_resource_profile(
            LightingProfile::Fast,
            LightingQuantization::CompensatedFloor,
        )
    }
}
#[test]
fn q13_storage_and_all_interpolation_points_match_independent_equations() {
    let mut max_ideal = 0_f64;
    let mut max_old = 0_u64;
    let mut relative = 0_f64;
    let mut squares = 0_f64;
    let mut prior = u64::MAX;
    for address in 0..128 {
        let parity = address / 64;
        let segment = address % 64;
        let endpoint = |i: usize| {
            (8192.0 / ((1.0 + i as f64 / 64.0) * (1_u32 << parity) as f64).sqrt()).round_ties_even()
                as u64
        };
        let stored = RSQRT_Q13_RAW[address];
        let base = stored & 16383;
        let delta = (stored >> 14) + RSQRT_Q13_BIASES[address / 8];
        assert_eq!(base, endpoint(segment));
        assert_eq!(delta, base - endpoint(segment + 1));
        assert!(stored < (1 << 18) && delta < 64);
        let old = RSQRT_RAW[address];
        if segment == 0 {
            prior = u64::MAX;
        }
        for fraction in 0..256 {
            // The selected Floor kernel floors the nonnegative correction.
            let value = (base << 2) - (((delta << 2) * fraction) >> 8);
            let previous = (old & 65535) - (((old >> 16) * fraction) >> 8);
            max_old = max_old.max(value.abs_diff(previous));
            let ideal = 32768.0
                / ((1.0 + (segment as f64 + fraction as f64 / 256.0) / 64.0)
                    * (1_u32 << parity) as f64)
                    .sqrt();
            let error = (value as f64 - ideal).abs();
            max_ideal = max_ideal.max(error);
            relative = relative.max(error / ideal);
            squares += error * error;
            assert!(value <= prior);
            prior = value;
        }
    }
    assert!(max_ideal < 3.224, "{max_ideal}");
    assert!(max_old <= 2, "{max_old}");
    assert!(relative < 0.00017133, "{relative}");
    println!("Q13 Floor lookup: max_ideal_q15={max_ideal}, max_relative={relative}, rms_q15={}, max_delta_q15={max_old}",(squares/32768.0).sqrt());
}

#[test]
fn q13_counted_trace_retains_raw_18_bit_reads_and_matches_independent_stages() {
    let mut cases = support::representative();
    let mut random = support::Random(0xf438_672a_9423_dacf);
    for i in 0..1700 {
        cases.push((
            PixelInput {
                normal: std::array::from_fn(|_| ((random.next() % 4096) as i16 - 2048) * 16),
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
            Projection::default(),
        ));
    }
    for (pixel, material, light, projection) in cases {
        let report = counted::evaluate_with_config(
            pixel,
            material,
            light,
            projection,
            support::MAX_EVENTS * 2,
            config(),
        )
        .unwrap();
        report.frame.audit().unwrap();
        assert!(report
            .frame
            .memories
            .iter()
            .any(|m| m.name == "RSQRT_Q13" && m.format.bits == 18 && m.rows == 128));
        let golden = oracle::evaluate(
            pixel,
            material,
            light,
            projection,
            oracle::Config::from_counted(config()),
        )
        .unwrap();
        assert_eq!(
            (report.output.g as i128, report.output.h as i128),
            (golden.g, golden.h),
            "{pixel:?}"
        );
        for stage in report.frame.outputs {
            assert_eq!(
                stage.raw,
                golden
                    .stages
                    .iter()
                    .find(|(name, _)| *name == stage.name)
                    .unwrap()
                    .1,
                "{} {pixel:?}",
                stage.name
            );
        }
    }
}

#[test]
fn q13_selected_stream_keeps_different_pixels_ids_and_epochs_under_stalls() {
    let q = LightingQuantization::CompensatedFloor;
    let calendar = UnifiedCalendar::selected(q);
    let options = calendar.options(q);
    let plans = calendar.plans(q).unwrap();
    let generated =
        rtl::generate_with_schedule_plans(LightingProfile::Fast, options, &plans).unwrap();
    assert_eq!(
        (
            generated.latency,
            generated.specular_ii,
            generated.normalization_roms
        ),
        (38, 2, 3)
    );
    let mut emu =
        LightingEmu::with_schedule_plans(LightingProfile::Fast, options, &plans, 30000).unwrap();
    let mut random = support::Random(0x769e_b0f4_c43a_9fed);
    let idle = LightingTick {
        reset: false,
        ce: true,
        context: None,
        input: None,
        output_ready: true,
    };
    for context_index in 0..22 {
        let context = LightingContext {
            epoch: 0x8300 + context_index,
            material: Material {
                shininess_code: (context_index % 17) as u8,
                specular_color: if context_index == 1 { [0; 3] } else { [255; 3] },
                ..Default::default()
            },
            light: Light {
                direction: random.direction(),
                directional: if context_index == 0 { 0 } else { 256 },
                ..Default::default()
            },
            projection: Projection::default(),
        };
        assert!(
            emu.tick(LightingTick {
                context: Some(context),
                ..idle
            })
            .unwrap()
            .context_ready
        );
        let requests: Vec<_> = (0..32)
            .map(|i| LightingRequest {
                id: 0x8000_0000_u32
                    .wrapping_add((context_index as u32 * 32 + i).wrapping_mul(0x1f123bb5)),
                pixel: CompactPixelInput {
                    normal: std::array::from_fn(|_| (random.next() % 4096) as i16 - 2048),
                    ndc: std::array::from_fn(|_| (random.next() % 32769) as i32 - 16384),
                }
                .expanded()
                .unwrap(),
            })
            .collect();
        let mut sent = 0;
        let mut received = 0;
        let mut pending = VecDeque::new();
        let mut held = None;
        for cycle in 0..2000 {
            let tick = LightingTick {
                ce: cycle % 13 != 4,
                output_ready: cycle % 11 != 2 && cycle % 11 != 3,
                input: requests.get(sent).copied(),
                ..idle
            };
            let signals = emu.signals(tick);
            if let Some(before) = held {
                assert_eq!(signals.output, Some(before));
            }
            held = signals.output.filter(|_| !tick.ce || !tick.output_ready);
            if tick.ce && signals.input_ready && tick.input.is_some() {
                let request = requests[sent];
                let g = oracle::evaluate_output(
                    request.pixel,
                    context.material,
                    context.light,
                    context.projection,
                    oracle::Config::from_counted(config()),
                )
                .unwrap();
                pending.push_back(LightingResult {
                    id: request.id,
                    epoch: context.epoch,
                    output: LightingOutput {
                        g: g.g as u16,
                        h: g.h as u16,
                    },
                });
                sent += 1;
            }
            if let Some(result) = signals.output.filter(|_| tick.ce && tick.output_ready) {
                assert_eq!(Some(result), pending.pop_front());
                received += 1;
            }
            emu.tick(tick).unwrap();
            if received == requests.len() {
                break;
            }
        }
        assert_eq!((sent, received, pending.len()), (32, 32, 0));
    }
    let workbench = Workbench::with_calendar(calendar, q).unwrap();
    assert!(workbench
        .nodes
        .iter()
        .filter(|n| n.read_memory.as_deref() == Some("RSQRT_Q13"))
        .all(|n| n.bits == 18 && n.read_word_bits == Some(18)));
}

#[test]
fn q13_physical_certificate_pairs_six_reads_into_three_tdp_banks() {
    use gpu_v2::lighting::sim::timed::*;
    let hardware = Hardware {
        kernel: config(),
        ..Hardware::lighting_functions_ii2()
    };
    let plan = plan(
        &[PixelInput {
            normal: [7120, -512, 13568],
            ndc: [5789, 3142],
        }],
        Material::default(),
        Light::default(),
        Projection::default(),
        hardware,
        Storage::Registers,
        Strategy::Interleaved,
    )
    .unwrap();
    let schedule = PeriodicSchedule::search(&plan, 2, 4).unwrap();
    let physical = schedule
        .audit_physical(
            &plan,
            audited::physical::GowinMemoryBudget {
                bsram_blocks: 5,
                ssram_cells: 2048,
            },
            &audited::lifecycle::RegisterBudget {
                total_bits: 1_000_000,
                by_width: Default::default(),
            },
        )
        .unwrap();
    let banks: Vec<_> = physical
        .layout
        .banks
        .iter()
        .filter(|b| b.name.starts_with("normalize."))
        .collect();
    assert_eq!(banks.len(), 3);
    assert!(banks
        .iter()
        .all(|b| b.width == 18 && b.depth == 1024 && b.ports.len() == 2));
    assert!(physical.accesses.iter().any(|a| a.ports.contains(&1)));
}
