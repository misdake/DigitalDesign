use gpu_v2::{
    framebuffer::{ports as fb, sim::fixture::Fixture},
    lighting::ports::LightingOutput,
    system::pixel::*,
};
use std::collections::VecDeque;
#[path = "support/pixel.rs"]
mod support;

#[test]
fn final_unorm8_division_ties_saturation_and_invalid_intensities() {
    for tint in 0..=255 {
        for texture in 0..=255 {
            let light = LightingOutput { g: 256, h: 0 };
            assert_eq!(
                final_rgb([tint; 3], [texture; 3], light, [255; 3]).unwrap(),
                support::rgb([tint; 3], [texture; 3], light, [255; 3])
            );
        }
    }
    for g in 0..=511 {
        for h in [0, 1, 127, 128, 256] {
            for value in [0, 1, 3, 127, 128, 254, 255] {
                let light = LightingOutput { g, h };
                assert_eq!(
                    final_rgb([value; 3], [255; 3], light, [173; 3]).unwrap(),
                    support::rgb([value; 3], [255; 3], light, [173; 3])
                );
            }
        }
    }
    assert_eq!(
        final_rgb([1; 3], [255; 3], LightingOutput { g: 128, h: 0 }, [0; 3]).unwrap(),
        [0; 3]
    );
    assert_eq!(
        final_rgb([3; 3], [255; 3], LightingOutput { g: 128, h: 0 }, [0; 3]).unwrap(),
        [2; 3]
    );
    assert!(final_rgb([0; 3], [0; 3], LightingOutput { g: 512, h: 0 }, [0; 3]).is_err());
    assert!(final_rgb([0; 3], [0; 3], LightingOutput { g: 0, h: 257 }, [0; 3]).is_err());
}
#[test]
fn bounded_join_defaults_wrap_ce_backpressure_and_full_memory_golden() {
    let inputs = support::synthetic(80);
    for (alpha, blend) in [
        (0, fb::Blend::SrcOver),
        (255, fb::Blend::Replace),
        (97, fb::Blend::SrcOver),
    ] {
        let context = Context {
            alpha,
            rop: fb::Context {
                blend,
                ..support::context().rop
            },
            ..support::context()
        };
        let initial = support::image();
        let expected = support::golden(initial.clone(), &inputs, context);
        let mut memory = Fixture::new(initial);
        memory.request_period = 5;
        memory.beat_period = 3;
        memory.ack_delay = 17;
        let mut model = Model::new(context, 100_000).unwrap();
        let proof = support::replay(&mut model, &mut memory, &inputs, true, 100_000);
        assert_eq!(memory.bytes, expected);
        assert!(memory.idle());
        assert_eq!(model.stats.peak_live, 16);
        assert!(model.stats.admitted > 3 * 16 && model.stats.dropped > 0);
        assert_eq!(model.stats.retired, model.stats.admitted);
        assert!(proof.ce_local_return && proof.ce_memory_return && proof.out_of_order);
        assert!(proof.output_stall && proof.captured_before_commit);
        let lights: u64 = inputs
            .iter()
            .filter(|s| !s.quad.default_light)
            .map(|s| u64::from(s.quad.header.mask.count_ones()))
            .sum();
        let samples: u64 = inputs
            .iter()
            .filter(|s| !s.quad.default_sample)
            .map(|s| u64::from(s.quad.header.mask.count_ones()))
            .sum();
        assert_eq!(model.stats.light_reads, lights);
        assert_eq!(model.stats.light_writes, lights);
        assert_eq!(model.stats.sample_reads, samples);
        assert_eq!(model.stats.sample_writes, samples);
        assert_eq!(
            model.stats.basic_reads,
            inputs
                .iter()
                .map(|s| u64::from(s.quad.header.mask.count_ones()) * 2)
                .sum::<u64>()
        );
    }
}
fn admitted() -> (Model, Fixture, Ticket) {
    let mut model = Model::new(support::context(), 10_000).unwrap();
    let mut memory = Fixture::new(support::image());
    let quad = support::synthetic(3)[2].quad; // mask6, neither branch bypassed below.
    let quad = QuadInput {
        default_light: false,
        default_sample: false,
        ..quad
    };
    let c = model
        .step(
            Tick {
                quad: Some(quad),
                ..Default::default()
            },
            &mut memory,
        )
        .unwrap();
    (model, memory, c.ticket.unwrap())
}
#[test]
fn invalid_stale_uncovered_and_duplicate_results_reject_without_done() {
    for kind in 0..6 {
        let (mut model, mut memory, ticket) = admitted();
        let key = PixelKey { ticket, lane: 1 };
        let good = LightWrite {
            key,
            value: LightingOutput { g: 256, h: 0 },
        };
        let mut bad = good;
        let expected = match kind {
            0 => {
                bad.key.ticket.serial += 16;
                "pixel result stale/unallocated ticket"
            }
            1 => {
                bad.key.lane = 0;
                "pixel result uncovered lane"
            }
            2 => {
                bad.key.lane = 4;
                "pixel result key bounds"
            }
            3 => {
                bad.value.g = 512;
                "pixel light range"
            }
            4 => {
                bad.value.h = 257;
                "pixel light range"
            }
            _ => {
                assert!(
                    model
                        .step(
                            Tick {
                                light: Some(good),
                                ..Default::default()
                            },
                            &mut memory
                        )
                        .unwrap()
                        .light_accepted
                );
                "pixel light duplicate/default write"
            }
        };
        let before = model.stats.light_writes;
        let c = model
            .step(
                Tick {
                    light: Some(bad),
                    sample: Some(SampleWrite { key, rgb: [255; 3] }),
                    ..Default::default()
                },
                &mut memory,
            )
            .unwrap();
        assert!(c
            .events
            .iter()
            .any(|e| matches!(e, Event::Rejected(reason) if reason == expected)));
        assert!(!c.light_accepted && !c.sample_accepted);
        assert_eq!(model.stats.light_writes, before);
        assert!(model.drained() && !model.complete());
    }
    let (mut model, mut memory, ticket) = admitted();
    let sample = SampleWrite {
        key: PixelKey { ticket, lane: 2 },
        rgb: [31; 3],
    };
    assert!(
        model
            .step(
                Tick {
                    sample: Some(sample),
                    ..Default::default()
                },
                &mut memory
            )
            .unwrap()
            .sample_accepted
    );
    let c = model
        .step(
            Tick {
                sample: Some(sample),
                ..Default::default()
            },
            &mut memory,
        )
        .unwrap();
    assert!(c.events.iter().any(
        |e| matches!(e, Event::Rejected(reason) if reason == "pixel sample duplicate/default write")
    ));
}
#[test]
fn default_payloads_are_never_read_and_writes_to_bypass_are_rejected() {
    let mut s = support::synthetic(2).pop().unwrap();
    s.quad.default_light = true;
    s.quad.default_sample = true;
    s.quad.header.mask = 15;
    let mut model = Model::new(support::context(), 10_000).unwrap();
    let mut memory = Fixture::new(support::image());
    support::replay(&mut model, &mut memory, &[s.clone()], true, 10_000);
    assert_eq!(
        model.stats.light_reads
            + model.stats.sample_reads
            + model.stats.light_writes
            + model.stats.sample_writes,
        0
    );
    for light in [false, true] {
        let mut model = Model::new(support::context(), 1000).unwrap();
        let mut memory = Fixture::new(support::image());
        let ticket = model
            .step(
                Tick {
                    quad: Some(s.quad),
                    ..Default::default()
                },
                &mut memory,
            )
            .unwrap()
            .ticket
            .unwrap();
        let key = PixelKey { ticket, lane: 0 };
        let t = model
            .step(
                Tick {
                    light: light.then_some(LightWrite {
                        key,
                        value: s.light[0],
                    }),
                    sample: (!light).then_some(SampleWrite {
                        key,
                        rgb: s.sample[0],
                    }),
                    ..Default::default()
                },
                &mut memory,
            )
            .unwrap();
        assert!(t.events.iter().any(|e| matches!(e, Event::Rejected(_))));
        assert!(model.drained());
    }
}
#[test]
fn accepted_memory_drains_under_abort_and_ce_zero_without_render_completion() {
    for fail in [false, true] {
        let mut quad = support::synthetic(2).pop().unwrap().quad;
        quad.default_light = true;
        quad.default_sample = true;
        let mut model = Model::new(support::context(), 10_000).unwrap();
        let mut memory = Fixture::new(support::image());
        if fail {
            memory.fail_request = Some(1);
        }
        let mut admitted = false;
        let mut aborted = false;
        let mut returned_after_abort = false;
        for _ in 0..10_000 {
            let tick = Tick {
                ce: !aborted,
                quad: (!admitted).then_some(quad),
                finish: admitted,
                ..Default::default()
            };
            let t = model.step(tick, &mut memory).unwrap();
            admitted |= t.quad_accepted;
            if !fail && t.framebuffer.response.accepted && !aborted {
                model.abort();
                aborted = true;
            }
            returned_after_abort |= aborted && t.framebuffer.response.read.is_some();
            if model.drained() {
                break;
            }
        }
        assert!(model.drained() && memory.idle() && !model.complete());
        if !fail {
            assert!(aborted && returned_after_abort);
        }
    }
}
#[test]
fn permanent_result_stall_hits_wall_watchdog_without_false_retirement() {
    let mut model = Model::new(support::context(), 32).unwrap();
    let mut memory = Fixture::new(support::image());
    let quad = support::synthetic(3)[2].quad;
    model
        .step(
            Tick {
                quad: Some(quad),
                ..Default::default()
            },
            &mut memory,
        )
        .unwrap();
    for _ in 1..32 {
        model
            .step(
                Tick {
                    finish: true,
                    ..Default::default()
                },
                &mut memory,
            )
            .unwrap();
    }
    assert_eq!(
        model.step(Tick::default(), &mut memory).unwrap_err(),
        "pixel wall-cycle watchdog; drain transport externally"
    );
    assert_eq!(model.stats.retired, 0);
    assert!(!model.complete());
}

#[test]
fn actual_slot_reuse_rejects_old_result_even_when_payload_remains() {
    let mut model = Model::new(support::context(), 20_000).unwrap();
    let mut memory = Fixture::new(support::image());
    let mut q = support::synthetic(2).pop().unwrap().quad;
    q.default_light = true;
    q.default_sample = true;
    let mut count = 0;
    let mut first = None;
    let mut reused = false;
    for _ in 0..20_000 {
        let t = model
            .step(
                Tick {
                    quad: (count < 17).then_some(q),
                    ..Default::default()
                },
                &mut memory,
            )
            .unwrap();
        if let Some(ticket) = t.ticket {
            count += 1;
            if ticket.serial == 0 {
                first = Some(ticket);
            }
            if ticket.serial == 16 {
                assert_eq!(ticket.quad, first.unwrap().quad);
                let bad = model
                    .step(
                        Tick {
                            light: Some(LightWrite {
                                key: PixelKey {
                                    ticket: first.unwrap(),
                                    lane: 0,
                                },
                                value: LightingOutput { g: 256, h: 0 },
                            }),
                            ..Default::default()
                        },
                        &mut memory,
                    )
                    .unwrap();
                assert!(bad.events.iter().any(|e| matches!(e, Event::Rejected(s) if s == "pixel result stale/unallocated ticket")));
                assert!(!bad.light_accepted);
                reused = true;
                break;
            }
        }
    }
    assert!(reused && count == 17);
    for _ in 0..1000 {
        if model.drained() {
            break;
        }
        model
            .step(
                Tick {
                    ce: false,
                    ..Default::default()
                },
                &mut memory,
            )
            .unwrap();
    }
    assert!(model.drained() && memory.idle() && !model.complete());
}

#[test]
fn reserved_return_captures_under_final_backpressure_then_holds_without_bypass() {
    let mut model = Model::new(support::context(), 1000).unwrap();
    let mut memory = Fixture::new(support::image());
    let mut q = support::synthetic(2).pop().unwrap().quad;
    q.default_light = true;
    q.default_sample = true;
    model
        .step(
            Tick {
                quad: Some(q),
                ..Default::default()
            },
            &mut memory,
        )
        .unwrap();
    let mut issued = false;
    for _ in 0..32 {
        let t = model.step(Tick::default(), &mut memory).unwrap();
        if t.events
            .iter()
            .any(|e| matches!(e, Event::ReadIssued { .. }))
        {
            issued = true;
            break;
        }
    }
    assert!(issued && model.snapshot().read_pending);
    let t = model
        .step(
            Tick {
                ce: false,
                final_ready: false,
                ..Default::default()
            },
            &mut memory,
        )
        .unwrap();
    assert!(t.events.iter().any(|e| matches!(e, Event::Captured { .. })));
    assert!(t.snapshot.return_valid && !t.snapshot.read_pending && !t.snapshot.output_valid);
    for _ in 0..7 {
        let t = model
            .step(
                Tick {
                    final_ready: false,
                    ..Default::default()
                },
                &mut memory,
            )
            .unwrap();
        assert!(t.snapshot.return_valid);
        assert!(t.accesses.is_empty() && t.events.is_empty());
    }
    let t = model.step(Tick::default(), &mut memory).unwrap();
    assert!(t.events.iter().any(|e| matches!(e, Event::Consumed { .. })));
    assert!(!t.framebuffer.input_accepted && t.snapshot.output_valid);
    model.abort();
    model.step(Tick::default(), &mut memory).unwrap();
    assert!(model.drained());
}

#[test]
fn invalid_headers_reject_before_admission_and_ce_gates_finish_and_writes() {
    for header in [
        fb::Header {
            x: u16::MAX,
            y: 0,
            mask: 1,
        },
        fb::Header {
            x: 1,
            y: 0,
            mask: 15,
        },
        fb::Header {
            x: 0,
            y: 0,
            mask: 16,
        },
    ] {
        let mut model = Model::new(support::context(), 100).unwrap();
        let mut memory = Fixture::new(support::image());
        let t = model
            .step(
                Tick {
                    quad: Some(QuadInput {
                        header,
                        ..support::synthetic(2)[1].quad
                    }),
                    ..Default::default()
                },
                &mut memory,
            )
            .unwrap();
        assert!(!t.quad_accepted && model.stats.admitted == 0);
        assert!(t
            .events
            .iter()
            .any(|e| matches!(e, Event::Rejected(s) if s == "quad bounds/alignment/mask")));
        assert!(model.drained());
    }
    let (mut model, mut memory, ticket) = admitted();
    let tick = Tick {
        ce: false,
        finish: true,
        light: Some(LightWrite {
            key: PixelKey { ticket, lane: 1 },
            value: LightingOutput { g: 256, h: 0 },
        }),
        ..Default::default()
    };
    let t = model.step(tick, &mut memory).unwrap();
    assert!(!t.light_accepted && t.accesses.is_empty());
    assert_eq!(t.snapshot.phase, Phase::Running);
    model.abort();
    model.step(tick, &mut memory).unwrap();
    assert!(model.drained());
}

#[test]
fn same_address_depth_and_alpha_require_submission_order_and_preserve_guards() {
    let context = support::context();
    let mut initial = support::image();
    let start = context.surface.depth_base_bytes as usize;
    let length = usize::from(context.surface.width) * usize::from(context.surface.height) * 2;
    initial[start..start + length].fill(255);
    let mut inputs = support::synthetic(48);
    for (i, s) in inputs.iter_mut().enumerate() {
        s.quad.header = fb::Header {
            x: 0,
            y: 0,
            mask: 15,
        };
        s.quad.default_light = false;
        s.quad.default_sample = false;
        for b in &mut s.quad.basic {
            // An older/deeper late arrival must fail depth instead of altering
            // the shallower already committed color, even with SrcOver.
            b.depth = (if i % 3 == 2 { 61000 } else { 60000 - i * 500 }) as u16;
        }
    }
    let expected = support::golden(initial.clone(), &inputs, context);
    let mut reverse = inputs.clone();
    reverse.reverse();
    assert_ne!(
        expected,
        support::golden(initial.clone(), &reverse, context),
        "golden must detect reversed alpha/depth order"
    );
    let color = context.surface.color_base_bytes as usize;
    assert_ne!(expected[color..color + 512], initial[color..color + 512]);
    assert_ne!(expected[start..start + 512], initial[start..start + 512]);
    let mut memory = Fixture::new(initial);
    memory.beat_period = 3;
    memory.ack_delay = 17;
    let mut model = Model::new(context, 100_000).unwrap();
    let proof = support::replay(&mut model, &mut memory, &inputs, true, 100_000);
    assert!(proof.out_of_order && proof.retired.len() == 48 && memory.idle());
    assert_eq!(memory.bytes, expected); // Includes every untouched word and guard.
}

fn write_addresses(cycles: &[Cycle], store: Store) -> Vec<u8> {
    let mut addresses: Vec<u8> = cycles
        .iter()
        .flat_map(|c| &c.accesses)
        .filter(|a| a.store == store && a.write)
        .map(|a| a.address)
        .collect();
    addresses.sort_unstable();
    addresses
}

/// Admit one stimulus, offer exactly its non-default branch payloads, and step
/// until the quad retires. Bounded; only the directed boundary tests use it.
fn drain_quad(
    model: &mut Model,
    memory: &mut Fixture,
    s: &support::Stimulus,
    max_steps: u64,
) -> (Ticket, Vec<Cycle>) {
    let mut cycles = Vec::new();
    let first = model
        .step(
            Tick {
                quad: Some(s.quad),
                ..Default::default()
            },
            memory,
        )
        .unwrap();
    assert!(first.quad_accepted, "direct stimulus was not admitted");
    let ticket = first.ticket.expect("admitted stimulus has a ticket");
    cycles.push(first);
    let mut light = VecDeque::new();
    let mut sample = VecDeque::new();
    for lane in (0..4u8).rev() {
        if s.quad.header.mask >> lane & 1 == 0 {
            continue;
        }
        if !s.quad.default_light {
            light.push_front(LightWrite {
                key: PixelKey { ticket, lane },
                value: s.light[usize::from(lane)],
            });
        }
        if !s.quad.default_sample {
            sample.push_front(SampleWrite {
                key: PixelKey { ticket, lane },
                rgb: s.sample[usize::from(lane)],
            });
        }
    }
    let mut retired = false;
    for _ in 0..max_steps {
        let t = model
            .step(
                Tick {
                    light: light.front().copied(),
                    sample: sample.front().copied(),
                    ..Default::default()
                },
                memory,
            )
            .unwrap();
        assert!(
            !t.events.iter().any(|e| matches!(e, Event::Rejected(_))),
            "direct replay rejected: {:?}",
            t.events
        );
        if t.light_accepted {
            light.pop_front();
        }
        if t.sample_accepted {
            sample.pop_front();
        }
        let now_retired = t.events.iter().any(|e| matches!(e, Event::Retired(_)));
        cycles.push(t);
        if now_retired {
            retired = true;
            break;
        }
    }
    assert!(
        retired,
        "direct stimulus did not retire within {max_steps} edges"
    );
    assert!(
        light.is_empty() && sample.is_empty(),
        "branch writes left queued"
    );
    (ticket, cycles)
}

/// Request finish and step until the single flush/ACK completes. Bounded.
fn complete_render(model: &mut Model, memory: &mut Fixture, max_steps: u64) -> Vec<Cycle> {
    let mut cycles = Vec::new();
    for _ in 0..max_steps {
        let t = model
            .step(
                Tick {
                    finish: true,
                    ..Default::default()
                },
                memory,
            )
            .unwrap();
        let complete = t.events.iter().any(|e| matches!(e, Event::Complete));
        cycles.push(t);
        if complete {
            return cycles;
        }
    }
    panic!("pixel render did not complete within {max_steps} edges");
}

#[test]
fn reused_global_slot_after_real_payloads_alternates_bypass_without_stale_reads() {
    let context = support::context();
    let mut initial = support::image();
    let depth_start = context.surface.depth_base_bytes as usize;
    let plane = usize::from(context.surface.width) * usize::from(context.surface.height) * 2;
    // Force every depth test to pass so each addressed pixel commits, including
    // the deliberately reused addresses 0 and 1.
    initial[depth_start..depth_start + plane].fill(0xff);
    let base = support::synthetic(24);
    let inputs: Vec<support::Stimulus> = base
        .iter()
        .take(22)
        .enumerate()
        .map(|(i, b)| {
            let mut s = b.clone();
            s.quad.header = fb::Header {
                x: ((i % 10) * 16) as u16,
                y: ((i / 10) % 2 * 16) as u8,
                mask: 15,
            };
            let tint = s.quad.basic;
            s.quad.basic = std::array::from_fn(|lane| Basic {
                tint: tint[lane].tint,
                depth: (60_000 - i * 100 - lane * 7) as u16,
            });
            let (default_light, default_sample) = match i {
                0..=15 => (false, false),
                _ => match (i - 16) % 3 {
                    0 => (false, true),
                    1 => (true, false),
                    _ => (true, true),
                },
            };
            s.quad.default_light = default_light;
            s.quad.default_sample = default_sample;
            s
        })
        .collect();
    let expected = support::golden(initial.clone(), &inputs, context);
    let mut memory = Fixture::new(initial);
    memory.request_period = 3;
    memory.ack_delay = 5;
    let mut model = Model::new(context, 200_000).unwrap();
    let mut tickets: Vec<Ticket> = Vec::new();
    let mut all_cycles: Vec<Vec<Cycle>> = Vec::new();
    let mut totals = [0u64; 4]; // light reads/writes, sample reads/writes.
    for (i, s) in inputs.iter().enumerate() {
        let (ticket, cycles) = drain_quad(&mut model, &mut memory, s, 4000);
        assert_eq!(
            ticket.serial, i as u64,
            "serial must follow admission order"
        );
        assert_eq!(
            ticket.quad,
            (i as u8) & 15,
            "slot index must follow wrap order"
        );
        let covered = u64::from(s.quad.header.mask.count_ones());
        let light_w = cycles
            .iter()
            .flat_map(|c| &c.events)
            .filter(|e| matches!(e, Event::LightDone(_)))
            .count() as u64;
        let sample_w = cycles
            .iter()
            .flat_map(|c| &c.events)
            .filter(|e| matches!(e, Event::SampleDone(_)))
            .count() as u64;
        let light_r = cycles
            .iter()
            .flat_map(|c| &c.accesses)
            .filter(|a| a.store == Store::Light && !a.write)
            .count() as u64;
        let sample_r = cycles
            .iter()
            .flat_map(|c| &c.accesses)
            .filter(|a| a.store == Store::Sample && !a.write)
            .count() as u64;
        if s.quad.default_light {
            assert_eq!(light_w + light_r, 0, "default light touched its stale bank");
        } else {
            assert_eq!((light_r, light_w), (covered, covered));
        }
        if s.quad.default_sample {
            assert_eq!(
                sample_w + sample_r,
                0,
                "default sample touched its stale bank"
            );
        } else {
            assert_eq!((sample_r, sample_w), (covered, covered));
        }
        for (store, bypass) in [
            (Store::Light, s.quad.default_light),
            (Store::Sample, s.quad.default_sample),
        ] {
            let expected_addresses: Vec<u8> = if bypass {
                vec![]
            } else {
                (0..4).map(|lane| ticket.quad * 4 + lane).collect()
            };
            let mut reads: Vec<_> = cycles
                .iter()
                .flat_map(|c| &c.accesses)
                .filter(|a| a.store == store && !a.write)
                .map(|a| a.address)
                .collect();
            reads.sort_unstable();
            assert_eq!(reads, expected_addresses, "wrong result read cell");
            assert_eq!(
                write_addresses(&cycles, store),
                expected_addresses,
                "wrong result write cell"
            );
        }
        totals[0] += light_r;
        totals[1] += light_w;
        totals[2] += sample_r;
        totals[3] += sample_w;
        if i >= 16 {
            // The wrap-16 slot is the same header allocation as slot zero.
            assert_eq!(ticket.quad, tickets[i - 16].quad, "global slot not reused");
            assert_ne!(ticket.serial, tickets[i - 16].serial);
            if !s.quad.default_light {
                assert_eq!(
                    write_addresses(&cycles, Store::Light),
                    write_addresses(&all_cycles[i - 16], Store::Light),
                    "reused light bank cell not identical"
                );
            }
            if !s.quad.default_sample {
                assert_eq!(
                    write_addresses(&cycles, Store::Sample),
                    write_addresses(&all_cycles[i - 16], Store::Sample),
                    "reused sample bank cell not identical"
                );
            }
        }
        tickets.push(ticket);
        all_cycles.push(cycles);
    }
    assert_eq!(
        [
            model.stats.light_reads,
            model.stats.light_writes,
            model.stats.sample_reads,
            model.stats.sample_writes,
        ],
        totals
    );
    let finish = complete_render(&mut model, &mut memory, 200_000);
    assert!(finish
        .iter()
        .any(|c| c.events.iter().any(|e| matches!(e, Event::FlushRequested))));
    assert!(model.complete() && memory.idle());
    assert_eq!(memory.bytes, expected);
}

#[test]
fn finish_holds_closing_for_late_result_during_mc_stall_and_completes_after_ack() {
    let context = support::context();
    let mut initial = support::image();
    let depth = context.surface.depth_base_bytes as usize;
    let plane = usize::from(context.surface.width) * usize::from(context.surface.height) * 2;
    initial[depth..depth + plane].fill(255); // Both quads must dirty depth.
    let mut memory = Fixture::new(initial.clone());
    // No request is accepted at this period, so the downstream MC stays stalled.
    memory.request_period = u64::MAX / 2;
    memory.ack_delay = 12;
    let mut model = Model::new(context, 200_000).unwrap();
    // Quad A is fully bypassed; it retires while leaving a cold refill pending.
    let mut a = support::synthetic(2).pop().unwrap();
    a.quad.header = fb::Header {
        x: 0,
        y: 0,
        mask: 15,
    };
    let tint = a.quad.basic;
    a.quad.basic = std::array::from_fn(|lane| Basic {
        tint: tint[lane].tint,
        depth: 1000 + lane as u16,
    });
    a.quad.default_light = true;
    a.quad.default_sample = true;
    drain_quad(&mut model, &mut memory, &a, 4000);
    let mut presented = false;
    for _ in 0..16 {
        let t = model.step(Tick::default(), &mut memory).unwrap();
        presented |= t.framebuffer.request.is_some() && !t.framebuffer.response.accepted;
    }
    assert!(presented, "cold refill was not presented to the stalled MC");
    assert_eq!(memory.requests, 0, "stalled MC accepted a request");
    assert_eq!(model.snapshot().phase, Phase::Running);
    assert!(!model.complete());

    // Quad B needs real branch results that are deliberately withheld.
    let mut b = support::synthetic(3)[1].clone();
    b.quad.header = fb::Header {
        x: 32,
        y: 0,
        mask: 15,
    };
    let tint = b.quad.basic;
    b.quad.basic = std::array::from_fn(|lane| Basic {
        tint: tint[lane].tint,
        depth: 2000 + lane as u16,
    });
    b.quad.default_light = false;
    b.quad.default_sample = false;
    let expected = support::golden(initial.clone(), &[a.clone(), b.clone()], context);
    let color = context.surface.color_base_bytes as usize;
    assert_ne!(
        expected[color..color + plane],
        initial[color..color + plane]
    );
    assert_ne!(
        expected[depth..depth + plane],
        initial[depth..depth + plane]
    );
    let first = model
        .step(
            Tick {
                quad: Some(b.quad),
                ..Default::default()
            },
            &mut memory,
        )
        .unwrap();
    assert!(first.quad_accepted);
    let ticket = first.ticket.unwrap();
    let mut drained = false;
    for _ in 0..64 {
        let t = model.step(Tick::default(), &mut memory).unwrap();
        assert!(!t
            .events
            .iter()
            .any(|e| matches!(e, Event::Complete | Event::FlushRequested)));
        if !t.snapshot.ingress {
            drained = true;
            break;
        }
    }
    assert!(drained, "basic ingress did not drain");
    assert_eq!(model.snapshot().live, 1);

    // finish arrives while allocated work still waits for its branch result.
    let c = model
        .step(
            Tick {
                finish: true,
                quad: Some(b.quad),
                ..Default::default()
            },
            &mut memory,
        )
        .unwrap();
    assert!(!c.quad_accepted, "new work admitted after finish");
    assert_eq!(model.stats.admitted, 2);
    assert_eq!(c.snapshot.phase, Phase::Closing);
    assert!(!c
        .events
        .iter()
        .any(|e| matches!(e, Event::FlushRequested | Event::Complete)));

    // A CE pause preserves the closing state and accepts nothing.
    let before = model.snapshot();
    let paused = model
        .step(
            Tick {
                ce: false,
                finish: true,
                quad: Some(b.quad),
                light: Some(LightWrite {
                    key: PixelKey { ticket, lane: 0 },
                    value: b.light[0],
                }),
                ..Default::default()
            },
            &mut memory,
        )
        .unwrap();
    assert_eq!(paused.snapshot, before);
    assert!(!paused.quad_accepted && !paused.light_accepted && !paused.sample_accepted);
    assert!(paused.accesses.is_empty());
    assert!(!model.complete());

    // Already allocated results can still arrive while closing.
    for lane in 0..4u8 {
        if b.quad.header.mask >> lane & 1 == 0 {
            continue;
        }
        let t = model
            .step(
                Tick {
                    light: Some(LightWrite {
                        key: PixelKey { ticket, lane },
                        value: b.light[usize::from(lane)],
                    }),
                    ..Default::default()
                },
                &mut memory,
            )
            .unwrap();
        assert!(t.light_accepted && t.snapshot.phase == Phase::Closing);
    }
    for lane in 0..4u8 {
        if b.quad.header.mask >> lane & 1 == 0 {
            continue;
        }
        let t = model
            .step(
                Tick {
                    sample: Some(SampleWrite {
                        key: PixelKey { ticket, lane },
                        rgb: b.sample[usize::from(lane)],
                    }),
                    ..Default::default()
                },
                &mut memory,
            )
            .unwrap();
        assert!(t.sample_accepted && t.snapshot.phase == Phase::Closing);
    }

    // Retire and request the real flush, still under the stalled MC.
    let mut retired = false;
    let mut flush = false;
    for _ in 0..20_000 {
        let t = model.step(Tick::default(), &mut memory).unwrap();
        assert!(
            !t.events.iter().any(|e| matches!(e, Event::Complete)),
            "complete before flush ACK"
        );
        retired |= t.events.iter().any(|e| matches!(e, Event::Retired(_)));
        flush |= t.events.iter().any(|e| matches!(e, Event::FlushRequested));
        if flush {
            break;
        }
    }
    assert!(retired && flush, "retire/flush did not happen");
    assert_eq!(model.snapshot().phase, Phase::Flushing);
    assert!(!model.complete());

    // No completion and no new admission while the downstream MC stays stalled.
    for _ in 0..1000 {
        let t = model
            .step(
                Tick {
                    quad: Some(b.quad),
                    ..Default::default()
                },
                &mut memory,
            )
            .unwrap();
        assert!(!t.quad_accepted, "new work admitted after finish");
        assert!(
            !t.events.iter().any(|e| matches!(e, Event::Complete)),
            "premature complete during MC stall"
        );
    }
    assert_eq!(memory.requests, 0, "stalled MC accepted a request");
    assert!(!model.complete());

    // Release the MC and distinguish refill completion from flush writeback
    // ACK. No dirty eviction can occur with these two tiles in the eight lines.
    memory.request_period = 1;
    let mut complete = false;
    let mut active_write = None;
    let mut write_beats = 0;
    let mut last_write_beat = None;
    let mut last_write_ack = None;
    let mut read_acks = 0;
    let mut write_acks = 0;
    let mut delayed_write_edges = 0;
    for _ in 0..20_000 {
        let t = model
            .step(
                Tick {
                    finish: true,
                    quad: Some(b.quad),
                    ..Default::default()
                },
                &mut memory,
            )
            .unwrap();
        assert!(!t.quad_accepted, "new work admitted after finish");
        let response = t.framebuffer.response;
        if response.accepted {
            assert!(active_write.is_none());
            let request = t.framebuffer.request.unwrap();
            active_write = Some(request.write);
            write_beats = 0;
            last_write_beat = None;
            if request.write {
                assert_eq!(
                    t.snapshot.phase,
                    Phase::Flushing,
                    "write is not flush maintenance"
                );
            }
        }
        if response.write_accepted {
            assert_eq!(active_write, Some(true));
            write_beats += 1;
            if write_beats == 16 {
                last_write_beat = Some(t.wall);
                assert!(!model.complete(), "last beat is not ACK");
            }
        }
        if let Some(success) = response.complete {
            assert!(success);
            if active_write.take().expect("terminal without transaction") {
                assert_eq!(write_beats, 16);
                assert!(t.wall - last_write_beat.unwrap() >= memory.ack_delay);
                write_acks += 1;
                last_write_ack = Some(t.wall);
            } else {
                read_acks += 1;
            }
        } else if active_write == Some(true) && last_write_beat.is_some() {
            delayed_write_edges += 1;
            assert!(!model.complete(), "render completed before writeback ACK");
        }
        if t.events.iter().any(|e| matches!(e, Event::Complete)) {
            assert!(last_write_ack.is_some_and(|ack| ack <= t.wall));
            assert!(active_write.is_none());
            complete = true;
            break;
        }
    }
    assert!(complete, "render did not complete after ACK");
    assert!(model.complete() && memory.idle());
    assert!(memory.requests > 0, "completion without a real MC request");
    assert!(read_acks > 0 && write_acks > 0 && delayed_write_edges > 0);
    assert_eq!(memory.bytes, expected); // Both planes and all guards.
}
