use gpu_v2::lighting::{
    ports::*,
    sim::{counted, oracle, timed::*},
};

const MAX_EVENTS: usize = 4096;
fn inputs(n: usize) -> Vec<CompactPixelInput> {
    (0..n)
        .map(|i| CompactPixelInput {
            normal: [[-2048, 2047, 1], [377, -286, 939], [1, 0, -1], [0; 3]][i % 4],
            ndc: [(i as i32 * 7919 % 131073) - 65536, 31457],
        })
        .collect()
}
fn material(full: bool) -> Material {
    Material {
        specular_color: if full { [255; 3] } else { [0; 3] },
        ..Default::default()
    }
}
fn run(p: &[CompactPixelInput], full: bool, hardware: Hardware, storage: Storage) -> Plan {
    plan_compact(
        p,
        material(full),
        Light::default(),
        Projection::default(),
        hardware,
        storage,
        Strategy::Interleaved,
    )
    .unwrap()
}
fn stages(r: &counted::Report) -> Vec<(String, i128)> {
    r.frame
        .outputs
        .iter()
        .map(|v| (v.name.clone(), v.raw))
        .collect()
}

#[test]
fn exact_narrow_prescale_preserves_all_stages_and_removes_normal_rounding() {
    // Every exponent boundary, both signs, the signed minimum and off-axis tails.
    let mut vectors = vec![[0; 3], [-2048, 2047, 0], [2047, -2048, 17]];
    for exponent in 0..12 {
        for delta in [-1, 0, 1] {
            let m = ((1 << exponent) + delta).clamp(1, 2047) as i16;
            for sign in [-1, 1] {
                vectors.push([m * sign, -m / 3, 1]);
            }
        }
    }
    for normal in vectors {
        let p = CompactPixelInput {
            normal,
            ndc: [-17329, 65536],
        }
        .expanded()
        .unwrap();
        for full in [false, true] {
            for (scalar, scalar_normal) in [(false, false), (true, false), (true, true)] {
                let kernel = counted::Config {
                    compact_normal: true,
                    block_prescale: scalar,
                    scalar_norm: scalar,
                    scalar_normal,
                    ..counted::Config::architecture()
                };
                let before = counted::evaluate_with_config(
                    p,
                    material(full),
                    Light::default(),
                    Projection::default(),
                    MAX_EVENTS,
                    kernel,
                )
                .unwrap();
                let after = counted::evaluate_with_config(
                    p,
                    material(full),
                    Light::default(),
                    Projection::default(),
                    MAX_EVENTS,
                    counted::Config {
                        compact_prescale: true,
                        ..kernel
                    },
                )
                .unwrap();
                after.frame.audit().unwrap();
                assert_eq!(stages(&after), stages(&before));
                let golden = oracle::evaluate(
                    p,
                    material(full),
                    Light::default(),
                    Projection::default(),
                    oracle::Config {
                        scalar_norm: scalar,
                        scalar_normal,
                        rounding: oracle::RoundingPolicy {
                            power: oracle::Rounding::Floor,
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                )
                .unwrap();
                assert_eq!(stages(&after), golden.stages);
                let prescaled: Vec<_> = after
                    .frame
                    .values
                    .iter()
                    .filter(|v| {
                        v.name
                            .as_deref()
                            .is_some_and(|s| s.starts_with("normal.compact.prescaled."))
                    })
                    .collect();
                assert_eq!(prescaled.len(), 3);
                assert!(prescaled
                    .iter()
                    .all(|v| v.format.bits == 13 && v.format.signed));
                let rounds = |r: &counted::Report| {
                    r.frame
                        .events
                        .iter()
                        .filter(|e| matches!(e.resource, Some(audited::Resource::RoundControl(_))))
                        .count()
                };
                assert!(rounds(&after) < rounds(&before));
            }
        }
    }
    assert!(counted::evaluate_with_config(
        inputs(1)[0].expanded().unwrap(),
        material(true),
        Light::default(),
        Projection::default(),
        MAX_EVENTS,
        counted::Config {
            compact_prescale: true,
            ..Default::default()
        }
    )
    .is_err());
}

#[test]
fn demand_rows_are_mode_specific_and_corrupted_source_calendars_are_rejected() {
    let p = inputs(8);
    let hardware = Hardware {
        kernel: counted::Config {
            compact_prescale: true,
            ..counted::Config::optimized()
        },
        ..Hardware::lighting_optimized_ii2()
    };
    for lanes in [1, 2] {
        for order in [ReadOrder::PixelMajor, ReadOrder::NormalFirst] {
            for mode in 0..4 {
                let m = Material {
                    unlit: mode == 0,
                    specular_color: if mode == 3 { [255; 3] } else { [0; 3] },
                    ..Default::default()
                };
                let l = Light {
                    directional: if mode == 1 { 0 } else { 224 },
                    ..Default::default()
                };
                let make = || {
                    plan_compact(
                        &p,
                        m,
                        l,
                        Projection::default(),
                        hardware,
                        Storage::DemandRows {
                            read_lanes: lanes,
                            latency: 2,
                            order,
                        },
                        Strategy::Interleaved,
                    )
                    .unwrap()
                };
                let candidate = make();
                candidate.audit().unwrap();
                candidate
                    .compare_oracle(
                        &p.iter().map(|p| p.expanded().unwrap()).collect::<Vec<_>>(),
                        m,
                        l,
                        Projection::default(),
                    )
                    .unwrap();
                let rows = if mode < 2 {
                    0
                } else if mode == 2 {
                    1
                } else {
                    2
                };
                assert_eq!(candidate.source_reads, 4 + rows * p.len());
                assert!(candidate
                    .reads
                    .iter()
                    .filter(|r| r.pixel.is_some())
                    .all(|r| r.row < rows));
                if mode == 3 {
                    let mut bad = make();
                    bad.reads.last_mut().unwrap().row = 0; // duplicate normal, missing NDC
                    assert!(bad.audit().is_err());
                    let mut bad = make();
                    let r = bad
                        .reads
                        .iter()
                        .find(|r| r.pixel == Some(0) && r.row == 0)
                        .unwrap()
                        .clone();
                    let event = bad.events.iter_mut().find(|r| matches!(bad.template.events[r.event].operation, audited::Operation::Read { memory, row: 0 } if bad.template.memories[memory].name == "pixel.compact-rows")).unwrap();
                    event.issue = r.ready - 1;
                    event.ready = event.issue;
                    assert!(bad.audit().is_err());
                    let mut bad = make();
                    bad.reads[1] = bad.reads[0].clone();
                    assert!(bad.audit().is_err());
                }
            }
        }
    }
    assert!(plan(
        &[p[0].expanded().unwrap()],
        material(true),
        Light::default(),
        Projection::default(),
        Hardware::default(),
        Storage::DemandRows {
            read_lanes: 1,
            latency: 2,
            order: ReadOrder::PixelMajor
        },
        Strategy::Interleaved
    )
    .is_err());
}

#[test]
fn matched_batches_compare_width_read_order_and_bounded_search() {
    for n in [1, 4, 16] {
        let p = inputs(n);
        for full in [false, true] {
            let hardware = Hardware::lighting_optimized_ii2();
            let old = run(
                &p,
                full,
                hardware,
                Storage::Rows {
                    read_lanes: 1,
                    latency: 2,
                },
            );
            let hardware = Hardware {
                kernel: counted::Config {
                    compact_prescale: true,
                    ..hardware.kernel
                },
                ..hardware
            };
            let narrow = run(
                &p,
                full,
                hardware,
                Storage::Rows {
                    read_lanes: 1,
                    latency: 2,
                },
            );
            let demand = run(
                &p,
                full,
                hardware,
                Storage::DemandRows {
                    read_lanes: 1,
                    latency: 2,
                    order: ReadOrder::PixelMajor,
                },
            );
            let normal = run(
                &p,
                full,
                hardware,
                Storage::DemandRows {
                    read_lanes: 1,
                    latency: 2,
                    order: ReadOrder::NormalFirst,
                },
            );
            let two_ports = run(
                &p,
                full,
                hardware,
                Storage::DemandRows {
                    read_lanes: 2,
                    latency: 2,
                    order: ReadOrder::PixelMajor,
                },
            );
            let before = demand.cycles;
            let (search, _) = demand.optimize(8).unwrap();
            assert!(search.cycles <= before);
            for r in [&narrow, &search, &normal, &two_ports] {
                r.audit().unwrap();
                assert_eq!(old.outputs, r.outputs);
            }
            println!("batch n={n} full={full} old={} narrow={} demand={} normal_first={} two_ports={} searched={} reads_old={} reads_demand={}", old.cycles, narrow.cycles, before, normal.cycles, two_ports.cycles, search.cycles, old.source_reads, search.source_reads);
        }
    }
}

#[test]
fn narrow_periodic_calendars_keep_ii_and_measure_retained_values() {
    let p = inputs(1);
    for full in [false, true] {
        let ii = if full { 2 } else { 1 };
        let mut observations = Vec::new();
        for version in 0..3 {
            let hardware = Hardware {
                kernel: counted::Config {
                    compact_normal: version > 0,
                    compact_prescale: version > 1,
                    ..counted::Config::architecture()
                },
                ..Hardware::lighting_architecture_ii2()
            };
            let plan = plan(
                &[p[0].expanded().unwrap()],
                material(full),
                Light::default(),
                Projection::default(),
                hardware,
                Storage::Registers,
                Strategy::Interleaved,
            )
            .unwrap();
            let raw_calendar = PeriodicSchedule::search(&plan, ii, 16).unwrap();
            let raw_bits = raw_calendar.retained_values(&plan).unwrap().peak_bits;
            let alap_bits = raw_calendar
                .clone()
                .compact_lifetimes(&plan)
                .unwrap()
                .retained_values(&plan)
                .unwrap()
                .peak_bits;
            assert!(raw_calendar
                .clone()
                .compact_storage_bounded(&plan, 0)
                .is_err());
            assert!(raw_calendar
                .clone()
                .compact_storage_bounded(&plan, 257)
                .is_err());
            let capture_ages: Vec<_> = raw_calendar
                .slots
                .iter()
                .map(|s| (s.issue, s.lane))
                .collect();
            let before_commit = raw_calendar.write_issue;
            let calendar = raw_calendar.compact_storage_bounded(&plan, 128).unwrap();
            assert_eq!(calendar.write_issue, before_commit);
            for (id, slot) in calendar.slots.iter().enumerate() {
                if slot.kind.is_some() {
                    assert_eq!(slot.issue % ii, capture_ages[id].0 % ii);
                    assert_eq!(slot.lane, capture_ages[id].1);
                } else if matches!(
                    plan.template.events[id].operation,
                    audited::Operation::Read { .. }
                ) {
                    assert_eq!(slot.issue, capture_ages[id].0);
                }
            }
            let phy = calendar
                .audit_physical(
                    &plan,
                    audited::physical::GowinMemoryBudget {
                        bsram_blocks: 46,
                        ssram_cells: 2048,
                    },
                    &audited::lifecycle::RegisterBudget {
                        total_bits: 1_000_000,
                        by_width: Default::default(),
                    },
                )
                .unwrap();
            assert!(phy.retained.peak_bits <= raw_bits);
            assert!(phy.retained.peak_bits <= alap_bits);
            println!("periodic full={full} version={version} ii={ii} latency={} retained={} raw={} alap={} bsram={} ssram={}", calendar.latency, phy.retained.peak_bits, raw_bits, alap_bits, phy.memory_cells.bsram_blocks, phy.memory_cells.ssram_cells);
            println!(
                "work full={full} version={version} resources={:?}",
                plan.template.counts.resources
            );
            let adders = calendar.adder_inventory(&plan).unwrap();
            println!(
                "adders full={full} version={version} normal={:?} increment_or_negate={:?}",
                adders.normal_sites_by_width, adders.increment_sites_by_width
            );
            if version == 2 {
                for output in &plan.template.outputs {
                    println!(
                        "stage full={full} name={} ready={}",
                        output.name,
                        calendar.slots[plan.template.values[output.value].producer].ready
                    );
                }
            }
            observations.push((calendar.latency, phy.retained.peak_bits));
        }
        assert!(observations[2].0 <= observations[1].0);
        // Different modulo phases can change the peak; report it rather than
        // assuming every local width reduction must reduce aggregate storage.
    }
}
