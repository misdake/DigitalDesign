use gpu_v2::lighting::{ports::*, sim::timed::*};

fn pixels(n: usize) -> Vec<PixelInput> {
    (0..n)
        .map(|i| PixelInput {
            normal: [7123, -519, 13567],
            ndc: [i as i32 * 7919 % 131073 - 65536, 12345],
        })
        .collect()
}
fn run(n: usize, strategy: Strategy, hardware: Hardware, storage: Storage) -> Plan {
    let p = pixels(n);
    let r = plan(
        &p,
        Material::default(),
        Light::default(),
        Projection::default(),
        hardware,
        storage,
        strategy,
    )
    .unwrap();
    r.audit().unwrap();
    r.compare_oracle(
        &p,
        Material::default(),
        Light::default(),
        Projection::default(),
    )
    .unwrap();
    r
}
#[test]
fn interleaved_batches_preserve_outputs_and_fill_bubbles() {
    for n in [1, 2, 4, 8, 16] {
        let s = run(n, Strategy::Serial, Hardware::default(), Storage::Registers);
        let i = run(
            n,
            Strategy::Interleaved,
            Hardware::default(),
            Storage::Registers,
        );
        assert_eq!(s.outputs, i.outputs);
        assert!(i.cycles <= s.cycles);
        if n > 1 {
            assert!(i.cycles < s.cycles);
        }
        assert_eq!(i.work(), s.work());
        println!("batch={n} serial={} interleaved={}", s.cycles, i.cycles);
    }
}
#[test]
fn physical_rows_and_restricted_hardware_are_checked() {
    let h = Hardware {
        small_multiply: 1,
        large_multiply: 1,
        normalize_reads: 1,
        ..Hardware::default()
    };
    for lanes in [1, 2] {
        let r = run(
            8,
            Strategy::Interleaved,
            h,
            Storage::Rows {
                read_lanes: lanes,
                latency: 2,
            },
        );
        assert_eq!(r.source_reads, 28);
        assert_eq!(r.pixel_payload_bits, 672);
        println!("restricted lanes={lanes} cycles={}", r.cycles);
    }
}
#[test]
fn corrupted_reservations_are_rejected_without_panics() {
    let fresh = || {
        run(
            2,
            Strategy::Interleaved,
            Hardware::default(),
            Storage::Rows {
                read_lanes: 1,
                latency: 2,
            },
        )
    };
    let mut r = fresh();
    r.events[0].event = usize::MAX;
    assert!(r.audit().is_err());
    let mut r = fresh();
    r.events.pop();
    assert!(r.audit().is_err());
    let mut r = fresh();
    r.events.iter_mut().find(|r| r.lane.is_some()).unwrap().lane = Some(usize::MAX);
    assert!(r.audit().is_err());
    let mut r = fresh();
    r.reads[0].issue = u64::MAX;
    assert!(r.audit().is_err());
    let mut r = fresh();
    r.reads[1] = r.reads[0].clone();
    assert!(r.audit().is_err());
    let mut r = fresh();
    r.writes[1].pixel = 0;
    assert!(r.audit().is_err());
    let mut r = fresh();
    let id = r.events.iter().position(|r| r.kind.is_some()).unwrap();
    r.events[id].ready += 1;
    assert!(r.audit().is_err());
}
#[test]
fn modes_and_limits_have_explicit_boundaries() {
    for m in [
        Material {
            unlit: true,
            ..Material::default()
        },
        Material {
            specular_color: [0; 3],
            ..Material::default()
        },
    ] {
        let p = pixels(4);
        let r = plan(
            &p,
            m,
            Light::default(),
            Projection::default(),
            Hardware::default(),
            Storage::Registers,
            Strategy::Interleaved,
        )
        .unwrap();
        r.compare_oracle(&p, m, Light::default(), Projection::default())
            .unwrap();
    }
    for (p, h, s) in [
        (pixels(65), Hardware::default(), Storage::Registers),
        (
            pixels(1),
            Hardware {
                max_cycles: 2,
                ..Hardware::default()
            },
            Storage::Registers,
        ),
        (
            pixels(1),
            Hardware {
                large_multiply: 0,
                ..Hardware::default()
            },
            Storage::Registers,
        ),
        (
            pixels(1),
            Hardware::default(),
            Storage::Rows {
                read_lanes: 0,
                latency: 1,
            },
        ),
    ] {
        assert!(plan(
            &p,
            Material::default(),
            Light::default(),
            Projection::default(),
            h,
            s,
            Strategy::Interleaved
        )
        .is_err());
    }
}
#[test]
fn pixel_rows_roundtrip_boundaries_and_reject_unused_bits() {
    for ndc in [-65536, -1, 0, 1, 65536] {
        let p = PixelInput {
            normal: [i16::MIN, i16::MAX, -1],
            ndc: [ndc, -ndc],
        };
        let q = PixelRows::encode(p).unwrap().decode().unwrap();
        assert_eq!(p.normal, q.normal);
        assert_eq!(p.ndc, q.ndc);
    }
    assert!(PixelRows([1 << 32, 0, 0]).decode().is_err());
    assert!(PixelRows([0, 0, 65537]).decode().is_err());
}
#[test]
fn bounded_search_preserves_work_and_independently_checks_candidates() {
    for n in [1, 4, 8, 16] {
        let base = run(
            n,
            Strategy::Interleaved,
            Hardware::default(),
            Storage::Rows {
                read_lanes: 2,
                latency: 2,
            },
        );
        let cycles = base.cycles;
        let work = base.work();
        let outputs = base.outputs.clone();
        let (best, search) = base.optimize(16).unwrap();
        assert_eq!(best.work(), work);
        assert_eq!(best.outputs, outputs);
        assert!(best.cycles <= cycles);
        assert_eq!(search.candidates.len(), 16);
        println!(
            "search batch={n} baseline={cycles} best={} strategy={} pressure={}",
            best.cycles,
            search.best_candidate().label,
            search.best_candidate().live_pressure
        );
    }
}

#[test]
fn dsp_binding_covers_dot_groups_and_preserves_multiplier_budget() {
    let old = run(
        8,
        Strategy::Interleaved,
        Hardware::default(),
        Storage::Registers,
    );
    let h = Hardware::lighting_dsp();
    assert_eq!(
        h.multiplier_half_slots(),
        Hardware::default().multiplier_half_slots()
    );
    assert_eq!(
        h.multiplier_macros(),
        Hardware::default().multiplier_macros()
    );
    let new = run(8, Strategy::Interleaved, h, Storage::Registers);
    assert_eq!(old.outputs, new.outputs);
    assert_eq!(new.fused_groups().len(), 2);
    assert_eq!(new.work().get(&LaneKind::PairMultiplyAdd), Some(&16));
    assert_eq!(new.work().get(&LaneKind::Increment(18)), Some(&(34 * 8)));
    assert_eq!(new.work().get(&LaneKind::Add(18)), Some(&(42 * 8)));
    assert_eq!(new.work().get(&LaneKind::Add(36)), Some(&(15 * 8)));
    assert!(new.cycles < old.cycles);
    let (new, _) = new.optimize(16).unwrap();
    new.audit().unwrap();
}

#[test]
fn fused_profiles_match_goldens_for_boundaries_codes_and_modes() {
    let p = [
        PixelInput {
            normal: [0; 3],
            ndc: [0; 2],
        },
        PixelInput {
            normal: [i16::MIN, 0, 0],
            ndc: [-65536, 65536],
        },
        PixelInput {
            normal: [32767; 3],
            ndc: [65536, -65536],
        },
        PixelInput {
            normal: [0, 0, 4],
            ndc: [0, 0],
        },
        PixelInput {
            normal: [63, -64, 65],
            ndc: [12345, -54321],
        },
    ];
    for code in 0..17 {
        for direction in [[0, 0, 16384], [0, 0, -16384]] {
            let m = Material {
                shininess_code: code,
                ..Material::default()
            };
            let l = Light {
                direction,
                ..Light::default()
            };
            let r = plan(
                &p,
                m,
                l,
                Projection::default(),
                Hardware::lighting_dsp(),
                Storage::Rows {
                    read_lanes: 2,
                    latency: 2,
                },
                Strategy::Interleaved,
            )
            .unwrap();
            r.compare_oracle(&p, m, l, Projection::default()).unwrap();
        }
    }
    for m in [
        Material {
            unlit: true,
            ..Material::default()
        },
        Material {
            specular_color: [0; 3],
            ..Material::default()
        },
    ] {
        let r = plan(
            &p,
            m,
            Light::default(),
            Projection::default(),
            Hardware::lighting_dsp(),
            Storage::Registers,
            Strategy::Interleaved,
        )
        .unwrap();
        r.compare_oracle(&p, m, Light::default(), Projection::default())
            .unwrap();
        assert_eq!(r.fused_groups().len(), usize::from(!m.unlit));
    }
}

#[test]
fn fused_binding_rejects_independent_product_escape_and_timing_tampering() {
    let fresh = || {
        run(
            1,
            Strategy::Interleaved,
            Hardware::lighting_dsp(),
            Storage::Registers,
        )
    };
    let mut p = fresh();
    let g = p.fused_groups()[0].clone();
    // An absorbed product is not physically available before macro completion.
    let e = g.absorbed_events[0];
    p.events[e].issue = 0;
    p.events[e].ready = 0;
    assert!(p.audit().is_err());
    let mut p = fresh();
    let g = p.fused_groups()[0].clone();
    let value = p.template.events[g.absorbed_events[0]].output.unwrap();
    let mut observation = p.template.outputs[0].clone();
    observation.name = "escaped-product".into();
    observation.value = value;
    observation.format = p.template.values[value].format;
    observation.raw = p.template.values[value].raw;
    p.template.outputs.push(observation);
    assert!(p.audit().is_err());
}

#[test]
fn ii2_periodic_calendar_preserves_budget_dependencies_and_output_spacing() {
    let h = Hardware::lighting_ii2();
    assert_eq!(
        h.multiplier_half_slots(),
        Hardware::default().multiplier_half_slots()
    );
    assert_eq!(
        h.multiplier_macros(),
        Hardware::default().multiplier_macros()
    );
    let reference = run(1, Strategy::Interleaved, h, Storage::Registers);
    let periodic = PeriodicSchedule::search(&reference, 2, 16).unwrap();
    periodic.audit(&reference).unwrap();
    for n in [1, 4, 16, 64] {
        let p = run(n, Strategy::Interleaved, h, Storage::Registers);
        let outputs = p.outputs.clone();
        let expanded = periodic.expand(p).unwrap();
        assert_eq!(expanded.outputs, outputs);
        assert_eq!(expanded.cycles, periodic.latency + 2 * (n as u64 - 1));
        assert!(expanded
            .writes
            .windows(2)
            .all(|w| w[1].ready == w[0].ready + 2));
        expanded.audit().unwrap();
    }
}

#[test]
fn periodic_checker_rejects_cross_iteration_collision_and_undersized_hardware() {
    let old = run(
        1,
        Strategy::Interleaved,
        Hardware::lighting_dsp(),
        Storage::Registers,
    );
    assert!(PeriodicSchedule::search(&old, 2, 8)
        .unwrap_err()
        .contains("capacity"));
    let p = run(
        1,
        Strategy::Interleaved,
        Hardware::lighting_ii2(),
        Storage::Registers,
    );
    let good = PeriodicSchedule::search(&p, 2, 8).unwrap();
    let mut bad = good.clone();
    let ids: Vec<_> = bad
        .slots
        .iter()
        .filter(|s| s.kind == Some(LaneKind::SmallMultiply))
        .map(|s| s.event)
        .collect();
    let a = bad.slots[ids[0]].clone();
    let b = &mut bad.slots[ids[1]];
    // Distinct local cycles may still alias a lane when iterations overlap.
    b.lane = a.lane;
    b.issue = a.issue + 2;
    b.ready = a.ready + 2;
    assert_eq!(bad.audit(&p).unwrap_err(), "periodic lane collision");
    let mut bad = good.clone();
    bad.write_issue = 0;
    bad.latency = 1;
    assert_eq!(bad.audit(&p).unwrap_err(), "periodic result latency");
    assert!(PeriodicSchedule::search(&p, 1, 8).is_err());
    assert!(PeriodicSchedule::search(&p, 0, 8).is_err());
    assert!(PeriodicSchedule::search(&p, 2, 65).is_err());
}

#[test]
fn periodic_boundary_codes_and_short_modes_match_oracle_at_fixed_ii() {
    let pixels = [
        PixelInput {
            normal: [0; 3],
            ndc: [0; 2],
        },
        PixelInput {
            normal: [32767; 3],
            ndc: [65536, -65536],
        },
        PixelInput {
            normal: [i16::MIN, 0, 0],
            ndc: [-65536, 65536],
        },
        PixelInput {
            normal: [0, 0, 4],
            ndc: [0; 2],
        },
    ];
    let mut materials: Vec<_> = (0..17)
        .map(|shininess_code| Material {
            shininess_code,
            ..Material::default()
        })
        .collect();
    materials.extend([
        Material {
            unlit: true,
            ..Material::default()
        },
        Material {
            specular_color: [0; 3],
            ..Material::default()
        },
    ]);
    for m in materials {
        for direction in [[0, 0, 16384], [0, 0, -16384]] {
            let l = Light {
                direction,
                ..Light::default()
            };
            let p = plan(
                &pixels,
                m,
                l,
                Projection::default(),
                Hardware::lighting_ii2(),
                Storage::Registers,
                Strategy::Interleaved,
            )
            .unwrap();
            let calendar = PeriodicSchedule::search(&p, 2, 4).unwrap();
            let expanded = calendar.expand(p).unwrap();
            expanded
                .compare_oracle(&pixels, m, l, Projection::default())
                .unwrap();
            assert!(expanded
                .writes
                .windows(2)
                .all(|w| w[1].ready == w[0].ready + 2));
        }
    }
    let rows = run(
        1,
        Strategy::Interleaved,
        Hardware::lighting_ii2(),
        Storage::Rows {
            read_lanes: 2,
            latency: 1,
        },
    );
    assert!(PeriodicSchedule::search(&rows, 2, 4)
        .unwrap_err()
        .contains("register inputs"));
}

#[test]
fn optimized_ii2_avoids_secondary_abs_and_preserves_macro_budget() {
    let h = Hardware::lighting_optimized_ii2();
    let old = Hardware::lighting_ii2();
    assert_eq!(h.multiplier_half_slots(), old.multiplier_half_slots());
    assert_eq!(h.multiplier_macros(), old.multiplier_macros());
    let p = run(64, Strategy::Interleaved, h, Storage::Registers);
    let work = p.work();
    assert_eq!(work.get(&LaneKind::Add(18)), Some(&(33 * 64)));
    assert_eq!(work.get(&LaneKind::Increment(18)), Some(&(33 * 64)));
    assert_eq!(work.get(&LaneKind::Compare(18)), Some(&(45 * 64)));
    assert_eq!(work.get(&LaneKind::Select(18)), Some(&(61 * 64)));
    assert_eq!(work.get(&LaneKind::SmallMultiply), Some(&(14 * 64)));
    let calendar = PeriodicSchedule::search(&p, 2, 16).unwrap();
    let mut expanded = calendar.expand(p).unwrap();
    assert!(expanded
        .writes
        .windows(2)
        .all(|w| w[1].ready == w[0].ready + 2));
    expanded.audit().unwrap();
    expanded.hardware.kernel = gpu_v2::lighting::sim::counted::Config::default();
    assert_eq!(expanded.audit().unwrap_err(), "kernel config certificate");
}
