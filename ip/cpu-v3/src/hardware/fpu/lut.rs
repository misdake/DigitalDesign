//! FPU v2 special-function reference model: the hidden-BSRAM LUT contents and
//! the pure-integer RCP/RSQRT/SINCOS datapath (design `fpu-design-v2` section
//! 9.4, Stage 7a).
//!
//! This module is the single source of truth for the three LUTs. Later steps
//! emit the Verilog BSRAM `initial` block and mirror the same arrays in the
//! emulator from here; the arrays are checked into the source as literals so
//! that the emulator path never builds a table at run time. The literals were
//! generated offline with the f64 formulas below and are re-derived by a test
//! so a transcription slip cannot survive `cargo test`.
//!
//! ```text
//! RCP_LUT[i]    = round(1 / (1 + i/128)        * 2^16), i = 0..127
//! RSQRT_LUT[i]  = round(1 / sqrt(1 + 3i/128)   * 2^16), i = 0..127
//! SINCOS_LUT[i] = round(sin(pi/2 * i/128)      * 2^16), i = 0..=128
//! ```
//!
//! The interpolation arithmetic is exactly the integer shape the RTL will use:
//! `delta = next - current` (signed), `current + ((delta * residue) >> 9)` with
//! an arithmetic `>> 9`. `|delta|` is far below `i16::MAX` (max 508 / 755 / 804
//! for RCP / RSQRT / SINCOS; see the delta test), so the product is a plain
//! 16x16-class signed multiply and cannot overflow `i32`.
//!
//! The LUTs and datapath have no in-crate consumer until the special-function
//! path and its emulator mirror land (Stage 7b/7c); `dead_code` is allowed for
//! that window only.
#![allow(dead_code)]

pub(crate) const RCP_LUT: [u32; 128] = [
    0x00010000, 0x0000FE04, 0x0000FC10, 0x0000FA23, 0x0000F83E, 0x0000F660, 0x0000F48A, 0x0000F2BA,
    0x0000F0F1, 0x0000EF2F, 0x0000ED73, 0x0000EBBE, 0x0000EA0F, 0x0000E866, 0x0000E6C3, 0x0000E526,
    0x0000E38E, 0x0000E1FC, 0x0000E070, 0x0000DEE9, 0x0000DD68, 0x0000DBEB, 0x0000DA74, 0x0000D902,
    0x0000D794, 0x0000D62C, 0x0000D4C7, 0x0000D368, 0x0000D20D, 0x0000D0B7, 0x0000CF64, 0x0000CE17,
    0x0000CCCD, 0x0000CB87, 0x0000CA46, 0x0000C908, 0x0000C7CE, 0x0000C698, 0x0000C566, 0x0000C437,
    0x0000C30C, 0x0000C1E5, 0x0000C0C1, 0x0000BFA0, 0x0000BE83, 0x0000BD69, 0x0000BC52, 0x0000BB3F,
    0x0000BA2F, 0x0000B921, 0x0000B817, 0x0000B710, 0x0000B60B, 0x0000B50A, 0x0000B40B, 0x0000B30F,
    0x0000B216, 0x0000B120, 0x0000B02C, 0x0000AF3B, 0x0000AE4C, 0x0000AD60, 0x0000AC77, 0x0000AB8F,
    0x0000AAAB, 0x0000A9C8, 0x0000A8E8, 0x0000A80B, 0x0000A72F, 0x0000A656, 0x0000A57F, 0x0000A4AA,
    0x0000A3D7, 0x0000A306, 0x0000A238, 0x0000A16B, 0x0000A0A1, 0x00009FD8, 0x00009F11, 0x00009E4D,
    0x00009D8A, 0x00009CC9, 0x00009C0A, 0x00009B4C, 0x00009A91, 0x000099D7, 0x0000991F, 0x00009869,
    0x000097B4, 0x00009701, 0x00009650, 0x000095A0, 0x000094F2, 0x00009446, 0x0000939B, 0x000092F1,
    0x00009249, 0x000091A3, 0x000090FE, 0x0000905A, 0x00008FB8, 0x00008F17, 0x00008E78, 0x00008DDA,
    0x00008D3E, 0x00008CA3, 0x00008C09, 0x00008B70, 0x00008AD9, 0x00008A43, 0x000089AE, 0x0000891B,
    0x00008889, 0x000087F8, 0x00008768, 0x000086D9, 0x0000864C, 0x000085BF, 0x00008534, 0x000084AA,
    0x00008421, 0x00008399, 0x00008312, 0x0000828D, 0x00008208, 0x00008185, 0x00008102, 0x00008081,
];

pub(crate) const RSQRT_LUT: [u32; 128] = [
    0x00010000, 0x0000FD0D, 0x0000FA34, 0x0000F773, 0x0000F4C8, 0x0000F234, 0x0000EFB3, 0x0000ED46,
    0x0000EAEC, 0x0000E8A3, 0x0000E66B, 0x0000E443, 0x0000E22A, 0x0000E020, 0x0000DE23, 0x0000DC34,
    0x0000DA51, 0x0000D87B, 0x0000D6B0, 0x0000D4F1, 0x0000D33C, 0x0000D192, 0x0000CFF1, 0x0000CE5A,
    0x0000CCCD, 0x0000CB48, 0x0000C9CC, 0x0000C858, 0x0000C6EB, 0x0000C587, 0x0000C42A, 0x0000C2D4,
    0x0000C185, 0x0000C03C, 0x0000BEFA, 0x0000BDBE, 0x0000BC89, 0x0000BB59, 0x0000BA2F, 0x0000B90A,
    0x0000B7EA, 0x0000B6D0, 0x0000B5BB, 0x0000B4AB, 0x0000B39F, 0x0000B298, 0x0000B196, 0x0000B097,
    0x0000AF9D, 0x0000AEA7, 0x0000ADB6, 0x0000ACC8, 0x0000ABDD, 0x0000AAF7, 0x0000AA14, 0x0000A934,
    0x0000A858, 0x0000A77F, 0x0000A6AA, 0x0000A5D8, 0x0000A508, 0x0000A43C, 0x0000A373, 0x0000A2AC,
    0x0000A1E9, 0x0000A128, 0x0000A069, 0x00009FAE, 0x00009EF5, 0x00009E3E, 0x00009D8A, 0x00009CD8,
    0x00009C29, 0x00009B7B, 0x00009AD0, 0x00009A28, 0x00009981, 0x000098DD, 0x0000983A, 0x0000979A,
    0x000096FB, 0x0000965E, 0x000095C4, 0x0000952B, 0x00009494, 0x000093FF, 0x0000936B, 0x000092D9,
    0x00009249, 0x000091BB, 0x0000912E, 0x000090A3, 0x00009019, 0x00008F91, 0x00008F0A, 0x00008E85,
    0x00008E01, 0x00008D7E, 0x00008CFD, 0x00008C7E, 0x00008C00, 0x00008B83, 0x00008B07, 0x00008A8D,
    0x00008A13, 0x0000899C, 0x00008925, 0x000088AF, 0x0000883B, 0x000087C8, 0x00008756, 0x000086E5,
    0x00008675, 0x00008606, 0x00008599, 0x0000852C, 0x000084C1, 0x00008456, 0x000083EC, 0x00008384,
    0x0000831C, 0x000082B5, 0x00008250, 0x000081EB, 0x00008187, 0x00008124, 0x000080C2, 0x00008060,
];

pub(crate) const SINCOS_LUT: [u32; 129] = [
    0x00000000, 0x00000324, 0x00000648, 0x0000096C, 0x00000C90, 0x00000FB3, 0x000012D5, 0x000015F7,
    0x00001918, 0x00001C38, 0x00001F56, 0x00002274, 0x00002590, 0x000028AB, 0x00002BC4, 0x00002EDC,
    0x000031F1, 0x00003505, 0x00003817, 0x00003B27, 0x00003E34, 0x0000413F, 0x00004447, 0x0000474D,
    0x00004A50, 0x00004D50, 0x0000504D, 0x00005348, 0x0000563E, 0x00005932, 0x00005C22, 0x00005F0F,
    0x000061F8, 0x000064DD, 0x000067BE, 0x00006A9B, 0x00006D74, 0x00007049, 0x0000731A, 0x000075E6,
    0x000078AD, 0x00007B70, 0x00007E2F, 0x000080E8, 0x0000839C, 0x0000864C, 0x000088F6, 0x00008B9A,
    0x00008E3A, 0x000090D4, 0x00009368, 0x000095F7, 0x00009880, 0x00009B03, 0x00009D80, 0x00009FF7,
    0x0000A268, 0x0000A4D2, 0x0000A736, 0x0000A994, 0x0000ABEB, 0x0000AE3C, 0x0000B086, 0x0000B2C9,
    0x0000B505, 0x0000B73A, 0x0000B968, 0x0000BB8F, 0x0000BDAF, 0x0000BFC7, 0x0000C1D8, 0x0000C3E2,
    0x0000C5E4, 0x0000C7DE, 0x0000C9D1, 0x0000CBBC, 0x0000CD9F, 0x0000CF7A, 0x0000D14D, 0x0000D318,
    0x0000D4DB, 0x0000D696, 0x0000D848, 0x0000D9F2, 0x0000DB94, 0x0000DD2D, 0x0000DEBE, 0x0000E046,
    0x0000E1C6, 0x0000E33C, 0x0000E4AA, 0x0000E610, 0x0000E76C, 0x0000E8BF, 0x0000EA0A, 0x0000EB4B,
    0x0000EC83, 0x0000EDB3, 0x0000EED9, 0x0000EFF5, 0x0000F109, 0x0000F213, 0x0000F314, 0x0000F40C,
    0x0000F4FA, 0x0000F5DF, 0x0000F6BA, 0x0000F78C, 0x0000F854, 0x0000F913, 0x0000F9C8, 0x0000FA73,
    0x0000FB15, 0x0000FBAD, 0x0000FC3B, 0x0000FCC0, 0x0000FD3B, 0x0000FDAC, 0x0000FE13, 0x0000FE71,
    0x0000FEC4, 0x0000FF0E, 0x0000FF4E, 0x0000FF85, 0x0000FFB1, 0x0000FFD4, 0x0000FFEC, 0x0000FFFB,
    0x00010000,
];

/// RCP endpoint: `1/m` at `m = 2`, i.e. index 127's `next` (design section
/// 9.4). The 128-entry table does not store it, so the datapath supplies the
/// exact constant instead of reading out of bounds.
const RCP_ENDPOINT: i32 = 0x8000;

/// RSQRT endpoint: `1/sqrt(m)` at `m = 4` (index 127's `next`).
const RSQRT_ENDPOINT: i32 = 0x8000;

/// `round(2/pi * 2^30)`, the Q2.30 range-reduction constant. `round(2/pi *
/// 2^16)` only carries ~16 bits of `2/pi`, whose error times `|a|` blows the
/// 4 ulp bound by `|a| ~ 60`; the Q2.30 form pushes the constant error to
/// `~2^-30`.
const TWO_OVER_PI_Q2_30: i32 = 683_565_276;

/// Q16.16 multiply with the FPU's standard narrowing `product[47:16]` (wrap):
/// an arithmetic `>> 16` of the 64-bit product, truncated to 32 bits. This is
/// the same operation the shared mul pipe's write port performs, so the
/// reference model and RTL see identical wraparound at every refinement step.
#[inline]
fn q16_mul(a: i32, b: i32) -> i32 {
    ((i64::from(a) * i64::from(b)) >> 16) as i32
}

/// Linear interpolation of the 128-entry tables shared by RCP and RSQRT.
#[inline]
fn interpolate(lut: &[u32; 128], index: usize, residue: i32, endpoint: i32) -> i32 {
    let current = lut[index] as i32;
    let next = if index == 127 {
        endpoint
    } else {
        lut[index + 1] as i32
    };
    let delta = next - current;
    current + ((delta * residue) >> 9)
}

/// RCP approximation of `x` in Q16.16 (design section 9.4):
///
/// * `sign out = sign in`, `x == 0 -> 0x7FFF_FFFF`;
/// * normalize `|x| = m * 2^e`, `m in [1,2)` (CLZ on the magnitude);
/// * index the LUT with the top 7 fraction bits of `m`, interpolate with the
///   next 9 bits, then rescale by `2^-e` with a saturating barrel shift.
pub(crate) fn rcp_q16(x: i32) -> i32 {
    if x == 0 {
        return i32::MAX;
    }
    let negative = x < 0;
    let magnitude = x.unsigned_abs();
    let clz = magnitude.leading_zeros();
    let normalized = magnitude << clz;
    let index = ((normalized >> 24) & 0x7F) as usize;
    let residue = ((normalized >> 15) & 0x1FF) as i32;
    let interpolated = interpolate(&RCP_LUT, index, residue, RCP_ENDPOINT);
    // Real exponent of |x| is e = clz - 16, so 1/|x| rescales by 2^(15 - clz).
    let shift = clz as i32 - 15;
    let scaled = if shift >= 0 {
        let wide = (interpolated as u64) << shift;
        if wide > i32::MAX as u64 {
            i32::MAX
        } else {
            wide as i32
        }
    } else {
        interpolated >> (-shift)
    };
    if negative {
        -scaled
    } else {
        scaled
    }
}

/// RSQRT approximation of `x` in Q16.16: `x <= 0 -> 0`, otherwise normalize
/// `x = m * 2^(2k)` with `m in [1,4)` (CLZ, even exponent), look up
/// `1/sqrt(m)` from the table uniform in `m`, refine with **one**
/// Newton-Raphson step, and rescale by `2^-k`.
///
/// The refinement is `y1 = y0 * (3 - m*y0^2) / 2`, with `y0` the interpolated
/// seed and `m` the normalized mantissa in Q16.16 (`normalized >> 14`, the
/// low 14 bits of the `m * 2^30` representation are dropped by the Q16.16
/// cast). Each product is the shared pipe's Q16.16 narrowing
/// (`q16_mul`), the `3` is `3 << 16`, and `/2` is a final arithmetic `>> 1`.
pub(crate) fn rsqrt_q16(x: i32) -> i32 {
    if x <= 0 {
        return 0;
    }
    let magnitude = x as u32;
    let clz = magnitude.leading_zeros();
    let binary_exponent = 31 - clz;
    let exponent = binary_exponent as i32 - 16;
    // Even part of `x`'s binary exponent, and the `2^(30)`/`2^(31)` fixed point
    // that puts `m` in `[1,4)`.
    let (normalize_shift, scale_shift) = if exponent & 1 == 0 {
        ((30 - binary_exponent) as i32, exponent / 2)
    } else {
        ((31 - binary_exponent) as i32, (exponent - 1) / 2)
    };
    let normalized = magnitude << normalize_shift as u32;
    // `normalized = m * 2^30`; the table is uniform over `m = 1 + 3i/128`, so
    // `u = (m - 1) / 3` in Q16.16 supplies the 7-bit index and 9-bit residue.
    let offset = normalized - (1u32 << 30);
    let fraction = (u64::from(offset) << 16) / (3u64 << 30);
    let index = ((fraction >> 9) & 0x7F) as usize;
    let residue = (fraction & 0x1FF) as i32;
    let seed = interpolate(&RSQRT_LUT, index, residue, RSQRT_ENDPOINT);
    let mantissa = (normalized >> 14) as i32;
    let y0_squared = q16_mul(seed, seed);
    let m_y0_squared = q16_mul(mantissa, y0_squared);
    let correction = (3i32 << 16).wrapping_sub(m_y0_squared);
    let refined = q16_mul(seed, correction) >> 1;
    if scale_shift >= 0 {
        refined >> scale_shift
    } else {
        refined << (-scale_shift)
    }
}

/// Single sample of `sin(pi/2 * u)` for `u` in Q16.16 over `[0,1]`, from the
/// 129-entry quarter-wave sine table. `u == 1.0` is the exact endpoint entry.
#[inline]
fn sine_quarter(u: i32) -> i32 {
    if u >= 0x1_0000 {
        return SINCOS_LUT[128] as i32;
    }
    let index = ((u >> 9) & 0x7F) as usize;
    let residue = u & 0x1FF;
    let current = SINCOS_LUT[index] as i32;
    let next = SINCOS_LUT[index + 1] as i32;
    let delta = next - current;
    current + ((delta * residue) >> 9)
}

/// `sin(pi/2 * (quadrant + fraction))` via the quarter-wave symmetries.
#[inline]
fn quadrant_sine(quadrant: i32, fraction: i32) -> i32 {
    match quadrant & 3 {
        0 => sine_quarter(fraction),
        1 => sine_quarter(0x1_0000 - fraction),
        2 => -sine_quarter(fraction),
        _ => -sine_quarter(0x1_0000 - fraction),
    }
}

/// SINCOS of `a` (radians, Q16.16): `t = a * 2/pi` via the shared mul pipe,
/// quadrant `q = floor(t) mod 4`, fraction `f = frac(t)`, then
/// `sin = quarter(q, f)` and `cos = quarter(q + 1, f)`.
///
/// Range reduction multiplies the Q16.16 input by the Q2.30 constant
/// [`TWO_OVER_PI_Q2_30`] with a full 64-bit `i64` product. A Q16.16 x Q2.30
/// product carries `16 + 30 = 46` fraction bits, so converting the product
/// back to Q16.16 drops 30 of them; the narrowing implemented here is an
/// arithmetic `>> 30` followed by a 32-bit wrap (`as i32`), i.e. product bits
/// `[61:30]` of the full 64-bit product.
///
/// The Stage-7 design text phrased this as "product bits `[45:14]`"; that is
/// the Q16.16 window for a **Q2.14** constant, i.e. 16 bits too low for Q2.30
/// (it drops `a`'s own 16 fraction bits twice). Taken literally it wraps for
/// `|a| > ~0.8` and measures 131071 LSB, so the constant's precision, the
/// `~2^-30` constant error and the Q16.16 result all force `[61:30]`.
pub(crate) fn sincos_q16(a: i32) -> (i32, i32) {
    let product = i64::from(a) * i64::from(TWO_OVER_PI_Q2_30);
    let t = ((product >> 30) & 0xFFFF_FFFF) as u32 as i32;
    let quadrant = (t >> 16) & 3;
    let fraction = t & 0xFFFF;
    (
        quadrant_sine(quadrant, fraction),
        quadrant_sine(quadrant + 1, fraction),
    )
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn lut_literals_match_f64_generation() {
        for i in 0..128 {
            let rcp = ((1.0 / (1.0 + i as f64 / 128.0)) * 65536.0).round() as u32;
            assert_eq!(RCP_LUT[i], rcp, "RCP_LUT[{i}]");
            let rsqrt = ((1.0 / (1.0 + 3.0 * i as f64 / 128.0).sqrt()) * 65536.0).round() as u32;
            assert_eq!(RSQRT_LUT[i], rsqrt, "RSQRT_LUT[{i}]");
        }
        for (i, &entry) in SINCOS_LUT.iter().enumerate() {
            let sine =
                ((std::f64::consts::PI / 2.0 * i as f64 / 128.0).sin() * 65536.0).round() as u32;
            assert_eq!(entry, sine, "SINCOS_LUT[{i}]");
        }
        assert_eq!(RCP_LUT[0], 0x1_0000);
        assert_eq!(RSQRT_LUT[0], 0x1_0000);
        assert_eq!(SINCOS_LUT[0], 0);
        assert_eq!(SINCOS_LUT[128], 0x1_0000);
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
            rsqrt_max = rsqrt_max.max((RSQRT_LUT[i + 1] as i32 - RSQRT_LUT[i] as i32).abs());
        }
        rsqrt_max = rsqrt_max.max((RSQRT_ENDPOINT - RSQRT_LUT[127] as i32).abs());
        let mut sincos_max = 0i32;
        for i in 0..128 {
            sincos_max = sincos_max.max(SINCOS_LUT[i + 1] as i32 - SINCOS_LUT[i] as i32);
        }
        eprintln!(
            "max interpolation |delta|: rcp={rcp_max}, rsqrt={rsqrt_max}, sincos={sincos_max}"
        );
        assert!(rcp_max <= i32::from(i16::MAX));
        assert!(rsqrt_max <= i32::from(i16::MAX));
        assert!(sincos_max <= i32::from(i16::MAX));
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

    #[test]
    fn sincos_accuracy() {
        // The full Q16.16 input domain: |a| up to 0x7FFF_0000 (32768 rad).
        let limit = 0x7FFF_0000i32;
        let mut max_error = 0i32;
        let mut worst = (0i32, 0i32, 0i32);
        let mut check = |a: i32| {
            let radians = f64::from(a) / 65536.0;
            let (sin, cos) = sincos_q16(a);
            let sin_exact = (radians.sin() * 65536.0).round() as i32;
            let cos_exact = (radians.cos() * 65536.0).round() as i32;
            let error = (sin - sin_exact).abs().max((cos - cos_exact).abs());
            if error > max_error {
                max_error = error;
                worst = (a, sin, cos);
            }
        };
        // Coarse sweep over the whole range: 1 ulp of output is 1 Q16.16 LSB.
        let mut a = -limit;
        while a <= limit {
            check(a);
            a += 2048;
        }
        // Fine sweep around every quadrant boundary k*pi/2, where sin/cos cross
        // zero and the interpolation kink meets the range-reduction error.
        let quarter = std::f64::consts::FRAC_PI_2 * 65536.0;
        let boundary_count = (f64::from(limit) / quarter).floor() as i32;
        for k in -boundary_count..=boundary_count {
            let center = (f64::from(k) * quarter).round() as i32;
            for offset in -8..=8 {
                let sample = center + offset;
                if sample >= -limit && sample <= limit {
                    check(sample);
                }
            }
        }
        eprintln!(
            "SINCOS: max {max_error} Q16.16 LSB at a={} (sin={}, cos={})",
            worst.0, worst.1, worst.2
        );
        assert!(
            max_error <= 4,
            "SINCOS absolute error {max_error} ulp > 4 at a={}",
            worst.0
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
}
