//! Fixed-point discipline carried forward from the former raster sim's
//! `fixed.rs`: formats are types, raw conversions are explicit, and every
//! arithmetic boundary chooses wrapping, saturation, or checked range.
//!
//! The old rasterizer's specific reciprocal tables and guard-band formats are
//! not v2 contracts. They remain in `../oldcode` until the specification fixes
//! those algorithms and widths.

use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NumericFault {
    InvalidFormat,
    OutOfRange,
}

/// `INT` integer bits excluding sign, `FRAC` fractional bits, and an explicit
/// signedness. A value's raw representation always has `INT + FRAC + SIGNED`
/// bits. This is the reusable `Fx` representation from the old `fixed.rs`.
#[derive(Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Fx<const INT: u32, const FRAC: u32, const SIGNED: bool> {
    raw: i64,
}

impl<const INT: u32, const FRAC: u32, const SIGNED: bool> Fx<INT, FRAC, SIGNED> {
    pub const WIDTH: u32 = INT + FRAC + SIGNED as u32;

    pub fn from_raw(raw: i128) -> Result<Self, NumericFault> {
        if Self::WIDTH == 0 || Self::WIDTH > 63 {
            return Err(NumericFault::InvalidFormat);
        }
        let (low, high) = if SIGNED {
            (
                -(1_i128 << (Self::WIDTH - 1)),
                (1_i128 << (Self::WIDTH - 1)) - 1,
            )
        } else {
            (0, (1_i128 << Self::WIDTH) - 1)
        };
        if !(low..=high).contains(&raw) {
            return Err(NumericFault::OutOfRange);
        }
        Ok(Self { raw: raw as i64 })
    }

    /// The raw integer is returned only at a named device or format boundary.
    pub const fn raw(self) -> i64 {
        self.raw
    }

    pub fn checked_add(self, other: Self) -> Result<Self, NumericFault> {
        Self::from_raw(i128::from(self.raw) + i128::from(other.raw))
    }

    pub fn checked_sub(self, other: Self) -> Result<Self, NumericFault> {
        Self::from_raw(i128::from(self.raw) - i128::from(other.raw))
    }

    /// A hardware wrap is explicit, never the default for `Fx` arithmetic.
    pub fn wrapping_from_raw(raw: i128) -> Result<Self, NumericFault> {
        if Self::WIDTH == 0 || Self::WIDTH > 63 {
            return Err(NumericFault::InvalidFormat);
        }
        let modulus = 1_i128 << Self::WIDTH;
        let bits = raw.rem_euclid(modulus);
        let signed = if SIGNED && bits >= (modulus >> 1) {
            bits - modulus
        } else {
            bits
        };
        Ok(Self { raw: signed as i64 })
    }

    /// Saturation is explicit and remains distinguishable from a legal value.
    pub fn saturating_from_raw(raw: i128) -> Result<(Self, bool), NumericFault> {
        if Self::WIDTH == 0 || Self::WIDTH > 63 {
            return Err(NumericFault::InvalidFormat);
        }
        let (low, high) = if SIGNED {
            (
                -(1_i128 << (Self::WIDTH - 1)),
                (1_i128 << (Self::WIDTH - 1)) - 1,
            )
        } else {
            (0, (1_i128 << Self::WIDTH) - 1)
        };
        Ok((
            Self {
                raw: raw.clamp(low, high) as i64,
            },
            raw < low || raw > high,
        ))
    }
}

impl<const INT: u32, const FRAC: u32, const SIGNED: bool> fmt::Debug for Fx<INT, FRAC, SIGNED> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fx<{INT},{FRAC},{SIGNED}>({})", self.raw)
    }
}

/// A checked integer intermediate wider than the `i64` storage of `Fx`.
/// `BITS` includes the sign bit when `SIGNED` is true. Keeping the host
/// carrier at `i128` does not grant the modeled datapath 128 bits: every
/// construction and arithmetic result is checked against `BITS`.
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct WideFx<const BITS: u32, const SIGNED: bool> {
    raw: i128,
}

impl<const BITS: u32, const SIGNED: bool> WideFx<BITS, SIGNED> {
    pub const WIDTH: u32 = BITS;

    pub fn from_raw(raw: i128) -> Result<Self, NumericFault> {
        if !(64..=127).contains(&BITS) {
            return Err(NumericFault::InvalidFormat);
        }
        let (low, high) = if SIGNED {
            (-(1_i128 << (BITS - 1)), (1_i128 << (BITS - 1)) - 1)
        } else {
            (
                0,
                if BITS == 127 {
                    i128::MAX
                } else {
                    (1_i128 << BITS) - 1
                },
            )
        };
        if !(low..=high).contains(&raw) {
            return Err(NumericFault::OutOfRange);
        }
        Ok(Self { raw })
    }

    pub const fn raw(self) -> i128 {
        self.raw
    }

    pub fn checked_add(self, other: Self) -> Result<Self, NumericFault> {
        Self::from_raw(
            self.raw
                .checked_add(other.raw)
                .ok_or(NumericFault::OutOfRange)?,
        )
    }

    pub fn checked_sub(self, other: Self) -> Result<Self, NumericFault> {
        Self::from_raw(
            self.raw
                .checked_sub(other.raw)
                .ok_or(NumericFault::OutOfRange)?,
        )
    }

    pub fn checked_mul_i128(self, factor: i128) -> Result<Self, NumericFault> {
        Self::from_raw(
            self.raw
                .checked_mul(factor)
                .ok_or(NumericFault::OutOfRange)?,
        )
    }

    pub fn checked_shl(self, bits: u32) -> Result<Self, NumericFault> {
        if bits > 126 {
            return Err(NumericFault::OutOfRange);
        }
        let scale = 1_i128.checked_shl(bits).ok_or(NumericFault::OutOfRange)?;
        let shifted = self
            .raw
            .checked_mul(scale)
            .ok_or(NumericFault::OutOfRange)?;
        Self::from_raw(shifted)
    }
}

impl<const BITS: u32, const SIGNED: bool> fmt::Debug for WideFx<BITS, SIGNED> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "WideFx<{BITS},{SIGNED}>({})", self.raw)
    }
}

/// The existing vertex input/output profile. V2 may replace this alias after
/// the vertex/setup numerical contract is frozen.
pub type Q16 = Fx<15, 16, true>;

/// Trial signed Q2.14 normal and normal-matrix format.
pub type Q14 = Fx<1, 14, true>;

/// Signed 36-bit DSP operands and 72-bit product use exact `i128` arithmetic.
pub fn mul_signed_36(a: i64, b: i64) -> Result<i128, NumericFault> {
    let min = -(1_i64 << 35);
    let max = (1_i64 << 35) - 1;
    if !(min..=max).contains(&a) || !(min..=max).contains(&b) {
        return Err(NumericFault::OutOfRange);
    }
    Ok(i128::from(a) * i128::from(b))
}

/// Signed round-to-nearest, ties-to-even when dropping `shift` fractional
/// bits. Euclidean division makes negative half ties symmetric.
pub fn round_shift_ties_even(raw: i128, shift: u32) -> Result<i128, NumericFault> {
    if shift > 126 {
        return Err(NumericFault::InvalidFormat);
    }
    if shift == 0 {
        return Ok(raw);
    }
    let denominator = 1_i128 << shift;
    let quotient = raw >> shift;
    let remainder = raw & (denominator - 1);
    let half = denominator >> 1;
    Ok(quotient + i128::from(remainder > half || (remainder == half && quotient & 1 != 0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_ranges_and_explicit_overflow_match_bit_patterns() {
        assert_eq!(
            Q16::from_raw(i32::MIN.into()).unwrap().raw(),
            i64::from(i32::MIN)
        );
        assert_eq!(
            Q16::from_raw(i32::MAX.into()).unwrap().raw(),
            i64::from(i32::MAX)
        );
        assert_eq!(Q16::from_raw(1_i128 << 31), Err(NumericFault::OutOfRange));
        assert_eq!(
            Q16::wrapping_from_raw(1_i128 << 31).unwrap().raw(),
            i64::from(i32::MIN)
        );
        let (value, saturated) = Q16::saturating_from_raw(1_i128 << 31).unwrap();
        assert!(saturated);
        assert_eq!(value.raw(), i64::from(i32::MAX));
    }

    #[test]
    fn wide_intermediate_checks_width_after_each_operation() {
        type Signed72 = WideFx<72, true>;
        let max = (1_i128 << 71) - 1;
        assert_eq!(Signed72::from_raw(max).unwrap().raw(), max);
        assert_eq!(Signed72::from_raw(max + 1), Err(NumericFault::OutOfRange));
        assert_eq!(
            Signed72::from_raw(-(1_i128 << 71)).unwrap().raw(),
            -(1_i128 << 71)
        );
        assert_eq!(
            Signed72::from_raw(-(1_i128 << 71) - 1),
            Err(NumericFault::OutOfRange)
        );
        assert_eq!(
            Signed72::from_raw(max)
                .unwrap()
                .checked_add(Signed72::from_raw(1).unwrap()),
            Err(NumericFault::OutOfRange)
        );
        assert_eq!(
            Signed72::from_raw(1_i128 << 70).unwrap().checked_shl(1),
            Err(NumericFault::OutOfRange)
        );
        assert_eq!(
            WideFx::<128, false>::from_raw(0),
            Err(NumericFault::InvalidFormat)
        );
        assert_eq!(
            WideFx::<127, false>::from_raw(i128::MAX).unwrap().raw(),
            i128::MAX
        );
        assert_eq!(
            Signed72::from_raw(-1).unwrap().checked_shl(71),
            Ok(Signed72::from_raw(-(1_i128 << 71)).unwrap())
        );
    }

    #[test]
    fn signed_half_ties_round_to_even_on_both_sides() {
        for (raw, expected) in [(0x8000, 0), (0x18000, 2), (-0x8000, 0), (-0x18000, -2)] {
            assert_eq!(round_shift_ties_even(raw, 16), Ok(expected));
        }
    }

    #[test]
    fn dsp_36_limits_are_exact() {
        assert_eq!(
            mul_signed_36(-(1 << 35), (1 << 35) - 1),
            Ok(-(1_i128 << 35) * ((1_i128 << 35) - 1))
        );
        assert_eq!(mul_signed_36(1 << 35, 1), Err(NumericFault::OutOfRange));
    }
}
