use super::*;
use crate::texture::ports::{Filter, Group4};
#[path = "physical.rs"]
mod physical;
const BASE: u32 = 4096;
fn pattern(n: u8, x: usize, y: usize) -> u16 {
    [0xf800, 0x07e0, 0x001f, 0xffff, 0xffe0, 0xf81f, 0x07ff, 0][(x / 4 + y + usize::from(n)) % 8]
}
fn fixture() -> (Vec<Slot>, Vec<u8>) {
    let mut bytes = vec![];
    for n in 0..=5 {
        let logical = 1_usize << n;
        let side = logical.div_ceil(8);
        for ty in 0..side {
            for tx in 0..side {
                for y in 0..8 {
                    for x in 0..8 {
                        bytes.extend(
                            pattern(n, (tx * 8 + x) % logical, (ty * 8 + y) % logical)
                                .to_le_bytes(),
                        );
                    }
                }
            }
        }
    }
    (
        vec![Slot {
            base_address: BASE,
            has_full_mip: true,
            max_size_log2: 5,
            valid: true,
        }],
        bytes,
    )
}
fn input(id: u8, mask: u8, uv: [f64; 2], filter: Filter) -> QuadInput {
    let mut q = QuadInput {
        quad_id: id,
        mask,
        uv: [uv; 4],
        slot: 0,
        material_size_log2: 5,
        filter,
        lod_bias: 0.5,
    };
    q.uv[1][0] += 1.0 / 32.0;
    q.uv[2][1] += 1.0 / 32.0;
    q
}
fn expand(word: u16) -> [u32; 3] {
    let r = u32::from(word >> 11);
    let g = u32::from(word >> 5 & 63);
    let b = u32::from(word & 31);
    [
        (r << 3) | (r >> 2),
        (g << 2) | (g >> 4),
        (b << 3) | (b >> 2),
    ]
}
/// Independent scalar integer reference on this dyadic/rho1 fixture. No sampler,
/// counted Frame, packet/color helper or cache provenance is read here.
fn golden(q: &QuadInput) -> Vec<color::Output> {
    let mut out = vec![];
    for lane in 0..4 {
        if q.mask >> lane & 1 == 0 {
            continue;
        }
        let mut sums = [0_u32; 3];
        for (which, n, parent) in [
            (
                0,
                5,
                if q.filter == Filter::Trilinear {
                    255
                } else {
                    511
                },
            ),
            (1, 4, 256),
        ] {
            if which == 1 && q.filter != Filter::Trilinear {
                continue;
            }
            let nearest = q.filter == Filter::Nearest;
            let mut axes = [(0_i32, 0_u32); 2];
            for (a, axis) in axes.iter_mut().enumerate() {
                let uv = ((q.uv[lane][a] * 262144.0).round_ties_even() as i64).rem_euclid(262144);
                let coord = (uv * (1 << n) * 256 / 262144) as i32 - if nearest { 0 } else { 128 };
                *axis = (
                    coord.div_euclid(256),
                    if nearest {
                        0
                    } else {
                        coord.rem_euclid(256) as u32
                    },
                );
            }
            let bottom = parent * axes[1].1 / 256;
            let top = parent - bottom;
            let br = bottom * axes[0].1 / 256;
            let tr = top * axes[0].1 / 256;
            for (tap, weight) in [top - tr, tr, bottom - br, br].into_iter().enumerate() {
                let x = (axes[0].0 + (tap & 1) as i32).rem_euclid(1 << n) as usize;
                let y = (axes[1].0 + (tap >> 1) as i32).rem_euclid(1 << n) as usize;
                for (sum, value) in sums.iter_mut().zip(expand(pattern(n, x, y))) {
                    *sum += value * weight;
                }
            }
        }
        out.push(color::Output {
            key: q.quad_id * 4 + lane as u8,
            rgb: sums.map(|s| ((s + 255) / 511) as u8),
        });
    }
    out
}
fn bounds(h: &Hybrid, e: &Edge) {
    let pool = e.cache.snapshot.packet_pool.as_ref().unwrap();
    assert!(pool.producer <= 16 && pool.groups <= 32 && pool.rows <= 64);
    assert!(pool.heads + usize::from(pool.pending) <= 2);
    assert!(h.work_count <= 16 && h.coordinate_count <= 6 && h.members.len <= 8);
    assert_eq!(
        usize::from(h.coordinate_count),
        h.coordinates.len + h.coordinate_ready.len
    );
    assert_eq!(u64::from(h.work_count), h.stats.reserved - h.stats.ack);
    assert_eq!(
        pool.unwritten, h.packets.len,
        "P16 owns every in-flight single packet"
    );
    assert!(h.color.snapshot().result_credits <= 16);
    assert_eq!(
        e.post_work,
        e.pre_work + e.coefficient.work_reserved - u8::from(e.ack)
    );
    if e.coefficient.accepted {
        assert!(16 - e.pre_work >= e.offer_cost);
        assert_eq!(e.coefficient.work_reserved, e.offer_cost);
    }
    if !e.base_ce {
        assert!(e.packet.is_none() && e.plane.is_none() && !e.ack && !e.coefficient.accepted);
    }
}
fn totals(h: &Hybrid, name: &str) {
    assert!(h.idle(), "{name}: failed bounded drain at wall{}", h.wall);
    assert!(
        h.stats.accepted > 0
            && h.stats.products > 0
            && h.stats.returns > 0
            && h.stats.planes > 0
            && h.stats.packets > 0
            && h.stats.captures > 0
            && h.stats.consumes > 0
    );
    assert!(h.cache.stats.beats > 0 && h.cache.stats.refills > 0);
    assert_eq!(h.stats.reserved, h.stats.ack);
    println!("HYBRID {name} wall={} base={} coefficient={} accepted={} products={} returns={} planes={} packet_issues={} W={} MC_beats={} READY={} captures={} consumes={} reserved={} ACK={} work_peak={} coordinate_peak={} membership_peak={} poisoned={}",
        h.wall,h.enabled,h.coefficient.snapshot().enabled,h.stats.accepted,h.stats.products,h.stats.returns,
        h.stats.planes,h.stats.issues,h.stats.packets,h.cache.stats.beats,h.cache.stats.refills,
        h.stats.captures,h.stats.consumes,h.stats.reserved,h.stats.ack,h.stats.peak_work,
        h.stats.peak_coordinate,h.stats.peak_members,h.stats.poisoned);
}
fn literal_packets() -> Vec<i128> {
    let mut packets = vec![];
    for lane in [0, 3] {
        for tap in 0..4 {
            // Independent hand layout: TL/TR/BL/BR land in four distinct
            // fine tiles; coarse coordinates3/4 all share tile0.
            packets.push(
                5 << 4
                    | (tap & 1) << 8
                    | (tap >> 1) << 15
                    | 7 << 22
                    | 7 << 25
                    | (if tap == 3 { 63 } else { 64 }) << (28 + tap * 9)
                    | i128::from(tap == 0) << 64
                    | 3 << 66
                    | lane << 70,
            );
        }
        packets.push(
            4 << 4
                | 3 << 22
                | 3 << 25
                | 64 << 28
                | 64 << 37
                | 64 << 46
                | 64 << 55
                | 1 << 65
                | 3 << 66
                | lane << 70,
        );
    }
    packets
}
fn inventory(h: &Hybrid) {
    let base = bound::inventory::describe(
        &h.binding,
        bound::control::Hardware {
            storage: bound::control::Storage::Packed,
            ..Default::default()
        },
        h.cache.external_context().1,
    )
    .unwrap();
    let mut report =
        String::from("{\"schema\":1,\"physical_area_claim\":false,\"baseline_rows\":[");
    for (i, row) in base.rows.iter().enumerate() {
        if i > 0 {
            report.push(',');
        }
        report.push_str(&format!("{{\"owner\":\"{}\",\"FF\":{},\"sdp4\":{},\"ram16x1\":{},\"BSRAM\":{},\"hard\":{},\"ports\":\"{}\"}}",row.name,row.ff_bits,row.sdp4_cells,row.ram16x1_cells,row.bsram,row.hard_pipeline_bits,row.ports));
    }
    let work = base
        .rows
        .iter()
        .find(|r| r.name == "plane work records")
        .unwrap();
    let color_bits =
        color::ALLOCATION.datapath_ff_bits + color::ALLOCATION.result_and_control_ff_bits;
    report.push_str(&format!("],\"base_cache_preparation_FF\":{},\"unchanged_color_emu_FF\":{},\"unchanged_link_FF\":266,\"work_transport_added_FF\":1504,\"work_removed_sdp4\":{},\"work_removed_ram16x1\":{},\"queue_control_increment\":13,\"independent_fault_increment\":1,\"ready_plane_increment\":1,\"candidate_composition_FF\":{},\"candidate_sdp4\":{},\"candidate_ram16x1\":{},\"BSRAM\":{},\"hard_pipeline_bits\":{},\"fitted\":false}}",
        base.ff_bits,color_bits,work.sdp4_cells,work.ram16x1_cells,base.ff_bits+color_bits as u64+266+1519,
        base.sdp4_cells-work.sdp4_cells,base.ram16x1_cells-work.ram16x1_cells,base.bsram,
        base.hard_pipeline_bits+color::ALLOCATION.hard_product_bits as u64));
    // Optional export never affects execution or readiness. All evidence stays target.
    if let Ok(path) = std::env::var("GPU_COEFFICIENT_HYBRID_OUTPUT") {
        std::fs::write(std::path::Path::new(&path).join("inventory.json"), report).unwrap();
    }
}
#[test]
fn bounded_live_coefficient_hybrid_qualification() {
    let (slots, bytes) = fixture();
    // Literal, seam, sparse masks, poison and genuine same-ID reuse in one instance.
    let mut h = Hybrid::new(&slots).unwrap();
    inventory(&h);
    let mut mc = physical::Physical::new(u64::from(BASE), bytes.clone(), true, false);
    let a = input(3, 9, [0.25, 0.25], Filter::Trilinear);
    let b = input(3, 9, [-0.03125, 0.46875], Filter::Bilinear);
    let c = input(7, 1, [0.625, 0.15625], Filter::Nearest);
    let mut accepted = 0;
    let mut got = vec![];
    let mut literal = 0;
    let mut mapped = 0;
    let mut blocked_reuse = 0;
    let mut actual_packets = vec![];
    for _ in 0..BOUND {
        let offer = match accepted {
            0 => Some(&a),
            1 => Some(&b),
            2 => Some(&c),
            _ => None,
        };
        let e = h.step(&mut mc, offer, Stimulus::run(), true).unwrap();
        bounds(&h, &e);
        if accepted == 1 && !e.accepted {
            blocked_reuse += 1;
        }
        if e.accepted {
            accepted += 1;
        }
        if let Some((output, which)) = e.plane {
            if output.metadata.key / 4 == 3 && accepted <= 1 {
                assert_eq!(output.weights, [[64, 64, 64, 63], [64, 64, 64, 64]]);
                assert_eq!(output.metadata.levels, [5, 4]);
                assert_eq!(output.metadata.coordinates, [[7, 8, 7, 8], [3, 4, 3, 4]]);
                literal += 1;
            }
            assert_eq!(
                output.weights[which].iter().sum::<u16>(),
                if which == 0 {
                    if output.metadata.last_fine {
                        511
                    } else {
                        255
                    }
                } else {
                    256
                }
            );
        }
        if let Some((key, ordinal)) = e.issue_ordinal {
            if key == 15 {
                assert_eq!(ordinal, 1);
                mapped += 1;
            }
        }
        if let Some(payload) = e.packet {
            assert!(Group4::unpack72(payload).is_ok());
            actual_packets.push(payload);
        }
        got.extend(e.results);
        if accepted == 3 && h.idle() {
            break;
        }
    }
    let expected = [golden(&a), golden(&b), golden(&c)].concat();
    assert_eq!(
        expected.iter().map(|o| o.rgb).collect::<Vec<_>>(),
        vec![
            [96, 96, 96],
            [96, 96, 96],
            [127, 127, 255],
            [127, 127, 255],
            [0, 0, 0]
        ]
    );
    assert_ne!(golden(&a), golden(&b));
    assert_eq!(got, expected);
    assert_eq!(&actual_packets[..10], literal_packets());
    assert!(literal >= 4 && mapped > 0 && blocked_reuse > 0 && h.stats.poisoned > 0);
    assert_eq!(mc.cycles, h.wall);
    assert!(mc.init_cycles > 0 && mc.init_cycles < 20_000);
    totals(&h, "literal-poison-reuse");

    // Fill actual ready2 after some owned membership work is older. Hold only
    // its downstream handshake; then release packet drain while ready remains full.
    let mut h = Hybrid::new(&slots).unwrap();
    let mut mc = physical::Physical::new(u64::from(BASE), bytes.clone(), true, false);
    let qs: Vec<_> = (0..4)
        .map(|i| input(i, 15, [0.25, 0.25], Filter::Trilinear))
        .collect();
    let mut accepted = 0;
    let mut phase = 0;
    let mut held = 0;
    let mut drained = 0;
    let mut group_drain = 0;
    let mut got = vec![];
    for _ in 0..BOUND {
        if phase == 0 && h.stats.planes >= 4 {
            phase = 1;
        }
        if phase == 1 && h.coefficient.snapshot().queued == 2 && h.work.len > 0 {
            phase = 2;
        }
        if phase == 2 && held >= 40 {
            phase = 3;
        }
        let before = h.coefficient.snapshot();
        let e = h
            .step(
                &mut mc,
                qs.get(accepted),
                Stimulus {
                    control: timed::Control::default(),
                    membership_ready: phase == 0 || phase == 3,
                    packet_ready: phase >= 2,
                },
                true,
            )
            .unwrap();
        bounds(&h, &e);
        if e.accepted {
            accepted += 1;
        }
        if phase == 2 {
            assert_eq!(before.queued, 2);
            assert_eq!(e.coefficient.snapshot.enabled, before.enabled);
            assert_eq!(e.coefficient.snapshot.numeric_words, before.numeric_words);
            assert_eq!(
                e.coefficient.snapshot.product_registers,
                before.product_registers
            );
            assert_eq!(e.coefficient.snapshot.phase, before.phase);
            assert!(e.base_ce);
            held += 1;
            drained += usize::from(e.issue_ordinal.is_some() || e.packet.is_some());
            group_drain += e
                .cache
                .packet_events
                .iter()
                .filter(|e| matches!(e, timed::packet::Event::Transfer { .. }))
                .count();
        }
        got.extend(e.results);
        if accepted == 4 && h.idle() {
            break;
        }
    }
    assert!(phase == 3 && held == 40 && drained > 0 && group_drain > 0);
    assert_eq!(got, qs.iter().flat_map(golden).collect::<Vec<_>>());
    assert_eq!(mc.cycles, h.wall);
    totals(&h, "ready2-local-hold");
    println!("HOLD full_ready_edges={held} downstream_issue_or_W_edges={drained} Group_transfers={group_drain}");

    // Actual work16 pressure. Hold packet operand capture until reservation full.
    // A real submitted miss then runs to READY with caller CE0. Continue consumer
    // pressure and drain, without reconstructing any machine per quad.
    let mut h = Hybrid::new(&slots).unwrap();
    let mut mc = physical::Physical::new(u64::from(BASE), bytes, true, true);
    let qs: Vec<_> = (0..6)
        .map(|i| input(i, 15, [0.25, 0.25], Filter::Trilinear))
        .collect();
    let mut accepted = 0;
    let mut release_packets = false;
    let mut full = 0;
    let mut one = 0;
    let mut same_ack = 0;
    let mut pause = 0;
    let mut paused_once = false;
    let mut beat_off = 0;
    let mut ready_off = 0;
    let mut result_peak = 0;
    let mut got = vec![];
    for _ in 0..BOUND {
        if h.work_count == 16 {
            release_packets = true;
        }
        let caller_ce = pause == 0;
        let before = h.coefficient.snapshot();
        let e = h
            .step(
                &mut mc,
                qs.get(accepted),
                Stimulus {
                    control: timed::Control {
                        ce: caller_ce,
                        result_ready: h.wall > 3000,
                    },
                    membership_ready: true,
                    packet_ready: release_packets,
                },
                true,
            )
            .unwrap();
        bounds(&h, &e);
        if e.accepted {
            accepted += 1;
        }
        if e.pre_work == 16 && e.offer_cost > 0 {
            full += 1;
            assert!(!e.coefficient.accepted);
        }
        if e.pre_work == 15 && e.offer_cost == 2 {
            one += 1;
            assert!(!e.coefficient.accepted);
        }
        if e.ack && e.offer_cost > 16 - e.pre_work {
            same_ack += 1;
            assert!(!e.coefficient.accepted);
        }
        if !paused_once
            && e.cache
                .events
                .iter()
                .any(|e| matches!(e, timed::Event::Submitted { .. }))
        {
            pause = 600;
            paused_once = true;
        } else if pause > 0 {
            pause -= 1;
        }
        if !caller_ce {
            assert_eq!(e.coefficient.snapshot.enabled, before.enabled);
            beat_off += e
                .cache
                .events
                .iter()
                .filter(|e| matches!(e, timed::Event::Beat { .. }))
                .count();
            ready_off += e
                .cache
                .events
                .iter()
                .filter(|e| matches!(e, timed::Event::Ready { .. }))
                .count();
            assert!(e.results.is_empty());
        }
        result_peak = result_peak.max(h.color.snapshot().result_credits);
        got.extend(e.results);
        if accepted == 6 && h.idle() {
            break;
        }
    }
    assert!(full > 0 && one > 0 && same_ack > 0 && beat_off > 0 && ready_off > 0);
    assert_eq!(result_peak, 16);
    assert_eq!(got, qs.iter().flat_map(golden).collect::<Vec<_>>());
    assert_eq!(mc.cycles, h.wall);
    totals(&h, "work16-CE0-result-pressure");
    println!("PRESSURE work_full_edges={full} free1_requires2_edges={one} blocked_same_edge_ACK={same_ack} CE0_beats={beat_off} CE0_READY={ready_off} result_peak={result_peak}");
}
