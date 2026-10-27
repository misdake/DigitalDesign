//! Independent actual Runtime context ports, ownership and metadata semantics.
use super::{qualification as q, *};
use crate::texture::emu::lod;
use std::{fmt::Write, path::Path};

fn header(h: derivative::Header) -> u128 {
    u128::from(h.quad)
        | (u128::from(h.mask) << 4)
        | (u128::from(h.slot) << 8)
        | (u128::from(h.max_n) << 12)
        | (u128::from(h.has_mip) << 16)
        | (u128::from(h.filter) << 17)
}
fn raw(i: derivative::Input) -> u128 {
    (i.bias as u16 as u128) | (u128::from(i.force_coarsest) << 16) | (header(i.header) << 17)
}
fn derived(i: lod::Input) -> u128 {
    u128::from(i.slope) | ((i.bias as u16 as u128) << 18) | (header(i.header) << 34)
}
fn ready(i: lod::Output) -> u128 {
    let c = i.context;
    ((c.shift as u32 as u128) & 0x3ffff)
        | (u128::from(c.nearest) << 18)
        | (u128::from(c.halve) << 19)
        | (((c.side[0] as u16 as u128) & 4095) << 20)
        | (((c.side[1] as u16 as u128) & 4095) << 32)
        | (u128::from(c.parents[0]) << 44)
        | (u128::from(c.parents[1]) << 53)
        | (u128::from(c.levels[0]) << 62)
        | (u128::from(c.levels[1]) << 66)
        | (u128::from(c.last_fine) << 70)
        | (u128::from(i.quad) << 71)
        | (u128::from(i.mask) << 75)
        | (u128::from(i.slot) << 79)
}
fn field(word: u128, low: u32, width: u32) -> u128 {
    (word >> low) & ((1 << width) - 1)
}
#[test]
fn context_storage_runtime_calendars() {
    calendars(false);
}
#[test]
#[ignore = "requires Icarus and GOWIN_HOME; all76 actual Runtime calendars"]
fn context_storage_runtime_native_replay() {
    calendars(true);
}
fn calendars(replay_native: bool) {
    const LIMIT: u64 = 50_000;
    let output = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/sampling-d-shared-20261009/runtime-context")
        .join(if replay_native {
            "runtime-rtl"
        } else {
            "runtime"
        });
    std::fs::create_dir_all(&output).unwrap();
    let out = output.as_path();
    let mut cases=String::from("case,contexts,coordinate_credits,early,mode,offer_phase,edges,enabled,accepted,pixels,peak_contexts,max_union_reads,max_union_writes,max_raw_reads,max_derived_reads,max_ready_reads,extra_reclaim_read,scalar_ordinal_checks\n");
    let mut totals = [0usize; 8];
    for (contexts, coordinates, early) in [(1, 1, true), (3, 2, false), (8, 6, true), (8, 6, false)]
    {
        for mode in 0..5 {
            for offer_phase in 0..8 {
                // Mode0 retains all offers; one copy suffices. Other modes target admission phases.
                if (mode == 0 || mode >= 3) && offer_phase != 0 {
                    continue;
                }
                let name = format!("N{contexts}-C{coordinates}-E{early}-M{mode}-P{offer_phase}");
                let mut replay =
                    crate::texture::emu::context_store::qualification::Replay::new_lod_ready(
                        contexts,
                    );
                let (slots, bytes) = q::fixture();
                let mut r = q::runtime(
                    &slots,
                    super::super::control::Hardware {
                        contexts,
                        coordinate_credits: coordinates,
                        release_after_capture: early,
                        max_cycles: LIMIT,
                        ..Default::default()
                    },
                );
                let mut mc = q::physical::Physical::new(4096, bytes, true, mode == 2);
                let input: Vec<_> = (0..48)
                    .map(|i| {
                        q::input(
                            (i % 16) as u8,
                            if mode >= 3 {
                                15
                            } else {
                                [15, 0, 1, 10, 6, 15, 3, 0][(i + i / 16) % 8]
                            },
                            [-0.03125 + (i % 7) as f64 / 32., 0.46875],
                            if mode >= 3 {
                                crate::texture::ports::Filter::Trilinear
                            } else {
                                [
                                    crate::texture::ports::Filter::Nearest,
                                    crate::texture::ports::Filter::Bilinear,
                                    crate::texture::ports::Filter::Trilinear,
                                ][i % 3]
                            },
                        )
                    })
                    .collect();
                let golden: Vec<_> = input.iter().flat_map(q::golden).collect();
                let mut full_trace = String::new();
                let mut pixels = vec![];
                let mut accepted = 0usize;
                let mut rng = 0x38295761u32;
                let mut raw_rows = vec![0u128; contexts];
                let mut derived_rows = raw_rows.clone();
                let mut ready_rows = raw_rows.clone();
                let mut owners = vec![None; contexts];
                let mut kind = vec![0u8; contexts];
                let mut current_ordinal = 0;
                let mut maxima = [0usize; 5];
                let mut extra_reclaim = 0;
                let mut ordinal_checks = 0;
                let mut csv=String::from("wall,enabled_before,CE,Dphase,Lphase,Cphase,allocate,reclaim,Chead,occupied,RawCount,DerivedCount,ReadyCount,raw_R,raw_identity_R,derived_R,derived_identity_R,ready_R,reclaim_R,raw_W,derived_W,ready_W,raw_word,derived_word,ready_word,Cissue,ordinal,scalar_ordinal,union_R,union_W\n");
                for _ in 0..LIMIT {
                    rng ^= rng << 13;
                    rng ^= rng >> 17;
                    rng ^= rng << 5;
                    let before = r.preparation.ordered_state();
                    let (_, _, dp, lp) = r.preparation.d_lod_state();
                    let (_, cp) = r.preparation.coordinate_state();
                    let enabled = r.stats.link.enabled;
                    r.preparation.membership_ready = mode != 3 || r.stats.link.wall > 900;
                    r.preparation.packet_ready = mode != 4 || r.stats.link.wall > 900;
                    let offer = if mode == 0 || mode >= 3 || dp == offer_phase {
                        input.get(accepted)
                    } else {
                        None
                    };
                    let s = r
                        .step(
                            &mut mc,
                            offer,
                            timed::Control {
                                ce: mode != 2 || rng % 7 > 1,
                                result_ready: mode != 2
                                    || (r.stats.link.wall > 600 && rng % 13 > 2),
                            },
                        )
                        .unwrap();
                    let after = r.preparation.ordered_state();
                    let tr = &r.preparation.trace;
                    // Exact payload accesses executed by source. Identity reads stay explicit.
                    let dr = tr.d_input.map(|_| before.cursors[3]);
                    let lr = tr.l_input.map(|_| before.cursors[6]);
                    let cr =
                        (s.effective_ce && before.cursors[10] != 0).then_some(before.cursors[9]);
                    let dw = tr.d_capture.map(|_| before.cursors[5]);
                    let lw = tr.l_capture.map(|_| before.cursors[8]);
                    let rw = s.accepted.then_some(before.cursors[0]);
                    let expected_reads = [
                        dr.or(dw).map(|a| (a, raw_rows[a])),
                        lr.or(lw).map(|a| (a, derived_rows[a])),
                        cr.map(|a| (a, ready_rows[a])),
                    ];
                    let expected_writes =
                        [
                            rw.map(|a| {
                                (
                                    a,
                                    raw(derivative::Input::capture(&input[accepted], slots[0])
                                        .unwrap()),
                                )
                            }),
                            dw.map(|a| (a, derived(lod::Input::from(tr.d_capture.unwrap())))),
                            lw.map(|a| (a, ready(tr.l_capture.unwrap()))),
                        ];
                    assert_eq!(tr.metadata_edge.reads, expected_reads);
                    assert_eq!(
                        tr.metadata_edge.writes,
                        [expected_writes[0], None, expected_writes[2]]
                    );
                    let mut generic_reference_edge = tr.metadata_edge;
                    // The independent generic SRAM reference retains its Derived bank.
                    // Runtime loan RTL is qualified by derived_qualification separately.
                    generic_reference_edge.writes[1] = expected_writes[1];
                    replay.edge(tr.metadata_tick, Ok(generic_reference_edge));
                    assert_eq!(tr.metadata_tick.raw_issue, dr);
                    assert_eq!(tr.metadata_tick.derived_issue, lr);
                    assert_eq!(tr.metadata_tick.ready_read, cr);
                    let reclaim = s.preparation.events.iter().find_map(|e| {
                        if let PreparationEvent::SharedRelease { slot, .. } = e {
                            Some(*slot)
                        } else {
                            None
                        }
                    });
                    let read_counts = [
                        usize::from(dr.is_some()) + usize::from(dw.is_some()),
                        usize::from(lr.is_some()) + usize::from(lw.is_some()),
                        usize::from(cr.is_some()) + usize::from(reclaim.is_some() && reclaim != cr),
                    ];
                    let writes = usize::from(rw.is_some())
                        + usize::from(dw.is_some())
                        + usize::from(lw.is_some());
                    let reads = read_counts.iter().sum::<usize>();
                    for (i, value) in [
                        reads,
                        writes,
                        read_counts[0],
                        read_counts[1],
                        read_counts[2],
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        maxima[i] = maxima[i].max(value);
                    }
                    if reclaim.is_some() && cr.is_some() && reclaim != cr {
                        extra_reclaim += 1;
                    }
                    if writes > 1 {
                        totals[0] += 1;
                    }
                    if reads > 1 {
                        totals[1] += 1;
                    }
                    if let (Some(a), Some(b)) = (dr, dw) {
                        assert_ne!(a, b);
                    }
                    if let (Some(a), Some(b)) = (lr, lw) {
                        assert_ne!(a, b);
                    }
                    // Each split payload bank has one semantic consumer. Return identity
                    // reads are recorded separately, never silently borrowed as another port.
                    for (a, b) in [(dr, rw), (lr, dw), (cr, lw)] {
                        if let (Some(a), Some(b)) = (a, b) {
                            assert_ne!(a, b, "owned split-bank address collision");
                        }
                    }
                    if let Some(slot) = dr {
                        assert_eq!(raw(tr.d_input.unwrap()), raw_rows[slot]);
                        assert_eq!(kind[slot], 1);
                        kind[slot] = 2;
                        assert_eq!(owners[slot], Some(tr.d_input.unwrap().header.quad));
                    }
                    if let Some(slot) = lr {
                        assert_eq!(derived(tr.l_input.unwrap()), derived_rows[slot]);
                        assert_eq!(kind[slot], 3);
                        kind[slot] = 4;
                        assert_eq!(owners[slot], Some(tr.l_input.unwrap().header.quad));
                    }
                    if let Some(slot) = cr {
                        assert_eq!(kind[slot], 5);
                        let w = ready_rows[slot];
                        let ordinal = before.contexts[slot].unwrap().1;
                        assert_eq!(ordinal, current_ordinal, "single C ordinal differs");
                        ordinal_checks += 1;
                        assert_eq!(owners[slot], Some(field(w, 71, 4) as u8));
                        if let Some(i) = tr.c_input {
                            assert_eq!((i.shift as u32 as u128) & 0x3ffff, field(w, 0, 18));
                            assert_eq!(u128::from(i.nearest), field(w, 18, 1));
                            assert_eq!(u128::from(i.halve), field(w, 19, 1));
                            assert_eq!((i.side[0] as u16 as u128) & 4095, field(w, 20, 12));
                            assert_eq!((i.side[1] as u16 as u128) & 4095, field(w, 32, 12));
                            let owner = tr.coordinate_store_tick.issue.unwrap();
                            assert_eq!(
                                owner.parents,
                                [field(w, 44, 9) as u16, field(w, 53, 9) as u16]
                            );
                            assert_eq!(
                                owner.levels,
                                [field(w, 62, 4) as u8, field(w, 66, 4) as u8]
                            );
                            assert_eq!(owner.slot, field(w, 79, 4) as u8);
                            assert_eq!(owner.last_fine, field(w, 70, 1) != 0);
                            current_ordinal += 1;
                            totals[2] += 1;
                        }
                        if after.cursors[9] != before.cursors[9]
                            || after.cursors[10] < before.cursors[10] + usize::from(lw.is_some())
                        {
                            current_ordinal = 0;
                        }
                    }
                    // Non-current payloads are never allowed to hide an independent live ordinal.
                    for (slot, ready_word) in ready_rows.iter().enumerate() {
                        if slot != before.cursors[9] {
                            if let Some((_, next)) = before.contexts[slot] {
                                assert!(
                                    next == 0
                                        || next == field(*ready_word, 75, 4).count_ones() as usize
                                );
                            }
                        }
                    }
                    if let Some(slot) = dw {
                        let d = tr.d_capture.unwrap();
                        assert_eq!(owners[slot], Some(d.header.quad));
                        assert_eq!(kind[slot], 2);
                        derived_rows[slot] = derived(lod::Input::from(d));
                        kind[slot] = 3;
                        totals[3] += 1;
                    }
                    if let Some(slot) = lw {
                        let l = tr.l_capture.unwrap();
                        assert_eq!(owners[slot], Some(l.quad));
                        assert_eq!(kind[slot], 4);
                        ready_rows[slot] = ready(l);
                        kind[slot] = 5;
                        totals[4] += 1;
                    }
                    if let Some(slot) = reclaim {
                        let event = s
                            .preparation
                            .events
                            .iter()
                            .find_map(|e| {
                                if let PreparationEvent::SharedRelease { program, slot: s } = e {
                                    (*s == slot).then_some(*program)
                                } else {
                                    None
                                }
                            })
                            .unwrap();
                        assert_eq!(owners[slot], Some(event as u8));
                        kind[slot] = 0;
                        owners[slot] = None;
                        totals[5] += 1;
                    }
                    if let Some(slot) = rw {
                        assert!(owners[slot].is_none());
                        let d = derivative::Input::capture(&input[accepted], slots[0]).unwrap();
                        raw_rows[slot] = raw(d);
                        assert!(raw_rows[slot] < (1 << 36));
                        owners[slot] = Some(d.header.quad);
                        kind[slot] = 1;
                        accepted += 1;
                        totals[6] += 1;
                    }
                    if !s.effective_ce {
                        assert_eq!(before, after);
                        assert_eq!(reads, 0);
                        assert_eq!(writes, 0);
                        totals[7] += 1;
                    }
                    let addr = |x: Option<usize>| x.map_or(-1, |v| v as i32);
                    writeln!(csv,"{},{},{},{dp},{lp},{cp},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{:x},{:x},{:x},{},{},{},{reads},{writes}",s.cycle,enabled,s.effective_ce,before.cursors[0],before.cursors[1],before.cursors[9],before.cursors[2],before.cursors[4],before.cursors[7],before.cursors[10],addr(dr),addr(dw),addr(lr),addr(lw),addr(cr),addr(reclaim),addr(rw),addr(dw),addr(lw),rw.map_or(0,|a|raw_rows[a]),dw.map_or(0,|a|derived_rows[a]),lw.map_or(0,|a|ready_rows[a]),tr.c_input.is_some(),before.contexts[before.cursors[9]].map_or(0,|x|x.1),current_ordinal).unwrap();
                    writeln!(
                        full_trace,
                        "{}|{}|{}|{:?}|{:?}",
                        s.cycle, s.effective_ce, s.accepted, s.preparation.events, s.results
                    )
                    .unwrap();
                    pixels.extend(s.results);
                    if accepted == input.len() && r.idle() {
                        break;
                    }
                }
                assert!(r.idle());
                assert_eq!(accepted, 48);
                assert_eq!(pixels, golden);
                assert!(owners.iter().all(Option::is_none));
                writeln!(cases,"{name},{contexts},{coordinates},{early},{mode},{offer_phase},{},{},{accepted},{},{},{},{},{},{},{},{extra_reclaim},{ordinal_checks}",r.stats.link.wall,r.stats.link.enabled,pixels.len(),r.preparation.stats.peak_contexts,maxima[0],maxima[1],maxima[2],maxima[3],maxima[4]).unwrap();
                std::fs::write(out.join(format!("{name}.csv")), csv).unwrap();
                std::fs::write(out.join(format!("{name}.full.txt")), full_trace).unwrap();
                let replay_dir = out.join(&name);
                replay.finish(&replay_dir);
                if replay_native {
                    crate::texture::emu::context_store::qualification::run(&replay_dir);
                }
            }
        }
    }
    std::fs::write(out.join("cases.csv"), cases).unwrap();
    std::fs::write(out.join("totals.json"),format!("{{\"multiwrite_edges\":{},\"multiread_edges\":{},\"C_issues\":{},\"D_returns\":{},\"LOD_returns\":{},\"reclaims\":{},\"admissions\":{},\"CE0_edges\":{}}}",totals[0],totals[1],totals[2],totals[3],totals[4],totals[5],totals[6],totals[7])).unwrap();
}
