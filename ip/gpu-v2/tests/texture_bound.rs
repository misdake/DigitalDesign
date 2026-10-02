#[path = "support/sdram/physical_texture.rs"]
mod physical;
#[path = "support/texture.rs"]
#[allow(dead_code)]
mod support;
use gpu_v2::texture::{
    ports::*,
    sim::{oracle, staged::bound},
};
use support::*;
#[test]
fn universal_calendar_matches_numeric_shapes_and_conservative_lifetimes() {
    let b = bound::Binding::build().unwrap();
    for (name, p) in [
        ("D", &b.derivative),
        ("LOD", &b.lod),
        ("coord", &b.coordinate),
        ("coeff", &b.coefficient),
        ("plane", &b.plane),
        ("packet", &b.packet),
    ] {
        println!(
            "{name} II={} latency={} FF={} DSPreg={} RAM16={} sites={:?}",
            p.ii(),
            p.span(),
            p.fixed_ff_bits,
            p.dsp_pipeline_bits,
            p.rom_ram16_cells,
            p.sites
        );
    }
    for n in 0..=10 {
        for filter in [Filter::Nearest, Filter::Bilinear, Filter::Trilinear] {
            let mut q = input(n, filter, [0.99999, -0.0001]);
            q.uv[1][0] += 0.003;
            q.lod_bias = 0.5;
            let p = bound::prepare(&q, &[slot(n, true)]).unwrap();
            b.audit(&p).unwrap();
            let gold = oracle::prepare(&q, &[slot(n, true)], Config::counted()).unwrap();
            let packets: Vec<_> = gold
                .pixels
                .iter()
                .flat_map(|p| p.groups.iter().map(|g| g.pack72().unwrap() as i128))
                .collect();
            assert_eq!(p.payloads, packets);
        }
    }
    // Modes, missing mips and omitted planes must reuse one fixed calendar;
    // they cannot choose a shorter input-specific arithmetic schedule.
    for n in [0, 1, 3, 9, 10] {
        for mip in [false, true] {
            for mask in [0, 1, 9, 15] {
                for bias in [-32.0, 0.0, 0.5, 9.5, 32.0] {
                    let mut q = input(n, Filter::Trilinear, [0.0; 2]);
                    q.uv[3][0] += 1.0 / 262144.0;
                    q.mask = mask;
                    q.lod_bias = bias;
                    let s = slot(n, mip);
                    let p = bound::prepare(&q, &[s]).unwrap();
                    b.audit(&p).unwrap();
                    let g = oracle::prepare(&q, &[s], Config::counted()).unwrap();
                    assert_eq!(
                        p.payloads,
                        g.pixels
                            .iter()
                            .flat_map(|p| p.groups.iter())
                            .map(|g| g.pack72().unwrap() as i128)
                            .collect::<Vec<_>>()
                    );
                }
            }
        }
    }
}

#[test]
fn universal_binding_rejects_timing_storage_and_resource_mutations() {
    let mut q = input(9, Filter::Trilinear, [0.003; 2]);
    q.uv[1][0] += 1.0 / 512.0;
    q.lod_bias = 0.5;
    let p = bound::prepare(&q, &[slot(9, true)]).unwrap();
    let mut b = bound::Binding::build().unwrap();
    let b = std::sync::Arc::get_mut(&mut b).unwrap();
    b.audit(&p).unwrap();
    let original = b.lod.rom_ram16_cells;
    b.lod.rom_ram16_cells = 0;
    assert!(b.audit(&p).is_err());
    b.lod.rom_ram16_cells = original;
    b.packet.ff_banks[0].slots += 1;
    assert!(b.audit(&p).is_err());
    b.packet.ff_banks[0].slots -= 1;
    b.coordinate.times[0].ready += 1;
    assert!(b.audit(&p).is_err());
    b.coordinate.times[0].ready -= 1;
    b.coefficient.graph.resources[0].lanes += 1;
    assert!(b.audit(&p).is_err());
    b.coefficient.graph.resources[0].lanes -= 1;
    b.audit(&p).unwrap();
}
#[test]
fn bound_end_to_end_uses_serial_cycle_memory_and_commits_oracle_pixels() {
    use gpu_v2::texture::sim::{staged::bound::control::Hardware, timed};
    let s = slot(9, true);
    let bytes = asset(s, pattern);
    let inputs: Vec<_> = (0..16)
        .map(|i| {
            let mut q = input(
                9,
                [Filter::Nearest, Filter::Bilinear, Filter::Trilinear][i % 3],
                [i as f64 / 129.0, 0.001],
            );
            q.quad_id = (i % 4) as u8;
            q.mask = [15, 1, 6, 0][i % 4];
            q.uv[1][0] += 1.0 / 512.0;
            q.lod_bias = 0.5;
            q
        })
        .collect();
    let mut image = Image {
        bytes: bytes.clone(),
        requests: vec![],
    };
    let mut cache = oracle::Cache::new(vec![s]).unwrap();
    let expected: Vec<_> = inputs
        .iter()
        .flat_map(|q| {
            oracle::sample(q, &mut cache, &mut image, Config::counted())
                .unwrap()
                .pixels
                .into_iter()
                .map(|p| timed::PixelResult {
                    quad_id: q.quad_id,
                    lane: p.lane,
                    rgb: p.rgb,
                })
        })
        .collect();
    for (storage, loaded) in [
        (bound::control::Storage::Dedicated, false),
        (bound::control::Storage::Packed, false),
        (bound::control::Storage::Packed, true),
    ] {
        let mut memory = physical::Physical::new(u64::from(BASE), bytes.clone(), true, loaded);
        let mut report = bound::system::run(
            &inputs,
            &[s],
            &mut memory,
            Hardware {
                storage,
                ..Default::default()
            },
            timed::Hardware {
                prefetch: false,
                ..Default::default()
            },
            |c| timed::Control {
                ce: c % 23 > 7,
                result_ready: c > 200 && c % 19 > 3,
            },
        )
        .unwrap();
        assert_eq!(report.cache.pixels, expected);
        assert!(memory.requests > 0);
        assert!(report
            .cache
            .steps
            .iter()
            .any(|s| !s.control.ce && !s.responses.is_empty()));
        println!(
            "cycle serial loaded={loaded} init={} batch={} pixels={} refills={}",
            memory.init_cycles,
            report.cache.stats.wall_cycles,
            expected.len(),
            memory.requests
        );
        let at = report
            .preparation
            .iter()
            .position(|s| {
                s.events
                    .iter()
                    .any(|e| matches!(e, bound::control::Event::Packet { .. }))
            })
            .unwrap();
        report.preparation[at].ready = false;
        assert!(report.audit().is_err());
    }
}

#[test]
fn physical_bit_layout_rejects_alias_lifetime_address_and_bill_mutations() {
    let mut q = input(9, Filter::Trilinear, [0.003; 2]);
    q.uv[1][0] += 1.0 / 512.0;
    q.lod_bias = 0.5;
    let p = bound::prepare(&q, &[slot(9, true)]).unwrap();
    let mut binding = bound::Binding::build().unwrap();
    let b = std::sync::Arc::get_mut(&mut binding).unwrap();
    let original = b.packet.packed.clone();
    b.packet.packed.placements[0].low = b.packet.packed.ff_bits;
    assert!(b.audit(&p).is_err());
    b.packet.packed = original.clone();
    b.packet.packed.placements[0].live_phases = 0;
    assert!(b.audit(&p).is_err());
    b.packet.packed = original.clone();
    b.packet.packed.read_selector_tree_bits -= 1;
    assert!(b.audit(&p).is_err());
    b.packet.packed = original;
    b.packet.packed_fields[0].source_low += 1;
    assert!(b.audit(&p).is_err());
    b.packet.packed_fields[0].source_low -= 1;
    b.audit(&p).unwrap();
    let frame = &p.lanes[0].packets[0][0].frame;
    let mut replay = bound::storage::Replay::new(&b.packet);
    replay.issue(0, frame).unwrap();
    assert!(replay.issue(0, frame).is_err());
    replay.tick(0).unwrap();
    assert!(replay.tick(0).is_err()); // Owner checking catches duplicate writes even for identical values.
}

#[test]
fn physical_storage_rejects_unsorted_lookup_and_duplicate_idle_edges() {
    let binding = bound::Binding::build().unwrap();
    let plan = &binding.packet;
    let mut layout = plan.packed.clone();
    layout.placements.reverse();
    assert!(
        layout.audit(&plan.packed_fields, plan.ii()).is_err(),
        "binary-search placement certificate accepted an unsorted table"
    );

    let mut replay = bound::storage::Replay::new(plan);
    replay.tick(0).unwrap();
    assert!(
        replay.tick(0).is_err(),
        "an idle duplicate edge must also be rejected"
    );
    assert!(
        replay.tick(2).is_err(),
        "enabled edges cannot be silently skipped"
    );
    replay.tick(1).unwrap();
}
#[test]
fn bound_pipeline_preserves_masks_context_reuse_and_backpressure() {
    use bound::control::*;
    let b = bound::Binding::build().unwrap();
    let inputs: Vec<_> = (0..48)
        .map(|i| {
            let mut q = input(
                9,
                [Filter::Nearest, Filter::Bilinear, Filter::Trilinear][i % 3],
                [i as f64 / 65.0, 0.013],
            );
            q.quad_id = (i % 16) as u8;
            q.mask = [15, 1, 6, 0][i % 4];
            q.uv[1][0] += 1.0 / 512.0;
            q.lod_bias = 0.5;
            q
        })
        .collect();
    let programs: Vec<_> = inputs
        .iter()
        .map(|q| bound::Program::compile(q, &[slot(9, true)], b.clone()).unwrap())
        .collect();
    for release_after_capture in [false, true] {
        for (contexts, coordinate_credits, work_credits, packet_credits) in
            [(5, 6, 8, 16), (1, 1, 2, 1), (8, 16, 32, 32)]
        {
            let mut m = Machine::new(
                b.clone(),
                Hardware {
                    contexts,
                    coordinate_credits,
                    work_credits,
                    packet_credits,
                    release_after_capture,
                    ..Default::default()
                },
            )
            .unwrap();
            let mut next = 0;
            let mut words = vec![];
            while next < programs.len() || !m.idle() {
                let t = m.stats.cycles + 1;
                let s = m
                    .step(
                        programs.get(next).map(|p| (next, p.clone())),
                        t % 11 > 2,
                        t > 250 && t % 17 >= 5,
                    )
                    .unwrap();
                if s.accepted {
                    next += 1;
                }
                for e in s.events {
                    if let Event::Packet { payload, .. } = e {
                        words.push(payload);
                    }
                }
            }
            assert_eq!(
                words,
                programs
                    .iter()
                    .flat_map(|p| p.preparation().payloads.iter().copied())
                    .collect::<Vec<_>>()
            );
            assert_eq!(m.stats.released, 48);
            gpu_v2::texture::sim::timed::Hardware::default()
                .inventory()
                .unwrap()
                .audit_issues(&m.dsp_issues, None, 2_000_000)
                .unwrap();
        }
    }
}
