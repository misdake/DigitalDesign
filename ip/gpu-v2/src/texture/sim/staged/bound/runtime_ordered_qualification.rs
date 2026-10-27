//! Ordered owners, shared UV lifetime and split-write Coordinate reservations.
use super::super::control;
use super::{qualification as q, *};
use std::{collections::VecDeque, fmt::Write};

#[test]
fn ordered_context_ring_and_coordinate_rows_keep_owners_through_wrap_and_stalls() {
    const LIMIT: u64 = 40_000;
    for (contexts, coordinates, early) in [(1, 1, true), (3, 2, false), (8, 6, true), (8, 6, false)]
    {
        let (slots, bytes) = q::fixture();
        let mut r = q::runtime(
            &slots,
            control::Hardware {
                contexts,
                coordinate_credits: coordinates,
                release_after_capture: early,
                max_cycles: LIMIT,
                ..Default::default()
            },
        );
        let mut mc = q::physical::Physical::new(4096, bytes, true, true);
        let inputs: Vec<_> = (0..48)
            .map(|i| {
                let mut input = q::input(
                    (i % 16) as u8,
                    [15, 0, 1, 10, 0, 6, 15, 3][i % 8],
                    [-0.03125 + (i % 7) as f64 / 32.0, 0.46875],
                    crate::texture::ports::Filter::Trilinear,
                );
                input.lod_bias = 0.5;
                input
            })
            .collect();
        let expected: Vec<_> = inputs.iter().flat_map(q::golden).collect();
        let mut got = vec![];
        let mut accepted = 0;
        let mut raw = VecDeque::new();
        let mut d_flight = VecDeque::new();
        let mut derived = VecDeque::new();
        let mut l_flight = VecDeque::new();
        let mut owners = VecDeque::new();
        let mut lanes = VecDeque::new();
        let mut released = 0;
        let mut frozen = 0;
        let mut random = 0x8317_64e3_u32;
        let mut peaks = [0usize; 4];
        let mut csv = String::from("wall,enabled,event,quad,slot,uv_R,uv_W,meta_R,meta_W\n");
        let mut ids = [0usize; 16];
        let mut expected_uv = vec![[0i64; 8]; contexts];
        for _ in 0..LIMIT {
            random ^= random << 13;
            random ^= random >> 17;
            random ^= random << 5;
            let ce = random & 7 > 1;
            let before = r.preparation.ordered_state();
            let s = r
                .step(
                    &mut mc,
                    inputs.get(accepted),
                    timed::Control {
                        ce,
                        result_ready: r.stats.link.wall > 800 && random % 13 > 2,
                    },
                )
                .unwrap();
            let after = r.preparation.ordered_state();
            let trace = &r.preparation.trace;
            if !s.effective_ce {
                assert_eq!(before, after, "owner/UV state advanced while held");
                frozen += 1;
            }
            let d_read = trace.d_input.is_some();
            let l_read = trace.l_input.is_some();
            let ports = [
                usize::from(trace.uv.read.is_some()),
                usize::from(trace.uv.write.is_some()),
                usize::from(d_read)
                    + usize::from(l_read)
                    + usize::from(s.effective_ce && before.cursors[10] != 0),
                usize::from(s.accepted)
                    + usize::from(trace.d_capture.is_some())
                    + usize::from(trace.l_capture.is_some()),
            ];
            for i in 0..4 {
                peaks[i] = peaks[i].max(ports[i]);
            }
            // All assertions use independent admission-order queues. Production
            // code has only bounded cursors/owners, not these test scoreboards.
            for e in &s.preparation.events {
                match *e {
                    PreparationEvent::Accept { program, slot } => {
                        assert_eq!(slot, accepted % contexts);
                        assert!(
                            before.cursors[2] < contexts,
                            "borrowed same-edge context release"
                        );
                        expected_uv[slot] = derivative::Input::capture(
                            &inputs[accepted],
                            slots[usize::from(inputs[accepted].slot)],
                        )
                        .unwrap()
                        .uv;
                        assert_eq!(after.uv[slot][..2], expected_uv[slot][..2]);
                        ids[program] = slot;
                        raw.push_back(program);
                        owners.push_back((program, slot));
                        accepted += 1;
                        writeln!(
                            csv,
                            "{},{},admit,{program},{slot},{},{},{},{}",
                            s.cycle, r.stats.link.enabled, ports[0], ports[1], ports[2], ports[3]
                        )
                        .unwrap();
                    }
                    PreparationEvent::Issue {
                        stage: "derivative",
                        program,
                        ..
                    } => {
                        assert_eq!(raw.pop_front(), Some(program));
                        d_flight.push_back(program);
                        assert_eq!(trace.d_input.unwrap().uv, expected_uv[ids[program]]);
                        writeln!(
                            csv,
                            "{},{},D_read,{program},{},{},{},{},{}",
                            s.cycle,
                            r.stats.link.enabled,
                            ids[program],
                            ports[0],
                            ports[1],
                            ports[2],
                            ports[3]
                        )
                        .unwrap();
                    }
                    PreparationEvent::Issue {
                        stage: "lod",
                        program,
                        ..
                    } => {
                        assert_eq!(derived.pop_front(), Some(program));
                        l_flight.push_back(program);
                        writeln!(
                            csv,
                            "{},{},LOD_read,{program},{},{},{},{},{}",
                            s.cycle,
                            r.stats.link.enabled,
                            ids[program],
                            ports[0],
                            ports[1],
                            ports[2],
                            ports[3]
                        )
                        .unwrap();
                    }
                    PreparationEvent::Issue {
                        stage: "coordinate",
                        program,
                        lane,
                        ..
                    } => {
                        let mask = inputs
                            .iter()
                            .find(|q| usize::from(q.quad_id) == program)
                            .unwrap()
                            .mask;
                        // Repeated ID16 preserves this workload's mask pattern.
                        let physical = (0..4u8).filter(|l| mask >> l & 1 != 0).nth(lane).unwrap();
                        let slot = ids[program];
                        assert!(trace.c_edge.as_ref().unwrap().accepted);
                        assert_eq!(
                            trace.c_input.unwrap().uv,
                            [
                                expected_uv[slot][usize::from(physical) * 2] as u32 & 0xffff,
                                expected_uv[slot][usize::from(physical) * 2 + 1] as u32 & 0xffff
                            ]
                        );
                        lanes.push_back((program as u8 * 4 + physical, false));
                        writeln!(
                            csv,
                            "{},{},coordinate{lane},{program},{slot},{},{},{},{}",
                            s.cycle, r.stats.link.enabled, ports[0], ports[1], ports[2], ports[3]
                        )
                        .unwrap();
                    }
                    PreparationEvent::KeyIssue {
                        stage: "coefficient",
                        key: issued_key,
                        ..
                    } => {
                        let (key, ready) = lanes.pop_front().unwrap();
                        assert!(ready, "Coefficient consumed an unreturned row");
                        assert_eq!(key, issued_key);
                    }
                    PreparationEvent::SharedRelease { program, slot } => {
                        assert_eq!(owners.pop_front(), Some((program, slot)));
                        assert_eq!(slot, released % contexts);
                        released += 1;
                        writeln!(
                            csv,
                            "{},{},reclaim,{program},{slot},{},{},{},{}",
                            s.cycle, r.stats.link.enabled, ports[0], ports[1], ports[2], ports[3]
                        )
                        .unwrap();
                    }
                    _ => {}
                }
            }
            if let Some(d) = trace.d_capture {
                assert_eq!(d_flight.pop_front(), Some(usize::from(d.header.quad)));
                let slot = ids[usize::from(d.header.quad)];
                assert_eq!(d.uv, expected_uv[slot].map(|v| v as u32 & 0xffff));
                derived.push_back(usize::from(d.header.quad));
                writeln!(
                    csv,
                    "{},{},D_return,{},{},{},{},{},{}",
                    s.cycle,
                    r.stats.link.enabled,
                    d.header.quad,
                    slot,
                    ports[0],
                    ports[1],
                    ports[2],
                    ports[3]
                )
                .unwrap();
            }
            if let Some(l) = trace.l_capture {
                assert_eq!(l_flight.pop_front(), Some(usize::from(l.quad)));
                writeln!(
                    csv,
                    "{},{},LOD_return,{},{},{},{},{},{}",
                    s.cycle,
                    r.stats.link.enabled,
                    l.quad,
                    ids[usize::from(l.quad)],
                    ports[0],
                    ports[1],
                    ports[2],
                    ports[3]
                )
                .unwrap();
            }
            if let Some(c) = trace.coordinate {
                let next = lanes.iter_mut().find(|(_, ready)| !ready).unwrap();
                assert_eq!(next.0, c.metadata.key);
                next.1 = true;
            }
            assert_eq!(after.cursors[2], after.contexts.iter().flatten().count());
            assert!(after.cursors[2] <= contexts);
            assert_eq!(
                after.cursors[14],
                after.coordinates.iter().flatten().count()
            );
            assert!(after.cursors[14] <= coordinates);
            assert_eq!(
                after.cursors[15],
                after
                    .coordinates
                    .iter()
                    .flatten()
                    .filter(|(_, ready)| !ready)
                    .count()
            );
            got.extend(s.results);
            if accepted == inputs.len() && r.idle() {
                break;
            }
        }
        assert!(r.idle() && frozen > 0);
        assert_eq!((accepted, released), (48, 48));
        assert!(
            raw.is_empty()
                && d_flight.is_empty()
                && derived.is_empty()
                && l_flight.is_empty()
                && owners.is_empty()
                && lanes.is_empty()
        );
        assert_eq!(got, expected);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/sampling-coordinate-storage-20261008");
        std::fs::create_dir_all(&dir).unwrap();
        let name = format!("owners-N{contexts}-C{coordinates}-early{early}");
        std::fs::write(dir.join(format!("{name}.csv")), csv).unwrap();
        std::fs::write(dir.join(format!("{name}.json")),format!("{{\"accepted\":{accepted},\"reclaimed\":{released},\"frozen_edges\":{frozen},\"peak_contexts\":{},\"peak_coordinates\":{},\"uv_read_write_meta_read_write_peaks\":{peaks:?}}}\n",r.preparation.stats.peak_contexts,r.preparation.stats.peak_coordinates)).unwrap();
    }
}
