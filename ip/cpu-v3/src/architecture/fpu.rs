//! Architectural Q16.16 arithmetic for the CpuV3 FPU v2.
//!
//! Every architectural F register is a signed Q16.16 value: bit 31 is the sign
//! bit, bits 30:16 the integer part, and bits 15:0 the fraction. The numeric
//! range is `[-32768, +32767.9999847]`. All arithmetic wraps (two's-complement
//! modulo `2^32`): there is no saturation, no rounding flag, and no exception
//! output (design `fpu-design-v2` sections 2, 8 and 25).
//!
//! The FPU's wide accumulator is a signed 64-bit Q32.32 value. Every Q16.16
//! product is exact, while accumulation keeps the low 64 bits. Extreme legal
//! operands can therefore wrap the ACC (two `i32::MIN` squares already reach
//! `2^63`); `DOTSTORE` then takes bits `[47:16]` from that wrapped sum.
//!
//! The special-function reference arithmetic lives in [`super::fpu_lut`]; this
//! module re-exports it so the simulator has one place to reach the whole
//! numeric contract. The hardware layer depends on the same reference, so the
//! tables and formulas are never copied.

use std::cmp::Ordering;

pub use super::fpu_lut::{rcp_q16, rsqrt_q16, sincos_q16};

/// Fractional bits of an architectural F register.
pub const FIX16_FRACTION_BITS: u32 = 16;
/// Raw Q16.16 encoding of `1.0`.
pub const FIX16_ONE: i32 = 1 << FIX16_FRACTION_BITS;
/// One half unit in the last place, the round-half-up bias (`floor(x + 0.5)`).
pub const FIX16_HALF: i32 = 1 << (FIX16_FRACTION_BITS - 1);

/// Number of architectural F registers (`F0..F63`).
pub const FPU_REGISTER_COUNT: usize = 64;
/// Width of the wide dot-product accumulator (`Q32.32`).
pub const FPU_ACC_BITS: u32 = 64;

/// Wrapping Q16.16 addition.
pub fn fix16_add(a: i32, b: i32) -> i32 {
    a.wrapping_add(b)
}

/// Wrapping Q16.16 subtraction.
pub fn fix16_sub(a: i32, b: i32) -> i32 {
    a.wrapping_sub(b)
}

/// Wrapping Q16.16 negation; `FIX16_MIN` maps to itself.
pub fn fix16_neg(a: i32) -> i32 {
    a.wrapping_neg()
}

/// Wrapping Q16.16 absolute value; `FIX16_MIN` maps to itself, exactly as the
/// scalar ALU's `0 - a` does.
pub fn fix16_abs(a: i32) -> i32 {
    a.wrapping_abs()
}

/// Q16.16 multiply: the full signed 64-bit product narrowed at bit 16, which is
/// the RTL's `product[47:16]` write port.
pub fn fix16_mul(a: i32, b: i32) -> i32 {
    ((i64::from(a) * i64::from(b)) >> FIX16_FRACTION_BITS) as i32
}

/// Clears the 16 fractional bits, i.e. rounds toward negative infinity.
pub fn fix16_floor(a: i32) -> i32 {
    a & !0xffff
}

/// Mathematical ceiling: the floor plus one when a fraction remains.
pub fn fix16_ceil(a: i32) -> i32 {
    if a & 0xffff == 0 {
        a
    } else {
        fix16_floor(a).wrapping_add(FIX16_ONE)
    }
}

/// Round-half-up (ties toward `+infinity`): `floor(x + 0x8000)`.
pub fn fix16_round(a: i32) -> i32 {
    a.wrapping_add(FIX16_HALF) & !0xffff
}

/// Truncation toward zero.
pub fn fix16_trunc(a: i32) -> i32 {
    if a < 0 {
        fix16_ceil(a)
    } else {
        fix16_floor(a)
    }
}

/// Signed Q16.16 ordering.
pub fn fix16_compare(a: i32, b: i32) -> Ordering {
    a.cmp(&b)
}

/// Adds one exact Q16.16 product to the wide accumulator, keeping the low 64
/// bits (`Q32.32` wrap), exactly as the RTL accumulate does.
pub fn fix16_accumulate_product(acc: i64, a: i32, b: i32) -> i64 {
    acc.wrapping_add(i64::from(a).wrapping_mul(i64::from(b)))
}

/// Narrows the wide accumulator to Q16.16, taking `ACC[47:16]` as the RTL
/// `DOTSTORE` write port does.
pub fn fix16_from_acc(acc: i64) -> i32 {
    (acc >> FIX16_FRACTION_BITS) as i32
}

/// Sign-extends a 16-bit integer into a Q16.16 value (`I16TOF`).
pub fn fix16_from_i16(value: i16) -> i32 {
    i32::from(value) << FIX16_FRACTION_BITS
}

/// Converts a Q16.16 value to a 16-bit integer by truncating toward zero
/// (`FTOI16`); the result wraps on overflow.
pub fn fix16_to_i16(value: i32) -> i16 {
    (fix16_trunc(value) >> FIX16_FRACTION_BITS) as i16
}

/// A `vec3` of Q16.16 components, the operand shape of the prescale library
/// contract. On the target each component occupies one consecutive F register.
pub type FpuVec3 = [i32; 3];

/// Prescale exponent frozen by C0: `k = max(0, bit_length(max|component|) - 22)`
/// on the **raw** Q16.16 magnitude.
///
/// The rule guarantees `|component >> k| < 2^22` in raw units (a real value
/// below 64), so every scaled square is below `(2^22)^2 = 2^44` and a
/// three-component sum stays below `3 * 2^44 < 2^45.6`. Narrowing that sum once
/// at bit 16 stays below `2^29.6`, well inside `i32`. A vector whose largest
/// raw component is below `2^22` (real value below 64) is not scaled at all
/// (`k = 0`), so ordinary game coordinates keep their full 16 fractional bits;
/// only genuinely large coordinates lose `k` low bits to arithmetic shifts.
pub fn prescale_shift_for_max_abs(max_abs: u64) -> u32 {
    (64 - max_abs.leading_zeros()).saturating_sub(22)
}

/// Prescale exponent for the approximate **ordinary-distance** comparison
/// ([`v3_distance_gt`]): one extra bit of headroom over
/// [`prescale_shift_for_max_abs`], because both input vectors are shifted before
/// the subtraction and their difference can reach twice the largest input
/// magnitude. `k = max(0, bit_length(max|input component|) - 21)` keeps
/// `|a_i >> k| < 2^21`, so `|(a_i >> k) - (b_i >> k)| < 2^22`, every scaled
/// square stays below `2^44`, and the narrowed three-component sum stays well
/// inside `i32`. The plain [`prescale_shift_for_max_abs`] rule would only bound
/// the shifted inputs, not their difference.
pub fn prescale_shift_for_input_difference(max_abs: u64) -> u32 {
    (64 - max_abs.leading_zeros()).saturating_sub(21)
}

/// Largest absolute component as an unsigned magnitude that handles
/// `i32::MIN` (whose magnitude is `2^31`).
fn max_abs_component(components: &[i32]) -> u64 {
    components
        .iter()
        .map(|value| u64::from(value.unsigned_abs()))
        .max()
        .unwrap_or(0)
}

/// `v3_length2_shift(vec3) -> u16`: the `k` of the shared `2^-k` prescale,
/// `0..=10`. Exposed so a caller can interpret [`v3_length2_scaled`] as the
/// approximate the original metric as `scaled * 2^(2k)`. The component shifts
/// and the final Q16.16 narrowing are lossy when `k > 0`.
pub fn v3_length2_shift(v: FpuVec3) -> u32 {
    prescale_shift_for_max_abs(max_abs_component(&v))
}

/// `v3_length2_scaled(vec3) -> fix16`: the **prescaled** squared length
/// `(sum_i (v_i >> k)^2) >> 16 = |v|^2 * 2^-2k` (truncated toward negative
/// infinity), with `k` from [`prescale_shift_for_max_abs`]. This is not an
/// unscaled length: combine it with [`v3_length2_shift`] for a scaled-back
/// approximation. A vector whose largest component is below 64 (`k = 0`) returns
/// the exact squared length in Q16.16. The single narrowing matches the
/// target's `DOTSTORE` (`ACC[47:16]`), so the result never overflows `i32` and
/// a zero vector yields zero.
pub fn v3_length2_scaled(v: FpuVec3) -> i32 {
    let k = v3_length2_shift(v);
    let mut sum = 0_i64;
    for component in v {
        let scaled = component >> k;
        sum += i64::from(scaled) * i64::from(scaled);
    }
    (sum >> FIX16_FRACTION_BITS) as i32
}

/// `v3_normalize_safe(vec3) -> vec3`: `v / |v|` computed from the same 2^-k
/// scaling, so the squared length never overflows. A zero vector normalizes to
/// a zero vector. The sign of every component is preserved.
pub fn v3_normalize_safe(v: FpuVec3) -> FpuVec3 {
    let k = v3_length2_shift(v);
    let length2 = v3_length2_scaled(v);
    if length2 == 0 {
        return [0; 3];
    }
    // rsqrt(s) = 2^k / |v|, so (v_i >> k) * rsqrt(s) = v_i / |v|.
    let inverse = rsqrt_q16(length2);
    [
        fix16_mul(v[0] >> k, inverse),
        fix16_mul(v[1] >> k, inverse),
        fix16_mul(v[2] >> k, inverse),
    ]
}

/// `v3_distance_gt(vec3, vec3, fix16) -> bool`: an approximate **ordinary**
/// distance comparison `|a - b| > threshold` built from the FPU v2 ISA.
///
/// Both inputs are arithmetic-shifted by one shared `k` derived from the largest
/// absolute **input** component ([`prescale_shift_for_input_difference`]) before
/// the subtraction, so the subtraction cannot overflow. The scaled squared
/// distance `s = sum_i ((a_i >> k) - (b_i >> k))^2 >> 16` is narrowed once, the
/// ordinary distance is approximated as `s * rsqrt(s)` (the target's `RSQRT`
/// plus a scalar `MUL`), and the ordinary Q16.16 `threshold` is shifted by the
/// same `k` before the scalar `CMP`. The shifts, the single `DOTSTORE`
/// narrowing, and the `RSQRT`/`MUL` approximation make the boundary approximate;
/// the predicate is not the exact squared-distance boundary. A negative
/// threshold is always exceeded by the non-negative distance, and equal points
/// never exceed a zero threshold.
pub fn v3_distance_gt(a: FpuVec3, b: FpuVec3, threshold: i32) -> bool {
    let max_abs = a
        .iter()
        .chain(b.iter())
        .map(|value| u64::from(value.unsigned_abs()))
        .max()
        .unwrap_or(0);
    let k = prescale_shift_for_input_difference(max_abs);
    let scaled = |component: i32| component >> k;
    let difference = [
        i64::from(scaled(a[0])) - i64::from(scaled(b[0])),
        i64::from(scaled(a[1])) - i64::from(scaled(b[1])),
        i64::from(scaled(a[2])) - i64::from(scaled(b[2])),
    ];
    let sum = difference.iter().map(|d| d * d).sum::<i64>();
    // `DOTSTORE` narrows `ACC[47:16]`; the headroom keeps this inside `i32`.
    let squared = (sum >> FIX16_FRACTION_BITS) as i32;
    let distance = fix16_mul(squared, rsqrt_q16(squared));
    // The ordinary Q16.16 threshold carries the same `2^-k` scale.
    distance > (threshold >> k)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_wraps_instead_of_saturating() {
        assert_eq!(fix16_add(i32::MAX, 1), i32::MIN);
        assert_eq!(fix16_sub(i32::MIN, 1), i32::MAX);
        assert_eq!(fix16_neg(i32::MIN), i32::MIN);
        assert_eq!(fix16_abs(i32::MIN), i32::MIN);
    }

    #[test]
    fn multiplication_narrows_the_full_product_at_bit_16() {
        // 1.5 * 2.0 = 3.0
        assert_eq!(fix16_mul(FIX16_ONE + 0x8000, 2 * FIX16_ONE), 3 * FIX16_ONE);
        // 1.5 * 1.5 = 2.25
        assert_eq!(fix16_mul(FIX16_ONE + 0x8000, FIX16_ONE + 0x8000), 0x2_4000);
        // The most negative product keeps the RTL `product[47:16]` slice.
        assert_eq!(fix16_mul(i32::MIN, FIX16_ONE), i32::MIN);
    }

    #[test]
    fn rounding_family_matches_the_frozen_rules() {
        assert_eq!(fix16_floor(0x1_8000), 0x1_0000);
        assert_eq!(fix16_floor(-0x1_8000), -0x2_0000);
        assert_eq!(fix16_ceil(0x1_8000), 0x2_0000);
        assert_eq!(fix16_ceil(-0x1_8000), -0x1_0000);
        assert_eq!(fix16_ceil(-0x1_0000), -0x1_0000);
        // Ties round toward +infinity.
        assert_eq!(fix16_round(0x1_8000), 0x2_0000);
        assert_eq!(fix16_round(-0x1_8000), -0x1_0000);
        assert_eq!(fix16_trunc(0x1_8000), 0x1_0000);
        assert_eq!(fix16_trunc(-0x1_8000), -0x1_0000);
    }

    #[test]
    fn accumulator_wraps_at_64_bits_and_narrows_at_bit_16() {
        let mut acc = 0_i64;
        for _ in 0..4 {
            acc = fix16_accumulate_product(acc, i32::MIN, i32::MIN);
        }
        // 4 * 2^62 == 2^64, kept modulo 2^64.
        assert_eq!(acc, 0);
        assert_eq!(fix16_from_acc(1_i64 << 47), 1 << 31);
    }

    #[test]
    fn integer_bridges_are_raw_half_moves_and_truncating_conversions() {
        assert_eq!(fix16_from_i16(-3), -3 * FIX16_ONE);
        assert_eq!(fix16_to_i16(3 * FIX16_ONE + 0x8000), 3);
        assert_eq!(fix16_to_i16(-(3 * FIX16_ONE) - 0x8000), -3);
        assert_eq!(fix16_to_i16(i32::MIN), i16::MIN);
    }

    #[test]
    fn prescale_shift_boundaries_are_frozen() {
        assert_eq!(prescale_shift_for_max_abs(0), 0);
        assert_eq!(prescale_shift_for_max_abs(1), 0);
        assert_eq!(prescale_shift_for_max_abs((1 << 22) - 1), 0);
        // The first bit length that needs scaling.
        assert_eq!(prescale_shift_for_max_abs(1 << 22), 1);
        assert_eq!(prescale_shift_for_max_abs((1 << 23) - 1), 1);
        assert_eq!(prescale_shift_for_max_abs(1 << 23), 2);
        // i32::MIN's magnitude is 2^31 (bit length 32).
        assert_eq!(prescale_shift_for_max_abs(1 << 31), 10);
        // The distance helper keeps one extra bit of headroom for the difference.
        assert_eq!(prescale_shift_for_input_difference(0), 0);
        assert_eq!(prescale_shift_for_input_difference((1 << 21) - 1), 0);
        assert_eq!(prescale_shift_for_input_difference(1 << 21), 1);
        assert_eq!(prescale_shift_for_input_difference(1 << 31), 11);
    }

    /// Higher-precision reference for the frozen prescale algorithm. `i128`
    /// leaves the implementation's `i64` intermediate with no truncation, so
    /// the two must agree bit for bit across the whole input range.
    fn reference_length2_scaled(v: [i32; 3]) -> i32 {
        let max_abs = v
            .iter()
            .map(|value| u64::from(value.unsigned_abs()))
            .max()
            .unwrap();
        let k = prescale_shift_for_max_abs(max_abs);
        let sum = v
            .iter()
            .map(|&component| {
                let scaled = i128::from(component) >> k;
                scaled * scaled
            })
            .sum::<i128>();
        (sum >> FIX16_FRACTION_BITS) as i32
    }

    /// Exact Euclidean distance of two Q16.16 vectors as `f64`, used only to
    /// check the approximate predicate away from its accepted boundary band.
    fn exact_distance(a: [i32; 3], b: [i32; 3]) -> f64 {
        let squared = a
            .iter()
            .zip(b.iter())
            .map(|(&x, &y)| {
                let d = f64::from(x) - f64::from(y);
                d * d
            })
            .sum::<f64>();
        (squared.sqrt()) / 65536.0
    }

    #[test]
    fn length2_scaled_and_normalize_safe_hold_the_zero_and_sign_rules() {
        assert_eq!(v3_length2_shift([0, 0, 0]), 0);
        assert_eq!(v3_length2_scaled([0, 0, 0]), 0);
        assert_eq!(v3_normalize_safe([0, 0, 0]), [0, 0, 0]);
        // 3-4-5 style exact small vector, no scaling, true squared length.
        assert_eq!(v3_length2_shift([3 << 16, 4 << 16, 0]), 0);
        assert_eq!(v3_length2_scaled([3 << 16, 4 << 16, 0]), 25 << 16);
        let n = v3_normalize_safe([3 << 16, 4 << 16, 0]);
        assert_eq!(
            n,
            [
                fix16_mul(3 << 16, rsqrt_q16(25 << 16)),
                fix16_mul(4 << 16, rsqrt_q16(25 << 16)),
                0
            ]
        );
        // The normalized magnitude is ~1.0 and signs are preserved.
        assert!(n[0] > 0 && n[1] > 0);
        let neg = v3_normalize_safe([-(3 << 16), 4 << 16, 0]);
        assert!(neg[0] < 0 && neg[1] > 0);
    }

    #[test]
    fn length2_scaled_matches_the_high_precision_reference_at_every_boundary() {
        let mut cases: Vec<[i32; 3]> = vec![];
        // Every prescale boundary 2^b and its neighbours.
        for bits in 0..=31u32 {
            let center = 1_i32.wrapping_shl(bits);
            for delta in [-1i32, 0, 1] {
                let value = center.wrapping_add(delta);
                cases.push([value, 0, 0]);
                cases.push([value, value, value]);
                cases.push([value.wrapping_neg(), value.wrapping_neg() / 3, value / 2]);
            }
        }
        // Extremes and mixed-sign combinations.
        for value in [0, 1, -1, i32::MIN, i32::MIN + 1, i32::MAX, i32::MAX - 1] {
            cases.push([value, value, value]);
            cases.push([value, i32::MIN, i32::MAX]);
            cases.push([i32::MAX, value, i32::MIN]);
        }
        for v in cases {
            assert_eq!(
                v3_length2_scaled(v),
                reference_length2_scaled(v),
                "components {v:?}"
            );
            // The prescaled result always fits the Q16.16 write port.
            let scaled = v3_length2_scaled(v);
            assert!((-0x4000_0000..=0x4000_0000).contains(&scaled), "{v:?}");
        }
    }

    #[test]
    fn length2_scaled_is_the_true_length_when_no_prescale_is_needed() {
        // Below the first boundary (max raw component < 2^22, real value < 64)
        // the function returns the exact Q16.16 squared length.
        for v in [
            [0, 0, 0],
            [3 << 16, 4 << 16, 0],
            [-(1 << 21), (1 << 21) - 1, 0],
            [1 << 15, -(1 << 15), 1 << 14],
        ] {
            assert_eq!(v3_length2_shift(v), 0, "{v:?}");
            let exact = v
                .iter()
                .map(|&c| i128::from(c) * i128::from(c))
                .sum::<i128>()
                >> 16;
            assert_eq!(i128::from(v3_length2_scaled(v)), exact, "{v:?}");
        }
    }

    #[test]
    fn distance_gt_compares_the_ordinary_distance() {
        // 3-4-5: ordinary distance 5.0. The boundary is approximate (shift,
        // DOTSTORE, RSQRT and MUL), so the test keeps a margin around it.
        let a = [0, 0, 0];
        let b = [3 << 16, 4 << 16, 0];
        assert!(v3_distance_gt(a, b, 4 << 16));
        assert!(v3_distance_gt(a, b, (9 << 15) - 1)); // 4.5 - 1 ulp
        assert!(!v3_distance_gt(a, b, 11 << 15)); // 5.5
        assert!(!v3_distance_gt(a, b, 6 << 16));
    }

    #[test]
    fn distance_gt_handles_negative_thresholds_and_zero_vectors() {
        let a = [0, 0, 0];
        let b = [3 << 16, 4 << 16, 0];
        // A negative threshold is always exceeded by the non-negative distance.
        assert!(v3_distance_gt(a, b, -1));
        assert!(v3_distance_gt(a, a, -1));
        // Equal points never exceed a zero (or positive) threshold.
        assert!(!v3_distance_gt(a, a, 0));
        assert!(!v3_distance_gt(a, a, 1));
        // Two zero vectors stay zero.
        assert!(!v3_distance_gt([0, 0, 0], [0, 0, 0], 0));
        // This small distance is below the largest Q16.16 threshold.
        assert!(!v3_distance_gt(a, b, i32::MAX));
    }

    #[test]
    fn distance_gt_prescale_prevents_subtraction_overflow_at_the_extremes() {
        // `a - b` in raw `i32` would overflow; shifting both inputs first keeps
        // the difference, the squared sum, and the narrowed `DOTSTORE` in range.
        let a = [i32::MAX, i32::MAX, i32::MAX];
        let b = [i32::MIN, i32::MIN, i32::MIN];
        assert!(v3_distance_gt(a, b, 0));
        // The extreme distance (~113511.0) still exceeds the largest threshold.
        assert!(v3_distance_gt(a, b, i32::MAX));
        // Identical extreme points have distance zero.
        assert!(!v3_distance_gt(a, a, 0));
        assert!(!v3_distance_gt(a, a, i32::MAX));
        assert!(v3_distance_gt(a, a, -1));
    }

    #[test]
    fn distance_gt_matches_the_exact_distance_away_from_the_boundary_band() {
        let mut cases: Vec<([i32; 3], [i32; 3])> = vec![];
        for bits in 0..=31u32 {
            let magnitude = 1_i32.wrapping_shl(bits);
            for delta in [-1i32, 0, 1] {
                let d = magnitude.wrapping_add(delta);
                cases.push(([0, 0, 0], [d, 0, 0]));
                cases.push(([0, 0, 0], [d, d, d]));
                cases.push(([d, d.wrapping_neg(), 0], [0, 0, d]));
            }
        }
        for a in [i32::MIN, i32::MIN + 1, i32::MAX, 0] {
            for b in [i32::MIN, i32::MAX, 0] {
                cases.push(([a, b, a], [b, a, b]));
            }
        }
        for (a, b) in cases {
            let exact = exact_distance(a, b);
            for threshold in [
                i32::MIN,
                -1,
                0,
                1 << 10,
                (9 << 15) - 1,
                9 << 15,
                (9 << 15) + 1,
                11 << 15,
                i32::MAX - 1,
                i32::MAX,
            ] {
                let t = f64::from(threshold) / 65536.0;
                // Skip the accepted approximation band around the true distance.
                if (exact - t).abs() <= exact * 0.02 + 0.01 {
                    continue;
                }
                assert_eq!(
                    v3_distance_gt(a, b, threshold),
                    exact > t,
                    "a={a:?} b={b:?} threshold={threshold} exact={exact}"
                );
            }
        }
    }
}
