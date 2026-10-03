#[path = "support/sdram/physical_texture.rs"]
#[allow(dead_code)]
mod physical;
#[path = "support/texture.rs"]
#[allow(dead_code)]
mod support;
use gpu_v2::texture::{
    emu::color::{Output, RESULT_CAPACITY},
    ports::*,
    sim::{oracle, staged::bound, timed},
};

fn fixture() -> (Vec<Slot>, Vec<u8>) {
    let a = support::slot(5, true);
    let mut bytes = support::asset(a, support::pattern);
    let b = Slot {
        base_address: support::BASE + bytes.len() as u32,
        max_size_log2: 4,
        has_full_mip: false,
        valid: true,
    };
    bytes.extend(support::asset(b, |n, x, y| {
        support::pattern(n, y, x) ^ 0x8410
    }));
    (vec![a, b], bytes)
}

fn runtime(slots: &[Slot], max_wall: u64) -> bound::runtime::Runtime {
    bound::runtime::Runtime::new(
        slots,
        bound::control::Hardware {
            storage: bound::control::Storage::Packed,
            ..Default::default()
        },
        timed::Hardware {
            prefetch: false,
            // Runtime capacity is real live storage, not a constructor list.
            max_quads: 1,
            ..Default::default()
        },
        max_wall,
    )
    .unwrap()
}

fn expected(inputs: &[QuadInput], slots: &[Slot], bytes: &[u8]) -> Vec<Output> {
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
                .map(|p| Output {
                    key: q.quad_id * 4 + p.lane,
                    rgb: p.rgb,
                })
        })
        .collect()
}

fn check_bounds(s: &bound::runtime::Step) {
    assert_eq!(
        s.accepted,
        s.offered.is_some() && s.input_ready && s.control.ce
    );
    assert_eq!(s.accepted, s.cache.accepted);
    assert_eq!(s.accepted, s.preparation.accepted);
    assert!(s.snapshot.preparation_live.count_ones() <= 16);
    assert!(s.snapshot.cache.descriptors <= 4);
    assert!(s.snapshot.color.result_credits <= RESULT_CAPACITY);
    let p = s.snapshot.cache.packet_pool.as_ref().unwrap();
    assert!(p.producer <= 16 && p.groups <= 32 && p.rows <= 64);
    assert!(p.heads + usize::from(p.pending) <= 2);
    if !s.control.ce || !s.control.result_ready {
        assert!(s.results.is_empty());
    }
}

#[test]
fn runtime_streams_different_content_and_masks_without_a_precompiled_input_list() {
    let (slots, bytes) = fixture();
    let mut r = runtime(&slots, 40_000);
    let mut memory = physical::Physical::new(u64::from(support::BASE), bytes.clone(), true, true);
    let mut accepted = vec![];
    let mut got = vec![];
    let mut offered = support::input(5, Filter::Nearest, [0.13, 0.07]);
    let mut ce_rejected = 0;
    let mut busy_rejected = 0;
    for t in 1..=40_000 {
        let st = r
            .step(
                &mut memory,
                (accepted.len() < 48).then_some(&offered),
                timed::Control {
                    ce: t % 17 > 3,
                    result_ready: t > 500 && t % 29 > 5,
                },
            )
            .unwrap();
        check_bounds(&st);
        ce_rejected += usize::from(st.offered.is_some() && !st.control.ce);
        busy_rejected += usize::from(st.offered.is_some() && st.control.ce && !st.accepted);
        got.extend(st.results);
        if st.accepted {
            accepted.push(offered.clone());
            // Generate the next payload only after acceptance. Runtime has no
            // access to future inputs or to the oracle/checker below.
            let i = accepted.len();
            offered.quad_id = (i % 16) as u8;
            offered.mask = [15, 1, 6, 0, 9][i % 5];
            offered.slot = (i % 2) as u8;
            offered.material_size_log2 = slots[usize::from(offered.slot)].max_size_log2;
            offered.filter = [Filter::Nearest, Filter::Bilinear, Filter::Trilinear][i % 3];
            offered.uv = [[0.13 + (i % 7) as f64 / 32.0, -0.07]; 4];
            offered.uv[1][0] += 1.0 / 32.0;
            offered.uv[2][1] += 1.0 / 32.0;
            offered.uv[3][0] += 1.0 / 32.0;
            offered.uv[3][1] += 1.0 / 32.0;
            offered.lod_bias = [0.0, 0.5, -32.0, 10.5][i % 4];
            if i % 7 == 6 {
                offered.uv = [[0.0; 2]; 4];
                offered.uv[3][0] = 1.0 / 262144.0;
            }
        }
        if accepted.len() == 48 && r.idle() {
            break;
        }
    }
    assert!(r.idle(), "runtime stream failed to drain");
    assert_eq!(accepted.len(), 48);
    assert_eq!(r.stats.compilations, 48);
    assert!(ce_rejected > 0 && busy_rejected > 0);
    assert!(r.stats.peak_preparation_programs <= 16);
    assert!(r.preparation_stats().peak_contexts <= 8);
    assert_eq!(got, expected(&accepted, &slots, &bytes));
    assert_eq!(
        memory.cycles, r.stats.link.wall,
        "one MC step per wall edge"
    );
}

#[test]
fn rejected_payload_can_be_replaced_and_same_key_reuse_waits_for_actual_consumption() {
    let (slots, bytes) = fixture();
    let mut r = runtime(&slots, 15_000);
    let mut memory = physical::Physical::new(u64::from(support::BASE), bytes.clone(), true, false);
    let mut a = support::input(5, Filter::Nearest, [0.07, 0.11]);
    a.quad_id = 3;
    a.mask = 1;
    // Even invalid arithmetic data on a CE-off edge is caller-owned; no host
    // compiler is invoked. Replace it with valid content before acceptance.
    let mut rejected = a.clone();
    rejected.uv = [[f64::NAN; 2]; 4];
    let st = r
        .step(
            &mut memory,
            Some(&rejected),
            timed::Control {
                ce: false,
                result_ready: false,
            },
        )
        .unwrap();
    assert!(st.input_ready && !st.accepted);
    assert_eq!(r.stats.compilations, 0);
    assert!(
        r.step(&mut memory, Some(&a), timed::Control::default())
            .unwrap()
            .accepted
    );
    let mut shadow = false;
    let mut queued = false;
    for _ in 0..1_500 {
        let st = r
            .step(
                &mut memory,
                Some(&rejected),
                timed::Control {
                    ce: true,
                    result_ready: false,
                },
            )
            .unwrap();
        check_bounds(&st);
        assert!(!st.accepted && !st.input_ready && st.results.is_empty());
        shadow |= !st.cache_commits.is_empty();
        queued |= st.color.output.is_some();
    }
    assert!(
        shadow && queued,
        "shadow retirement and actual held result exercised"
    );
    assert_eq!(
        r.stats.compilations, 1,
        "rejected data must not be compiled"
    );
    assert_eq!(r.snapshot().result_lanes[3], 1);
    assert_eq!(r.snapshot().preparation_live, 0);
    assert_eq!(r.snapshot().preparation_masks, [0; 16]);
    let mut replacement = a.clone();
    replacement.uv = [[0.71, 0.63]; 4];
    let mut replacement_accepted = None;
    let mut first_consumed = None;
    let mut got = vec![];
    for _ in 0..10_000 {
        let st = r
            .step(
                &mut memory,
                replacement_accepted.is_none().then_some(&replacement),
                timed::Control::default(),
            )
            .unwrap();
        check_bounds(&st);
        if !st.results.is_empty() && first_consumed.is_none() {
            first_consumed = Some(st.cycle);
        }
        if st.accepted {
            replacement_accepted = Some(st.cycle);
        }
        got.extend(st.results);
        if replacement_accepted.is_some() && r.idle() {
            break;
        }
    }
    assert!(r.idle());
    assert!(replacement_accepted.unwrap() > first_consumed.unwrap());
    assert_eq!(r.stats.compilations, 2);
    let want = expected(&[a, replacement], &slots, &bytes);
    assert_eq!(want[0].key, want[1].key);
    assert_ne!(want[0].rgb, want[1].rgb);
    assert_eq!(
        got, want,
        "only accepted replacement data may produce results"
    );
}

#[test]
fn real_full_result_credits_backpressure_and_ce_pause_keep_mc_beats_draining() {
    let (slots, bytes) = fixture();
    let mut r = runtime(&slots, 40_000);
    let mut memory = physical::Physical::new(u64::from(support::BASE), bytes.clone(), true, true);
    let mut accepted = vec![];
    let mut got = vec![];
    let mut offered = support::input(5, Filter::Bilinear, [0.0; 2]);
    offered.quad_id = 0;
    let mut held = None;
    let mut stable = 0;
    let mut closed_beats = 0;
    let mut paused_beats = 0;
    let mut peak_live = 0;
    let mut peak_producer = 0;
    let mut peak_groups = 0;
    let mut peak_heads = 0;
    for t in 1..=40_000 {
        let control = timed::Control {
            ce: t % 23 > 7,
            result_ready: t > 7_000,
        };
        let st = r
            .step(
                &mut memory,
                (accepted.len() < 16).then_some(&offered),
                control,
            )
            .unwrap();
        check_bounds(&st);
        if let Some(old) = held {
            assert_eq!(
                st.color.output,
                Some(old),
                "blocked output must remain stable"
            );
            stable += 1;
        }
        held = st
            .color
            .output
            .filter(|_| !control.ce || !control.result_ready);
        let beats = st
            .cache
            .events
            .iter()
            .filter(|e| matches!(e, timed::Event::Beat { .. }))
            .count();
        closed_beats += usize::from(!control.result_ready) * beats;
        paused_beats += usize::from(!control.ce) * beats;
        peak_live = peak_live.max(st.snapshot.result_lanes.iter().filter(|m| **m != 0).count());
        let p = st.snapshot.cache.packet_pool.as_ref().unwrap();
        peak_producer = peak_producer.max(p.producer);
        peak_groups = peak_groups.max(p.groups);
        peak_heads = peak_heads.max(p.heads + usize::from(p.pending));
        got.extend(st.results);
        if st.accepted {
            accepted.push(offered.clone());
            let i = accepted.len();
            offered.quad_id = (i % 16) as u8;
            offered.uv = [[(i % 4) as f64 / 4.0, (i / 4) as f64 / 4.0]; 4];
        }
        if accepted.len() == 16 && r.idle() {
            break;
        }
    }
    assert!(r.idle());
    assert_eq!(accepted.len(), 16);
    assert_eq!(r.stats.link.peak_color_credits, RESULT_CAPACITY);
    assert_eq!(r.stats.link.peak_captured_input, 1);
    assert!(r.stats.link.color_gated_edges > 0 && r.stats.rejected > 0);
    assert!(closed_beats > 0 && paused_beats > 0 && stable > 0);
    assert_eq!(
        peak_live, 16,
        "all public quad identities must actually be owned"
    );
    assert_eq!(peak_producer, 16, "actual P16 full condition");
    assert_eq!(peak_groups, 32, "actual G32 full condition");
    assert_eq!(peak_heads, 2);
    assert_eq!(got, expected(&accepted, &slots, &bytes));
    println!("full credits: P={peak_producer} G={peak_groups} heads={peak_heads} ids={peak_live} paused_beats={paused_beats} closed_beats={closed_beats}");
}

#[test]
fn partial_helper_derivatives_empty_mask_default_omission_and_warm_reuse() {
    let (slots, bytes) = fixture();
    let mut r = runtime(&slots, 20_000);
    let mut memory = physical::Physical::new(u64::from(support::BASE), bytes.clone(), true, false);
    // The upstream default branch produces its own constant and offers nothing.
    for _ in 0..32 {
        let st = r
            .step(&mut memory, None, timed::Control::default())
            .unwrap();
        assert!(st.results.is_empty() && !st.accepted && r.idle());
    }
    assert_eq!(r.stats.compilations, 0);
    assert_eq!(memory.requests, 0);
    let mut fine = support::input(5, Filter::Bilinear, [0.13, 0.07]);
    fine.mask = 1;
    let mut partial = fine.clone();
    partial.uv[3][0] += 0.25; // uncovered helper must change the covered pixel LOD
    let mut zero = partial.clone();
    zero.mask = 0;
    let qs = [partial.clone(), zero, partial.clone(), fine];
    let mut got = vec![];
    let mut request_counts = vec![];
    for q in &qs {
        assert!(r.input_ready(q.quad_id));
        assert!(
            r.step(&mut memory, Some(q), timed::Control::default())
                .unwrap()
                .accepted
        );
        for _ in 0..5_000 {
            let st = r
                .step(&mut memory, None, timed::Control::default())
                .unwrap();
            check_bounds(&st);
            got.extend(st.results);
            if r.idle() {
                break;
            }
        }
        assert!(r.idle());
        assert_eq!(r.snapshot().preparation_masks, [0; 16]);
        request_counts.push(memory.requests);
    }
    assert_eq!(
        request_counts[0], request_counts[1],
        "mask0 produces no refill"
    );
    assert_eq!(
        request_counts[0], request_counts[2],
        "same instance retains warm cache"
    );
    assert_eq!(got.len(), 3, "only the covered lane is public");
    assert_ne!(got[0].rgb, got[2].rgb, "uncovered helper must affect LOD");
    assert_eq!(got, expected(&qs, &slots, &bytes));
}

#[test]
fn terminal_input_fault_leaves_external_mc_work_for_separate_drain() {
    let (slots, bytes) = fixture();
    let mut r = runtime(&slots, 10_000);
    let mut memory = physical::Physical::new(u64::from(support::BASE), bytes, true, false);
    let mut q = support::input(5, Filter::Nearest, [0.13, 0.07]);
    q.quad_id = 0;
    q.mask = 1;
    assert!(
        r.step(&mut memory, Some(&q), timed::Control::default())
            .unwrap()
            .accepted
    );
    let mut transaction = None;
    for _ in 0..2_000 {
        let st = r
            .step(&mut memory, None, timed::Control::default())
            .unwrap();
        transaction = st.cache.events.iter().find_map(|e| match e {
            timed::Event::Submitted { id, .. } => Some(*id),
            _ => None,
        });
        if transaction.is_some() {
            break;
        }
    }
    let transaction = transaction.expect("real refill request accepted before fault");
    let mut bad = q.clone();
    bad.quad_id = 1;
    bad.uv = [[f64::NAN; 2]; 4];
    assert!(r.input_ready(bad.quad_id));
    assert!(r
        .step(&mut memory, Some(&bad), timed::Control::default())
        .is_err());
    assert!(r.faulted() && !r.idle() && !r.input_ready(2));
    let stopped = memory.cycles;
    assert!(r
        .step(&mut memory, None, timed::Control::default())
        .unwrap_err()
        .contains("terminal fault"));
    assert_eq!(
        stopped, memory.cycles,
        "fault is not fake continued completion"
    );
    let mut drained = false;
    let mut beats = 0;
    for _ in 0..2_000 {
        for e in memory.step().unwrap() {
            match e {
                RefillEvent::Beat { id, .. } if id == transaction => beats += 1,
                RefillEvent::Complete { id } if id == transaction => drained = true,
                _ => {}
            }
        }
        if drained {
            break;
        }
    }
    assert!(
        drained && beats == 16,
        "external owner drains all accepted beats"
    );
    assert_eq!(r.stats.link.results, 0);
}

#[test]
fn runtime_wall_watchdog_is_terminal_even_when_no_input_is_offered() {
    let (slots, bytes) = fixture();
    let mut r = runtime(&slots, 2);
    let mut memory = physical::Physical::new(u64::from(support::BASE), bytes, true, false);
    for _ in 0..2 {
        r.step(&mut memory, None, timed::Control::default())
            .unwrap();
    }
    assert!(r
        .step(&mut memory, None, timed::Control::default())
        .unwrap_err()
        .contains("watchdog"));
    assert!(r.faulted() && !r.idle());
    assert_eq!(bound::runtime::LINK_STATE_BITS, 266);
}
