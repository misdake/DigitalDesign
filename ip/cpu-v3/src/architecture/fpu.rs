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

/// `v3_distance2_gt(vec3, vec3, fix16) -> bool`: `|a - b|^2 > threshold`, with
/// both sides scaled by the same `2^-2k` derived from the difference's largest
/// component.
///
/// The exact predicate is the scaled one: it compares
/// `sum_i ((a_i - b_i) >> k)^2` against `threshold * 2^(16 - 2k)` in `i128`, so
/// the comparison itself never overflows and a negative threshold is always
/// exceeded by the non-negative squared distance. The `>> k` truncation is
/// shared by both sides, so for a difference whose largest raw component is
/// below `2^22` (`k = 0`) it is exactly `|a - b|^2 > threshold`.
pub fn v3_distance2_gt(a: FpuVec3, b: FpuVec3, threshold: i32) -> bool {
    let difference = [
        i128::from(a[0]) - i128::from(b[0]),
        i128::from(a[1]) - i128::from(b[1]),
        i128::from(a[2]) - i128::from(b[2]),
    ];
    let max_abs = difference
        .iter()
        .map(|value| value.unsigned_abs())
        .max()
        .unwrap_or(0);
    // `max_abs` fits 33 bits, so the u64 shift helper takes it directly.
    let k = prescale_shift_for_max_abs(max_abs as u64);
    let mut sum = 0_i128;
    for component in difference {
        let scaled = component >> k;
        sum += scaled * scaled;
    }
    // Q16.16 threshold -> raw Q32.32 scale, then the same 2^-2k shift.
    let scaled_threshold = (i128::from(threshold) << 16) >> (2 * k);
    sum > scaled_threshold
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

    /// Higher-precision reference for the frozen distance predicate.
    fn reference_distance2_gt(a: [i32; 3], b: [i32; 3], threshold: i32) -> bool {
        let difference: [i128; 3] = [
            i128::from(a[0]) - i128::from(b[0]),
            i128::from(a[1]) - i128::from(b[1]),
            i128::from(a[2]) - i128::from(b[2]),
        ];
        let max_abs = difference
            .iter()
            .map(|value| value.unsigned_abs())
            .max()
            .unwrap();
        let k = prescale_shift_for_max_abs(max_abs as u64);
        let sum = difference
            .iter()
            .map(|&component| {
                let scaled = component >> k;
                scaled * scaled
            })
            .sum::<i128>();
        sum > (i128::from(threshold) << 16) >> (2 * k)
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
    fn distance2_gt_matches_the_high_precision_reference_at_every_boundary() {
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
            // Probe a threshold just below, at, and just above the true scaled
            // squared distance, so the predicate's boundary is exercised.
            for threshold in [
                i32::MIN,
                -1,
                0,
                1 << 10,
                (25 << 16) - 1,
                25 << 16,
                (25 << 16) + 1,
                i32::MAX - 1,
                i32::MAX,
            ] {
                assert_eq!(
                    v3_distance2_gt(a, b, threshold),
                    reference_distance2_gt(a, b, threshold),
                    "a={a:?} b={b:?} threshold={threshold}"
                );
            }
        }
    }

    #[test]
    fn distance2_gt_scales_both_sides_and_handles_negative_thresholds() {
        // 3-4-5: squared distance 25.0.
        let a = [0, 0, 0];
        let b = [3 << 16, 4 << 16, 0];
        assert!(v3_distance2_gt(a, b, 24 << 16));
        assert!(!v3_distance2_gt(a, b, 25 << 16));
        assert!(!v3_distance2_gt(a, b, 26 << 16));
        // A negative threshold is always exceeded by a non-negative distance.
        assert!(v3_distance2_gt(a, b, -1));
        // Equal points never exceed a zero threshold.
        assert!(!v3_distance2_gt(a, a, 0));
        // Large coordinates: |a - b| = 2^20 in one component; both sides scale.
        let far = [1 << 20, 0, 0];
        assert!(v3_distance2_gt(a, far, 0));
        assert!(!v3_distance2_gt(a, far, i32::MAX));
    }
}
