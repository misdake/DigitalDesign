//! Accepted-data boundary regressions; hand-derived integer semantics, no oracle
//! or numerical Frame values supply expected packets, RGB or runtime control.
#[path = "support/sdram/physical_texture.rs"]
#[allow(dead_code)]
mod physical;
#[path = "support/texture.rs"]
#[allow(dead_code)]
mod support;
use gpu_v2::texture::{
    emu::color::Output,
    ports::*,
    sim::{staged::bound, timed},
};

fn stimulus(n: u8, x: usize, y: usize) -> u16 {
    let n = usize::from(n);
    (((x * 5 + y * 3 + n * 7) & 31) as u16) << 11
        | (((x * 7 + y * 11 + n * 13) & 63) as u16) << 5
        | ((x * 13 + y * 17 + n * 19) & 31) as u16
}

fn expanded(n: u8, x: usize, y: usize) -> [u32; 3] {
    // Independent semantic mapping of the external RGB565 stimulus.
    let word = u32::from(stimulus(n, x, y));
    let r = (word >> 11) & 31;
    let g = (word >> 5) & 63;
    let b = word & 31;
    [r * 8 + r / 4, g * 4 + g / 16, b * 8 + b / 4]
}

fn runtime(slot: Slot) -> bound::runtime::Runtime {
    bound::runtime::Runtime::new(
        &[slot],
        bound::control::Hardware {
            storage: bound::control::Storage::Packed,
            ..Default::default()
        },
        timed::Hardware {
            prefetch: false,
            ..Default::default()
        },
        20_000,
    )
    .unwrap()
}

fn field(word: i128, shift: u32, bits: u32) -> u32 {
    ((word as u128 >> shift) & ((1 << bits) - 1)) as u32
}

#[test]
fn accepted_q16_ties_at_1024_repeat_edge_survive_caller_mutation_and_ce_stalls() {
    let slot = support::slot(10, true);
    let bytes = support::asset(slot, stimulus);
    let mut memory = physical::Physical::new(u64::from(support::BASE), bytes, true, false);
    let mut r = runtime(slot);
    let mut q = support::input(10, Filter::Nearest, [0.0; 2]);
    q.quad_id = 6;
    q.mask = 9;
    // RNE capture: -0.5 -> 0, +0.5 -> 0; -1.5 -> -2, 255.5 -> 256.
    // Differences give rho=1 and LOD=0. With n=10, Q8 coordinates are the
    // wrapped Q16 codes shifted by two, so required texels are (0,0),(1023,1).
    q.uv = [
        [-0.5 / 65536.0, 0.5 / 65536.0],
        [-1.5 / 65536.0, 63.5 / 65536.0],
        [-0.5 / 65536.0, 0.5 / 65536.0],
        [-1.5 / 65536.0, 63.5 / 65536.0],
    ];
    assert!(
        r.step(&mut memory, Some(&q), timed::Control::default())
            .unwrap()
            .accepted
    );
    // Caller storage is no longer a source for the accepted quad. Change every
    // relevant field before derivative/coordinate/packet stages are launched.
    q.quad_id = 15;
    q.mask = 0;
    q.slot = 15;
    q.material_size_log2 = 0;
    q.filter = Filter::Trilinear;
    q.lod_bias = f64::NAN;
    q.uv = [[f64::NAN; 2]; 4];
    assert_eq!(
        (q.quad_id, q.mask, q.slot, q.material_size_log2, q.filter),
        (15, 0, 15, 0, Filter::Trilinear)
    );
    assert!(q.lod_bias.is_nan() && q.uv.iter().flatten().all(|v| v.is_nan()));
    let mut pixels = vec![];
    let mut packets = vec![];
    let mut addresses = vec![];
    let mut ce_off = 0;
    let mut beats = 0;
    for t in 1..=19_000 {
        let control = timed::Control {
            ce: t % 11 > 2,
            result_ready: t > 300 && t % 13 > 3,
        };
        let st = r.step(&mut memory, None, control).unwrap();
        assert!(!st.accepted);
        ce_off += usize::from(!control.ce);
        if !control.ce || !control.result_ready {
            assert!(st.results.is_empty());
        }
        for e in st.cache.events {
            match e {
                timed::Event::Produced { payload, .. } => packets.push(payload),
                timed::Event::Submitted { address, .. } => addresses.push(address),
                timed::Event::Beat { .. } => beats += 1,
                _ => {}
            }
        }
        pixels.extend(st.results);
        if r.idle() {
            break;
        }
    }
    assert!(r.idle(), "accepted capture did not drain");
    assert_eq!(packets.len(), 2);
    for (word, (lane, tx, ty, lx, ly)) in packets.iter().zip([(0, 0, 0, 0, 0), (3, 127, 0, 7, 1)]) {
        assert_eq!(field(*word, 0, 4), 0);
        assert_eq!(field(*word, 4, 4), 10);
        assert_eq!(field(*word, 8, 7), tx);
        assert_eq!(field(*word, 15, 7), ty);
        assert_eq!(field(*word, 22, 3), lx);
        assert_eq!(field(*word, 25, 3), ly);
        assert_eq!(
            (0..4)
                .map(|i| field(*word, 28 + 9 * i, 9))
                .collect::<Vec<_>>(),
            [511, 0, 0, 0]
        );
        assert_eq!(field(*word, 64, 2), 3); // each nearest group is first and last
        assert_eq!(field(*word, 66, 4), 6);
        assert_eq!(field(*word, 70, 2), lane);
    }
    // Full-mip prefix for n=10 is 5464 tiles, each 128 bytes; largest layer
    // has 128 tiles/row. No sampler address helper computes this golden.
    assert_eq!(
        addresses,
        [
            u64::from(support::BASE) + 128 * 5464,
            u64::from(support::BASE) + 128 * (5464 + 127)
        ]
    );
    assert_eq!(
        pixels,
        [
            Output {
                key: 24,
                rgb: expanded(10, 0, 0).map(|c| c as u8)
            },
            Output {
                key: 27,
                rgb: expanded(10, 1023, 1).map(|c| c as u8)
            },
        ]
    );
    assert!(ce_off > 0 && beats == 32);
    assert_eq!(r.stats.compilations, 1);
    assert_eq!(r.stats.link.captures, 2);
    println!("Q16 capture: packets=2 beats={beats} CE-off={ce_off}, RGB={pixels:?}");
}

#[test]
fn helper_only_midpoint_lod_emits_eight_hand_derived_unorm9_seam_packets() {
    let slot = support::slot(5, true);
    let bytes = support::asset(slot, stimulus);
    let mut memory = physical::Physical::new(u64::from(support::BASE), bytes, true, true);
    let mut r = runtime(slot);
    let mut q = support::input(5, Filter::Trilinear, [0.0; 2]);
    q.quad_id = 2;
    q.mask = 1;
    // Only uncovered lane3 has a nonzero helper derivative: rho=2^-11.
    // bias=11.5 gives LOD=0.5. RNE(511*128/256)=256, parents=[255,256].
    q.uv[3][0] = 1.0 / 65536.0;
    q.lod_bias = 11.5;
    assert!(
        r.step(&mut memory, Some(&q), timed::Control::default())
            .unwrap()
            .accepted
    );
    let mut packets = vec![];
    let mut pixels = vec![];
    let mut paused_beats = 0;
    for t in 1..=19_000 {
        let st = r
            .step(
                &mut memory,
                None,
                timed::Control {
                    ce: t % 17 > 3,
                    result_ready: t > 500 && t % 19 > 4,
                },
            )
            .unwrap();
        if !st.control.ce {
            paused_beats += st
                .cache
                .events
                .iter()
                .filter(|e| matches!(e, timed::Event::Beat { .. }))
                .count();
        }
        for e in st.cache.events {
            if let timed::Event::Produced { payload, .. } = e {
                packets.push(payload);
            }
        }
        pixels.extend(st.results);
        if r.idle() {
            break;
        }
    }
    assert!(r.idle(), "midpoint seam did not drain");
    // Centered UV=0 has fu=fv=128 at both mips. Conserving floor splits:
    // parent255 -> rows[128,127] -> [64,64,64,63]; parent256 -> four64.
    let weights = [[64_u32, 64, 64, 63], [64, 64, 64, 64]];
    assert_eq!(packets.len(), 8);
    let mut sum = [0_u32; 3];
    for (i, word) in packets.iter().enumerate() {
        let plane = i / 4;
        let tap = i % 4;
        let n = 5 - plane as u8;
        let edge = (1_usize << n) - 1;
        let x = if tap & 1 == 0 { edge } else { 0 };
        let y = if tap & 2 == 0 { edge } else { 0 };
        assert_eq!(field(*word, 4, 4), u32::from(n));
        assert_eq!(field(*word, 8, 7), (x / 8) as u32);
        assert_eq!(field(*word, 15, 7), (y / 8) as u32);
        assert_eq!(field(*word, 22, 3), 7);
        assert_eq!(field(*word, 25, 3), 7);
        for j in 0..4 {
            assert_eq!(
                field(*word, 28 + 9 * j, 9),
                if j as usize == tap {
                    weights[plane][tap]
                } else {
                    0
                }
            );
        }
        assert_eq!(field(*word, 64, 1), u32::from(i == 0));
        assert_eq!(field(*word, 65, 1), u32::from(i == 7));
        assert_eq!(field(*word, 66, 4), 2);
        assert_eq!(field(*word, 70, 2), 0);
        for (s, c) in sum.iter_mut().zip(expanded(n, x, y)) {
            *s += c * weights[plane][tap];
        }
    }
    // Independent integer division with nearest rounding (odd denominator).
    let rgb = sum.map(|n| ((n + 255) / 511) as u8);
    assert_eq!(pixels, [Output { key: 8, rgb }]);
    assert!(
        paused_beats > 0,
        "accepted MC beats must return on CE-off edges"
    );
    assert_eq!(r.stats.link.captures, 8);
    assert_eq!(r.stats.link.results, 1);
    assert_eq!(r.cache_stats().refills, 8);
    println!(
        "midpoint UNORM9: sum={sum:?} RGB={rgb:?} packets=8 refills=8 paused_beats={paused_beats}"
    );
}
