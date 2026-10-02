#[path = "support/texture.rs"]
mod support;
use gpu_v2::texture::{ports::*, sim::oracle::*};
use support::*;

fn image(s: Slot) -> Image {
    Image {
        bytes: asset(s, pattern),
        requests: vec![],
    }
}
fn precision() -> Config {
    Config {
        uv_fraction: None,
        coordinate_fraction: 16,
        coefficient_fraction: 16,
        lod_fraction: 16,
        lod_method: LodMethod::Exact,
        ..Default::default()
    }
}

#[test]
fn mip_prefixes_keys_and_material_errors() {
    let expected = [0, 1, 2, 3, 4, 8, 24, 88, 344, 1368, 5464];
    for n in 0..=10 {
        assert_eq!(layer_offset(n).unwrap(), expected[usize::from(n)]);
        let key = TileKey {
            slot: 0,
            n,
            x: 0,
            y: 0,
        };
        assert_eq!(
            key.address(&[slot(10, true)]).unwrap(),
            u64::from(BASE) + u64::from(expected[usize::from(n)]) * 128
        );
    }
    let mut bad = input(4, Filter::Bilinear, [0.0; 2]);
    assert!(prepare(&bad, &[slot(5, true)], Config::default()).is_err());
    bad.uv[3][0] = f64::NAN;
    assert!(prepare(&bad, &[slot(4, true)], Config::default()).is_err());
    for s in [
        Slot {
            base_address: BASE + 1,
            ..slot(4, true)
        },
        Slot {
            valid: false,
            ..slot(4, true)
        },
        Slot {
            max_size_log2: 11,
            ..slot(4, true)
        },
        Slot {
            base_address: 0xffff_ff80,
            ..slot(4, true)
        },
    ] {
        assert!(s.validate().is_err());
    }
    assert!(TileKey {
        slot: 0,
        n: 255,
        x: 0,
        y: 0
    }
    .address(&[slot(4, true)])
    .is_err());
    assert!(TileKey {
        slot: 0,
        n: 3,
        x: 1,
        y: 0
    }
    .address(&[slot(4, true)])
    .is_err());
    assert!(TileKey {
        slot: 0,
        n: 3,
        x: 0,
        y: 0
    }
    .address(&[slot(4, false)])
    .is_err());
}

#[test]
fn four_banks_are_bijective_and_every_virtual_quad_has_four_distinct_banks() {
    let mut seen = [[false; 16]; 4];
    for y in 0..8 {
        for x in 0..8 {
            let (bank, local) = bank_local(x, y);
            assert!(!seen[bank][local]);
            seen[bank][local] = true;
            let mut banks = [
                bank_local(x, y).0,
                bank_local(x + 1, y).0,
                bank_local(x, y + 1).0,
                bank_local(x + 1, y + 1).0,
            ];
            banks.sort();
            assert_eq!(banks, [0, 1, 2, 3]);
        }
    }
    assert!(seen.into_iter().flatten().all(|v| v));
}

#[test]
fn all_bilinear_fractions_conserve_and_trilinear_endpoints_conserve() {
    let slots = [slot(4, true)];
    let mut q = input(4, Filter::Bilinear, [0.0; 2]);
    q.mask = 1;
    for fu in 0..256 {
        for fv in 0..256 {
            q.uv = [[
                (5.5 + f64::from(fu) / 256.0) / 16.0,
                (5.5 + f64::from(fv) / 256.0) / 16.0,
            ]; 4];
            let p = prepare(&q, &slots, Config::default()).unwrap();
            let weights = p.pixels[0].layers[0].coefficients;
            assert_eq!(weights.iter().sum::<u32>(), 256);
            assert_eq!(weights[1], ((256 - fv) * fu) >> 8);
            assert_eq!(weights[3], (fv * fu) >> 8);
        }
    }
    q.filter = Filter::Trilinear;
    for lambda in 0..=256 {
        q.lod_bias = f64::from(lambda) / 256.0;
        q.uv[1][0] = q.uv[0][0] + 1.0 / 16.0; // rho=1, permits finite bias
        let p = prepare(&q, &slots, Config::default()).unwrap();
        assert_eq!(
            p.pixels[0]
                .groups
                .iter()
                .flat_map(|g| g.coefficients)
                .sum::<u32>(),
            256
        );
    }
}

#[test]
fn nine_bit_coefficient_encodings_conserve_and_preserve_constant_colors() {
    let s = slot(4, true);
    for encoding in [CoefficientEncoding::Unorm, CoefficientEncoding::FixedPoint] {
        let config = Config {
            coefficient_fraction: 9,
            coefficient_encoding: encoding,
            ..Default::default()
        };
        let scale = if encoding == CoefficientEncoding::Unorm {
            511
        } else {
            512
        };
        assert_eq!(config.coefficient_scale(), scale);
        let mut q = input(4, Filter::Bilinear, [0.0; 2]);
        q.mask = 1;
        for fu in 0..256 {
            for fv in 0..256 {
                q.uv = [[
                    (5.5 + f64::from(fu) / 256.0) / 16.0,
                    (5.5 + f64::from(fv) / 256.0) / 16.0,
                ]; 4];
                let p = prepare(&q, &[s], config).unwrap();
                let weights = p.pixels[0].layers[0].coefficients;
                assert_eq!(weights.iter().sum::<u32>(), scale);
                assert!(weights.iter().all(|&w| w <= scale));
            }
        }
        q.filter = Filter::Trilinear;
        q.uv[1][0] = q.uv[0][0] + 1.0 / 16.0;
        for lambda in 0..=256 {
            q.lod_bias = f64::from(lambda) / 256.0;
            let p = prepare(&q, &[s], config).unwrap();
            assert_eq!(
                p.pixels[0]
                    .groups
                    .iter()
                    .flat_map(|g| g.coefficients)
                    .sum::<u32>(),
                scale
            );
        }
        for (word, want) in [
            (0, [0, 0, 0]),
            (0xffff, [255, 255, 255]),
            (0xf800, [255, 0, 0]),
            (0x07e0, [0, 255, 0]),
            (0x001f, [0, 0, 255]),
        ] {
            let mut memory = Image {
                bytes: asset(s, |_, _, _| word),
                requests: vec![],
            };
            let mut cache = Cache::new(vec![s]).unwrap();
            for filter in [Filter::Nearest, Filter::Bilinear, Filter::Trilinear] {
                q.filter = filter;
                for bias in [0.0, 0.5, 4.0] {
                    q.lod_bias = bias;
                    assert_eq!(
                        sample(&q, &mut cache, &mut memory, config).unwrap().pixels[0].rgb,
                        want
                    );
                }
            }
        }
    }
    // Odd UNORM denominator has no exact halfway remainder.
    for (value, want) in [(255, 0), (256, 1), (766, 1), (767, 2), (511 * 255, 255)] {
        assert_eq!(rne_div(value, 511), want);
    }
}

#[test]
fn zero_mask_distinguishes_all_513_weights_without_widening_coefficient_fields() {
    let p = prepare(
        &input(4, Filter::Nearest, [0.5; 2]),
        &[slot(4, true)],
        Config::default(),
    )
    .unwrap();
    let mut group = p.pixels[0].groups[0].clone();
    for weight in 0..=512 {
        group.coefficients = [weight, 512 - weight, 0, 1];
        let word = group.pack76_zero_mask().unwrap();
        assert!(word < 1_u128 << 76);
        for tap in 0..4 {
            let code = ((word >> (28 + 9 * tap)) & 511) as u32;
            let is_zero = word >> (72 + tap) & 1 != 0;
            let decoded = if is_zero {
                0
            } else if code == 0 {
                512
            } else {
                code
            };
            assert_eq!(decoded, group.coefficients[tap]);
        }
    }
    group.coefficients = [511, 0, 0, 0];
    assert_eq!((group.pack72().unwrap() >> 28) & 511, 511);
    group.coefficients[0] = 512;
    assert!(group.pack72().is_err());
    group.coefficients[0] = 513;
    assert!(group.pack76_zero_mask().is_err());
}

#[test]
fn wrap_seams_groups_zero_omission_and_saturated_smallest_mip() {
    let slots = [slot(4, true)];
    let q = input(4, Filter::Bilinear, [0.5, 0.5]);
    let p = prepare(&q, &slots, Config::default()).unwrap();
    assert_eq!(p.pixels[0].groups.len(), 4);
    assert_eq!(
        p.pixels[0]
            .groups
            .iter()
            .map(|g| (g.key.x, g.key.y))
            .collect::<Vec<_>>(),
        [(0, 0), (1, 0), (0, 1), (1, 1)]
    );
    let q = input(4, Filter::Bilinear, [0.0, 0.0]);
    let p = prepare(&q, &slots, Config::default()).unwrap();
    assert_eq!(
        p.pixels[0].layers[0].taps,
        [[15, 15], [0, 15], [15, 0], [0, 0]]
    );
    let slots = [slot(5, true)];
    let mut q = input(5, Filter::Trilinear, [0.0, 0.0]);
    q.uv[1][0] += 1.0 / 32.0;
    q.lod_bias = 0.5;
    let p = prepare(&q, &slots, Config::default()).unwrap();
    assert_eq!(p.pixels[0].groups.len(), 8); // four tiles in each layer, including wrap
    assert!(p.pixels[0].groups[0].first);
    assert!(p.pixels[0].groups[7].last);
    assert!(p.pixels[0].groups[1..].iter().all(|g| !g.first));
    assert!(p.pixels[0].groups[..7].iter().all(|g| !g.last));
    assert!(p.pixels[0]
        .groups
        .iter()
        .all(|g| g.pack72().unwrap() < 1_u128 << 72));
    q.lod_bias = 100.0;
    let p = prepare(&q, &slots, Config::default()).unwrap();
    assert_eq!(p.pixels[0].layers.len(), 1);
    assert_eq!(p.pixels[0].layers[0].n, 0);
    let p = prepare(
        &input(5, Filter::Bilinear, [0.5 / 32.0; 2]),
        &slots,
        Config::default(),
    )
    .unwrap();
    assert_eq!(p.pixels[0].groups.len(), 1);
    assert_eq!(p.pixels[0].groups[0].coefficients, [256, 0, 0, 0]);
}

#[test]
fn helper_derivatives_are_unwrapped_mask_independent_and_overflow_saturates() {
    let mut q = input(10, Filter::Trilinear, [0.999, 0.1]);
    q.mask = 1;
    q.uv[1][0] = 1.001;
    let p = prepare(
        &q,
        &[slot(10, true)],
        Config {
            uv_fraction: None,
            lod_method: LodMethod::Exact,
            ..Default::default()
        },
    )
    .unwrap();
    assert!((p.lod.rho - 2.048).abs() < 1e-9);
    assert_eq!(p.pixels.len(), 1);
    q.uv[3][1] = 0.1 + 0.125;
    let p = prepare(&q, &[slot(10, true)], Config::default()).unwrap();
    assert_eq!(p.lod.selected, 7.0);
    q.uv[3][1] = 10.0;
    q.lod_bias = -100.0;
    let p = prepare(&q, &[slot(10, true)], Config::default()).unwrap();
    assert!(p.lod.overflow);
    assert_eq!(p.lod.selected, 10.0);
    let p = prepare(&q, &[slot(10, false)], Config::default()).unwrap();
    assert_eq!(p.lod.selected, 0.0);
    assert_eq!(p.pixels[0].layers[0].n, 10);
    q.mask = 0;
    assert!(prepare(&q, &[slot(10, true)], Config::default())
        .unwrap()
        .pixels
        .is_empty());
}

#[test]
fn nearest_and_small_mip_periodic_payloads_match_logical_texels() {
    for n in 0..=4 {
        let s = slot(n, false);
        let mut source = image(s);
        let mut cache = Cache::new(vec![s]).unwrap();
        for uv in [
            [0.0, 0.0],
            [1.0, 1.0],
            [-0.25, 1.25],
            [0.13, 0.77],
            [0.999, 0.501],
        ] {
            for filter in [Filter::Nearest, Filter::Bilinear, Filter::Trilinear] {
                let q = input(n, filter, uv);
                let got = sample(&q, &mut cache, &mut source, precision()).unwrap();
                let ideal = reference(&q, &[s], &mut source, MipSelection::Nearest).unwrap();
                for (pixel, (_, rgb)) in got.pixels.iter().zip(ideal) {
                    for (actual, want) in pixel.rgb.iter().zip(rgb) {
                        assert!((f64::from(*actual) - want).abs() <= 0.501);
                    }
                }
            }
        }
    }
}

#[test]
fn constants_remain_exact_across_eight_groups_and_only_last_rounds() {
    for color in [0, 0xffff, 0xf800, 0x07e0, 0x001f, 0x39e7] {
        let s = slot(5, true);
        let mut source = Image {
            bytes: asset(s, |_, _, _| color),
            requests: vec![],
        };
        let mut cache = Cache::new(vec![s]).unwrap();
        let mut q = input(5, Filter::Trilinear, [0.0, 0.0]);
        q.uv[1][0] = 1.0 / 32.0;
        q.lod_bias = 0.375;
        let got = sample(&q, &mut cache, &mut source, Config::default()).unwrap();
        for pixel in &got.pixels {
            assert_eq!(pixel.rgb, expand565(color));
            assert_eq!(
                pixel.groups.last().unwrap().accumulator,
                expand565(color).map(|v| u64::from(v) * 256)
            );
            assert!(pixel
                .groups
                .iter()
                .all(|g| g.accumulator.iter().all(|&v| v <= 65280)));
        }
        assert_eq!(got.pixels[0].groups.len(), 8);
        if color == 0xffff {
            assert_eq!(
                got.pixels[0]
                    .groups
                    .iter()
                    .map(|g| rne_div(g.partial[0], 256))
                    .sum::<u64>(),
                256
            );
            assert_eq!(got.pixels[0].rgb[0], 255); // per-group rounding would overflow white
        }
    }
    for (sum, want) in [
        (127, 0),
        (128, 0),
        (129, 1),
        (384, 2),
        (640, 2),
        (65280, 255),
    ] {
        assert_eq!(rne_div(sum, 256), want);
    }
    // A single nonzero tile among four groups retains its unrounded raw sum.
    let s = slot(4, false);
    let mut source = Image {
        bytes: asset(s, |_, x, y| if x < 8 && y < 8 { 0x0800 } else { 0 }),
        requests: vec![],
    };
    let mut cache = Cache::new(vec![s]).unwrap();
    let out = sample(
        &input(4, Filter::Bilinear, [0.5; 2]),
        &mut cache,
        &mut source,
        Config::default(),
    )
    .unwrap();
    assert_eq!(
        out.pixels[0].groups.last().unwrap().accumulator,
        [512, 0, 0]
    );
    assert_eq!(out.pixels[0].rgb, [2, 0, 0]);
}

#[test]
fn cache_requeries_logical_keys_plru_prefetch_and_rebind() {
    let s = slot(6, false);
    let mut source = image(s);
    let mut cache = Cache::new(vec![s]).unwrap();
    let key = |x, y| TileKey {
        slot: 0,
        n: 6,
        x,
        y,
    };
    let keys = [key(0, 0), key(4, 0), key(0, 4), key(4, 4)];
    for k in keys {
        cache.prefetch(k, &mut source).unwrap();
    }
    assert_eq!(cache.stats.refills, 4);
    assert_eq!(cache.plru_order(0)[0], 0);
    let events = cache.prefetch(keys[0], &mut source).unwrap();
    assert!(matches!(
        events[0],
        CacheEvent::Hit {
            access: Access::Prefetch,
            ..
        }
    ));
    assert_eq!(cache.plru_order(0)[0], 2);
    let other_slot = Slot {
        base_address: BASE + 8192,
        ..s
    };
    source.bytes.extend(asset(other_slot, |_, _, _| 0xffff));
    cache.rebind(vec![s, other_slot]).unwrap();
    for k in keys {
        cache.prefetch(k, &mut source).unwrap();
    }
    let fifth = TileKey { slot: 1, ..keys[0] };
    cache.prefetch(fifth, &mut source).unwrap();
    assert!(cache.lookup(keys[0]).is_none());
    assert_eq!(cache.lookup(fifth), Some((0, State::Ready)));
    let out = sample(
        &input(6, Filter::Nearest, [0.0; 2]),
        &mut cache,
        &mut source,
        Config::default(),
    )
    .unwrap();
    assert_eq!(out.pixels[0].rgb, expand565(pattern(6, 0, 0)));
    assert_eq!(cache.stats.beats, cache.stats.refills * 16);
    let refills = cache.stats.refills;
    cache.rebind(vec![other_slot]).unwrap();
    let out = sample(
        &input(6, Filter::Nearest, [0.0; 2]),
        &mut cache,
        &mut source,
        Config::default(),
    )
    .unwrap();
    assert_eq!(out.pixels[0].rgb, [255; 3]);
    assert_eq!(cache.stats.refills, refills + 1);
}

#[test]
fn truncated_refill_cannot_publish_ready_and_watchdogs_are_explicit() {
    struct Short;
    impl MemoryPort for Short {
        fn read_dma(&mut self, _: u64, _: usize) -> Result<Vec<u64>, String> {
            Ok(vec![u64::MAX; 15])
        }
    }
    let s = slot(0, false);
    let mut cache = Cache::new(vec![s]).unwrap();
    assert!(sample(
        &input(0, Filter::Nearest, [0.0; 2]),
        &mut cache,
        &mut Short,
        Config::default()
    )
    .is_err());
    assert_eq!(
        cache.lookup(TileKey {
            slot: 0,
            n: 0,
            x: 0,
            y: 0
        }),
        Some((0, State::Filling))
    );
    assert_eq!(cache.stats.refills, 0);
    assert!(cache.rebind(vec![s]).is_err());
    assert!(run(
        &[
            input(0, Filter::Nearest, [0.0; 2]),
            input(0, Filter::Nearest, [0.0; 2])
        ],
        &mut cache,
        &mut Short,
        Config {
            max_quads: 1,
            ..Default::default()
        }
    )
    .is_err());
}

#[test]
fn bounded_precision_sweep_matches_independent_float_reference() {
    let s = slot(6, true);
    let mut source = image(s);
    let mut cache = Cache::new(vec![s]).unwrap();
    let mut state = 0x3141_5926_u64;
    let mut worst = 0.0_f64;
    for i in 0..256 {
        // fixed sample/event bound
        let mut random = || {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            (state >> 32) as u32
        };
        let uv = [
            f64::from(random()) / f64::from(u32::MAX) * 4.0 - 2.0,
            f64::from(random()) / f64::from(u32::MAX) * 4.0 - 2.0,
        ];
        let mut q = input(
            6,
            if i % 2 == 0 {
                Filter::Bilinear
            } else {
                Filter::Trilinear
            },
            uv,
        );
        let slope = 2.0_f64.powf(f64::from(random() % 1400) / 256.0) / 64.0;
        q.uv[1][0] += slope;
        q.uv[2][1] += slope * 0.7;
        q.uv[3] = [uv[0] + slope, uv[1] + slope * 0.7];
        let ideal = reference(&q, &[s], &mut source, MipSelection::Nearest).unwrap();
        let out = sample(&q, &mut cache, &mut source, precision()).unwrap();
        for (pixel, (_, rgb)) in out.pixels.iter().zip(ideal) {
            for (actual, want) in pixel.rgb.iter().zip(rgb) {
                worst = worst.max((f64::from(*actual) - want).abs());
            }
        }
    }
    assert!(worst < 0.53, "16-bit oracle error {worst}");
}

#[test]
fn mip_rounding_is_explicit_and_fraction_table_has_a_bounded_error() {
    let s = slot(4, true);
    let mut source = Image {
        bytes: asset(s, |n, _, _| if n == 4 { 0xffff } else { 0 }),
        requests: vec![],
    };
    let mut q = input(4, Filter::Bilinear, [0.25; 2]);
    q.uv[1][0] += 1.0 / 16.0;
    q.lod_bias = 0.5;
    for (selection, expected) in [(MipSelection::Floor, 255), (MipSelection::Nearest, 0)] {
        let mut cache = Cache::new(vec![s]).unwrap();
        let out = sample(
            &q,
            &mut cache,
            &mut source,
            Config {
                mip_selection: selection,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(out.pixels[0].rgb, [expected; 3]);
    }
    q.filter = Filter::Trilinear;
    let mut cache = Cache::new(vec![s]).unwrap();
    let out = sample(&q, &mut cache, &mut source, Config::default()).unwrap();
    assert_eq!(out.pixels[0].rgb, [128; 3]); // one final ties-to-even round
    q.lod_bias = 0.0;
    for i in 0..=2048 {
        let rho = 1.0 + f64::from(i) / 2048.0;
        q.uv = [[0.0; 2], [rho / 16.0, 0.0], [0.0; 2], [rho / 16.0, 0.0]];
        let out = prepare(
            &q,
            &[s],
            Config {
                uv_fraction: None,
                ..Default::default()
            },
        )
        .unwrap();
        assert!((out.lod.selected - rho.log2()).abs() < 0.024);
    }
}

#[test]
fn all_sixty_four_refill_words_are_recovered_from_their_independent_asset_positions() {
    let s = slot(3, false);
    let mut source = image(s);
    let mut cache = Cache::new(vec![s]).unwrap();
    for y in 0..8 {
        for x in 0..8 {
            let out = sample(
                &input(
                    3,
                    Filter::Nearest,
                    [(x as f64 + 0.5) / 8.0, (y as f64 + 0.5) / 8.0],
                ),
                &mut cache,
                &mut source,
                Config::default(),
            )
            .unwrap();
            assert_eq!(out.pixels[0].rgb, expand565(pattern(3, x, y)));
        }
    }
    assert_eq!(source.requests, [(u64::from(BASE), 128)]);
}
