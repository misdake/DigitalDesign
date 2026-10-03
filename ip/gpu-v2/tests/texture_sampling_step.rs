#[path = "support/sdram/physical_texture.rs"]
#[allow(dead_code)]
mod physical;
#[path = "support/texture.rs"]
#[allow(dead_code)]
mod support;
use gpu_v2::texture::{
    emu::color::Output as ColorOutput,
    ports::*,
    sim::{oracle, staged::bound, timed},
};

fn slots() -> (Vec<Slot>, Vec<u8>) {
    let a = support::slot(5, true);
    let mut bytes = support::asset(a, support::pattern);
    let b = Slot {
        base_address: support::BASE + bytes.len() as u32,
        max_size_log2: 4,
        has_full_mip: true,
        valid: true,
    };
    bytes.extend(support::asset(b, |n, x, y| {
        support::pattern(n, y, x) ^ 0x8410
    }));
    (vec![a, b], bytes)
}

fn quads() -> Vec<QuadInput> {
    (0..24)
        .map(|i| {
            let filter = [Filter::Nearest, Filter::Bilinear, Filter::Trilinear][i % 3];
            let mut q = support::input(5, filter, [0.13, 0.07]);
            q.quad_id = (i % 16) as u8;
            q.mask = [15, 1, 6, 0][i % 4];
            q.slot = (i % 2) as u8;
            q.material_size_log2 = if i % 2 == 0 { 5 } else { 4 };
            q.uv[1][0] += 1.0 / 32.0;
            q.uv[2][1] += 1.0 / 32.0;
            q.uv[3][0] += 1.0 / 32.0;
            q.uv[3][1] += 1.0 / 32.0;
            if i % 5 == 4 {
                q.lod_bias = 0.5;
            }
            if i % 7 == 6 {
                q.uv = [[0.0; 2]; 4];
                q.uv[3][0] = 1.0 / 262144.0;
                q.lod_bias = 13.5;
            }
            q
        })
        .collect()
}

fn expected(inputs: &[QuadInput], slots: &[Slot], bytes: &[u8]) -> Vec<ColorOutput> {
    let mut cache = oracle::Cache::new(slots.to_vec()).unwrap();
    let mut image = support::Image {
        bytes: bytes.to_vec(),
        requests: vec![],
    };
    inputs
        .iter()
        .flat_map(|q| {
            oracle::sample(q, &mut cache, &mut image, Config::counted())
                .unwrap()
                .pixels
                .into_iter()
                .map(|p| ColorOutput {
                    key: q.quad_id * 4 + p.lane,
                    rgb: p.rgb,
                })
        })
        .collect()
}

#[derive(Default)]
struct Summary {
    accepted_edges: u64,
    capture_edges: u64,
    color_accept_edges: u64,
    enabled: u64,
    gated: u64,
    ce_off_edges: u64,
    beats_while_result_closed: u64,
    closed_commits: u64,
    max_color_credits: usize,
    max_skid: usize,
    stable_edges: u64,
}

fn run<F: FnMut(u64) -> timed::Control>(
    inputs: &[QuadInput],
    slots: &[Slot],
    bytes: Vec<u8>,
    loaded: bool,
    max_wall: u64,
    mut control: F,
) -> (
    Vec<ColorOutput>,
    Vec<timed::PixelResult>,
    bound::session::Stats,
    Summary,
) {
    let mut memory = physical::Physical::new(u64::from(support::BASE), bytes, true, loaded);
    let mut s = bound::session::Session::new(
        inputs,
        slots,
        bound::control::Hardware {
            storage: bound::control::Storage::Packed,
            ..Default::default()
        },
        timed::Hardware {
            prefetch: false,
            max_cycles: 100_000,
            max_quads: 64,
            ..Default::default()
        },
        max_wall,
    )
    .unwrap();
    let mut results = vec![];
    let mut commits = vec![];
    let mut summary = Summary::default();
    let mut t = 0_u64;
    let mut held_output = None;
    while !s.idle() {
        t += 1;
        assert!(t <= max_wall, "session did not drain in {max_wall} cycles");
        let ctl = control(t);
        let st = s.step(&mut memory, ctl).unwrap();
        if let Some(expected) = held_output {
            assert_eq!(st.color.output, Some(expected), "unconsumed result changed");
            summary.stable_edges += 1;
        }
        held_output = st.color.output.filter(|_| !ctl.ce || !ctl.result_ready);
        summary.enabled += u64::from(st.effective_ce);
        summary.accepted_edges += u64::from(st.accepted);
        summary.capture_edges += u64::from(
            st.cache
                .events
                .iter()
                .any(|e| matches!(e, timed::Event::Captured { .. })),
        );
        summary.color_accept_edges += u64::from(st.color.accepted);
        if !st.control.ce {
            summary.ce_off_edges += 1;
        }
        if st.control.ce && !st.effective_ce {
            summary.gated += 1;
        }
        if !st.control.result_ready {
            summary.closed_commits += st.results.len() as u64;
            summary.beats_while_result_closed += st
                .cache
                .events
                .iter()
                .filter(|e| matches!(e, timed::Event::Beat { .. }))
                .count() as u64;
        }
        summary.max_color_credits = summary
            .max_color_credits
            .max(st.snapshot.color.result_credits);
        summary.max_skid = summary.max_skid.max(usize::from(st.snapshot.color_input));
        results.extend(st.results.iter().copied());
        commits.extend(st.cache_commits.iter().cloned());
    }
    let stats = s.stats.clone();
    (results, commits, stats, summary)
}

#[test]
fn persistent_session_matches_oracle_across_quads_slots_filters_and_id_reuse() {
    let (slots, bytes) = slots();
    let qs = quads();
    let want = expected(&qs, &slots, &bytes);
    assert!(want.len() > 40, "scenario must cover many pixels");
    let (got, commits, stats, summary) =
        run(&qs, &slots, bytes, false, 20_000, |t| timed::Control {
            ce: t % 17 > 3,
            result_ready: t > 300 && t % 29 > 5,
        });
    assert_eq!(
        got, want,
        "actual ColorEmu results must match the sampler oracle"
    );
    let want_commits: Vec<_> = want
        .iter()
        .map(|o| timed::PixelResult {
            quad_id: o.key / 4,
            lane: o.key % 4,
            rgb: o.rgb,
        })
        .collect();
    assert_eq!(commits, want_commits, "closed cache color cross-check");
    assert!(stats.accepted >= 20, "continuous multi-quad admission");
    assert!(summary.accepted_edges >= 20);
    assert!(summary.capture_edges > 0 && summary.color_accept_edges > 0);
    assert!(summary.ce_off_edges > 0, "CE pause actually exercised");
    assert!(stats.captures >= want.len() as u64);
    assert!(stats.results as usize == want.len());
}

#[test]
fn long_result_backpressure_bounds_skid_gates_cache_and_recovers_in_order() {
    let (slots, bytes) = slots();
    let mut qs = quads();
    qs.truncate(16);
    let want = expected(&qs, &slots, &bytes);
    // Consume a few early results, then hold the result store closed for a long
    // window so both the ColorEmu credits and the local link actually fill.
    let (got, _, stats, summary) = run(&qs, &slots, bytes, true, 30_000, |t| timed::Control {
        ce: true,
        result_ready: !(120..=1_600).contains(&t),
    });
    assert_eq!(
        got, want,
        "long backpressure must not lose or reorder results"
    );
    assert_eq!(
        summary.closed_commits, 0,
        "no result may be consumed while the result store is closed"
    );
    assert!(
        summary.max_skid > 0,
        "the actual captured texels must be held before the result store drains"
    );
    assert_eq!(
        summary.max_skid, 1,
        "one captured-input register, no extra FIFO"
    );
    assert!(
        summary.beats_while_result_closed > 0,
        "accepted refill beats must drain while results are backpressured"
    );
    assert_eq!(
        summary.max_color_credits,
        gpu_v2::texture::emu::color::RESULT_CAPACITY,
        "credit reaches the real upper limit before blocking"
    );
    assert!(stats.color_stalls > 0, "the color link actually stalls");
    assert!(
        summary.stable_edges > 0,
        "actual blocked outputs remain stable"
    );
    assert!(
        summary.gated > 0,
        "the bounded link actually freezes the cache during backpressure"
    );
}

#[test]
fn cold_miss_and_ce_pause_keep_order_and_run_during_pause() {
    let (slots, bytes) = slots();
    let qs = quads();
    let want = expected(&qs, &slots, &bytes);
    // Cold start with foreground CE gated: refill completion must still advance.
    let (got, _, stats, summary) = run(&qs, &slots, bytes, false, 40_000, |t| timed::Control {
        ce: t % 23 > 7,
        result_ready: t > 900 && t % 31 > 7,
    });
    assert_eq!(got, want);
    assert!(stats.captures > 0);
    assert!(summary.ce_off_edges > 0);
    assert!(stats.color_gated_edges > 0);
}

#[test]
fn session_wall_fault_is_terminal_and_a_new_instance_runs() {
    let (slots, bytes) = slots();
    let qs = quads();
    let want = expected(&qs, &slots, &bytes);
    let mut memory = physical::Physical::new(u64::from(support::BASE), bytes.clone(), true, false);
    let mut s = bound::session::Session::new(
        &qs,
        &slots,
        bound::control::Hardware {
            storage: bound::control::Storage::Packed,
            ..Default::default()
        },
        timed::Hardware {
            prefetch: false,
            max_cycles: 100_000,
            max_quads: 64,
            ..Default::default()
        },
        8,
    )
    .unwrap();
    let err = (0..64)
        .find_map(|_| {
            s.step(
                &mut memory,
                timed::Control {
                    ce: true,
                    result_ready: false,
                },
            )
            .err()
        })
        .expect("bounded session watchdog must fire");
    assert!(err.contains("watchdog"), "unexpected terminal cause: {err}");
    assert!(s.faulted());
    assert!(s
        .step(&mut memory, timed::Control::default())
        .unwrap_err()
        .contains("terminal fault"));
    // The corrected approved instance semantics: recreate instead of in-place recovery.
    let (got, _, _, _) = run(&qs, &slots, bytes, false, 20_000, |_| {
        timed::Control::default()
    });
    assert_eq!(got, want);
}

#[test]
fn quad_identity_waits_for_the_actual_result_consumer() {
    let (slots, bytes) = slots();
    let mut a = support::input(5, Filter::Nearest, [0.07, 0.11]);
    a.quad_id = 3;
    a.mask = 1;
    let mut b = a.clone();
    b.uv = [[0.71, 0.63]; 4];
    let qs = [a, b];
    let want = expected(&qs, &slots, &bytes);
    assert_eq!(want.len(), 2);
    assert_ne!(want[0].rgb, want[1].rgb, "reuse must carry different data");
    let mut memory = physical::Physical::new(u64::from(support::BASE), bytes, true, false);
    let mut session = bound::session::Session::new(
        &qs,
        &slots,
        bound::control::Hardware {
            storage: bound::control::Storage::Packed,
            ..Default::default()
        },
        timed::Hardware {
            prefetch: false,
            max_cycles: 20_000,
            ..Default::default()
        },
        20_000,
    )
    .unwrap();
    let mut shadow_committed = false;
    let mut actual_queued = false;
    for _ in 0..2_000 {
        let st = session
            .step(
                &mut memory,
                timed::Control {
                    ce: true,
                    result_ready: false,
                },
            )
            .unwrap();
        shadow_committed |= !st.cache_commits.is_empty();
        actual_queued |= st.color.output.is_some();
        assert!(st.results.is_empty());
        assert!(
            session.stats.accepted <= 1,
            "shadow commit released a live public ID"
        );
        if actual_queued {
            assert_eq!(st.snapshot.result_lanes[3], 1);
        }
    }
    assert!(
        shadow_committed && actual_queued,
        "both ownership boundaries exercised"
    );
    assert_eq!(session.stats.accepted, 1);
    let mut results = Vec::new();
    for _ in 0..10_000 {
        let st = session
            .step(&mut memory, timed::Control::default())
            .unwrap();
        results.extend(st.results);
        if session.idle() {
            break;
        }
    }
    assert!(session.idle());
    assert_eq!(
        session.stats.accepted, 2,
        "real reuse happened after consumption"
    );
    assert_eq!(results, want);
}
