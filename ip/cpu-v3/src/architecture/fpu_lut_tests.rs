use super::*;

fn ulp_of(shift: i32) -> f64 {
    if shift > 0 {
        (1u64 << shift) as f64
    } else {
        1.0
    }
}

fn rcp_shift(x: i32) -> i32 {
    x.unsigned_abs().leading_zeros() as i32 - 15
}

fn rsqrt_scale_shift(x: i32) -> i32 {
    let magnitude = x as u32;
    let binary_exponent = 31 - magnitude.leading_zeros();
    let exponent = binary_exponent as i32 - 16;
    if exponent & 1 == 0 {
        exponent / 2
    } else {
        (exponent - 1) / 2
    }
}

/// `(max(|sin error|, |cos error|) in Q16.16 LSB, sin, cos)` for the
/// integer model against `f64`. The input `a` is an exact multiple of
/// `2^-16`, so the reference is trustworthy at every magnitude.
fn sincos_abs_error(a: i32) -> (i32, i32, i32) {
    let radians = f64::from(a) / 65536.0;
    let (sin, cos) = sincos_q16(a);
    let sin_exact = (radians.sin() * 65536.0).round() as i32;
    let cos_exact = (radians.cos() * 65536.0).round() as i32;
    (
        (sin - sin_exact).abs().max((cos - cos_exact).abs()),
        sin,
        cos,
    )
}

/// Deterministic xorshift64* used to sample the full `i32` input range.
fn xorshift64(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    *state = x;
    x.wrapping_mul(0x2545_F491_4F6C_DD1D)
}

#[test]
fn lut_literals_match_f64_generation() {
    for (i, &entry) in RCP_LUT.iter().enumerate() {
        let rcp = ((1.0 / (1.0 + i as f64 / 128.0)) * 65536.0).round() as u32;
        assert_eq!(entry, rcp, "RCP_LUT[{i}]");
    }
    for (i, &entry) in RSQRT_EVEN_LUT.iter().enumerate() {
        let rsqrt = ((1.0 / (1.0 + i as f64 / 128.0).sqrt()) * 65536.0).round() as u32;
        assert_eq!(entry, rsqrt, "RSQRT_EVEN_LUT[{i}]");
    }
    for (i, &entry) in RSQRT_ODD_LUT.iter().enumerate() {
        let rsqrt = ((1.0 / (2.0 * (1.0 + i as f64 / 128.0)).sqrt()) * 65536.0).round() as u32;
        assert_eq!(entry, rsqrt, "RSQRT_ODD_LUT[{i}]");
    }
    for (i, &entry) in SINCOS_LUT.iter().enumerate() {
        let sine = ((std::f64::consts::PI / 2.0 * i as f64 / 256.0).sin() * 65536.0).round() as u32;
        assert_eq!(entry, sine, "SINCOS_LUT[{i}]");
    }
    assert_eq!(RCP_LUT[0], 0x1_0000);
    assert_eq!(RSQRT_EVEN_LUT[0], 0x1_0000);
    assert_eq!(RSQRT_ODD_LUT[0], 0xB505);
    assert_eq!(SINCOS_LUT[0], 0);
    assert_eq!(SINCOS_LUT[256], 0x1_0000);
    assert_eq!(RSQRT_EVEN_ENDPOINT, 0xB505);
    assert_eq!(RSQRT_ODD_ENDPOINT, 0x8000);
}

/// The checked-in register-file Verilog must carry exactly the region the
/// renderer produces, so the file cannot drift from these tables silently.
#[test]
fn register_ram_lut_init_matches_rendered_region() {
    let source = include_str!("../hardware/fpu/cpu_v3_fpu_register_ram.v");
    assert_eq!(
        extract_lut_init_region(source),
        render_lut_init_region(),
        "cpu_v3_fpu_register_ram.v LUT init region is stale; \
             run `cargo run -p cpu-v3 --example fpu_lut_codegen`"
    );
}

#[test]
fn lut_init_region_rewrite_is_idempotent() {
    let source = format!("module m;\n{LUT_INIT_BEGIN}\n{LUT_INIT_END}\nendmodule\n");
    let once = replace_lut_init_region(&source);
    assert_eq!(extract_lut_init_region(&once), render_lut_init_region());
    assert_eq!(replace_lut_init_region(&once), once);
}

#[test]
fn range_reduction_constants_match_f64() {
    let two_over_pi = 2.0 / std::f64::consts::PI;
    let c0 = (two_over_pi * 65536.0).round() as i32;
    let c1 = ((two_over_pi - f64::from(c0) / 65536.0) * 4294967296.0).round() as i32;
    assert_eq!(TWO_OVER_PI_C0, c0);
    assert_eq!(TWO_OVER_PI_C1, c1);
}

#[test]
fn interpolation_deltas_fit_i16() {
    let mut rcp_max = 0i32;
    for i in 0..127 {
        rcp_max = rcp_max.max((RCP_LUT[i + 1] as i32 - RCP_LUT[i] as i32).abs());
    }
    rcp_max = rcp_max.max((RCP_ENDPOINT - RCP_LUT[127] as i32).abs());
    let mut rsqrt_max = 0i32;
    for i in 0..127 {
        rsqrt_max = rsqrt_max.max((RSQRT_EVEN_LUT[i + 1] as i32 - RSQRT_EVEN_LUT[i] as i32).abs());
        rsqrt_max = rsqrt_max.max((RSQRT_ODD_LUT[i + 1] as i32 - RSQRT_ODD_LUT[i] as i32).abs());
    }
    rsqrt_max = rsqrt_max.max((RSQRT_EVEN_ENDPOINT - RSQRT_EVEN_LUT[127] as i32).abs());
    rsqrt_max = rsqrt_max.max((RSQRT_ODD_ENDPOINT - RSQRT_ODD_LUT[127] as i32).abs());
    let mut sincos_max = 0i32;
    for i in 0..256 {
        sincos_max = sincos_max.max(SINCOS_LUT[i + 1] as i32 - SINCOS_LUT[i] as i32);
    }
    eprintln!("max interpolation |delta|: rcp={rcp_max}, rsqrt={rsqrt_max}, sincos={sincos_max}");
    assert!(rcp_max <= i32::from(i16::MAX));
    assert!(rsqrt_max <= i32::from(i16::MAX));
    assert!(sincos_max <= i32::from(i16::MAX));
}

#[test]
fn packed_intervals_round_trip_samples_and_signed_10_bit_deltas() {
    fn check(words: &[u32], samples: &[u32], endpoint: Option<u32>) {
        assert_eq!(words.len(), samples.len() - usize::from(endpoint.is_none()));
        for (index, &word) in words.iter().enumerate() {
            let current = word & 0x1_FFFF;
            let delta_bits = ((word >> 17) & 0x03FF) as i32;
            let delta = (delta_bits << 22) >> 22;
            let next = samples
                .get(index + 1)
                .copied()
                .or(endpoint)
                .expect("packed interval must have a next sample");
            assert_eq!(current, samples[index]);
            assert_eq!(delta, next as i32 - current as i32);
            assert!((-512..=511).contains(&delta));
            assert_eq!(word >> 27, 0);
        }
    }

    check(&packed_rcp_lut(), &RCP_LUT, Some(RCP_ENDPOINT as u32));
    check(
        &packed_rsqrt_even_lut(),
        &RSQRT_EVEN_LUT,
        Some(RSQRT_EVEN_ENDPOINT as u32),
    );
    check(
        &packed_rsqrt_odd_lut(),
        &RSQRT_ODD_LUT,
        Some(RSQRT_ODD_ENDPOINT as u32),
    );
    check(&packed_sincos_lut(), &SINCOS_LUT, None);
}

#[test]
fn packed_interpolation_is_bit_exact_for_every_index_and_residue() {
    fn packed_interpolate(word: u32, residue: i32, shift: u32) -> i32 {
        let current = (word & 0x1_FFFF) as i32;
        let delta_bits = ((word >> 17) & 0x03FF) as i32;
        let delta = (delta_bits << 22) >> 22;
        current + ((delta * residue) >> shift)
    }

    for (packed, samples, endpoint) in [
        (packed_rcp_lut(), &RCP_LUT[..], RCP_ENDPOINT),
        (
            packed_rsqrt_even_lut(),
            &RSQRT_EVEN_LUT[..],
            RSQRT_EVEN_ENDPOINT,
        ),
        (
            packed_rsqrt_odd_lut(),
            &RSQRT_ODD_LUT[..],
            RSQRT_ODD_ENDPOINT,
        ),
    ] {
        for (index, &word) in packed.iter().enumerate() {
            for residue in 0..512 {
                assert_eq!(
                    packed_interpolate(word, residue, 9),
                    interpolate(samples, index, residue, 9, endpoint),
                    "index={index} residue={residue}"
                );
            }
        }
    }

    let packed = packed_sincos_lut();
    for (index, &word) in packed.iter().enumerate() {
        for residue in 0..256 {
            assert_eq!(
                packed_interpolate(word, residue, 8),
                interpolate(&SINCOS_LUT, index, residue, 8, SINCOS_LUT[256] as i32),
                "sincos index={index} residue={residue}"
            );
        }
    }
}

fn rcp_inputs() -> Vec<i32> {
    let mut inputs = Vec::new();
    for raw in 1..=0x8000i32 {
        inputs.push(raw);
    }
    for bit in 0..16 {
        let power = 1i32 << bit;
        inputs.push(power);
        inputs.push(power + 1);
        if power > 1 {
            inputs.push(power - 1);
        }
    }
    inputs.push(1);
    inputs.push(2);
    inputs.push(3);
    inputs.push(0x7FFF_FFFF);
    inputs.push(0x4000_0000);
    inputs.push(0x2000_0000);
    inputs
}

#[test]
fn rcp_accuracy() {
    let mut max_relative = 0.0f64;
    let mut worst_relative = (0i32, 0.0f64, 0i32);
    let mut max_absolute = 0i32;
    let mut worst_absolute = (0i32, 0i32, 0i32);
    for &magnitude in &rcp_inputs() {
        for &x in &[magnitude, -magnitude] {
            let exact = (1.0 / (f64::from(x) / 65536.0)) * 65536.0;
            if exact.abs() > f64::from(i32::MAX) {
                continue;
            }
            let got = rcp_q16(x);
            let absolute = (f64::from(got) - exact).abs();
            if absolute > f64::from(max_absolute) {
                max_absolute = absolute.round() as i32;
                worst_absolute = (x, exact.round() as i32, got);
            }
            let relative = absolute / ulp_of(rcp_shift(x));
            if relative > max_relative {
                max_relative = relative;
                worst_relative = (x, exact, got);
            }
        }
    }
    eprintln!(
        "RCP: max {max_relative:.4} result-ulp at x={} (exact={:.3}, got={}), \
             max {max_absolute} raw Q16.16 LSB at x={} (exact={}, got={})",
        worst_relative.0,
        worst_relative.1,
        worst_relative.2,
        worst_absolute.0,
        worst_absolute.1,
        worst_absolute.2
    );
    assert!(
        max_relative <= 2.0,
        "RCP result-relative error {max_relative} ulp > 2 at x={}",
        worst_relative.0
    );
}

#[test]
fn rsqrt_accuracy() {
    let mut max_relative = 0.0f64;
    let mut worst_relative = (0i32, 0.0f64, 0i32);
    let mut max_absolute = 0i32;
    let mut worst_absolute = (0i32, 0i32, 0i32);
    let mut check = |magnitude: i32| {
        let exact = (1.0 / (f64::from(magnitude) / 65536.0).sqrt()) * 65536.0;
        let got = rsqrt_q16(magnitude);
        let absolute = (f64::from(got) - exact).abs();
        if absolute > f64::from(max_absolute) {
            max_absolute = absolute.round() as i32;
            worst_absolute = (magnitude, exact.round() as i32, got);
        }
        let relative = absolute / ulp_of(-rsqrt_scale_shift(magnitude).min(0));
        if relative > max_relative {
            max_relative = relative;
            worst_relative = (magnitude, exact, got);
        }
    };
    // Dense sweep of the small-magnitude range where the LUT dominates.
    for magnitude in 1..=0x8000i32 {
        check(magnitude);
    }
    // Every power of two and its neighbours catch the exponent boundaries.
    for bit in 0..31 {
        let power = 1i32 << bit;
        check(power);
        check(power + 1);
        if power > 1 {
            check(power - 1);
        }
    }
    // Deterministic stride through the whole positive i32 range.
    let mut magnitude = 0x1_0000i32;
    while let Some(next) = magnitude.checked_add(65_537) {
        check(magnitude);
        magnitude = next;
    }
    check(i32::MAX);
    eprintln!(
        "RSQRT: max {max_relative:.4} result-ulp at x={} (exact={:.3}, got={}), \
             max {max_absolute} raw Q16.16 LSB at x={} (exact={}, got={})",
        worst_relative.0,
        worst_relative.1,
        worst_relative.2,
        worst_absolute.0,
        worst_absolute.1,
        worst_absolute.2
    );
    assert!(
        max_relative <= 2.0,
        "RSQRT result-relative error {max_relative} ulp > 2 at x={}",
        worst_relative.0
    );
}

/// Full-range SINCOS error bound (Q16.16 LSB). The clamp is gone, so every
/// `i32` is reduced at its actual angle. The measured maximum over the dense
/// domain plus edges, a whole-range stride and 2^20 pseudo-random full-range
/// points is **3 LSB** (worst at `a = -2948768`, i.e. `-44.9946` rad); the
/// bound is set one LSB above that, matching the dense `<= 4` margin.
const SINCOS_FULL_RANGE_ULP: i32 = 4;

#[test]
fn sincos_full_range_accuracy() {
    // The 4 LSB guarantee still applies only to |a| <= 45 rad.
    let limit = 45 << 16;
    let mut dense_max = 0i32;
    let mut dense_worst = (0i32, 0i32, 0i32);
    let mut range_max = 0i32;
    let mut range_worst = (0i32, 0i32, 0i32);
    let mut check = |a: i32| {
        let (error, sin, cos) = sincos_abs_error(a);
        if error > range_max {
            range_max = error;
            range_worst = (a, sin, cos);
        }
        if a >= -limit && a <= limit && error > dense_max {
            dense_max = error;
            dense_worst = (a, sin, cos);
        }
    };

    // Dense sweep of the guaranteed domain, exactly as before the clamp was
    // removed (the clamp never applied here).
    for a in -limit..=limit {
        check(a);
    }

    // Every power of two and its neighbours on both signs: the sign and
    // exponent boundaries stress the `a_hi`/`a_lo` split.
    for bit in 0..31 {
        let power = 1i32 << bit;
        check(power);
        check(power + 1);
        check(power - 1);
        check(-power);
        check(-power + 1);
        check(-power - 1);
    }
    for a in [
        i32::MIN,
        i32::MIN + 1,
        i32::MIN + 2,
        i32::MAX,
        i32::MAX - 1,
        i32::MAX - 2,
    ] {
        check(a);
    }

    // Deterministic stride through the whole i32 range.
    let mut a = i32::MIN;
    while let Some(next) = a.checked_add(65_537) {
        check(a);
        a = next;
    }
    check(i32::MAX);

    // 2^20 deterministic pseudo-random full-range points.
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    for _ in 0..(1 << 20) {
        check(xorshift64(&mut state) as u32 as i32);
    }

    eprintln!(
        "SINCOS full range: dense |a|<=45 max {dense_max} LSB at a={} (sin={}, cos={}); \
             full i32 max {range_max} LSB at a={} (sin={}, cos={})",
        dense_worst.0, dense_worst.1, dense_worst.2, range_worst.0, range_worst.1, range_worst.2
    );
    assert!(
        dense_max <= 4,
        "SINCOS dense error {dense_max} ulp > 4 at a={}",
        dense_worst.0
    );
    assert!(
        range_max <= SINCOS_FULL_RANGE_ULP,
        "SINCOS full-range error {range_max} ulp > {SINCOS_FULL_RANGE_ULP} at a={}",
        range_worst.0
    );
}

#[test]
fn special_function_domains_are_defined() {
    assert_eq!(rcp_q16(0), i32::MAX);
    assert_eq!(rcp_q16(1), i32::MAX);
    assert_eq!(rcp_q16(-1), -i32::MAX);
    assert_eq!(rcp_q16(0x1_0000), 0x1_0000);
    assert_eq!(rcp_q16(-0x1_0000), -0x1_0000);
    assert_eq!(rsqrt_q16(0), 0);
    assert_eq!(rsqrt_q16(-5), 0);
    assert_eq!(rsqrt_q16(0x1_0000), 0x1_0000);
    assert_eq!(sincos_q16(0), (0, 0x1_0000));
}
