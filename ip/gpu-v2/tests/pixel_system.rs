use gpu_v2::{
    framebuffer::{ports as fb, sim::fixture::Fixture},
    lighting::ports::LightingOutput,
    system::pixel::*,
};
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
