//! Raw signed 32-bit arithmetic for physics. A fix32 carries bits here.
//! Multiplication is explicitly integer; ordinary fix32 `*` remains Q16.16.
use crate::dsl_rt::*;

pub fn from_i16(value: i16) -> fix32 {
    fix32::from_words(value as u16, (value >> 15) as u16)
}

pub fn mul(a: fix32, b: fix32) -> fix32 {
    fmul_lo32(a, b)
}

pub fn rate_step(value: fix32, force: fix32, dt: fix32) -> fix32 {
    let amount = fmul_lo32(force, dt);
    if value > amount {
        return value - amount;
    }
    if value < -amount {
        return value + amount;
    }
    fix32::zero()
}

pub fn decay(value: fix32, shift: u16, dt: fix32) -> fix32 {
    let result = value - sar(fmul_lo32(value, dt), shift);
    if (value < fix32::zero() && result > fix32::zero())
        || (value > fix32::zero() && result < fix32::zero())
    {
        return fix32::zero();
    }
    result
}

pub fn shl(value: fix32, count: u16) -> fix32 {
    let shift = count & 31;
    if shift == 0 {
        return value;
    }
    let lo = value.lo_bits();
    if shift >= 16 {
        return fix32::from_words(0, lo << (shift - 16));
    }
    fix32::from_words(
        lo << shift,
        (value.hi_bits() << shift) | (lo >> (16 - shift)),
    )
}

pub fn sar(value: fix32, count: u16) -> fix32 {
    let shift = count & 31;
    if shift == 0 {
        return value;
    }
    let hi = value.hi_bits() as i16;
    if shift >= 16 {
        return fix32::from_words((hi >> (shift - 16)) as u16, (hi >> 15) as u16);
    }
    fix32::from_words(
        (value.lo_bits() >> shift) | ((hi as u16) << (16 - shift)),
        (hi >> shift) as u16,
    )
}

/// Legacy sign/magnitude shift, including the wrapping INT_MIN behavior.
pub fn trunc_shift(value: fix32, count: u16) -> fix32 {
    if value < fix32::zero() {
        return -sar(-value, count);
    }
    sar(value, count)
}

/// Exact signed quotient. INT_MIN/-1 wraps; zero divisor halts with 0x7b01.
pub fn div(numerator: fix32, denominator: fix32) -> fix32 {
    if denominator == fix32::zero() {
        halt(0x7b01);
    }
    let negative = (numerator.hi_bits() ^ denominator.hi_bits()) & 0x8000 != 0;
    let mut n = numerator;
    let mut d = denominator;
    if n < fix32::zero() {
        n = -n;
    }
    if d < fix32::zero() {
        d = -d;
    }
    let mut qlo = n.lo_bits();
    let mut qhi = n.hi_bits();
    let dlo = d.lo_bits();
    let dhi = d.hi_bits();
    let mut rlo = 0u16;
    let mut rhi = 0u16;
    let mut bit = 0u16;
    // Seed the upper-word remainder when the quotient fits in one word.
    // The same restoring loop then consumes only the remaining 16 bits.
    if dhi == 0 && qhi < dlo {
        rlo = qhi;
        qhi = qlo;
        qlo = 0;
        bit = 16;
    }
    while bit < 32 {
        let next = qhi >> 15;
        qhi = (qhi << 1) | (qlo >> 15);
        qlo = qlo << 1;
        rhi = (rhi << 1) | (rlo >> 15);
        rlo = (rlo << 1) | next;
        if rhi > dhi || (rhi == dhi && rlo >= dlo) {
            let borrow = rlo < dlo;
            rlo -= dlo;
            rhi -= dhi;
            if borrow {
                rhi -= 1;
            }
            qlo |= 1;
        }
        bit += 1;
    }
    let quotient = fix32::from_words(qlo, qhi);
    if negative {
        return -quotient;
    }
    quotient
}

fn q12_ratio(value: i16, denominator: u16) -> i16 {
    let mut magnitude = value as u16;
    if value < 0i16 {
        magnitude = 0u16 - magnitude;
    }
    if denominator < 2048 || denominator > 32768 || magnitude > 4096 {
        let numerator = shl(from_i16(value), 12);
        return div(numerator, fix32::from_words(denominator, 0)).lo_bits() as i16;
    }
    let mut quotient = 0u16;
    // The integer part is at most two. The remaining 12 fractional steps
    // each keep a remainder below denominator and fit in an unsigned word.
    if magnitude >= denominator {
        magnitude -= denominator;
        quotient += 1;
    }
    if magnitude >= denominator {
        magnitude -= denominator;
        quotient += 1;
    }
    let mut bit = 0u16;
    while bit < 12 {
        magnitude = magnitude << 1;
        quotient = quotient << 1;
        if magnitude >= denominator {
            magnitude -= denominator;
            quotient += 1;
        }
        bit += 1;
    }
    if value < 0i16 {
        return 0i16 - (quotient as i16);
    }
    quotient as i16
}

/// Scale three signed components by 4096/denominator, truncating toward zero.
/// The result narrows to its low 16 bits. Zero divisor halts with 0x7b01.
pub fn scale3_q12(mut values: Array<i16>, denominator: u16) {
    if denominator == 0 {
        halt(0x7b01);
    }
    let mut i = 0u16;
    while i < 3 {
        values[i] = q12_ratio(values[i], denominator);
        i += 1;
    }
}
