#[path = "support/sdram/physical_texture.rs"]
mod physical;
#[path = "support/texture.rs"]
#[allow(dead_code)]
mod support;
use gpu_v2::texture::{
    emu::color::{ColorEmu, Event, Input, Output, Tick, LATENCY, RESULT_CAPACITY},
    ports::*,
    sim::{oracle, staged::bound, timed},
};
fn request(key: u8, first: bool, last: bool, weights: [u32; 4], texels: [u16; 4]) -> Input {
    let group = Group4 {
        key: TileKey {
            slot: 0,
            n: 5,
            x: 0,
            y: 0,
        },
        top_left_local: [0, 0],
        coefficients: weights,
        first,
        last,
        quad_id: key >> 2,
        lane: key & 3,
    };
    Input {
        payload: group.pack72().unwrap() as i128,
        texels,
    }
}
// Host reference uses ordinary integer sums and division, independent of the
// emu's staged carry normalization. Extract directly from packet bits.
fn contribution(p: Input) -> [u32; 3] {
    let mut sums = [0; 3];
    for t in 0..4 {
        let w = ((p.payload >> (28 + 9 * t)) & 511) as u32;
        let word = u32::from(p.texels[t]);
        let rgb = [word / 2048, word / 32 % 64, word % 32];
        let rgb = [
            rgb[0] * 8 + rgb[0] / 4,
            rgb[1] * 4 + rgb[1] / 16,
            rgb[2] * 8 + rgb[2] / 4,
        ];
        for c in 0..3 {
            sums[c] += rgb[c] * w;
        }
    }
    sums
}
fn golden(input: &[Input]) -> Vec<Output> {
    let mut out = vec![];
    let mut sum = [0; 3];
    for &p in input {
        if p.payload >> 64 & 1 != 0 {
            sum = [0; 3];
        }
        let value = contribution(p);
        for c in 0..3 {
            sum[c] += value[c];
        }
        if p.payload >> 65 & 1 != 0 {
            out.push(Output {
                key: (((p.payload >> 66) & 15) * 4 + ((p.payload >> 70) & 3)) as u8,
                rgb: sum.map(|v| ((v + 255) / 511) as u8),
            });
        }
    }
    out
}
#[test]
fn one_packet_has_real_stages_and_eight_enabled_edge_latency() {
    let p = request(37, true, true, [0, 511, 0, 0], [0, 0x08a3, 0, 0]);
    let mut m = ColorEmu::new(64).unwrap();
    let mut times = [None; 5];
    let mut output = vec![];
    for t in 0..16 {
        let s = m
            .tick(Tick {
                ce: true,
                input: (t == 0).then_some(p),
                output_ready: true,
            })
            .unwrap();
        for e in s.events {
            match e {
                Event::Accepted { .. } => times[0] = Some(t),
                Event::Products { values, .. } => {
                    assert_eq!(values[1], contribution(p));
                    times[1] = Some(t);
                }
                Event::Partial { value, .. } => {
                    assert_eq!(value, contribution(p));
                    times[2] = Some(t);
                }
                Event::Accumulate { value, .. } => {
                    assert_eq!(value, contribution(p));
                    times[3] = Some(t);
                }
                Event::Queued(_) => times[4] = Some(t),
                Event::Commit(value) => output.push(value),
            }
        }
    }
    assert_eq!(times, [Some(0), Some(3), Some(5), Some(6), Some(LATENCY)]);
    assert_eq!(output, golden(&[p]));
    assert!(m.idle());
}
fn replay(input: &[Input], stop: u64) -> (Vec<Output>, bool, bool) {
    let mut m = ColorEmu::new(20_000).unwrap();
    let mut cursor = 0;
    let mut out = vec![];
    let mut credit_stall = false;
    let mut frozen_pending = false;
    let mut drained = false;
    for t in 0..20_000_u64 {
        let before = m.snapshot();
        let ce = t % 17 > 4;
        let s = m
            .tick(Tick {
                ce,
                input: input.get(cursor).copied(),
                output_ready: t > stop && t % 13 > 3,
            })
            .unwrap();
        cursor += usize::from(s.accepted);
        if ce && cursor < input.len() && !s.input_ready {
            credit_stall = true;
        }
        if !ce && before.pipeline.into_iter().any(|v| v) {
            assert!(s.events.is_empty() && !s.accepted && !s.input_ready);
            assert_eq!(before.pipeline, s.snapshot.pipeline);
            assert_eq!(before.result_credits, s.snapshot.result_credits);
            assert_eq!(before.accumulator_owner, s.snapshot.accumulator_owner);
            frozen_pending = true;
        }
        for e in s.events {
            if let Event::Commit(value) = e {
                out.push(value);
            }
        }
        assert!(s.snapshot.result_credits <= RESULT_CAPACITY);
        if cursor == input.len() && m.idle() {
            drained = true;
            break;
        }
    }
    assert!(drained, "finite replay did not drain");
    (out, credit_stall, frozen_pending)
}
#[test]
fn multigroup_owners_ce_and_full_result_credit_recover_in_order() {
    let mut input = vec![];
    for sample in 0..48_u32 {
        let groups = sample % 8 + 1;
        for group in 0..groups {
            let weight = 511 / groups + u32::from(group < 511 % groups);
            let texels =
                std::array::from_fn(|t| (sample * 3001 + group * 577 + t as u32 * 8191) as u16);
            input.push(request(
                (sample % 64) as u8,
                group == 0,
                group + 1 == groups,
                [weight / 2, 0, weight - weight / 2, 0],
                texels,
            ));
        }
    }
    let (out, blocked, frozen) = replay(&input, 600);
    assert_eq!(out, golden(&input));
    assert!(blocked && frozen);
}
#[test]
fn full_credit_does_not_use_same_edge_output_return() {
    let mut m = ColorEmu::new(128).unwrap();
    for key in 0..16 {
        assert!(
            m.tick(Tick {
                ce: true,
                input: Some(request(key, true, true, [511, 0, 0, 0], [0xffff; 4])),
                output_ready: false
            })
            .unwrap()
            .accepted
        );
    }
    let p = request(16, true, true, [511, 0, 0, 0], [0; 4]);
    let s = m
        .tick(Tick {
            ce: true,
            input: Some(p),
            output_ready: true,
        })
        .unwrap();
    assert!(s.output.is_some());
    assert!(!s.input_ready && !s.accepted);
    assert_eq!(s.snapshot.result_credits, 15);
    assert!(
        m.tick(Tick {
            ce: true,
            input: Some(p),
            output_ready: true
        })
        .unwrap()
        .accepted
    );
}
#[test]
fn wrong_owner_is_terminal_but_new_instance_can_run_a_legal_stream() {
    let mut m = ColorEmu::new(64).unwrap();
    m.tick(Tick {
        ce: true,
        input: Some(request(2, true, false, [255, 0, 0, 0], [0; 4])),
        output_ready: true,
    })
    .unwrap();
    let err = m
        .tick(Tick {
            ce: true,
            input: Some(request(3, false, true, [256, 0, 0, 0], [0; 4])),
            output_ready: true,
        })
        .unwrap_err();
    assert_eq!(err, "color non-first owner/order");
    assert!(m.faulted());
    assert_eq!(
        m.tick(Tick {
            ce: true,
            input: None,
            output_ready: true
        })
        .unwrap_err(),
        "color terminal fault; recreate before reuse"
    );
    let legal = [
        request(2, true, false, [255, 0, 0, 0], [0xffff; 4]),
        request(2, false, true, [256, 0, 0, 0], [0xffff; 4]),
    ];
    assert_eq!(replay(&legal, 0).0, golden(&legal));
}
#[test]
fn backpressure_holds_full_queue_then_drains_and_reuses_owner_and_key() {
    let key_a = 1_u8;
    let key_b = 2_u8;
    let input: Vec<_> = (0..RESULT_CAPACITY)
        .map(|i| {
            request(
                if i % 2 == 0 { key_a } else { key_b },
                true,
                true,
                [127, 129, 128, 127],
                std::array::from_fn(|t| ((i + 1) * 1049 + t * 8191) as u16),
            )
        })
        .collect();
    let expected = golden(&input);
    assert!(expected.windows(2).all(|w| w[0].rgb != w[1].rgb));
    let mut m = ColorEmu::new(2_000).unwrap();
    for &p in &input {
        let s = m
            .tick(Tick {
                ce: true,
                input: Some(p),
                output_ready: false,
            })
            .unwrap();
        assert!(s.accepted && !s.events.iter().any(|e| matches!(e, Event::Commit(_))));
    }
    let full = m.snapshot();
    assert_eq!(full.result_credits, RESULT_CAPACITY);
    assert!(full.pipeline.into_iter().any(|v| v) && full.queued > 0);
    let blocked = request(key_a, true, true, [0, 511, 0, 0], [0, 0xf800, 0, 0]);
    // Pause while both queued results and in-flight reservations own all
    // credits. Even ready=1 cannot consume or admit while CE=0.
    for _ in 0..5 {
        let before = m.snapshot();
        let s = m
            .tick(Tick {
                ce: false,
                input: Some(blocked),
                output_ready: true,
            })
            .unwrap();
        assert!(!s.accepted && !s.input_ready && s.events.is_empty());
        assert_eq!(
            s.snapshot,
            gpu_v2::texture::emu::color::Snapshot {
                wall: before.wall + 1,
                ..before
            }
        );
        assert_eq!(s.output, Some(expected[0]));
    }
    // Hold the seventeenth packet until the old pipeline empties into the
    // genuinely full result queue; no result credit has yet been returned.
    for _ in 0..LATENCY + 2 {
        let s = m
            .tick(Tick {
                ce: true,
                input: Some(blocked),
                output_ready: false,
            })
            .unwrap();
        assert!(!s.accepted && !s.input_ready);
        assert_eq!(s.snapshot.result_credits, RESULT_CAPACITY);
        assert!(!s.events.iter().any(|e| matches!(e, Event::Commit(_))));
        assert_eq!(s.output, Some(expected[0]));
    }
    assert_eq!(m.snapshot().queued, RESULT_CAPACITY);
    assert!(!m.snapshot().pipeline.into_iter().any(|v| v));
    let s = m
        .tick(Tick {
            ce: true,
            input: Some(blocked),
            output_ready: true,
        })
        .unwrap();
    assert!(
        !s.accepted && !s.input_ready,
        "cannot borrow same-edge returned credit"
    );
    assert_eq!(s.snapshot.result_credits, RESULT_CAPACITY - 1);
    let mut outputs: Vec<_> = s
        .events
        .into_iter()
        .filter_map(|e| {
            if let Event::Commit(o) = e {
                Some(o)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(outputs, expected[..1]);
    let s = m
        .tick(Tick {
            ce: true,
            input: Some(blocked),
            output_ready: true,
        })
        .unwrap();
    assert!(s.accepted, "old credit now permits held packet");
    outputs.extend(s.events.into_iter().filter_map(|e| {
        if let Event::Commit(o) = e {
            Some(o)
        } else {
            None
        }
    }));
    for _ in 0..RESULT_CAPACITY as u64 + LATENCY + 5 {
        if m.idle() {
            break;
        }
        let s = m
            .tick(Tick {
                ce: true,
                input: None,
                output_ready: true,
            })
            .unwrap();
        outputs.extend(s.events.into_iter().filter_map(|e| {
            if let Event::Commit(o) = e {
                Some(o)
            } else {
                None
            }
        }));
    }
    let mut stream = input;
    stream.push(blocked);
    assert_eq!(outputs, golden(&stream));
    assert!(m.idle() && m.snapshot().result_credits == 0);

    // Reuse the SAME drained machine and both keys. Different multi-group
    // colors expose stale feedback or key/RGB mismatches; replay() would
    // instantiate another machine and would not establish this property.
    let reuse = [
        request(key_a, true, false, [255, 0, 0, 0], [0x07e0, 0, 0, 0]),
        request(key_a, false, true, [0, 256, 0, 0], [0, 0x001f, 0, 0]),
        request(key_b, true, false, [0, 0, 200, 0], [0, 0, 0xf800, 0]),
        request(key_b, false, true, [0, 0, 0, 311], [0, 0, 0, 0x8410]),
    ];
    let reuse_expected = golden(&reuse);
    assert_ne!(reuse_expected[0].rgb, reuse_expected[1].rgb);
    let mut cursor = 0;
    let mut reused = vec![];
    for wall in 0..128 {
        let s = m
            .tick(Tick {
                ce: wall % 11 > 2,
                input: reuse.get(cursor).copied(),
                output_ready: true,
            })
            .unwrap();
        cursor += usize::from(s.accepted);
        reused.extend(s.events.into_iter().filter_map(|e| {
            if let Event::Commit(o) = e {
                Some(o)
            } else {
                None
            }
        }));
        if cursor == reuse.len() && m.idle() {
            break;
        }
    }
    assert_eq!(cursor, reuse.len());
    assert_eq!(reused, reuse_expected);
    assert!(m.idle() && m.snapshot().result_credits == 0);
}

#[test]
fn real_mc_captured_texels_match_independent_sampler_oracle() {
    let slot = support::slot(5, true);
    let bytes = support::asset(slot, support::pattern);
    let qs: Vec<_> = (0..12)
        .map(|i| {
            let mut q = support::input(
                5,
                [Filter::Nearest, Filter::Bilinear, Filter::Trilinear][i % 3],
                [0.0; 2],
            );
            q.quad_id = (i % 4) as u8;
            q.mask = [15, 1, 6, 0][i % 4];
            q.lod_bias = 0.5;
            q.uv[1][0] += 1.0 / 32.0;
            q.uv[2][1] += 1.0 / 32.0;
            q.uv[3][0] += 1.0 / 32.0;
            q.uv[3][1] += 1.0 / 32.0;
            q
        })
        .collect();
    let mut image = support::Image {
        bytes: bytes.clone(),
        requests: vec![],
    };
    let mut cache = oracle::Cache::new(vec![slot]).unwrap();
    let expected: Vec<_> = qs
        .iter()
        .flat_map(|q| {
            oracle::sample(q, &mut cache, &mut image, Config::counted())
                .unwrap()
                .pixels
                .into_iter()
                .map(|p| Output {
                    key: q.quad_id * 4 + p.lane,
                    rgb: p.rgb,
                })
        })
        .collect();
    let mut memory = physical::Physical::new(u64::from(support::BASE), bytes, true, false);
    let r = bound::system::run_pooled(
        &qs,
        &[slot],
        &mut memory,
        bound::control::Hardware {
            storage: bound::control::Storage::Packed,
            ..Default::default()
        },
        timed::Hardware {
            prefetch: false,
            max_cycles: 100_000,
            ..Default::default()
        },
        |t| timed::Control {
            ce: t % 23 > 7,
            result_ready: t > 600 && t % 31 > 7,
        },
    )
    .unwrap();
    let input: Vec<_> = r
        .cache
        .steps
        .iter()
        .flat_map(|s| &s.events)
        .filter_map(|e| {
            if let timed::Event::Captured { group, words, .. } = e {
                Some(Input {
                    payload: group.pack72().unwrap() as i128,
                    texels: *words,
                })
            } else {
                None
            }
        })
        .collect();
    assert!(memory.requests > 0);
    assert!(memory.init_cycles > 0);
    assert!(
        input.len() > expected.len(),
        "actual seam/mip multi-group path must run"
    );
    assert!(
        r.cache
            .steps
            .iter()
            .flat_map(|s| &s.events)
            .any(|e| matches!(e, timed::Event::Captured { group, .. } if group.key.n < 5)),
        "actual coarse mip must be captured"
    );
    assert_eq!(golden(&input), expected);
    assert_eq!(replay(&input, 300).0, expected);
}
#[test]
fn interrupted_sample_bad_width_and_illegal_sum_hit_the_intended_checks() {
    let mut m = ColorEmu::new(32).unwrap();
    m.tick(Tick {
        ce: true,
        input: Some(request(0, true, false, [1, 0, 0, 0], [0; 4])),
        output_ready: false,
    })
    .unwrap();
    assert_eq!(
        m.tick(Tick {
            ce: true,
            input: Some(request(1, true, true, [511, 0, 0, 0], [0; 4])),
            output_ready: false
        })
        .unwrap_err(),
        "color first interrupted sample"
    );
    let mut m = ColorEmu::new(32).unwrap();
    assert_eq!(
        m.tick(Tick {
            ce: true,
            input: Some(Input {
                payload: 1_i128 << 72,
                texels: [0; 4]
            }),
            output_ready: true
        })
        .unwrap_err(),
        "Group4 payload width"
    );
    let mut m = ColorEmu::new(32).unwrap();
    m.tick(Tick {
        ce: true,
        input: Some(request(0, true, true, [256, 0, 256, 0], [0xffff; 4])),
        output_ready: true,
    })
    .unwrap();
    let mut failure = None;
    for _ in 0..8 {
        if let Err(e) = m.tick(Tick {
            ce: true,
            input: None,
            output_ready: true,
        }) {
            failure = Some(e);
            break;
        }
    }
    assert_eq!(
        failure.as_deref(),
        Some("color partial outside legal domain")
    );
    assert!(m.faulted());
}
#[test]
fn watchdog_bounds_even_a_frozen_clock() {
    let mut m = ColorEmu::new(2).unwrap();
    for _ in 0..2 {
        m.tick(Tick {
            ce: false,
            input: None,
            output_ready: false,
        })
        .unwrap();
    }
    assert_eq!(
        m.tick(Tick {
            ce: false,
            input: None,
            output_ready: false
        })
        .unwrap_err(),
        "color wall watchdog"
    );
}
