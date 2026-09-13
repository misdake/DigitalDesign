//! Bit-exact architectural arithmetic for the CpuV3 revision 0.7 fix16 FPU.

use std::cmp::Ordering;

#[path = "fpu_rom_data.rs"]
mod fpu_rom_data;
pub use fpu_rom_data::FPU_ROM_WORDS;

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
    let (index, exponent) = normalize_magnitude(value.unsigned_abs());
    Ok(scale_q15(
        FPU_ROM_WORDS[256 + index] as u16,
        exponent,
        value < 0,
    ))
}

pub fn fix16_reciprocal_sqrt(value: Fix16Raw) -> Result<Fix16Raw, FpuDomainError> {
    if value <= 0 {
        return Err(FpuDomainError::ReciprocalSqrtNonPositive);
    }
    let (index, exponent) = normalize_magnitude(value as u16);
    let odd_exponent = exponent.rem_euclid(2) != 0;
    let table = if odd_exponent { 768 } else { 512 };
    Ok(scale_q15(
        FPU_ROM_WORDS[table + index] as u16,
        exponent.div_euclid(2),
        false,
    ))
}

pub fn fix16_sin_cos(value: Fix16Raw) -> (Fix16Raw, Fix16Raw) {
    // 83443 / 65536 approximates 4/pi. The low eleven bits are a complete
    // modulo-2pi reduction into 2048 phase steps.
    let phase = round_shift_ties_even(i64::from(value) * 83_443, 16).rem_euclid(2048) as u16;
    (quarter_sine(phase), quarter_sine((phase + 512) & 2047))
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

#[cfg(test)]
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

fn normalize_magnitude(magnitude: u16) -> (usize, i32) {
    debug_assert!(magnitude != 0);
    let leading = magnitude.leading_zeros() as i32;
    let mut exponent = 7 - leading;
    let mut normalized = if exponent > 0 {
        round_shift_ties_even(i64::from(magnitude), exponent as u32) as u16
    } else if exponent == 0 {
        magnitude
    } else {
        magnitude << (-exponent as u32)
    };
    if normalized == 512 {
        normalized = 256;
        exponent += 1;
    }
    debug_assert!((256..512).contains(&normalized));
    (usize::from(normalized - 256), exponent)
}

fn scale_q15(value: u16, exponent: i32, negative: bool) -> Fix16Raw {
    let shift = 7 + exponent;
    let magnitude = if shift > 0 {
        round_shift_ties_even(i64::from(value), shift as u32)
    } else {
        i64::from(value) << (-shift as u32)
    };
    fix16_saturate(if negative { -magnitude } else { magnitude })
}

fn quarter_sine(phase: u16) -> Fix16Raw {
    let quadrant = phase >> 9;
    let offset = usize::from(phase & 511);
    let rising = || sine_sample(offset);
    let falling = || {
        if offset == 0 {
            FIX16_ONE
        } else {
            sine_sample(512 - offset)
        }
    };
    match quadrant {
        0 => rising(),
        1 => falling(),
        2 => -rising(),
        3 => -falling(),
        _ => unreachable!(),
    }
}

fn sine_sample(index: usize) -> Fix16Raw {
    if index >= 492 {
        return FIX16_ONE;
    }
    let packed = FPU_ROM_WORDS[index >> 1];
    ((packed >> ((index & 1) * 8)) & 0xff) as Fix16Raw
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

    #[test]
    fn shared_rom_error_is_bounded_over_the_complete_fix16_domain() {
        let mut reciprocal_error = 0_i32;
        let mut rsqrt_error = 0_i32;
        let mut sin_cos_error = 0_i32;
        let mut sin_cos_continuous_error = 0.0_f64;
        let mut sin_cos_squared_error_sum = 0.0_f64;
        for raw in i16::MIN..=i16::MAX {
            if raw != 0 {
                let ideal = quantize_f64(1.0 / (f64::from(raw) / 256.0));
                reciprocal_error = reciprocal_error
                    .max((i32::from(fix16_reciprocal(raw).unwrap()) - i32::from(ideal)).abs());
            }
            if raw > 0 {
                let ideal = quantize_f64(1.0 / (f64::from(raw) / 256.0).sqrt());
                rsqrt_error = rsqrt_error
                    .max((i32::from(fix16_reciprocal_sqrt(raw).unwrap()) - i32::from(ideal)).abs());
            }
            let radians = f64::from(raw) / 256.0;
            let (sin, cos) = fix16_sin_cos(raw);
            let sin_continuous_error = (f64::from(sin) / 256.0 - radians.sin()).abs();
            let cos_continuous_error = (f64::from(cos) / 256.0 - radians.cos()).abs();
            sin_cos_error = sin_cos_error
                .max((i32::from(sin) - i32::from(quantize_f64(radians.sin()))).abs())
                .max((i32::from(cos) - i32::from(quantize_f64(radians.cos()))).abs());
            sin_cos_continuous_error = sin_cos_continuous_error
                .max(sin_continuous_error)
                .max(cos_continuous_error);
            sin_cos_squared_error_sum +=
                sin_continuous_error.powi(2) + cos_continuous_error.powi(2);
        }
        let sin_cos_rms_error = (sin_cos_squared_error_sum / (2.0 * 65_536.0)).sqrt();
        eprintln!(
            "complete-domain FPU ROM errors: rcp={reciprocal_error}, rsqrt={rsqrt_error}, sincos_raw={sin_cos_error}, sincos_continuous={sin_cos_continuous_error:.9}, sincos_rms={sin_cos_rms_error:.9}"
        );
        assert!(reciprocal_error <= 2);
        assert!(rsqrt_error <= 2);
        assert!(sin_cos_error <= 1);
        assert!(sin_cos_continuous_error < 1.0 / 256.0);
        assert!(sin_cos_rms_error < 0.0013);
    }
}
