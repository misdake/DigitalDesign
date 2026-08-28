//! Bit-exact architectural arithmetic for the CpuV3 revision 0.7 fix16 FPU.

use std::cmp::Ordering;

pub type Fix16Raw = i16;
pub type FpuVector = [Fix16Raw; 4];

pub const FIX16_FRACTION_BITS: u32 = 8;
pub const FIX16_ONE: Fix16Raw = 1 << FIX16_FRACTION_BITS;
pub const FPU_ACC_BITS: u32 = 40;
pub const FPU_ACC_MIN: i64 = -(1_i64 << (FPU_ACC_BITS - 1));
pub const FPU_ACC_MAX: i64 = (1_i64 << (FPU_ACC_BITS - 1)) - 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FpuDomainError {
    ReciprocalZero,
    ReciprocalSqrtNonPositive,
}

pub fn fix16_saturate(value: i64) -> Fix16Raw {
    value.clamp(i64::from(i16::MIN), i64::from(i16::MAX)) as i16
}

pub fn acc_saturate(value: i128) -> i64 {
    value.clamp(i128::from(FPU_ACC_MIN), i128::from(FPU_ACC_MAX)) as i64
}

/// Divides by `2^shift`, rounding to nearest with ties to an even integer.
pub fn round_shift_ties_even(value: i64, shift: u32) -> i64 {
    debug_assert!(shift > 0 && shift < 63);
    let negative = value < 0;
    let magnitude = i128::from(value).abs();
    let divisor = 1_i128 << shift;
    let mut quotient = magnitude >> shift;
    let remainder = magnitude & (divisor - 1);
    let half = divisor >> 1;
    if remainder > half || (remainder == half && quotient & 1 != 0) {
        quotient += 1;
    }
    let rounded = if negative { -quotient } else { quotient };
    rounded as i64
}

pub fn fix16_add(a: Fix16Raw, b: Fix16Raw) -> Fix16Raw {
    fix16_saturate(i64::from(a) + i64::from(b))
}

pub fn fix16_sub(a: Fix16Raw, b: Fix16Raw) -> Fix16Raw {
    fix16_saturate(i64::from(a) - i64::from(b))
}

pub fn fix16_mul(a: Fix16Raw, b: Fix16Raw) -> Fix16Raw {
    fix16_saturate(round_shift_ties_even(
        i64::from(a) * i64::from(b),
        FIX16_FRACTION_BITS,
    ))
}

pub fn fix16_accumulate_product(acc: i64, a: Fix16Raw, b: Fix16Raw) -> i64 {
    acc_saturate(i128::from(acc) + i128::from(a) * i128::from(b))
}

pub fn fix16_from_acc(acc: i64) -> Fix16Raw {
    fix16_saturate(round_shift_ties_even(acc, FIX16_FRACTION_BITS))
}

pub fn fix16_compare(a: Fix16Raw, b: Fix16Raw) -> Ordering {
    a.cmp(&b)
}

pub fn fix16_reciprocal(value: Fix16Raw) -> Result<Fix16Raw, FpuDomainError> {
    if value == 0 {
        return Err(FpuDomainError::ReciprocalZero);
    }
    Ok(quantize_f64(1.0 / (f64::from(value) / 256.0)))
}

pub fn fix16_reciprocal_sqrt(value: Fix16Raw) -> Result<Fix16Raw, FpuDomainError> {
    if value <= 0 {
        return Err(FpuDomainError::ReciprocalSqrtNonPositive);
    }
    Ok(quantize_f64(1.0 / (f64::from(value) / 256.0).sqrt()))
}

pub fn fix16_sin_cos(value: Fix16Raw) -> (Fix16Raw, Fix16Raw) {
    let radians = f64::from(value) / 256.0;
    (quantize_f64(radians.sin()), quantize_f64(radians.cos()))
}

pub fn fix16_abs(value: Fix16Raw) -> Fix16Raw {
    if value == i16::MIN {
        i16::MAX
    } else {
        value.abs()
    }
}

pub fn fix16_neg(value: Fix16Raw) -> Fix16Raw {
    if value == i16::MIN {
        i16::MAX
    } else {
        -value
    }
}

pub fn fix16_floor(value: Fix16Raw) -> Fix16Raw {
    value & !0xff
}

pub fn fix16_ceil(value: Fix16Raw) -> Fix16Raw {
    if value & 0xff == 0 {
        value
    } else {
        fix16_saturate(i64::from(value & !0xff) + i64::from(FIX16_ONE))
    }
}

pub fn fix16_round(value: Fix16Raw) -> Fix16Raw {
    fix16_saturate(round_shift_ties_even(i64::from(value), 8) << 8)
}

pub fn fix16_saturate01(value: Fix16Raw) -> Fix16Raw {
    value.clamp(0, FIX16_ONE)
}

pub fn fix16_sign(value: Fix16Raw) -> Fix16Raw {
    match value.cmp(&0) {
        Ordering::Less => -FIX16_ONE,
        Ordering::Equal => 0,
        Ordering::Greater => FIX16_ONE,
    }
}

pub fn continuation_mask(value: FpuVector) -> u8 {
    u8::from(value[1..].iter().any(|&lane| lane != 0)) << 2
        | u8::from(value[2..].iter().any(|&lane| lane != 0)) << 1
        | u8::from(value[3] != 0)
}

fn quantize_f64(value: f64) -> Fix16Raw {
    let scaled = (value * 256.0).round_ties_even();
    if scaled <= f64::from(i16::MIN) {
        i16::MIN
    } else if scaled >= f64::from(i16::MAX) {
        i16::MAX
    } else {
        scaled as i16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_even_and_saturation_are_bit_exact() {
        assert_eq!(round_shift_ties_even(128, 8), 0);
        assert_eq!(round_shift_ties_even(384, 8), 2);
        assert_eq!(round_shift_ties_even(-128, 8), 0);
        assert_eq!(round_shift_ties_even(-384, 8), -2);
        assert_eq!(fix16_mul(384, 384), 576);
        assert_eq!(fix16_add(i16::MAX, 1), i16::MAX);
        assert_eq!(fix16_sub(i16::MIN, 1), i16::MIN);
    }

    #[test]
    fn accumulator_has_a_signed_40_bit_saturating_contract() {
        assert_eq!(acc_saturate(i128::MAX), FPU_ACC_MAX);
        assert_eq!(acc_saturate(i128::MIN), FPU_ACC_MIN);
        let four_max_products = (0..4).fold(0, |acc, _| {
            fix16_accumulate_product(acc, i16::MIN, i16::MIN)
        });
        assert_eq!(four_max_products, 1_i64 << 32);
        assert_eq!(fix16_from_acc(four_max_products), i16::MAX);
    }

    #[test]
    fn unary_domains_and_geometry_helpers_are_defined() {
        assert_eq!(fix16_reciprocal(0), Err(FpuDomainError::ReciprocalZero));
        assert_eq!(fix16_reciprocal(256), Ok(256));
        assert_eq!(fix16_reciprocal_sqrt(256), Ok(256));
        assert_eq!(
            fix16_reciprocal_sqrt(0),
            Err(FpuDomainError::ReciprocalSqrtNonPositive)
        );
        assert_eq!(fix16_sin_cos(0), (0, 256));
        assert_eq!(fix16_abs(i16::MIN), i16::MAX);
        assert_eq!(fix16_neg(i16::MIN), i16::MAX);
    }

    #[test]
    fn continuation_bits_are_derived_only_from_values() {
        assert_eq!(continuation_mask([0, 0, 0, 0]), 0b000);
        assert_eq!(continuation_mask([1, 2, 0, 0]), 0b100);
        assert_eq!(continuation_mask([1, 0, 3, 0]), 0b110);
        assert_eq!(continuation_mask([0, 0, 0, 4]), 0b111);
    }
}
