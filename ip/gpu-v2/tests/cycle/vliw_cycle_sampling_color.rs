//! Actual six-DSP Color with start/ACK DO loans and direct Public16 completion.
#[allow(dead_code)]
#[path = "../support/vliw_pixel_source.rs"]
mod actual;
#[path = "../support/vliw_sampling_color.rs"]
mod fixture;
#[allow(dead_code)]
#[path = "../support/vliw_cycle/mod.rs"]
mod transport_support;
#[allow(dead_code)]
#[path = "../support/vliw_vertex/mod.rs"]
mod vertex_support;
use gpu_v2::system::vliw_cycle::{
    sampling::{color::*, word::Word},
    vertex::Bits,
};
#[derive(Clone)]
struct Group {
    quad: usize,
    payload: Word,
    texels: [Bits; 4],
}
#[derive(Clone)]
struct Edge {
    i: Input,
    o: Output,
    s: Color,
}
fn mask(q: usize) -> u8 {
    [1, 5, 9, 15][q % 4]
}
fn data(epoch: usize, rgba4444: bool) -> (Vec<Group>, Vec<(u8, u32)>) {
    let mut groups = Vec::new();
    let mut expected = Vec::new();
    for q in 0..48 {
        for lane in 0..4 {
            if mask(q) & (1 << lane) == 0 {
                continue;
            }
            let id = q * 4 + lane;
            let coarse = [0u64, 64, 128, 256, 447][id % 5];
            let split = id % 3 == 1;
            let mut pixel_groups = Vec::new();
            let mut total = [0u32; 3];
            let mut alpha = 0u64;
            for plane in 0..2 {
                let parent = if plane == 0 { 511 - coarse } else { coarse };
                if parent == 0 {
                    continue;
                }
                let u = [0, 1, 64, 127, 128, 255][(id + plane) % 6];
                let v = [255, 128, 127, 64, 1, 0][(id / 5 + plane) % 6];
                let bottom = parent * v / 256;
                let top = parent - bottom;
                let br = bottom * u / 256;
                let tr = top * u / 256;
                let weights = [top - tr, tr, bottom - br, br];
                let texels = std::array::from_fn::<_, 4, _>(|t| {
                    [0u16, 0xffff, 0xf800, 0x07e0, 0x001f, 0x39e7, 0x7bef, 0xa54b]
                        [(id + plane * 3 + t * 5 + epoch) % 8]
                });
                for (t, &raw) in texels.iter().enumerate() {
                    let channels = if rgba4444 {
                        std::array::from_fn(|c| ((raw >> (12 - 4 * c)) & 15) * 17)
                    } else {
                        [
                            ((raw >> 11 & 31) << 3) | (raw >> 13),
                            ((raw >> 5 & 63) << 2) | (raw >> 9 & 3),
                            ((raw & 31) << 3) | (raw >> 2 & 7),
                        ]
                    };
                    alpha += weights[t] * u64::from(raw & 15);
                    for (c, v) in channels.into_iter().enumerate() {
                        total[c] += ((weights[t] * u64::from(v) + 256) / 512) as u32;
                    }
                }
                let taps: Vec<u8> = if split { vec![1, 2, 4, 8] } else { vec![15] };
                for selected in taps {
                    if !(0..4).any(|t| selected & (1 << t) != 0 && weights[t] != 0) {
                        continue;
                    }
                    let mut word = Word::new(0, 72);
                    for (t, w) in weights.iter().enumerate() {
                        word.set(
                            28 + 9 * t as u32,
                            9,
                            Bits::new(if selected & (1 << t) != 0 { *w } else { 0 }, 9),
                        );
                    }
                    word.set(66, 4, Bits::new((q % 16) as u64, 4));
                    word.set(70, 2, Bits::new(lane as u64, 2));
                    pixel_groups.push(Group {
                        quad: q,
                        payload: word,
                        texels: texels.map(|v| Bits::new(v.into(), 16)),
                    });
                }
            }
            pixel_groups
                .first_mut()
                .unwrap()
                .payload
                .set(64, 1, Bits::new(1, 1));
            pixel_groups
                .last_mut()
                .unwrap()
                .payload
                .set(65, 1, Bits::new(1, 1));
            assert!(total.iter().all(|v| *v <= 255));
            expected.push((
                ((q % 16) * 4 + lane) as u8,
                total[0]
                    | (total[1] << 8)
                    | (total[2] << 16)
                    | ((if rgba4444 {
                        ((17 * alpha + 255) / 511) as u32
                    } else {
                        255
                    }) << 24),
            ));
            groups.extend(pixel_groups);
        }
    }
    (groups, expected)
}
fn run(native: bool) -> Vec<Edge> {
    run_format(native, false)
}
fn run_format(native: bool, rgba4444: bool) -> Vec<Edge> {
    let mut d = if native {
        Color::native()
    } else {
        Color::default()
    };
    let mut rows = Vec::new();
    let mut peak = 0;
    let mut pauses = 0;
    let mut total_results = 0;
    for epoch in 0..2 {
        let (groups, expected) = data(epoch, rgba4444);
        let mut group = 0;
        let mut published = 0;
        let mut result = 0;
        let mut complete = false;
        for wall in 0..6000 {
            let reset = wall == 0;
            let proposal_ce = !reset && wall % 7 != 3;
            let commit_allow = wall % 11 != 4;
            let next = groups.get(group);
            let mut i = Input {
                rgba4444,
                reset,
                proposal_ce,
                commit_allow,
                check_quad: (published % 16) as u8,
                publish_quad: (published % 16) as u8,
                publish_mask: mask(published),
                valid: next.is_some_and(|g| g.quad < published),
                payload: next.map_or(Word::default(), |g| g.payload),
                texels: next.map_or([Bits::default(); 4], |g| g.texels),
                out_ready: proposal_ce && commit_allow,
                ..Default::default()
            };
            i.publish_valid = published < 48 && d.outputs(i).public_free;
            let o = d.outputs(i);
            if !reset {
                if o.valid {
                    let (k, rgb) = expected[result];
                    assert_eq!(o.key, Bits::new(k.into(), 6), "key epoch{epoch} wall{wall}");
                    assert_eq!(
                        o.rgb,
                        Bits::new(rgb.into(), 24),
                        "RGB epoch{epoch} wall{wall} key{k}"
                    );
                    assert_eq!(
                        o.alpha,
                        Bits::new(u64::from(rgb >> 24), 8),
                        "alpha epoch{epoch} wall{wall} key{k}"
                    );
                    assert_eq!(
                        o.last,
                        (mask(result_quad(&expected, result)) >> (k % 4 + 1)) == 0
                    );
                    if o.result_fire {
                        result += 1;
                        total_results += 1;
                    } else {
                        pauses += 1;
                    }
                }
                if o.publish_fire {
                    published += 1;
                }
                if o.input_fire {
                    group += 1;
                }
            }
            rows.push(Edge { i, o, s: d.clone() });
            d.clock(i)
                .unwrap_or_else(|e| panic!("epoch{epoch} wall{wall} group{group}: {e}"));
            peak = peak.max(d.live.count_ones());
            if epoch == 0 && wall == 527 {
                break;
            }
            if result == expected.len() && d.live == 0 && !d.busy && !d.done && !d.q_done {
                assert!(group == groups.len() && published == 48);
                complete = true;
                break;
            }
        }
        if epoch == 1 {
            assert!(complete);
        }
    }
    assert!(peak == 16 && pauses > 10 && total_results > 120);
    println!("COLOR6 native={native} walls={} pixels={total_results} live_peak={peak} held_results={pauses}",rows.len());
    rows
}
fn result_quad(expected: &[(u8, u32)], at: usize) -> usize {
    let mut quad = 0;
    let mut rank = 0;
    while rank + mask(quad).count_ones() as usize <= at {
        rank += mask(quad).count_ones() as usize;
        quad += 1;
    }
    assert!(at < expected.len());
    quad
}
#[test]
fn direct_color_matches_independent_per_tap_integer_rounding() {
    for native in [false, true] {
        for rgba4444 in [false, true] {
            run_format(native, rgba4444);
        }
    }
}
#[test]
fn publication_without_old_credit_is_not_accepted() {
    let mut c = Color::default();
    let i = Input {
        proposal_ce: true,
        commit_allow: true,
        publish_valid: true,
        publish_quad: 0,
        publish_mask: 1,
        ..Input::default()
    };
    assert!(c.outputs(i).publish_fire);
    c.clock(i).unwrap();
    assert!(!c.outputs(i).publish_fire);
    c.clock(i).unwrap();
    assert_eq!(c.live, 1);
}
fn root(label: &str) -> std::path::PathBuf {
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-cycle-draw")
        .join(format!(
            "{label}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&p).unwrap();
    p
}
#[test]
#[ignore = "two actual six-MULT9X9/ALU54D Color profiles"]
fn source_loan_and_direct_result_match_actual_color() {
    let root = root("color6");
    println!("COLOR6_ROOT {}", root.display());
    for native in [false, true] {
        for rgba4444 in [false, true] {
            let rows = run_format(native, rgba4444);
            let p = fixture::fields(&rows[0]);
            vertex_support::rtl::replay(
                &root,
                &format!("native{native}-rgba{rgba4444}"),
                rows.iter().map(fixture::fields).collect(),
                fixture::production(&p, native),
                false,
                None,
            );
        }
    }
}
#[test]
#[ignore = "three compiled tap-pair/public-owner mutations and completion monitor"]
fn tap_pair_loans_public_credit_and_completion_detect_counterexamples() {
    let rows = run(false);
    let root = root("color6-negative");
    println!("COLOR6_NEGATIVE_ROOT {}", root.display());
    let p = fixture::fields(&rows[0]);
    let source = fixture::production(&p, false);
    for(name,old,new)in[
        ("first_pair_load", "wire accum_add=!(accepted&&first_in);", "wire accum_add=1'b1;"),
        ("second_pair_uses_live_DO", "wire[8:0]w=busy?in_payload[46+9*t+:9]:in_payload[28+9*t+:9];", "wire[8:0]w=in_payload[28+9*t+:9];"),
        ("old_public_credit", "assign publish_ready=!reset && !fault_eff && !live[publish_quad] && (publish_mask!=0);", "assign publish_ready=!reset && !fault_eff && (publish_mask!=0);"),
    ]{
        assert_eq!(source.matches(old).count(),1,"{name}");
        vertex_support::rtl::replay(&root,name,rows.iter().map(fixture::fields).collect(),source.replacen(old,new,1),true,None);
    }
    monitor(&root, &rows, &source, true);
}
#[test]
#[ignore = "original unowned-input monitor while proposal commit is refused"]
fn unowned_color_proposal_is_monitored_without_commit() {
    let rows = run(false);
    let root = root("color6-monitor");
    println!("COLOR6_MONITOR_ROOT {}", root.display());
    let source = fixture::production(&fixture::fields(&rows[0]), false);
    monitor(&root, &rows, &source, false);
}
fn monitor(root: &std::path::Path, rows: &[Edge], source: &str, refusal: bool) {
    let at = rows
        .iter()
        .position(|e| {
            if refusal {
                e.o.valid && e.o.advance
            } else {
                e.s.busy && e.s.live != u16::MAX
            }
        })
        .unwrap();
    let mut d = rows[at].s.clone();
    let mut i = rows[at].i;
    if refusal {
        i.out_ready = false;
    } else {
        i.proposal_ce = true;
        i.commit_allow = false;
        i.out_ready = false;
        i.publish_valid = false;
        i.valid = true;
        i.payload
            .set(66, 4, Bits::new(u64::from((!d.live).trailing_zeros()), 4));
    }
    let o = d.outputs(i);
    let expected = if refusal {
        "closed Color result bank did not accept completion"
    } else {
        "GPU contract: preflight_fault"
    };
    assert_eq!(d.clock(i).unwrap_err().rule, expected);
    let mut tape: Vec<_> = rows[..at].iter().map(fixture::fields).collect();
    tape.push(fixture::fields(&Edge {
        i,
        o,
        s: rows[at].s.clone(),
    }));
    vertex_support::rtl::replay(
        root,
        if refusal {
            "result_refusal"
        } else {
            "unowned_input_without_commit"
        },
        tape,
        source.to_owned(),
        false,
        Some(expected),
    );
}
