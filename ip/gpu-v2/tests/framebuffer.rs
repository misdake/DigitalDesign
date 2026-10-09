use gpu_v2::framebuffer::{
    ports::*,
    sim::{
        bounded::{Model, OutputRow},
        fixture::{replay, Fixture},
        oracle::{self, Pixel},
    },
};
#[path = "support/sdram/burst.rs"]
mod burst;
fn surface() -> MaterializedSurface {
    MaterializedSurface {
        color_base_bytes: 0,
        depth_base_bytes: 16384,
        width: 160,
        height: 32,
    }
}
fn context() -> Context {
    Context {
        depth: DepthFunc::Always,
        depth_write: true,
        blend: Blend::SrcOver,
    }
}
fn quad(x: u16, y: u8, mask: u8, n: u8) -> Quad {
    Quad {
        header: Header { x, y, mask },
        pixels: std::array::from_fn(|i| Fragment {
            rgba: [n, 80 + i as u8 * 31, 255 - n, 97],
            depth: 1000 + u16::from(n) + i as u16,
        }),
    }
}
fn image() -> Vec<u8> {
    (0..32768)
        .map(|i| ((i * 71 + 19) ^ (i >> 3)) as u8)
        .collect()
}
// Independent real-number reference, separate address derivation and quantizer.
fn golden(mut bytes: Vec<u8>, qs: &[Quad], s: MaterializedSurface, c: Context) -> Vec<u8> {
    for q in qs {
        for lane in 0..4 {
            if q.header.mask & (1 << lane) == 0 {
                continue;
            }
            let x = q.header.x as usize + lane % 2;
            let y = q.header.y as usize + lane / 2;
            let offset =
                (((y / 16) * (s.width as usize / 16) + x / 16) * 256 + (y % 16) * 16 + x % 16) * 2;
            let ca = s.color_base_bytes as usize + offset;
            let da = s.depth_base_bytes as usize + offset;
            let old = u16::from_le_bytes([bytes[ca], bytes[ca + 1]]);
            let depth = u16::from_le_bytes([bytes[da], bytes[da + 1]]);
            let f = q.pixels[lane];
            let comparisons = [
                false,
                f.depth < depth,
                f.depth == depth,
                f.depth <= depth,
                f.depth > depth,
                f.depth != depth,
                f.depth >= depth,
                true,
            ];
            if !comparisons[c.depth as usize] {
                continue;
            }
            let codes = [
                (old / 2048) as u32,
                ((old / 32) % 64) as u32,
                (old % 32) as u32,
            ];
            let mut out = 0u16;
            for i in 0..3 {
                let bits = if i == 1 { 6 } else { 5 };
                let m = (1u32 << bits) - 1;
                let d = ((codes[i] << (8 - bits)) + (codes[i] >> (2 * bits - 8))) as f64;
                let v = if c.blend == Blend::Replace {
                    f.rgba[i] as f64
                } else {
                    (f.rgba[3] as f64 / 255.0 * f.rgba[i] as f64
                        + (1.0 - f.rgba[3] as f64 / 255.0) * d)
                        .round()
                };
                let quant = (v * m as f64 / 255.0).round() as u16;
                out |= quant << [11, 5, 0][i];
            }
            bytes[ca..ca + 2].copy_from_slice(&out.to_le_bytes());
            if c.depth_write {
                bytes[da..da + 2].copy_from_slice(&f.depth.to_le_bytes());
            }
        }
    }
    bytes
}
#[test]
fn bank_mapping_is_bijective_and_both_groups_have_four_banks() {
    let mut seen = std::collections::BTreeSet::new();
    for p in 0..2 {
        for l in 0..8 {
            for y in 0..16 {
                for x in 0..16 {
                    assert!(seen.insert(bank_address(p, l, x, y)));
                }
            }
        }
    }
    assert_eq!(seen.len(), 4096);
    for y in 0..16 {
        for x in (0..16).step_by(4) {
            let b: std::collections::BTreeSet<_> =
                (0..4).map(|i| bank_address(0, 0, x + i, y).0).collect();
            assert_eq!(b.len(), 4);
        }
    }
    for y in (0..16).step_by(2) {
        for x in (0..16).step_by(2) {
            let b: std::collections::BTreeSet<_> = (0..4)
                .map(|i| bank_address(0, 0, x + i % 2, y + i / 2).0)
                .collect();
            assert_eq!(b.len(), 4);
        }
    }
}
#[test]
fn all_depth_functions_and_alpha_zero_depth_write() {
    for code in 0..8 {
        let funcs = [
            DepthFunc::Never,
            DepthFunc::Less,
            DepthFunc::Equal,
            DepthFunc::LessEqual,
            DepthFunc::Greater,
            DepthFunc::NotEqual,
            DepthFunc::GreaterEqual,
            DepthFunc::Always,
        ];
        for new in [0, 499, 500, 501, 65535] {
            for write in [false, true] {
                for covered in [false, true] {
                    let c = Context {
                        depth: funcs[code],
                        depth_write: write,
                        blend: Blend::SrcOver,
                    };
                    let old = Pixel {
                        color: 0xa345,
                        depth: 500,
                    };
                    let f = Fragment {
                        rgba: [255, 0, 0, 0],
                        depth: new,
                    };
                    let r = oracle::pixel(old, f, covered, c);
                    let pass = covered
                        && [
                            false,
                            new < 500,
                            new == 500,
                            new <= 500,
                            new > 500,
                            new != 500,
                            new >= 500,
                            true,
                        ][code];
                    assert_eq!(r.pixel.color, old.color);
                    assert_eq!(r.pixel.depth, if pass && write { new } else { 500 });
                    assert_eq!(r.color_written, pass);
                    assert_eq!(r.depth_written, pass && write);
                }
            }
        }
    }
}
#[test]
fn quantization_roundtrip_and_channel_sweep() {
    for code in 0..=65535 {
        assert_eq!(oracle::quantize(oracle::expand(code)), code);
    }
    for v in 0..=255 {
        let expected = ((v as f64 * 31.0 / 255.0).round() as u16) << 11
            | ((v as f64 * 63.0 / 255.0).round() as u16) << 5
            | (v as f64 * 31.0 / 255.0).round() as u16;
        assert_eq!(oracle::quantize([v; 3]), expected);
    }
}
#[test]
fn independent_full_memory_golden_eviction_flush_ce_backpressure() {
    let s = surface();
    let mut qs = Vec::new();
    for round in 0..3 {
        for tile in 0..20 {
            qs.push(quad(
                (tile % 10) * 16,
                (tile / 10 * 16) as u8,
                [1, 5, 15][round],
                (tile + round as u16 * 30) as u8,
            ));
        }
    }
    for blend in [Blend::Replace, Blend::SrcOver] {
        let c = Context { blend, ..context() };
        let before = image();
        let expected = golden(before.clone(), &qs, s, c);
        let mut f = Fixture::new(before);
        f.request_period = 5;
        f.beat_period = 3;
        f.ack_delay = 11;
        let (m, f, t) = replay(s, c, &qs, f, 7, 100_000).unwrap();
        assert_eq!(f.bytes, expected);
        assert!(f.idle() && m.idle() && m.flush_complete);
        assert!(m.stats.misses > 8 && m.stats.ce_stalls > 0 && m.stats.input_stalls > 0);
        assert_eq!(m.stats.quads, 60);
        assert!(m.dirty().iter().all(|d| *d == [false; 2]));
        for cycle in t {
            for a in &cycle.accesses {
                assert!(
                    cycle
                        .accesses
                        .iter()
                        .filter(|b| a.bank == b.bank && a.port == b.port)
                        .count()
                        == 1
                );
            }
        }
    }
}
#[test]
fn repeated_quantized_blend_and_replay_are_deterministic() {
    let qs: Vec<_> = (0..40)
        .map(|i| quad(2, 2, if i % 2 == 0 { 15 } else { 3 }, i))
        .collect();
    let expected = golden(image(), &qs, surface(), context());
    let a = replay(surface(), context(), &qs, Fixture::new(image()), 0, 30_000).unwrap();
    let b = replay(surface(), context(), &qs, Fixture::new(image()), 0, 30_000).unwrap();
    assert_eq!(a.1.bytes, expected);
    assert_eq!(a.2, b.2);
    assert_eq!(a.0.stats.misses, 1);
    assert!(a.0.stats.serialized_wait > 0);
    let commits: Vec<_> =
        a.2.iter()
            .filter(|t| t.committed.is_some())
            .map(|t| t.cycle)
            .collect();
    assert!(commits.windows(2).all(|w| w[1] - w[0] == 13));
}
#[test]
fn read_failure_and_write_failure_drain_without_tag_reuse() {
    for failure in [1, 9] {
        let mut f = Fixture::new(image());
        f.fail_request = Some(failure);
        f.ack_delay = 17;
        let qs = [quad(0, 0, 15, 1)];
        let (m, f, t) = replay(surface(), context(), &qs, f, 3, 20_000).unwrap();
        assert!(m.drained() && f.idle());
        assert!(!m.flush_complete);
        assert_eq!(f.requests, failure);
        assert!(t.iter().any(|t| t.response.complete == Some(false)));
        if failure == 1 {
            assert!(m.tags().iter().all(Option::is_none));
        } else {
            assert!(m.dirty()[0][0]);
            assert_eq!(m.tags()[0], Some(0));
        }
    }
}
#[test]
fn accepted_read_and_write_drain_after_external_fault_with_ce_off() {
    for abort_write in [false, true] {
        let mut m = Model::new(surface(), context()).unwrap();
        let mut f = Fixture::new(image());
        let q = quad(0, 0, 15, 1);
        let mut cursor = 0;
        let mut aborted = false;
        let mut flush = false;
        for _ in 0..5000 {
            let input = (cursor < 8).then(|| OutputRow {
                header: q.header,
                row: cursor as u8,
                data: q.rows()[cursor],
            });
            let t = m.step(!aborted, input, &mut f).unwrap();
            if t.input_accepted {
                cursor += 1;
            }
            if cursor == 8 && !flush {
                m.request_flush().unwrap();
                flush = true;
            }
            if !aborted && t.response.accepted && t.request.unwrap().write == abort_write {
                m.abort();
                aborted = true;
            }
            if m.drained() {
                break;
            }
        }
        assert!(aborted && m.drained() && f.idle());
    }
}
#[test]
fn last_write_beat_does_not_finish_flush() {
    let mut m = Model::new(surface(), context()).unwrap();
    let mut f = Fixture::new(image());
    f.ack_delay = 20;
    let q = quad(0, 0, 15, 2);
    let mut cursor = 0;
    let mut flush = false;
    let mut last = 0;
    let mut acks = 0;
    for _ in 0..5000 {
        let row = (cursor < 8).then(|| OutputRow {
            header: q.header,
            row: cursor as u8,
            data: q.rows()[cursor],
        });
        let t = m.step(true, row, &mut f).unwrap();
        if t.input_accepted {
            cursor += 1;
        }
        if cursor == 8 && !flush {
            m.request_flush().unwrap();
            flush = true;
        }
        if t.response.write_accepted {
            last = t.cycle;
            assert!(!m.flush_complete);
        }
        if t.response.complete == Some(true) && last > 0 {
            assert!(t.cycle >= last + 20);
            acks += 1;
        }
        if m.flush_complete {
            break;
        }
    }
    assert!(m.flush_complete);
    assert_eq!(acks, 8);
}
#[test]
fn partial_output_is_not_visible_and_invalid_rows_are_rejected() {
    let mut m = Model::new(surface(), context()).unwrap();
    let mut f = Fixture::new(image());
    let q = quad(0, 0, 15, 1);
    for row in 0..7 {
        let t = m
            .step(
                true,
                Some(OutputRow {
                    header: q.header,
                    row,
                    data: q.rows()[row as usize],
                }),
                &mut f,
            )
            .unwrap();
        assert!(t.input_accepted);
        assert_eq!(m.stats.misses, 0);
    }
    assert!(m.request_flush().is_err());
    assert!(m
        .step(
            true,
            Some(OutputRow {
                header: q.header,
                row: 7,
                data: 1 << 16
            }),
            &mut f
        )
        .is_err());
}

#[test]
fn golden_all_depth_modes_with_partial_coverage_and_repeated_writes() {
    let funcs = [
        DepthFunc::Never,
        DepthFunc::Less,
        DepthFunc::Equal,
        DepthFunc::LessEqual,
        DepthFunc::Greater,
        DepthFunc::NotEqual,
        DepthFunc::GreaterEqual,
        DepthFunc::Always,
    ];
    for depth in funcs {
        for blend in [Blend::Replace, Blend::SrcOver] {
            for depth_write in [false, true] {
                let c = Context {
                    depth,
                    blend,
                    depth_write,
                };
                let qs: Vec<_> = (0..12).map(|i| quad(0, 0, 1 + (i % 15), i * 17)).collect();
                let expected = golden(image(), &qs, surface(), c);
                let (m, f, _) =
                    replay(surface(), c, &qs, Fixture::new(image()), 0, 10_000).unwrap();
                assert!(m.flush_complete);
                assert_eq!(f.bytes, expected);
            }
        }
    }
}

#[test]
fn failed_dirty_eviction_keeps_victim_tag_and_dirty_plane() {
    let qs: Vec<_> = (0..9).map(|i| quad(i * 16, 0, 15, i as u8)).collect();
    let mut fixture = Fixture::new(image());
    // Eight complete two-plane refills, then the first victim color segment fails.
    fixture.fail_request = Some(65);
    fixture.ack_delay = 31;
    let (m, f, t) = replay(surface(), context(), &qs, fixture, 0, 30_000).unwrap();
    assert!(m.drained() && f.idle());
    assert_eq!(m.stats.quads, 8);
    assert_eq!(m.tags()[0], Some(0));
    assert_eq!(m.dirty()[0], [true, true]);
    assert!(!m.tags().contains(&Some(8)));
    assert_eq!(f.requests, 65);
    assert_eq!(t.iter().filter(|c| c.response.write_accepted).count(), 16);
}

#[test]
fn pipeline_independent_hot_quads_commit_every_eight_cycles() {
    let qs: Vec<_> = (0..80)
        .map(|i| quad((i % 8) * 2, ((i / 8 % 8) * 2) as u8, 15, i as u8))
        .collect();
    let expected = golden(image(), &qs, surface(), context());
    let (m, f, t) = gpu_v2::framebuffer::sim::fixture::replay_pipelined(
        surface(),
        context(),
        &qs,
        Fixture::new(image()),
        0,
        30_000,
    )
    .unwrap();
    assert_eq!(f.bytes, expected);
    assert_eq!(m.stats.misses, 1);
    assert_eq!(m.stats.raw_wait, 0);
    let commits: Vec<_> = t
        .iter()
        .filter(|c| c.committed.is_some())
        .map(|c| c.cycle)
        .collect();
    assert_eq!(commits.len(), 80);
    assert!(commits.windows(2).all(|w| w[1] - w[0] == 8), "{commits:?}");
    assert_eq!(
        t.iter().filter(|c| c.payload_captured.is_some()).count(),
        80
    );
    assert!(t
        .iter()
        .any(|c| c.input_accepted && c.output_read.is_some()));
}

#[test]
fn pipeline_raw_stalls_and_all_depth_modes_match_independent_golden() {
    for depth in [
        DepthFunc::Never,
        DepthFunc::Less,
        DepthFunc::Equal,
        DepthFunc::LessEqual,
        DepthFunc::Greater,
        DepthFunc::NotEqual,
        DepthFunc::GreaterEqual,
        DepthFunc::Always,
    ] {
        for blend in [Blend::Replace, Blend::SrcOver] {
            for depth_write in [false, true] {
                let c = Context {
                    depth,
                    blend,
                    depth_write,
                };
                let qs: Vec<_> = (0..24).map(|i| quad(2, 2, 1 + i % 15, i * 7)).collect();
                let expected = golden(image(), &qs, surface(), c);
                let (m, f, t) = gpu_v2::framebuffer::sim::fixture::replay_pipelined(
                    surface(),
                    c,
                    &qs,
                    Fixture::new(image()),
                    5,
                    30_000,
                )
                .unwrap();
                assert_eq!(f.bytes, expected);
                assert!(m.stats.raw_wait > 0);
                assert!(t.iter().filter(|t| !t.ce).all(|t| t.output_read.is_none()
                    && t.committed.is_none()
                    && t.payload_captured.is_none()
                    && t.accesses.iter().all(|a| a.port == 'B')));
            }
        }
    }
}

#[test]
fn pipeline_eviction_fault_and_backpressure_share_maintenance_contract() {
    let qs: Vec<_> = (0..48)
        .map(|i| {
            quad(
                (i % 20 % 10) * 16,
                (i % 20 / 10 * 16) as u8,
                if i % 3 == 0 { 1 } else { 15 },
                i as u8,
            )
        })
        .collect();
    for fault in [None, Some(1), Some(65)] {
        let mut mem = Fixture::new(image());
        mem.request_period = 3;
        mem.beat_period = 2;
        mem.ack_delay = 17;
        mem.fail_request = fault;
        let (m, f, _) = gpu_v2::framebuffer::sim::fixture::replay_pipelined(
            surface(),
            context(),
            &qs,
            mem,
            7,
            100_000,
        )
        .unwrap();
        assert!(f.idle());
        if fault.is_some() {
            assert!(m.drained());
            if fault == Some(65) {
                assert_eq!(m.tags()[0], Some(0));
                assert_eq!(m.dirty()[0], [true, true]);
            }
        } else {
            assert!(m.flush_complete);
            assert_eq!(f.bytes, golden(image(), &qs, surface(), context()));
        }
    }
}

#[test]
fn pipeline_capture_publication_and_shared_result_survive_exact_ce_cuts() {
    use gpu_v2::framebuffer::sim::fixture::{replay_forwarding, replay_pipelined};
    for forwarding in [false, true] {
        let qs: Vec<_> = (0..32)
            .map(|i| {
                if forwarding {
                    quad(0, 0, 15, (i * 7) as u8)
                } else {
                    quad((i % 8) * 2, ((i / 8) * 2) as u8, 15, (i * 7) as u8)
                }
            })
            .collect();
        let runner = if forwarding {
            replay_forwarding
        } else {
            replay_pipelined
        };
        let (_, _, baseline) =
            runner(surface(), context(), &qs, Fixture::new(image()), 0, 30_000).unwrap();
        let publish = baseline
            .iter()
            .position(|t| t.output_published.is_some() && t.output_read.is_some_and(|r| r % 8 == 0))
            .unwrap();
        let capture = baseline
            .iter()
            .position(|t| t.payload_captured.is_some() && t.output_read.is_some())
            .unwrap();
        let shared = baseline
            .iter()
            .position(|t| {
                t.arithmetic_return.is_some_and(|(_, l)| l == 0)
                    && t.accesses
                        .iter()
                        .any(|a| a.port == 'A' && a.write && a.address >= 512)
            })
            .unwrap();
        let mut cuts = vec![publish, capture, shared];
        if forwarding {
            cuts.push(
                baseline
                    .iter()
                    .position(|t| {
                        t.forwarded.is_some_and(|(_, lane)| lane == 3)
                            && t.output_read.is_some_and(|r| r % 8 == 0)
                    })
                    .unwrap(),
            );
        }
        for cut in cuts {
            let mut m = if forwarding {
                Model::new_forwarding(surface(), context())
            } else {
                Model::new_pipelined(surface(), context())
            }
            .unwrap();
            let mut f = Fixture::new(image());
            let mut cursor = 0;
            let mut flushing = false;
            let mut committed = Vec::new();
            for logical in 0..30_000 {
                if logical == cut {
                    for _ in 0..3 {
                        let row = qs.get(cursor / 8).map(|q| OutputRow {
                            header: q.header,
                            row: (cursor % 8) as u8,
                            data: q.rows()[cursor % 8],
                        });
                        let t = m.step(false, row, &mut f).unwrap();
                        assert!(!t.input_accepted);
                        assert!(
                            t.output_read.is_none()
                                && t.payload_captured.is_none()
                                && t.output_published.is_none()
                                && t.arithmetic_return.is_none()
                                && t.forwarded.is_none()
                        );
                        assert!(t.accesses.is_empty());
                    }
                }
                let row = qs.get(cursor / 8).map(|q| OutputRow {
                    header: q.header,
                    row: (cursor % 8) as u8,
                    data: q.rows()[cursor % 8],
                });
                let t = m.step(true, row, &mut f).unwrap();
                if t.input_accepted {
                    cursor += 1;
                }
                if logical == cut {
                    if forwarding && baseline[cut].forwarded.is_some() {
                        assert!(t.forwarded.is_some());
                    }
                    if cut == publish {
                        assert!(t.output_published.is_some() && t.output_read.is_some());
                    }
                    if cut == capture {
                        assert!(t.payload_captured.is_some());
                    }
                    if cut == shared {
                        assert!(t.arithmetic_return.is_some());
                    }
                }
                if let Some(h) = t.committed {
                    committed.push(h);
                }
                if cursor == qs.len() * 8 && !flushing {
                    m.request_flush().unwrap();
                    flushing = true;
                }
                if m.flush_complete {
                    break;
                }
            }
            assert!(m.flush_complete);
            assert_eq!(committed, qs.iter().map(|q| q.header).collect::<Vec<_>>());
            assert_eq!(f.bytes, golden(image(), &qs, surface(), context()));
        }
    }
}

#[test]
fn pipeline_external_fault_drains_accepted_read_or_write_with_ce_off() {
    for forwarding in [false, true] {
        for write in [false, true] {
            let mut m = if forwarding {
                Model::new_forwarding(surface(), context())
            } else {
                Model::new_pipelined(surface(), context())
            }
            .unwrap();
            let mut f = Fixture::new(image());
            let q = quad(0, 0, 15, 9);
            let mut row = 0;
            let mut aborted = false;
            let mut flushing = false;
            for _ in 0..10_000 {
                let input = (row < 8).then(|| OutputRow {
                    header: q.header,
                    row: row as u8,
                    data: q.rows()[row],
                });
                let t = m.step(!aborted, input, &mut f).unwrap();
                if t.input_accepted {
                    row += 1;
                }
                if row == 8 && !flushing {
                    m.request_flush().unwrap();
                    flushing = true;
                }
                if !aborted && t.response.accepted && t.request.unwrap().write == write {
                    m.abort();
                    aborted = true;
                }
                if m.drained() {
                    break;
                }
            }
            assert!(aborted && m.drained() && f.idle());
            assert!(!m.flush_complete);
        }
    }
}

#[test]
fn real_cycle_mc_refill_dirty_rotation_and_flush_match_golden() {
    use digital_design_hardware_gowin::sdram_memory_controller::ports::OracleImage;
    let qs: Vec<_> = (0..24)
        .map(|i| {
            quad(
                (i % 10) * 16,
                (i / 10 % 2 * 16) as u8,
                if i % 3 == 0 { 5 } else { 15 },
                (i * 7) as u8,
            )
        })
        .collect();
    let initial = image();
    let expected = golden(initial.clone(), &qs, surface(), context());
    // The image is independent external stimulus, never an audited intermediate.
    let image =
        unsafe { OracleImage::from_host(0, initial, "framebuffer external test pattern") }.unwrap();
    let mut port = burst::Adapter::new(image, 100_000).unwrap();
    let mut m = Model::new_pipelined(surface(), context()).unwrap();
    let mut cursor = 0;
    let mut flushing = false;
    let mut writes = 0;
    let mut acks = 0;
    let mut last_write = 0;
    let mut trace = String::from("cycle,request_accepted,read_beat,write_accepted,completion\n");
    for wall in 0..100_000 {
        let row = qs.get(cursor / 8).map(|q| OutputRow {
            header: q.header,
            row: (cursor % 8) as u8,
            data: q.rows()[cursor % 8],
        });
        let t = m.step(wall % 7 != 0, row, &mut port).unwrap();
        if t.input_accepted {
            cursor += 1;
        }
        if t.response.write_accepted {
            writes += 1;
            last_write = t.cycle;
        }
        if t.response.complete == Some(true) && last_write > 0 && writes > acks * 16 {
            assert!(t.cycle > last_write);
            acks += 1;
        }
        if t.response.accepted
            || t.response.read.is_some()
            || t.response.write_accepted
            || t.response.complete.is_some()
        {
            use std::fmt::Write;
            writeln!(
                trace,
                "{},{},{:?},{},{:?}",
                t.cycle,
                t.response.accepted,
                t.response.read.map(|r| r.0),
                t.response.write_accepted,
                t.response.complete
            )
            .unwrap();
        }
        if cursor == qs.len() * 8 && !flushing {
            m.request_flush().unwrap();
            flushing = true;
        }
        if m.flush_complete {
            break;
        }
    }
    assert!(m.flush_complete && m.idle() && port.idle());
    assert_eq!(m.stats.quads, 24);
    assert!(m.stats.misses > 8);
    assert!(writes > 0);
    assert_eq!(writes, acks * 16);
    assert_eq!(port.combination.bridge.pins.bytes(), expected);
    assert_eq!(port.combination.bridge.write_chains, 0);
    assert_eq!(port.combination.bridge.read_chains, 0);
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/framebuffer-real-mc");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("transactions.csv"), trace).unwrap();
    std::fs::write(
        root.join("summary.txt"),
        format!("{:?}\nwrite_acknowledgements={acks}\n", m.stats),
    )
    .unwrap();
}

#[test]
fn early_terminal_error_is_not_a_short_success_and_blocked_request_drains() {
    struct Early {
        held: Option<Request>,
        waiting: u8,
        accepted: bool,
        beats: u8,
        complete: bool,
    }
    impl MemoryPort for Early {
        fn cycle(&mut self, request: Option<Request>, _: Option<u64>) -> Result<Response, String> {
            let mut r = Response::default();
            if self.complete {
                return Ok(r);
            }
            if self.accepted {
                if self.beats < 3 {
                    r.read = Some((self.beats, 0x5678_1234_9abc_def0));
                    self.beats += 1;
                } else {
                    r.complete = Some(false);
                    self.complete = true;
                }
            } else if let Some(q) = request {
                if let Some(h) = self.held {
                    assert_eq!(q, h);
                }
                self.held = Some(q);
                self.waiting += 1;
                if self.waiting == 4 {
                    r.accepted = true;
                    self.accepted = true;
                }
            } else {
                assert!(
                    self.held.is_none(),
                    "presented request withdrawn during drain"
                );
            }
            Ok(r)
        }
    }
    for abort_while_blocked in [false, true] {
        let mut port = Early {
            held: None,
            waiting: 0,
            accepted: false,
            beats: 0,
            complete: false,
        };
        let mut m = Model::new_pipelined(surface(), context()).unwrap();
        let q = quad(0, 0, 15, 3);
        let mut cursor = 0;
        let mut aborted = false;
        for _ in 0..1000 {
            let row = (cursor < 8).then(|| OutputRow {
                header: q.header,
                row: cursor as u8,
                data: q.rows()[cursor],
            });
            let t = m.step(!aborted, row, &mut port).unwrap();
            if t.input_accepted {
                cursor += 1;
            }
            if abort_while_blocked && !aborted && t.request.is_some() && !t.response.accepted {
                m.abort();
                aborted = true;
            }
            if m.drained() {
                break;
            }
        }
        assert!(m.drained() && port.complete);
        assert_eq!(m.stats.read_bytes, 24);
        assert!(m.tags().iter().all(Option::is_none));
        assert!(!m.flush_complete);
    }
}

#[test]
fn forwarding_long_same_address_chain_is_eight_cycles_and_matches_golden() {
    use gpu_v2::framebuffer::sim::fixture::replay_forwarding;
    let qs: Vec<_> = (0..96).map(|i| quad(0, 0, 15, i as u8)).collect();
    let (m, f, t) =
        replay_forwarding(surface(), context(), &qs, Fixture::new(image()), 0, 30_000).unwrap();
    assert_eq!(f.bytes, golden(image(), &qs, surface(), context()));
    assert!(m.flush_complete);
    assert_eq!(m.stats.forwarded_lanes, 95 * 4);
    assert_eq!(m.stats.raw_wait, 0);
    assert_eq!(m.stats.forward_fallbacks, 0);
    let commits: Vec<_> = t
        .iter()
        .filter(|t| t.committed.is_some())
        .map(|t| t.cycle)
        .collect();
    assert_eq!(commits.len(), 96);
    assert!(commits.windows(2).all(|w| w[1] - w[0] == 8));
    // B lane 3 consumes the old owner even while C reuses its descriptor and
    // reads depth. Descriptor reuse must not invalidate that forward entry.
    assert!(t.iter().any(|t| t.forwarded.is_some_and(|(_, l)| l == 3)
        && t.output_read.is_some_and(|r| r % 8 == 0)
        && t.accesses
            .iter()
            .any(|a| a.port == 'A' && !a.write && a.address >= 512)));
}

#[test]
fn forwarding_partial_masks_depth_fail_and_alpha_zero_all_modes() {
    use gpu_v2::framebuffer::sim::fixture::replay_forwarding;
    for depth in [
        DepthFunc::Never,
        DepthFunc::Less,
        DepthFunc::Equal,
        DepthFunc::LessEqual,
        DepthFunc::Greater,
        DepthFunc::NotEqual,
        DepthFunc::GreaterEqual,
        DepthFunc::Always,
    ] {
        for blend in [Blend::Replace, Blend::SrcOver] {
            for depth_write in [false, true] {
                let c = Context {
                    depth,
                    blend,
                    depth_write,
                };
                let qs: Vec<_> = (0..80)
                    .map(|i| {
                        let mut q = quad(2, 2, [1, 2, 5, 10, 15, 3, 12][i % 7], (i * 3) as u8);
                        for (lane, f) in q.pixels.iter_mut().enumerate() {
                            f.depth = [1000, 1000, 65000, 4][(i + lane) % 4];
                            f.rgba[3] = [0, 255, 91][i % 3];
                        }
                        q
                    })
                    .collect();
                let (m, f, _) =
                    replay_forwarding(surface(), c, &qs, Fixture::new(image()), 0, 30_000).unwrap();
                assert!(m.flush_complete);
                assert_eq!(m.stats.forward_fallbacks, 0);
                assert!(m.stats.forwarded_lanes > 0);
                assert_eq!(f.bytes, golden(image(), &qs, surface(), c));
            }
        }
    }
}

#[test]
fn forwarding_does_not_alias_other_quad_or_other_line_and_survives_stalls() {
    use gpu_v2::framebuffer::sim::fixture::replay_forwarding;
    let qs: Vec<_> = (0..96)
        .map(|i| {
            let (x, y) = match i % 3 {
                0 => (0, 0),
                1 => (2, 0),
                _ => (16, 0),
            };
            quad(x, y, 15, (i * 5) as u8)
        })
        .collect();
    let mut fixture = Fixture::new(image());
    fixture.request_period = 5;
    fixture.beat_period = 3;
    fixture.ack_delay = 17;
    let (m, f, _) = replay_forwarding(surface(), context(), &qs, fixture, 5, 100_000).unwrap();
    assert!(m.flush_complete);
    assert_eq!(m.stats.forwarded_lanes, 0);
    assert_eq!(f.bytes, golden(image(), &qs, surface(), context()));
    assert!(m.stats.input_stalls > 0 && m.stats.ce_stalls > 0);
}

#[test]
fn forwarding_fault_drops_tokens_and_drains_dirty_transactions() {
    use gpu_v2::framebuffer::sim::fixture::replay_forwarding;
    let qs: Vec<_> = (0..80).map(|i| quad(0, 0, 15, i)).collect();
    for failure in [1, 9] {
        let mut mem = Fixture::new(image());
        mem.fail_request = Some(failure);
        mem.beat_period = 3;
        let (m, f, _) = replay_forwarding(surface(), context(), &qs, mem, 3, 100_000).unwrap();
        assert!(m.drained() && f.idle());
        assert!(!m.flush_complete);
        if failure == 9 {
            assert!(m.stats.forwarded_lanes > 0);
            assert_eq!(m.dirty()[0], [true, true]);
        }
    }
}

#[test]
fn forwarding_real_cycle_mc_same_address_chain_matches_golden() {
    use digital_design_hardware_gowin::sdram_memory_controller::ports::OracleImage;
    let qs: Vec<_> = (0..80).map(|i| quad(0, 0, 15, i)).collect();
    let initial = image();
    // Independent initial surface data, not an audited intermediate injection.
    let external =
        unsafe { OracleImage::from_host(0, initial.clone(), "forwarding initial surface") }
            .unwrap();
    let mut port = burst::Adapter::new(external, 50_000).unwrap();
    let mut model = Model::new_forwarding(surface(), context()).unwrap();
    let mut cursor = 0;
    let mut flushing = false;
    let mut commits = Vec::new();
    for wall in 0..50_000 {
        let row = qs.get(cursor / 8).map(|q| OutputRow {
            header: q.header,
            row: (cursor % 8) as u8,
            data: q.rows()[cursor % 8],
        });
        let t = model.step(true, row, &mut port).unwrap();
        if t.input_accepted {
            cursor += 1;
        }
        if t.committed.is_some() {
            commits.push(wall);
        }
        if cursor == qs.len() * 8 && !flushing {
            model.request_flush().unwrap();
            flushing = true;
        }
        if model.flush_complete {
            break;
        }
    }
    assert!(model.flush_complete && port.idle());
    assert_eq!(commits.len(), 80);
    assert!(commits.windows(2).all(|w| w[1] - w[0] == 8));
    assert_eq!(model.stats.forwarded_lanes, 79 * 4);
    assert_eq!(
        port.combination.bridge.pins.bytes(),
        golden(initial, &qs, surface(), context())
    );
}

#[test]
fn forwarding_fault_during_live_dependency_clears_compute_tokens() {
    let qs: Vec<_> = (0..80).map(|i| quad(0, 0, 15, i)).collect();
    let mut model = Model::new_forwarding(surface(), context()).unwrap();
    let mut memory = Fixture::new(image());
    let mut cursor = 0;
    let mut aborted = false;
    for _ in 0..10_000 {
        let row = qs.get(cursor / 8).map(|q| OutputRow {
            header: q.header,
            row: (cursor % 8) as u8,
            data: q.rows()[cursor % 8],
        });
        let t = model.step(!aborted, row, &mut memory).unwrap();
        if t.input_accepted {
            cursor += 1;
        }
        if !aborted && t.forwarded.is_some() && t.arithmetic_return.is_some() {
            model.abort();
            aborted = true;
        }
        if model.drained() {
            break;
        }
    }
    assert!(aborted && model.drained() && memory.idle());
    assert!(!model.flush_complete);
}
