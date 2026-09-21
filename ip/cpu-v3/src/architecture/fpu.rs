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
pub const FIX32_FRACTION_BITS: u32 = 16;
/// Raw Q16.16 encoding of `1.0`.
pub const FIX32_ONE: i32 = 1 << FIX32_FRACTION_BITS;
/// One half unit in the last place, the round-half-up bias (`floor(x + 0.5)`).
pub const FIX32_HALF: i32 = 1 << (FIX32_FRACTION_BITS - 1);

/// Number of architectural F registers (`F0..F63`).
pub const FPU_REGISTER_COUNT: usize = 64;
/// Width of the wide dot-product accumulator (`Q32.32`).
pub const FPU_ACC_BITS: u32 = 64;

/// Wrapping Q16.16 addition.
pub fn fix32_add(a: i32, b: i32) -> i32 {
    a.wrapping_add(b)
}

/// Wrapping Q16.16 subtraction.
pub fn fix32_sub(a: i32, b: i32) -> i32 {
    a.wrapping_sub(b)
}

/// Wrapping Q16.16 negation; `i32::MIN` maps to itself.
pub fn fix32_neg(a: i32) -> i32 {
    a.wrapping_neg()
}

/// Wrapping Q16.16 absolute value; `i32::MIN` maps to itself, exactly as the
/// scalar ALU's `0 - a` does.
pub fn fix32_abs(a: i32) -> i32 {
    a.wrapping_abs()
}

/// Q16.16 multiply: the full signed 64-bit product narrowed at bit 16, which is
/// the RTL's `product[47:16]` write port.
pub fn fix32_mul(a: i32, b: i32) -> i32 {
    ((i64::from(a) * i64::from(b)) >> FIX32_FRACTION_BITS) as i32
}

/// Clears the 16 fractional bits, i.e. rounds toward negative infinity.
pub fn fix32_floor(a: i32) -> i32 {
    a & !0xffff
}

/// Mathematical ceiling: the floor plus one when a fraction remains.
pub fn fix32_ceil(a: i32) -> i32 {
    if a & 0xffff == 0 {
        a
    } else {
        fix32_floor(a).wrapping_add(FIX32_ONE)
    }
}

/// Round-half-up (ties toward `+infinity`): `floor(x + 0x8000)`.
pub fn fix32_round(a: i32) -> i32 {
    a.wrapping_add(FIX32_HALF) & !0xffff
}

/// Truncation toward zero.
pub fn fix32_trunc(a: i32) -> i32 {
    if a < 0 {
        fix32_ceil(a)
    } else {
        fix32_floor(a)
    }
}

/// Signed Q16.16 ordering.
pub fn fix32_compare(a: i32, b: i32) -> Ordering {
    a.cmp(&b)
}

/// Adds one exact Q16.16 product to the wide accumulator, keeping the low 64
/// bits (`Q32.32` wrap), exactly as the RTL accumulate does.
pub fn fix32_accumulate_product(acc: i64, a: i32, b: i32) -> i64 {
    acc.wrapping_add(i64::from(a).wrapping_mul(i64::from(b)))
}

/// Narrows the wide accumulator to Q16.16, taking `ACC[47:16]` as the RTL
/// `DOTSTORE` write port does.
pub fn fix32_from_acc(acc: i64) -> i32 {
    (acc >> FIX32_FRACTION_BITS) as i32
}

/// Sign-extends a 16-bit integer into a Q16.16 value (`I16TOF`).
pub fn fix32_from_i16(value: i16) -> i32 {
    i32::from(value) << FIX32_FRACTION_BITS
}

/// Converts a Q16.16 value to a 16-bit integer by truncating toward zero
/// (`FTOI16`); the result wraps on overflow.
pub fn fix32_to_i16(value: i32) -> i16 {
    (fix32_trunc(value) >> FIX32_FRACTION_BITS) as i16
}

/// A `vec3` of Q16.16 components, the operand shape of the v3 geometry
/// contract. On the target each component occupies one consecutive F register.
pub type FpuVec3 = [i32; 3];

/// Inclusive Q16.16 component bound of the checked length/normalize contract,
/// `[-104, +104]`: three components each at the bound give a squared sum of
/// `3 * 104^2 = 32448`, so a single `DOTSTORE` narrowing stays inside `i32`.
pub const V3_GEOMETRY_COMPONENT_LIMIT: i32 = 104;

/// Raw Q16.16 encoding of [`V3_GEOMETRY_COMPONENT_LIMIT`].
pub const V3_GEOMETRY_COMPONENT_LIMIT_Q16: i32 = V3_GEOMETRY_COMPONENT_LIMIT << FIX32_FRACTION_BITS;

/// Inclusive lower input-component bound of the checked distance contract,
/// `-16384` Q16.16: together with the upper bound this keeps a raw subtraction
/// inside `i32`.
pub const V3_GEOMETRY_DISTANCE_INPUT_MIN_Q16: i32 = -16384 << FIX32_FRACTION_BITS;

/// Inclusive upper input-component bound of the checked distance contract,
/// `+16383` Q16.16; see [`V3_GEOMETRY_DISTANCE_INPUT_MIN_Q16`].
pub const V3_GEOMETRY_DISTANCE_INPUT_MAX_Q16: i32 = 16383 << FIX32_FRACTION_BITS;

/// Fixed nonzero halt signal of [`v3_length2_checked`] on a violated range.
pub const V3_LENGTH2_CHECKED_HALT: u16 = 1;
/// Fixed nonzero halt signal of [`v3_normalize_checked`] on a violated range.
pub const V3_NORMALIZE_CHECKED_HALT: u16 = 2;
/// Fixed nonzero halt signal of [`v3_distance_gt_checked`] on a violated input
/// component range.
pub const V3_DISTANCE_GT_CHECKED_INPUT_HALT: u16 = 3;
/// Fixed nonzero halt signal of [`v3_distance_gt_checked`] on a violated
/// difference component range.
pub const V3_DISTANCE_GT_CHECKED_DIFFERENCE_HALT: u16 = 4;

fn component_in_range(component: i32, min: i32, max: i32) -> bool {
    (min..=max).contains(&component)
}

fn vec3_in_range(v: FpuVec3, min: i32, max: i32) -> bool {
    v.iter()
        .all(|&component| component_in_range(component, min, max))
}

/// `v3_length2(vec3) -> fix32`: the squared length computed by one `DOTSTORE`
/// of `v` with itself. Under the small-range contract (every component in
/// `[-104, +104]` Q16.16) the narrowed accumulator stays inside `i32`; outside
/// it the wide accumulator wraps exactly as the hardware does, so the result is
/// the wrapped `ACC[47:16]` rather than a clamped value.
pub fn v3_length2(v: FpuVec3) -> i32 {
    let mut acc = 0_i64;
    for component in v {
        acc = fix32_accumulate_product(acc, component, component);
    }
    fix32_from_acc(acc)
}

/// `v3_normalize(vec3) -> vec3`: `DOTSTORE`, `RSQRT`, then `VMULS` of the
/// vector by the reciprocal square root. `RSQRT(0) == 0`, so a zero vector
/// stays zero, and the sign of every component is preserved. Under the
/// small-range contract the squared length never overflows `i32`.
pub fn v3_normalize(v: FpuVec3) -> FpuVec3 {
    let inverse = rsqrt_q16(v3_length2(v));
    [
        fix32_mul(v[0], inverse),
        fix32_mul(v[1], inverse),
        fix32_mul(v[2], inverse),
    ]
}

/// `v3_distance_gt(vec3, vec3, fix32) -> bool`: `VSUB`, one `DOTSTORE` of the
/// difference, then the ordinary distance approximated as `s * RSQRT(s)` and
/// compared with the ordinary Q16.16 `threshold` through the scalar `CMP`.
/// `RSQRT(0) == 0`, so equal points have distance zero. The `RSQRT`/`MUL`
/// approximation makes the boundary approximate rather than exact.
pub fn v3_distance_gt(a: FpuVec3, b: FpuVec3, threshold: i32) -> bool {
    let difference = [
        fix32_sub(a[0], b[0]),
        fix32_sub(a[1], b[1]),
        fix32_sub(a[2], b[2]),
    ];
    let squared = v3_length2(difference);
    fix32_mul(squared, rsqrt_q16(squared)) > threshold
}

/// [`v3_length2`] with its small-range precondition checked: every component
/// must be inclusively within `[-104, +104]` Q16.16 (raw
/// [`V3_GEOMETRY_COMPONENT_LIMIT_Q16`]). A violation returns
/// [`V3_LENGTH2_CHECKED_HALT`] instead of a silently wrapped result.
pub fn v3_length2_checked(v: FpuVec3) -> Result<i32, u16> {
    let limit = V3_GEOMETRY_COMPONENT_LIMIT_Q16;
    if !vec3_in_range(v, -limit, limit) {
        return Err(V3_LENGTH2_CHECKED_HALT);
    }
    Ok(v3_length2(v))
}

/// [`v3_normalize`] with the same small-range precondition as
/// [`v3_length2_checked`]. A violation returns
/// [`V3_NORMALIZE_CHECKED_HALT`] instead of normalizing a wrapped length.
pub fn v3_normalize_checked(v: FpuVec3) -> Result<FpuVec3, u16> {
    let limit = V3_GEOMETRY_COMPONENT_LIMIT_Q16;
    if !vec3_in_range(v, -limit, limit) {
        return Err(V3_NORMALIZE_CHECKED_HALT);
    }
    Ok(v3_normalize(v))
}

/// [`v3_distance_gt`] with both numeric preconditions checked: every input
/// component must be inclusively within `[-16384, +16383]` Q16.16 (raw
/// [`V3_GEOMETRY_DISTANCE_INPUT_MIN_Q16`]/[`V3_GEOMETRY_DISTANCE_INPUT_MAX_Q16`])
/// so the 32-bit subtraction cannot overflow, and every component of the
/// difference must then be inclusively within `[-104, +104]` before the
/// `DOTSTORE`. The two violations return
/// [`V3_DISTANCE_GT_CHECKED_INPUT_HALT`] and
/// [`V3_DISTANCE_GT_CHECKED_DIFFERENCE_HALT`] respectively.
pub fn v3_distance_gt_checked(a: FpuVec3, b: FpuVec3, threshold: i32) -> Result<bool, u16> {
    if !vec3_in_range(
        a,
        V3_GEOMETRY_DISTANCE_INPUT_MIN_Q16,
        V3_GEOMETRY_DISTANCE_INPUT_MAX_Q16,
    ) || !vec3_in_range(
        b,
        V3_GEOMETRY_DISTANCE_INPUT_MIN_Q16,
        V3_GEOMETRY_DISTANCE_INPUT_MAX_Q16,
    ) {
        return Err(V3_DISTANCE_GT_CHECKED_INPUT_HALT);
    }
    let difference = [
        fix32_sub(a[0], b[0]),
        fix32_sub(a[1], b[1]),
        fix32_sub(a[2], b[2]),
    ];
    let limit = V3_GEOMETRY_COMPONENT_LIMIT_Q16;
    if !vec3_in_range(difference, -limit, limit) {
        return Err(V3_DISTANCE_GT_CHECKED_DIFFERENCE_HALT);
    }
    Ok(v3_distance_gt(a, b, threshold))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_wraps_instead_of_saturating() {
        assert_eq!(fix32_add(i32::MAX, 1), i32::MIN);
        assert_eq!(fix32_sub(i32::MIN, 1), i32::MAX);
        assert_eq!(fix32_neg(i32::MIN), i32::MIN);
        assert_eq!(fix32_abs(i32::MIN), i32::MIN);
    }

    #[test]
    fn multiplication_narrows_the_full_product_at_bit_16() {
        // 1.5 * 2.0 = 3.0
        assert_eq!(fix32_mul(FIX32_ONE + 0x8000, 2 * FIX32_ONE), 3 * FIX32_ONE);
        // 1.5 * 1.5 = 2.25
        assert_eq!(fix32_mul(FIX32_ONE + 0x8000, FIX32_ONE + 0x8000), 0x2_4000);
        // The most negative product keeps the RTL `product[47:16]` slice.
        assert_eq!(fix32_mul(i32::MIN, FIX32_ONE), i32::MIN);
    }

    #[test]
    fn rounding_family_matches_the_frozen_rules() {
        assert_eq!(fix32_floor(0x1_8000), 0x1_0000);
        assert_eq!(fix32_floor(-0x1_8000), -0x2_0000);
        assert_eq!(fix32_ceil(0x1_8000), 0x2_0000);
        assert_eq!(fix32_ceil(-0x1_8000), -0x1_0000);
        assert_eq!(fix32_ceil(-0x1_0000), -0x1_0000);
        // Ties round toward +infinity.
        assert_eq!(fix32_round(0x1_8000), 0x2_0000);
        assert_eq!(fix32_round(-0x1_8000), -0x1_0000);
        assert_eq!(fix32_trunc(0x1_8000), 0x1_0000);
        assert_eq!(fix32_trunc(-0x1_8000), -0x1_0000);
    }

    #[test]
    fn accumulator_wraps_at_64_bits_and_narrows_at_bit_16() {
        let mut acc = 0_i64;
        for _ in 0..4 {
            acc = fix32_accumulate_product(acc, i32::MIN, i32::MIN);
        }
        // 4 * 2^62 == 2^64, kept modulo 2^64.
        assert_eq!(acc, 0);
        assert_eq!(fix32_from_acc(1_i64 << 47), 1 << 31);
    }

    #[test]
    fn integer_bridges_are_raw_half_moves_and_truncating_conversions() {
        assert_eq!(fix32_from_i16(-3), -3 * FIX32_ONE);
        assert_eq!(fix32_to_i16(3 * FIX32_ONE + 0x8000), 3);
        assert_eq!(fix32_to_i16(-(3 * FIX32_ONE) - 0x8000), -3);
        assert_eq!(fix32_to_i16(i32::MIN), i16::MIN);
    }

    #[test]
    fn length2_is_one_dot_store_of_the_vector_with_itself() {
        assert_eq!(v3_length2([0, 0, 0]), 0);
        assert_eq!(v3_length2([3 << 16, 4 << 16, 0]), 25 << 16);
        assert_eq!(v3_length2([-(3 << 16), 4 << 16, 0]), 25 << 16);
        // At the checked bound the narrowed accumulator is still inside i32.
        let at_bound = [104 << 16, 104 << 16, 104 << 16];
        let exact =
            ((3_i64 * i64::from(104 << 16) * i64::from(104 << 16)) >> FIX32_FRACTION_BITS) as i32;
        assert_eq!(v3_length2(at_bound), exact);
        assert!(v3_length2(at_bound) > 0);
    }

    #[test]
    fn normalize_uses_rsqrt_and_keeps_zero_and_signs() {
        assert_eq!(v3_normalize([0, 0, 0]), [0, 0, 0]);
        let v = [3 << 16, 4 << 16, 0];
        let inverse = rsqrt_q16(25 << 16);
        assert_eq!(
            v3_normalize(v),
            [
                fix32_mul(v[0], inverse),
                fix32_mul(v[1], inverse),
                fix32_mul(v[2], inverse)
            ]
        );
        // Signs are preserved and the magnitude is ~1.0.
        let n = v3_normalize(v);
        assert!(n[0] > 0 && n[1] > 0);
        let neg = v3_normalize([-(3 << 16), 4 << 16, 0]);
        assert!(neg[0] < 0 && neg[1] > 0);
    }

    #[test]
    fn distance_gt_compares_the_approximate_ordinary_distance() {
        // 3-4-5: the ordinary distance is ~5.0, but the `RSQRT`/`MUL`
        // approximation makes the boundary approximate, so the test keeps a
        // margin around it.
        let a = [0, 0, 0];
        let b = [3 << 16, 4 << 16, 0];
        assert!(v3_distance_gt(a, b, 4 << 16));
        assert!(!v3_distance_gt(a, b, 6 << 16));
        // A negative threshold is always exceeded; equal points never exceed a
        // non-negative threshold.
        assert!(v3_distance_gt(a, a, -1));
        assert!(!v3_distance_gt(a, a, 0));
        assert!(!v3_distance_gt(a, a, 1));
        assert!(!v3_distance_gt([0, 0, 0], [0, 0, 0], 0));
    }

    #[test]
    fn checked_helpers_match_unchecked_on_valid_input() {
        for v in [
            [0, 0, 0],
            [3 << 16, 4 << 16, 0],
            [104 << 16, -104 << 16, 0],
            [1 << 15, -(1 << 15), 1 << 14],
        ] {
            assert_eq!(v3_length2_checked(v).unwrap(), v3_length2(v), "{v:?}");
            assert_eq!(v3_normalize_checked(v).unwrap(), v3_normalize(v), "{v:?}");
        }
        let a = [100 << 16, -100 << 16, 0];
        let b = [0, 0, 0];
        for threshold in [-1, 0, 100 << 16, i32::MAX] {
            assert_eq!(
                v3_distance_gt_checked(a, b, threshold).unwrap(),
                v3_distance_gt(a, b, threshold),
                "threshold={threshold}"
            );
        }
    }

    #[test]
    fn checked_helpers_halt_with_distinct_signals_on_violations() {
        // One component outside [-104, +104].
        let over = [105 << 16, 0, 0];
        assert_eq!(v3_length2_checked(over), Err(V3_LENGTH2_CHECKED_HALT));
        assert_eq!(v3_normalize_checked(over), Err(V3_NORMALIZE_CHECKED_HALT));
        // An input component outside [-16384, +16383].
        let big = [16384 << 16, 0, 0];
        assert_eq!(
            v3_distance_gt_checked(big, [0, 0, 0], 0),
            Err(V3_DISTANCE_GT_CHECKED_INPUT_HALT)
        );
        // In-range inputs whose difference leaves [-104, +104].
        let a = [100 << 16, 0, 0];
        let b = [-(100 << 16), 0, 0];
        assert_eq!(
            v3_distance_gt_checked(a, b, 0),
            Err(V3_DISTANCE_GT_CHECKED_DIFFERENCE_HALT)
        );
        // The signals are distinct.
        let signals = [
            V3_LENGTH2_CHECKED_HALT,
            V3_NORMALIZE_CHECKED_HALT,
            V3_DISTANCE_GT_CHECKED_INPUT_HALT,
            V3_DISTANCE_GT_CHECKED_DIFFERENCE_HALT,
        ];
        for (i, signal) in signals.iter().enumerate() {
            assert_ne!(*signal, 0);
            assert!(
                signals
                    .iter()
                    .enumerate()
                    .all(|(j, other)| i == j || other != signal),
                "signal {signal} is not distinct"
            );
        }
    }
}
