mod support;
use gpu_v2::lighting::{
    ports::*,
    sim::{counted, oracle, timed::*},
};
#[test]
fn exact_dataflow_and_prepared_ray_preserve_every_stage() {
    for (p, m, l, pr) in support::representative() {
        let baseline = counted::evaluate_with_config(
            p,
            m,
            l,
            pr,
            support::MAX_EVENTS,
            counted::Config::optimized(),
        )
        .unwrap();
        for c in [counted::Config::architecture(), counted::Config::prepared()] {
            let r = counted::evaluate_with_config(p, m, l, pr, support::MAX_EVENTS, c).unwrap();
            assert_eq!(r.output, baseline.output, "{p:?} {m:?}");
            let stages = |f: &audited::FrameReport| {
                f.outputs
                    .iter()
                    .map(|o| (o.name.clone(), o.raw))
                    .collect::<Vec<_>>()
            };
            assert_eq!(stages(&r.frame), stages(&baseline.frame), "{p:?} {m:?}");
            r.frame.audit().unwrap();
            if let Some(prep) = r.context_preparation {
                prep.audit().unwrap();
            }
            if let Some(prep) = r.ray_preparation {
                prep.audit().unwrap();
            }
        }
    }
}
#[test]
fn diffuse_is_a_genuinely_smaller_one_pixel_per_cycle_calendar() {
    let p = PixelInput {
        normal: [32767, -32768, 17],
        ndc: [16384, -16384],
    };
    let h = Hardware {
        kernel: counted::Config::architecture(),
        ..Hardware::lighting_optimized_ii2()
    };
    let d = Material {
        specular_color: [0; 3],
        ..Material::default()
    };
    let dp = plan(
        &[p],
        d,
        Light::default(),
        Projection::default(),
        h,
        Storage::Registers,
        Strategy::Interleaved,
    )
    .unwrap();
    let ds = PeriodicSchedule::search(&dp, 1, 32).unwrap();
    let fp = plan(
        &[p],
        Material::default(),
        Light::default(),
        Projection::default(),
        h,
        Storage::Registers,
        Strategy::Interleaved,
    )
    .unwrap();
    let fs = PeriodicSchedule::search(&fp, 2, 32).unwrap();
    assert!(ds.latency < fs.latency);
    assert!(PeriodicSchedule::search(&fp, 1, 32).is_err());
    assert!(dp.template.outputs.iter().all(|o| !o.name.starts_with("v.")
        && !o.name.starts_with("h.")
        && o.name != "nh"
        && o.name != "power"));
    let pixels = vec![p; 32];
    let expanded = ds
        .expand(
            plan(
                &pixels,
                d,
                Light::default(),
                Projection::default(),
                h,
                Storage::Registers,
                Strategy::Interleaved,
            )
            .unwrap(),
        )
        .unwrap();
    expanded
        .compare_oracle(&pixels, d, Light::default(), Projection::default())
        .unwrap();
    assert_eq!(expanded.cycles, ds.latency + 31);
    assert!(expanded
        .writes
        .windows(2)
        .all(|w| w[1].ready == w[0].ready + 1));
}
#[test]
fn prepared_ray_golden_matches_independent_ndc_equations_at_boundaries() {
    for x in [-16384, -16383, -1, 0, 1, 16383, 16384] {
        for y in [-16384, 0, 16384] {
            let p = PixelInput {
                normal: [0, 0, 16384],
                ndc: [x, y],
            };
            let (ray, report) = counted::prepare_ray(p, Projection::default(), 128).unwrap();
            let o = oracle::evaluate(
                p,
                Material::default(),
                Light::default(),
                Projection::default(),
                oracle::Config::default(),
            )
            .unwrap();
            for (i, value) in ray.iter().enumerate() {
                assert_eq!(
                    i128::from(*value),
                    o.stages
                        .iter()
                        .find(|(n, _)| n == &format!("ray.{i}"))
                        .unwrap()
                        .1
                );
            }
            assert_eq!(report.counts.resources[&audited::Resource::Dsp18], 2);
        }
    }
}

#[test]
fn exact_shared_half_has_checked_key_and_variable_normals() {
    let pixel = PixelInput {
        normal: [0, 0, 16384],
        ndc: [3086, -11420],
    };
    let c = counted::Config::architecture();
    let half =
        counted::prepare_half(pixel, Light::default(), Projection::default(), 2048, c).unwrap();
    half.ray_frame().audit().unwrap();
    half.half_frame().audit().unwrap();
    for normal in [
        [0, 0, 0],
        [32767, -32768, 15],
        [123, 567, 16384],
        [0, 0, -16384],
    ] {
        for code in [0, 8, 16] {
            let p = PixelInput { normal, ..pixel };
            let m = Material {
                shininess_code: code,
                ..Default::default()
            };
            let cached = counted::evaluate_reusing_half(
                p,
                m,
                Light::default(),
                Projection::default(),
                2048,
                c,
                &half,
            )
            .unwrap();
            let full = counted::evaluate_with_config(
                p,
                m,
                Light::default(),
                Projection::default(),
                2048,
                c,
            )
            .unwrap();
            assert_eq!(cached.output, full.output);
            assert!(cached
                .frame
                .outputs
                .iter()
                .all(|o| !o.name.starts_with("v.")));
        }
    }
    assert!(counted::evaluate_reusing_half(
        PixelInput {
            ndc: [3087, -11420],
            ..pixel
        },
        Material::default(),
        Light::default(),
        Projection::default(),
        2048,
        c,
        &half
    )
    .is_err());
    assert!(counted::evaluate_reusing_half(
        pixel,
        Material::default(),
        Light {
            direction: [0, 0, -16384],
            ..Light::default()
        },
        Projection::default(),
        2048,
        c,
        &half
    )
    .is_err());
}
#[test]
fn bounded_stream_preserves_ids_contexts_ce_and_outputs_across_modes() {
    let contexts = [
        StreamContext {
            material: Material::default(),
            light: Light::default(),
            projection: Projection::default(),
        },
        StreamContext {
            material: Material {
                specular_color: [0; 3],
                ..Default::default()
            },
            light: Light {
                ambient: 71,
                directional: 181,
                ..Light::default()
            },
            projection: Projection::default(),
        },
    ];
    let inputs: Vec<_> = (0..64)
        .map(|i| {
            (
                100 + i,
                PixelInput {
                    normal: [7123, -519, 13567],
                    ndc: [((i as i32 * 3000) % 131073 - 65536) / 4, 0],
                },
                if !(8..56).contains(&i) { 0 } else { 1 },
            )
        })
        .collect();
    let h = Hardware::lighting_architecture_ii2();
    let clean = stream(&inputs, &contexts, h, 10000, &Default::default()).unwrap();
    let pauses = (5..400)
        .filter(|i| i % 7 == 0 || (80..95).contains(i))
        .collect();
    let stalled = stream(&inputs, &contexts, h, 10000, &pauses).unwrap();
    assert_eq!(
        clean
            .results
            .iter()
            .map(|r| (r.id, r.accepted, r.completed, r.output))
            .collect::<Vec<_>>(),
        stalled
            .results
            .iter()
            .map(|r| (r.id, r.accepted, r.completed, r.output))
            .collect::<Vec<_>>()
    );
    assert!(stalled.trace.ticks.len() > clean.trace.ticks.len());
    for ((id, p, c), r) in inputs.iter().zip(&clean.results) {
        assert_eq!(*id, r.id);
        let ctx = contexts[*c];
        let o = oracle::evaluate(
            *p,
            ctx.material,
            ctx.light,
            ctx.projection,
            oracle::Config {
                rounding: oracle::RoundingPolicy {
                    power: oracle::Rounding::Floor,
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!((i128::from(r.output.g), i128::from(r.output.h)), (o.g, o.h));
    }
    clean.audit(&inputs, &contexts, h).unwrap();
    let mut tampered = stream(&inputs, &contexts, h, 10000, &Default::default()).unwrap();
    tampered.results[0].output.h ^= 1;
    assert!(tampered
        .audit(&inputs, &contexts, h)
        .unwrap_err()
        .contains("numerical"));
    let mut tampered = stream(&inputs, &contexts, h, 10000, &Default::default()).unwrap();
    tampered.results[0].accepted += 1;
    assert!(tampered.audit(&inputs, &contexts, h).is_err());
    assert!(clean.audit(&[], &contexts, h).is_err());
    let mut tampered = stream(&inputs, &contexts, h, 10000, &Default::default()).unwrap();
    tampered.results[0].accepted = u64::MAX;
    assert!(tampered.audit(&inputs, &contexts, h).is_err());
    for (small, large, reads, expected) in [(5, 5, 4, [3, 1]), (4, 4, 3, [4, 2])] {
        let hardware = Hardware {
            small_multiply: small,
            large_multiply: large,
            normalize_reads: reads,
            ..h
        };
        let run = stream(
            &inputs[..2],
            &contexts,
            hardware,
            10000,
            &Default::default(),
        )
        .unwrap();
        assert_eq!(
            run.calendars
                .iter()
                .map(|s| s.initiation_interval)
                .collect::<Vec<_>>(),
            expected
        );
        run.audit(&inputs[..2], &contexts, hardware).unwrap();
    }
    assert!(clean.results[8].accepted - clean.results[7].accepted > 2);
    assert!(clean.results[40..56]
        .windows(2)
        .all(|r| r[1].accepted == r[0].accepted + 1));
    assert!(stream(&inputs, &contexts, h, 8, &Default::default())
        .err()
        .unwrap()
        .contains("watchdog"));
}
#[test]
fn seeded_dataflow_vectors_and_cone_certificates_are_independent() {
    let mut random = support::Random(0x87312dbac);
    for _ in 0..128 {
        let p = PixelInput {
            normal: std::array::from_fn(|_| random.next() as i16),
            ndc: std::array::from_fn(|_| (random.next() % 32769) as i32 - 16384),
        };
        let l = Light {
            direction: random.direction(),
            ..Light::default()
        };
        let baseline = counted::evaluate_with_config(
            p,
            Material::default(),
            l,
            Projection::default(),
            2048,
            counted::Config::optimized(),
        )
        .unwrap();
        let exact = counted::evaluate_with_config(
            p,
            Material::default(),
            l,
            Projection::default(),
            2048,
            counted::Config::architecture(),
        )
        .unwrap();
        assert_eq!(baseline.output, exact.output);
    }
    let p = PixelInput {
        normal: [7123, -519, 13567],
        ndc: [5789, 3142],
    };
    let h = Hardware::lighting_architecture_ii2();
    let plan = plan(
        &[p],
        Material::default(),
        Light::default(),
        Projection::default(),
        h,
        Storage::Registers,
        Strategy::Interleaved,
    )
    .unwrap();
    let calendar = PeriodicSchedule::search(&plan, 2, 32).unwrap();
    let phy = calendar
        .audit_physical(
            &plan,
            audited::physical::GowinMemoryBudget {
                bsram_blocks: 46,
                ssram_cells: 2048,
            },
            &audited::lifecycle::RegisterBudget {
                total_bits: 1000000,
                by_width: Default::default(),
            },
        )
        .unwrap();
    assert!(phy.retained.peak_bits < 9693);
    assert!(!plan.logic_cones().is_empty());
    let mut broken = calendar.clone();
    let cone = &plan.logic_cones()[0];
    broken.slots[cone.absorbed_events[0]].issue += 1;
    broken.slots[cone.absorbed_events[0]].ready += 1;
    assert!(broken
        .audit_physical(
            &plan,
            audited::physical::GowinMemoryBudget {
                bsram_blocks: 46,
                ssram_cells: 2048
            },
            &audited::lifecycle::RegisterBudget {
                total_bits: 1000000,
                by_width: Default::default()
            }
        )
        .is_err());
}

#[test]
fn q28_scan_accumulator_replaces_per_pixel_multiply_without_drift() {
    for step in [-512, 0, 512] {
        let seed = PixelInput {
            normal: [0; 3],
            ndc: [(if step < 0 { 65536 } else { -65536 }) / 4, 3086],
        };
        let (rays, report) =
            counted::scanline_rays(seed, step, 64, Projection::default(), 2048).unwrap();
        assert_eq!(report.counts.resources[&audited::Resource::Dsp18], 3);
        for (i, ray) in rays.iter().enumerate() {
            let (individual, _) = counted::prepare_ray(
                PixelInput {
                    ndc: [seed.ndc[0] + i as i32 * step, seed.ndc[1]],
                    ..seed
                },
                Projection::default(),
                128,
            )
            .unwrap();
            assert_eq!(*ray, individual);
        }
    }
    let seed = PixelInput {
        normal: [0; 3],
        ndc: [16384, 0],
    };
    assert!(counted::scanline_rays(seed, 1, 2, Projection::default(), 128).is_err());
}

#[test]
fn flat_triangle_shares_n_nl_d_and_g_with_a_checked_owner() {
    for normal in [
        [0, 0, 0],
        [0, 0, 16384],
        [32767, -32768, 111],
        [0, 0, -16384],
    ] {
        let c = counted::Config::architecture();
        let flat = counted::prepare_flat(normal, Light::default(), 2048, c).unwrap();
        flat.frame().audit().unwrap();
        for ndc in [[0, 0], [-16384, 16384], [3086, -11420]] {
            let p = PixelInput { normal, ndc };
            let full = counted::evaluate_with_config(
                p,
                Material::default(),
                Light::default(),
                Projection::default(),
                2048,
                c,
            )
            .unwrap();
            let shared = counted::evaluate_reusing_flat(
                p,
                Material::default(),
                Light::default(),
                Projection::default(),
                2048,
                c,
                &flat,
            )
            .unwrap();
            assert_eq!(shared.output, full.output);
            assert!(!shared.frame.outputs.iter().any(|o| o.name == "n.q"));
            assert!(counted::evaluate_reusing_flat(
                PixelInput {
                    normal: [1, 1, 1],
                    ..p
                },
                Material::default(),
                Light::default(),
                Projection::default(),
                2048,
                c,
                &flat
            )
            .is_err());
        }
    }
    let p = PixelInput {
        normal: [0, 0, 16384],
        ndc: [0; 2],
    };
    let hardware = Hardware {
        kernel: counted::Config {
            flat_normal: true,
            ..counted::Config::architecture()
        },
        ..Hardware::lighting_architecture_ii2()
    };
    let material = Material {
        specular_color: [0; 3],
        ..Default::default()
    };
    let plan = plan(
        &[p],
        material,
        Light::default(),
        Projection::default(),
        hardware,
        Storage::Registers,
        Strategy::Interleaved,
    )
    .unwrap();
    let calendar = PeriodicSchedule::search(&plan, 1, 32).unwrap();
    assert!(calendar.latency < 10);
    assert_eq!(plan.work().get(&LaneKind::SmallMultiply), None);
    assert_eq!(plan.work().get(&LaneKind::LargeMultiply), None);
}
