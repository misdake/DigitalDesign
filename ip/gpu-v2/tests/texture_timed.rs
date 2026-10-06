#[path = "support/sdram/cycle.rs"]
mod cycle;
#[path = "support/texture.rs"]
mod support;
use digital_design_hardware_gowin::sdram_memory_controller::{
    ports::{OracleImage, Service},
    sim::{average, oracle as memory, traffic::*},
};
use gpu_v2::texture::{
    ports::*,
    sim::{oracle, timed::*},
};
use support::*;

fn image(bytes: &[u8]) -> OracleImage {
    // SAFETY: independent external input asset; never an audited intermediate.
    unsafe {
        OracleImage::from_host(u64::from(BASE), bytes.to_vec(), "timed RAW565 asset").unwrap()
    }
}
fn source(bytes: &[u8]) -> cycle::Adapter<average::Memory> {
    cycle::Adapter::new(
        average::Memory::new(
            image(bytes),
            average::Profile::gpu_default().unwrap(),
            Default::default(),
        )
        .unwrap(),
    )
}
fn expected(inputs: &[QuadInput], slot: Slot, bytes: &[u8]) -> Vec<PixelResult> {
    let mut cache = oracle::Cache::new(vec![slot]).unwrap();
    let mut asset = Image {
        bytes: bytes.to_vec(),
        requests: vec![],
    };
    inputs
        .iter()
        .flat_map(|q| {
            oracle::sample(q, &mut cache, &mut asset, Config::counted())
                .unwrap()
                .pixels
                .into_iter()
                .map(|p| PixelResult {
                    quad_id: q.quad_id,
                    lane: p.lane,
                    rgb: p.rgb,
                })
        })
        .collect()
}
fn workload(n: u8, count: usize) -> Vec<QuadInput> {
    (0..count)
        .map(|i| {
            let mut q = input(
                n,
                [Filter::Nearest, Filter::Bilinear, Filter::Trilinear][i % 3],
                [i as f64 * 0.034 - 0.005, (i % 9) as f64 * 0.039 - 0.005],
            );
            q.quad_id = (i % 16) as u8;
            q.mask = [15, 9, 6, 1, 0][i % 5];
            q.uv[1][0] += 1.0 / (1_u32 << n) as f64;
            q.uv[2][1] += 1.0 / (1_u32 << n) as f64;
            q.uv[3][0] += 1.0 / (1_u32 << n) as f64;
            q.uv[3][1] += 1.0 / (1_u32 << n) as f64;
            q.lod_bias = [0.0, 0.5, 3.7, -2.0][i % 4];
            q
        })
        .collect()
}
#[test]
fn reserved_and_prepared_cycle_execution_match_oracle() {
    for n in [0, 1, 3, 5, 9, 10] {
        for mip in [false, true] {
            let slot = slot(n, mip);
            let bytes = asset(slot, pattern);
            let inputs = workload(n, 18);
            let golden = expected(&inputs, slot, &bytes);
            for preparation in [PreparationMode::Reserved, PreparationMode::PreparedGroups] {
                let mut memory = source(&bytes);
                let h = Hardware {
                    preparation,
                    ..Default::default()
                };
                let report = run(&inputs, &[slot], &mut memory, h, |_| Control::default())
                    .unwrap_or_else(|e| panic!("n={n} mip={mip} mode={preparation:?}: {e:?}"));
                assert_eq!(
                    report.pixels, golden,
                    "n={n} mip={mip} mode={preparation:?}"
                );
                assert!(memory.service.idle());
                assert_eq!(report.stats.beats, report.stats.refills * 16);
                assert_eq!(memory.requests.len() as u64, report.stats.refills);
                assert!(memory
                    .requests
                    .iter()
                    .all(|&(_, a, b)| a & 127 == 0 && b == 128));
            }
        }
    }
}
#[test]
fn loaded_service_backpressure_and_ce_keep_inflight_beats_and_results() {
    let slot = slot(9, true);
    let bytes = asset(slot, pattern);
    let inputs = workload(9, 32);
    let golden = expected(&inputs, slot, &bytes);
    for chain in [
        ChainPolicy::ExistingUnchained,
        ChainPolicy::ChainedCandidate,
    ] {
        for load in [
            Load::solo(),
            Load::display(1),
            Load::cpu(),
            Load::display_and_cpu(50),
        ] {
            let model = memory::Memory::new(
                image(&bytes),
                memory::Config {
                    load,
                    chain,
                    ..Default::default()
                },
            )
            .unwrap();
            let mut service = cycle::Adapter::new(model);
            let h = Hardware {
                preparation: PreparationMode::PreparedGroups,
                result_capacity: 1,
                read_latency: 3,
                ..Default::default()
            };
            let report = run(&inputs, &[slot], &mut service, h, |c| Control {
                ce: c % 11 >= 3,
                result_ready: c > 700 && c % 19 >= 7,
            })
            .unwrap();
            assert_eq!(report.pixels, golden);
            assert!(
                report
                    .steps
                    .iter()
                    .any(|s| !s.control.ce
                        && s.events.iter().any(|e| matches!(e, Event::Beat { .. })))
            );
            assert!(report.stats.result_credit_stalls > 0);
            assert!(report.stats.peak_results <= 1);
            assert!(service.service.idle());
        }
    }
}

#[test]
fn queue_pressure_prefetch_overlap_and_same_set_replacement() {
    let slot = slot(9, true);
    let bytes = asset(slot, pattern);
    // Repeating boundary quads: 32 real groups, eight keys, shared sets across mips.
    let mut seam = input(9, Filter::Trilinear, [0.0; 2]);
    seam.uv[3][0] = 1.0 / 65536.0;
    seam.lod_bias = 7.5;
    let inputs: Vec<_> = (0..24)
        .map(|i| {
            let mut q = seam.clone();
            q.quad_id = (i % 16) as u8;
            q
        })
        .collect();
    let golden = expected(&inputs, slot, &bytes);
    for group_capacity in [16, 32] {
        for prefetch in [false, true] {
            let mut service = source(&bytes);
            let h = Hardware {
                preparation: PreparationMode::PreparedGroups,
                group_capacity,
                prefetch,
                ..Default::default()
            };
            let report = run(&inputs, &[slot], &mut service, h, |_| Control::default()).unwrap();
            assert_eq!(report.pixels, golden);
            assert_eq!(report.stats.reads, 24 * 32);
            assert_eq!(report.stats.refills, 8);
            assert_eq!(report.stats.peak_groups, group_capacity);
            assert!(report.stats.producer_stalls > 0);
            if prefetch {
                assert!(report.stats.hits_during_refill > 0);
                assert!(report.stats.hints_dropped > 0);
            }
            println!(
                "seam capacity={group_capacity} prefetch={prefetch}: {:?}",
                report.stats
            );
        }
    }
    // More than four tile identities mapping to one set; demand rechecks queued keys.
    let conflict: Vec<_> = (0..40)
        .map(|i| {
            let mut q = input(9, Filter::Bilinear, [(i % 12 * 32) as f64 / 512.0, 0.0]);
            q.quad_id = (i % 16) as u8;
            q
        })
        .collect();
    let golden = expected(&conflict, slot, &bytes);
    for read_latency in [1, 4] {
        let mut service = source(&bytes);
        let report = run(
            &conflict,
            &[slot],
            &mut service,
            Hardware {
                preparation: PreparationMode::PreparedGroups,
                read_latency,
                result_capacity: 1,
                ..Default::default()
            },
            |c| Control {
                ce: c % 7 != 0,
                result_ready: c > 1000 && c % 23 < 4,
            },
        )
        .unwrap();
        assert_eq!(report.pixels, golden);
        assert!(report.stats.refills > 4);
        assert_eq!(report.stats.peak_descriptors, 4);
    }
}

#[test]
fn ready_hits_execute_on_actual_prefetch_refill_beat_edges() {
    let slot = slot(9, false);
    let bytes = asset(slot, pattern);
    let inputs: Vec<_> = (0..28)
        .map(|i| {
            let mut q = input(9, Filter::Nearest, [if i == 16 { 0.2 } else { 0.01 }, 0.01]);
            q.quad_id = (i % 16) as u8;
            q
        })
        .collect();
    let mut service = source(&bytes);
    let report = run(
        &inputs,
        &[slot],
        &mut service,
        Hardware {
            preparation: PreparationMode::PreparedGroups,
            result_capacity: 4,
            ..Default::default()
        },
        |_| Control::default(),
    )
    .unwrap();
    assert_eq!(report.pixels, expected(&inputs, slot, &bytes));
    let simultaneous: Vec<_> = report
        .steps
        .iter()
        .filter(|s| {
            s.events.iter().any(|e| matches!(e, Event::Beat { .. }))
                && s.events.iter().any(|e| matches!(e, Event::Read { .. }))
        })
        .collect();
    assert!(!simultaneous.is_empty(), "{:?}", report.stats);
    // The overlapping transaction was allocated from a hint, before its demand.
    let cold_key = report.programs[16].preparation().groups[0].key;
    assert!(report
        .steps
        .iter()
        .flat_map(|s| &s.events)
        .any(|e| matches!(e, Event::Allocate { key, prefetch: true, .. } if *key == cold_key)));
    for step in simultaneous {
        assert!(step.events.iter().any(|e| matches!(
            e,
            Event::Read {
                refill_overlap: true,
                ..
            }
        )));
    }
}

#[test]
fn trace_and_reservation_certificates_reject_tampering() {
    let slot = slot(5, true);
    let bytes = asset(slot, pattern);
    let inputs = workload(5, 8);
    let mut service = source(&bytes);
    let mut report = run(
        &inputs,
        &[slot],
        &mut service,
        Hardware {
            preparation: PreparationMode::PreparedGroups,
            ..Default::default()
        },
        |_| Control::default(),
    )
    .unwrap();
    let at = report
        .steps
        .iter()
        .position(|s| s.events.iter().any(|e| matches!(e, Event::Captured { .. })))
        .unwrap();
    let saved = report.steps[at].clone();
    if let Some(Event::Captured { words, .. }) = report.steps[at]
        .events
        .iter_mut()
        .find(|e| matches!(e, Event::Captured { .. }))
    {
        words[0] ^= 1;
    }
    assert!(report.audit().is_err());
    report.steps[at] = saved.clone();
    report.steps[at].control.ce = false;
    assert!(report.audit().is_err());
    report.steps[at] = saved;
    report.stats.beats += 1;
    assert!(report.audit().is_err());
    report.stats.beats -= 1;
    let p = &report.programs[0];
    let mut plan = p.arithmetic().clone();
    let event = plan
        .schedule
        .nodes
        .iter()
        .position(|n| n.ready > n.issue)
        .unwrap();
    plan.schedule.nodes[event].ready += 1;
    assert!(plan
        .audit(&p.preparation().frame, &report.hardware)
        .is_err());
    let mut h = report.hardware.clone();
    h.preparation_register_bits = 1;
    assert!(p.arithmetic().audit(&p.preparation().frame, &h).is_err());
    report.audit().unwrap();
}

#[test]
fn fault_preserves_accepted_transaction_and_rebind_requires_drain() {
    struct Corrupt<S> {
        inner: cycle::Adapter<S>,
        armed: bool,
    }
    impl<S: Service> RefillPort for Corrupt<S> {
        fn submit_read(&mut self, a: u64, b: usize) -> Result<u64, String> {
            self.inner.submit_read(a, b)
        }
        fn step(&mut self) -> Result<Vec<RefillEvent>, String> {
            let mut events = self.inner.step()?;
            if self.armed {
                if let Some(RefillEvent::Beat { index, .. }) = events
                    .iter_mut()
                    .find(|e| matches!(e, RefillEvent::Beat { .. }))
                {
                    *index = 9;
                    self.armed = false;
                }
            }
            Ok(events)
        }
    }
    let slot = slot(5, true);
    let bytes = asset(slot, pattern);
    let h = Hardware {
        preparation: PreparationMode::PreparedGroups,
        ..Default::default()
    };
    let p = Program::compile(&input(5, Filter::Nearest, [0.0; 2]), &[slot], &h).unwrap();
    let key = p.preparation().groups[0].key;
    let mut machine = Machine::new(vec![slot], h.clone()).unwrap();
    let mut source = Corrupt {
        inner: source(&bytes),
        armed: true,
    };
    let mut accepted = false;
    let mut failed = false;
    for _ in 0..10000 {
        match machine.step(
            &mut source,
            (!accepted).then(|| (0, p.clone())),
            Control::default(),
        ) {
            Ok(s) => {
                accepted |= s.accepted;
                if accepted {
                    assert!(machine.rebind(vec![slot]).is_err());
                }
            }
            Err(_) => {
                failed = true;
                break;
            }
        }
    }
    assert!(failed && accepted && machine.faulted());
    assert_eq!(machine.lookup(key).unwrap().1, oracle::State::Filling);
    let requests = source.inner.requests.len();
    assert!(machine.step(&mut source, None, Control::default()).is_err());
    assert_eq!(source.inner.requests.len(), requests);
    assert!(machine.rebind(vec![slot]).is_err());
    // Accepted work belongs to Service; a sampler fault never cancels it.
    for _ in 0..10000 {
        if source.inner.service.idle() {
            break;
        }
        source.inner.service.step().unwrap();
    }
    assert!(source.inner.service.idle());
    let mut recovered = Machine::new(vec![slot], h).unwrap();
    recovered.rebind(vec![slot]).unwrap();
    let other = Slot {
        base_address: BASE + 128,
        ..slot
    };
    let mut source = source.inner;
    recovered.rebind(vec![other]).unwrap();
    assert!(recovered
        .step(&mut source, Some((0, p)), Control::default())
        .is_err());
}

#[test]
fn watchdog_bounds_permanent_stall_and_empty_work() {
    let slot = slot(0, false);
    let bytes = asset(slot, pattern);
    let mut service = source(&bytes);
    let mut q = input(0, Filter::Nearest, [0.0; 2]);
    q.mask = 0;
    let report = run(&[q], &[slot], &mut service, Hardware::default(), |_| {
        Control::default()
    })
    .unwrap();
    assert!(report.pixels.is_empty() && service.requests.is_empty());
    let mut service = source(&bytes);
    assert!(run(
        &[input(0, Filter::Nearest, [0.0; 2])],
        &[slot],
        &mut service,
        Hardware {
            max_cycles: 31,
            ..Default::default()
        },
        |_| Control {
            ce: false,
            result_ready: false
        }
    )
    .is_err());
    assert!(service.requests.is_empty());
}

#[test]
fn prefetch_pollution_requires_demand_to_recheck_queued_keys() {
    let slot = slot(9, false);
    let bytes = asset(slot, pattern);
    let inputs: Vec<_> = (0..48)
        .map(|i| {
            let x = if i == 0 { 0 } else { ((i - 1) % 8 + 1) * 32 };
            let mut q = input(9, Filter::Nearest, [x as f64 / 512.0 + 0.003, 0.003]);
            q.quad_id = (i % 16) as u8;
            q
        })
        .collect();
    let mut service = source(&bytes);
    let report = run(
        &inputs,
        &[slot],
        &mut service,
        Hardware {
            preparation: PreparationMode::PreparedGroups,
            result_capacity: 1,
            ..Default::default()
        },
        |c| Control {
            ce: true,
            result_ready: c > 500 && c % 50 < 5,
        },
    )
    .unwrap();
    assert_eq!(report.pixels, expected(&inputs, slot, &bytes));
    assert!(report.stats.refills > 9); // Nine unique tiles, then replacement/refetch.
    assert!(report.stats.result_credit_stalls > 0);
}

#[test]
fn configured_pipeline_latencies_and_capacities_are_checked() {
    let slot = slot(5, true);
    let bytes = asset(slot, pattern);
    let inputs = workload(5, 12);
    let golden = expected(&inputs, slot, &bytes);
    for (coefficient_lanes, multiply_latency, read_latency, descriptor_capacity) in
        [(1, 1, 1, 1), (3, 3, 2, 2), (4, 8, 4, 4)]
    {
        let h = Hardware {
            coefficient_lanes,
            multiply_latency,
            read_latency,
            descriptor_capacity,
            ..Default::default()
        };
        let mut service = source(&bytes);
        let report = run(&inputs, &[slot], &mut service, h, |c| Control {
            ce: c % 5 != 0,
            result_ready: c % 13 < 9,
        })
        .unwrap();
        assert_eq!(report.pixels, golden);
        assert!(report.stats.peak_descriptors <= descriptor_capacity);
    }
    let h = Hardware {
        preparation_register_bits: 1,
        ..Default::default()
    };
    assert!(Program::compile(&inputs[0], &[slot], &h).is_err());
    assert!(Hardware {
        descriptor_capacity: 5,
        ..Default::default()
    }
    .validate()
    .is_err());
}
