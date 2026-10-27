//! All84 actual calendars versus the preserved generic Pool and scalar pixels.
use super::{qualification as q, *};
use crate::texture::{
    emu::packet_store_ordered::{
        qualification::{run, Replay},
        Edge, Tick,
    },
    ports::Filter,
};
use std::{fmt::Write, path::Path};
fn calendars(rtl: bool) {
    const LIMIT: u64 = 50_000;
    let root = std::env::var_os("GPU_ORDERED_PACKET_STORAGE_OUTPUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/sampling-runtime-pool-integration")
        });
    let out = root.join(if rtl { "runtime-rtl" } else { "runtime" });
    std::fs::create_dir_all(&out).unwrap();
    let mut cases = String::from("case,edges,accepted,pixels,packets\n");
    for (contexts, coordinates, early) in [(1, 1, true), (3, 2, false), (8, 6, true), (8, 6, false)]
    {
        for mode in 0..7 {
            for phase in 0..8 {
                if (mode == 0 || mode >= 3) && phase != 0 {
                    continue;
                }
                let name = format!("N{contexts}-C{coordinates}-E{early}-M{mode}-P{phase}");
                let (mut slots, mut bytes) = q::fixture();
                if mode == 5 {
                    let template = bytes.clone();
                    let first = slots[0];
                    for i in 1..16 {
                        let mut s = first;
                        s.base_address += i * template.len() as u32;
                        slots.push(s);
                        bytes.extend(&template);
                    }
                }
                let config = super::super::control::Hardware {
                    contexts,
                    coordinate_credits: coordinates,
                    release_after_capture: early,
                    max_cycles: LIMIT,
                    ..Default::default()
                };
                let mut r = q::runtime(&slots, config);
                let mut reference = q::runtime(&slots, config);
                reference.cache.packet_legacy_reference();
                let mut mc =
                    q::physical::Physical::new(4096, bytes.clone(), true, mode == 2 || mode == 5);
                let mut reference_mc =
                    q::physical::Physical::new(4096, bytes, true, mode == 2 || mode == 5);
                let inputs: Vec<_> = (0..if mode >= 5 { 96 } else { 48 })
                    .map(|i| {
                        let mut v = q::input(
                            (i % 16) as u8,
                            if mode >= 3 {
                                15
                            } else {
                                [15, 0, 1, 10, 6, 15, 3, 0][(i + i / 16) % 8]
                            },
                            if mode == 6 {
                                [0.1, 0.1]
                            } else {
                                [-0.03125 + (i % 7) as f64 / 32., 0.46875]
                            },
                            if mode >= 3 {
                                Filter::Trilinear
                            } else {
                                [Filter::Nearest, Filter::Bilinear, Filter::Trilinear][i % 3]
                            },
                        );
                        if mode == 5 {
                            v.slot = (i % 16) as u8;
                        }
                        v
                    })
                    .collect();
                let golden: Vec<_> = inputs.iter().flat_map(q::golden).collect();
                let (mut accepted, mut packets) = (0, 0);
                let mut pixels = vec![];
                let mut rng = 0x38295761u32;
                let mut full = String::new();
                let mut events = String::new();
                let mut replay = Replay::new();
                let mut backend = super::backend_qualification::Replay::new();
                let initial = r.cache.packet_storage_state().unwrap();
                let mut producer_pipe = [None; 2];
                let mut issue_count = 0;
                let mut write_count = 0;
                replay.sample(
                    Tick {
                        reset: true,
                        ..Default::default()
                    },
                    Ok(Edge::default()),
                    None,
                    initial,
                );
                backend.sample(
                    Tick {
                        reset: true,
                        ..Default::default()
                    },
                    Edge::default(),
                    None,
                    true,
                    None,
                    initial,
                    r.preparation.work_state(),
                    r.preparation.numerical_banks().1,
                );
                for _ in 0..LIMIT {
                    rng ^= rng << 13;
                    rng ^= rng >> 17;
                    rng ^= rng << 5;
                    let (_, _, dp, _) = r.preparation.d_lod_state();
                    let mr = mode != 3 || r.stats.link.wall > 900;
                    let pr = mode != 4 || r.stats.link.wall > 900;
                    r.preparation.membership_ready = mr;
                    reference.preparation.membership_ready = mr;
                    r.preparation.packet_ready = pr;
                    reference.preparation.packet_ready = pr;
                    let offer = if mode == 0 || mode >= 3 || dp == phase {
                        inputs.get(accepted)
                    } else {
                        None
                    };
                    let control = timed::Control {
                        ce: !matches!(mode, 2 | 5) || rng % 7 > 1,
                        result_ready: !matches!(mode, 2 | 5)
                            || (r.stats.link.wall > 600 && rng % 13 > 2),
                    };
                    let before = r.cache.packet_storage_state().unwrap();
                    let s = r.step(&mut mc, offer, control).unwrap();
                    let old = reference.step(&mut reference_mc, offer, control).unwrap();
                    assert_eq!(s.preparation, old.preparation, "{name} preparation");
                    assert_eq!(s.results, old.results, "{name} pixels");
                    assert_eq!(s.snapshot, old.snapshot, "{name} public ownership");
                    assert_eq!(
                        format!("{:?}", s.cache),
                        format!("{:?}", old.cache),
                        "{name} every cache event/port/snapshot"
                    );
                    assert_eq!(
                        format!("{:?}", s.color),
                        format!("{:?}", old.color),
                        "{name} every color event/port"
                    );
                    assert_eq!(
                        (s.input_ready, s.accepted, s.effective_ce),
                        (old.input_ready, old.accepted, old.effective_ce)
                    );
                    let (t, e) = if s.effective_ce {
                        r.cache.packet_storage_trace().unwrap()
                    } else {
                        (
                            Tick::default(),
                            Edge {
                                next: before.0,
                                ..Default::default()
                            },
                        )
                    };
                    if s.effective_ce {
                        let trace = &r.preparation.trace;
                        assert_eq!(
                            t.reserve.is_some(),
                            trace.work.capture.is_some(),
                            "{name} actual E0 capture/reserve"
                        );
                        if let Some((member, _)) = trace.work.capture {
                            assert_eq!(t.reserve, Some(member.key()));
                        }
                        assert_eq!(
                            t.write.map(|w| w as i128),
                            trace.packet,
                            "{name} actual E2 write"
                        );
                        assert_eq!(
                            t.write.is_some(),
                            producer_pipe[1].is_some(),
                            "{name} no holes/cancel/delay"
                        );
                        if let Some(w) = t.write {
                            let key = ((((w >> 66) & 15) * 4) + ((w >> 70) & 3)) as u8;
                            assert_eq!(Some(key), producer_pipe[1], "{name} no overtaking");
                            write_count += 1;
                        }
                        producer_pipe = [t.reserve, producer_pipe[0]];
                        issue_count += usize::from(t.reserve.is_some());
                    }
                    replay.sample(t, Ok(e), before.1, r.cache.packet_storage_state().unwrap());
                    backend.sample(
                        t,
                        e,
                        r.preparation.trace.work_input,
                        pr,
                        before.1,
                        r.cache.packet_storage_state().unwrap(),
                        r.preparation.work_state(),
                        r.preparation.numerical_banks().1,
                    );
                    writeln!(
                        full,
                        "{}|{}|{}|{:?}|{:?}",
                        s.cycle, s.effective_ce, s.accepted, s.preparation.events, s.results
                    )
                    .unwrap();
                    writeln!(
                        events,
                        "{}|{:?}|{:?}|{:?}",
                        s.cycle, s.cache.packet_events, s.cache.events, s.color.events
                    )
                    .unwrap();
                    accepted += usize::from(s.accepted);
                    packets += usize::from(e.write.is_some());
                    pixels.extend(s.results);
                    if accepted == inputs.len() && r.idle() {
                        break;
                    }
                }
                assert_eq!(producer_pipe, [None; 2]);
                assert_eq!(issue_count, write_count);
                assert_eq!(accepted, inputs.len());
                assert!(r.idle() && reference.idle(), "{name} drain");
                assert_eq!(pixels, golden, "{name} scalar golden");
                std::fs::write(out.join(format!("{name}.full.txt")), full).unwrap();
                std::fs::write(out.join(format!("{name}.events.txt")), events).unwrap();
                writeln!(
                    cases,
                    "{name},{},{},{},{packets}",
                    r.stats.link.wall,
                    accepted,
                    pixels.len()
                )
                .unwrap();
                let p = out.join(&name);
                replay.finish(&p);
                let backend_path = out.join(format!("{name}-backend"));
                backend.finish(&backend_path);
                if rtl {
                    run(&p);
                    super::backend_qualification::Replay::run(&backend_path);
                }
            }
        }
    }
    std::fs::write(out.join("cases.csv"), cases).unwrap();
}
#[test]
fn packet_storage_runtime_84_baseline_calendars() {
    calendars(false);
}
#[test]
#[ignore = "requires Icarus and native Gowin memory primitives"]
fn packet_storage_runtime_84_native_replay() {
    calendars(true);
}
