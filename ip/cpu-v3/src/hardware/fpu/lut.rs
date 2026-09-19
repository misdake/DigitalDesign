//! FPU v2 special-function reference model: the hidden-BSRAM LUT contents and
//! the pure-integer RCP/RSQRT/SINCOS datapath (design `fpu-design-v2` section
//! 9.4, Stage 7a, revised freeze 2026-09-19).
//!
//! This module is the single source of truth for the three LUTs. Later steps
//! emit the Verilog BSRAM `initial` blocks and mirror the same arrays in the
//! emulator from here; the arrays are checked into the source as literals so
//! that the emulator path never builds a table at run time. The literals were
//! generated offline with the f64 formulas below and are re-derived by a test
//! so a transcription slip cannot survive `cargo test`.
//!
//! ```text
//! RCP_LUT[i]    = round(1 / (1 + i/128)      * 2^16), i = 0..127
//! RSQRT_LUT[i]  = round(1 / sqrt(1 + 3i/256) * 2^16), i = 0..255
//! SINCOS_LUT[i] = round(sin(pi/2 * i/256)    * 2^16), i = 0..=256
//! ```
//!
//! The hidden region's two SDPB mirrors carry different halves, which doubles
//! the effective read-only capacity to 896 words. That split is a Verilog
//! concern; this module only supplies the plain tables:
//!
//! ```text
//! mirror A: RCP   @  64..191, SINCOS @ 192..448
//! mirror B: RSQRT @  64..319, reserve @ 320..511
//! ```
//!
//! The interpolation arithmetic is exactly the integer shape the RTL will use:
//! `delta = next - current` (signed), `current + ((delta * residue) >> shift)`
//! with an arithmetic shift (`>> 9` for the 128-entry RCP, `>> 8` for the
//! 256-entry RSQRT/SINCOS). `|delta|` is far below `i16::MAX` (see the delta
//! test), so the product is a plain 16x16-class signed multiply that cannot
//! overflow `i32`.
//!
//! This revised freeze drops the RSQRT Newton step and the shared multiplication
//! pipe entirely: the finer tables (error ~ h^2) meet the targets on their own,
//! and the 32x32-class range reduction is replaced by a decomposed two-term
//! constant that only forms 16x16-class partial products.
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

pub(crate) const RSQRT_LUT: [u32; 256] = [
    0x00010000, 0x0000FE83, 0x0000FD0D, 0x0000FB9E, 0x0000FA34, 0x0000F8D0, 0x0000F773, 0x0000F61B,
    0x0000F4C8, 0x0000F37B, 0x0000F234, 0x0000F0F1, 0x0000EFB3, 0x0000EE7A, 0x0000ED46, 0x0000EC17,
    0x0000EAEC, 0x0000E9C5, 0x0000E8A3, 0x0000E785, 0x0000E66B, 0x0000E555, 0x0000E443, 0x0000E335,
    0x0000E22A, 0x0000E123, 0x0000E020, 0x0000DF20, 0x0000DE23, 0x0000DD2A, 0x0000DC34, 0x0000DB41,
    0x0000DA51, 0x0000D965, 0x0000D87B, 0x0000D794, 0x0000D6B0, 0x0000D5CF, 0x0000D4F1, 0x0000D415,
    0x0000D33C, 0x0000D266, 0x0000D192, 0x0000D0C0, 0x0000CFF1, 0x0000CF25, 0x0000CE5A, 0x0000CD93,
    0x0000CCCD, 0x0000CC09, 0x0000CB48, 0x0000CA89, 0x0000C9CC, 0x0000C911, 0x0000C858, 0x0000C7A0,
    0x0000C6EB, 0x0000C638, 0x0000C587, 0x0000C4D7, 0x0000C42A, 0x0000C37E, 0x0000C2D4, 0x0000C22B,
    0x0000C185, 0x0000C0E0, 0x0000C03C, 0x0000BF9A, 0x0000BEFA, 0x0000BE5B, 0x0000BDBE, 0x0000BD23,
    0x0000BC89, 0x0000BBF0, 0x0000BB59, 0x0000BAC3, 0x0000BA2F, 0x0000B99C, 0x0000B90A, 0x0000B879,
    0x0000B7EA, 0x0000B75D, 0x0000B6D0, 0x0000B645, 0x0000B5BB, 0x0000B532, 0x0000B4AB, 0x0000B424,
    0x0000B39F, 0x0000B31B, 0x0000B298, 0x0000B216, 0x0000B196, 0x0000B116, 0x0000B097, 0x0000B01A,
    0x0000AF9D, 0x0000AF22, 0x0000AEA7, 0x0000AE2E, 0x0000ADB6, 0x0000AD3E, 0x0000ACC8, 0x0000AC52,
    0x0000ABDD, 0x0000AB6A, 0x0000AAF7, 0x0000AA85, 0x0000AA14, 0x0000A9A4, 0x0000A934, 0x0000A8C6,
    0x0000A858, 0x0000A7EB, 0x0000A77F, 0x0000A714, 0x0000A6AA, 0x0000A640, 0x0000A5D8, 0x0000A570,
    0x0000A508, 0x0000A4A2, 0x0000A43C, 0x0000A3D7, 0x0000A373, 0x0000A30F, 0x0000A2AC, 0x0000A24A,
    0x0000A1E9, 0x0000A188, 0x0000A128, 0x0000A0C8, 0x0000A069, 0x0000A00B, 0x00009FAE, 0x00009F51,
    0x00009EF5, 0x00009E99, 0x00009E3E, 0x00009DE4, 0x00009D8A, 0x00009D31, 0x00009CD8, 0x00009C80,
    0x00009C29, 0x00009BD2, 0x00009B7B, 0x00009B26, 0x00009AD0, 0x00009A7C, 0x00009A28, 0x000099D4,
    0x00009981, 0x0000992F, 0x000098DD, 0x0000988B, 0x0000983A, 0x000097EA, 0x0000979A, 0x0000974A,
    0x000096FB, 0x000096AC, 0x0000965E, 0x00009611, 0x000095C4, 0x00009577, 0x0000952B, 0x000094DF,
    0x00009494, 0x00009449, 0x000093FF, 0x000093B5, 0x0000936B, 0x00009322, 0x000092D9, 0x00009291,
    0x00009249, 0x00009202, 0x000091BB, 0x00009174, 0x0000912E, 0x000090E8, 0x000090A3, 0x0000905D,
    0x00009019, 0x00008FD4, 0x00008F91, 0x00008F4D, 0x00008F0A, 0x00008EC7, 0x00008E85, 0x00008E43,
    0x00008E01, 0x00008DBF, 0x00008D7E, 0x00008D3E, 0x00008CFD, 0x00008CBD, 0x00008C7E, 0x00008C3F,
    0x00008C00, 0x00008BC1, 0x00008B83, 0x00008B45, 0x00008B07, 0x00008ACA, 0x00008A8D, 0x00008A50,
    0x00008A13, 0x000089D7, 0x0000899C, 0x00008960, 0x00008925, 0x000088EA, 0x000088AF, 0x00008875,
    0x0000883B, 0x00008801, 0x000087C8, 0x0000878F, 0x00008756, 0x0000871D, 0x000086E5, 0x000086AD,
    0x00008675, 0x0000863E, 0x00008606, 0x000085CF, 0x00008599, 0x00008562, 0x0000852C, 0x000084F6,
    0x000084C1, 0x0000848B, 0x00008456, 0x00008421, 0x000083EC, 0x000083B8, 0x00008384, 0x00008350,
    0x0000831C, 0x000082E9, 0x000082B5, 0x00008282, 0x00008250, 0x0000821D, 0x000081EB, 0x000081B9,
    0x00008187, 0x00008155, 0x00008124, 0x000080F3, 0x000080C2, 0x00008091, 0x00008060, 0x00008030,
];

pub(crate) const SINCOS_LUT: [u32; 257] = [
    0x00000000, 0x00000192, 0x00000324, 0x000004B6, 0x00000648, 0x000007DA, 0x0000096C, 0x00000AFE,
    0x00000C90, 0x00000E21, 0x00000FB3, 0x00001144, 0x000012D5, 0x00001466, 0x000015F7, 0x00001787,
    0x00001918, 0x00001AA8, 0x00001C38, 0x00001DC7, 0x00001F56, 0x000020E5, 0x00002274, 0x00002402,
    0x00002590, 0x0000271E, 0x000028AB, 0x00002A38, 0x00002BC4, 0x00002D50, 0x00002EDC, 0x00003067,
    0x000031F1, 0x0000337C, 0x00003505, 0x0000368E, 0x00003817, 0x0000399F, 0x00003B27, 0x00003CAE,
    0x00003E34, 0x00003FBA, 0x0000413F, 0x000042C3, 0x00004447, 0x000045CB, 0x0000474D, 0x000048CF,
    0x00004A50, 0x00004BD1, 0x00004D50, 0x00004ECF, 0x0000504D, 0x000051CB, 0x00005348, 0x000054C3,
    0x0000563E, 0x000057B9, 0x00005932, 0x00005AAA, 0x00005C22, 0x00005D99, 0x00005F0F, 0x00006084,
    0x000061F8, 0x0000636B, 0x000064DD, 0x0000664E, 0x000067BE, 0x0000692D, 0x00006A9B, 0x00006C08,
    0x00006D74, 0x00006EDF, 0x00007049, 0x000071B2, 0x0000731A, 0x00007480, 0x000075E6, 0x0000774A,
    0x000078AD, 0x00007A10, 0x00007B70, 0x00007CD0, 0x00007E2F, 0x00007F8C, 0x000080E8, 0x00008243,
    0x0000839C, 0x000084F5, 0x0000864C, 0x000087A1, 0x000088F6, 0x00008A49, 0x00008B9A, 0x00008CEB,
    0x00008E3A, 0x00008F88, 0x000090D4, 0x0000921F, 0x00009368, 0x000094B0, 0x000095F7, 0x0000973C,
    0x00009880, 0x000099C2, 0x00009B03, 0x00009C42, 0x00009D80, 0x00009EBC, 0x00009FF7, 0x0000A130,
    0x0000A268, 0x0000A39E, 0x0000A4D2, 0x0000A605, 0x0000A736, 0x0000A866, 0x0000A994, 0x0000AAC1,
    0x0000ABEB, 0x0000AD14, 0x0000AE3C, 0x0000AF62, 0x0000B086, 0x0000B1A8, 0x0000B2C9, 0x0000B3E8,
    0x0000B505, 0x0000B620, 0x0000B73A, 0x0000B852, 0x0000B968, 0x0000BA7D, 0x0000BB8F, 0x0000BCA0,
    0x0000BDAF, 0x0000BEBC, 0x0000BFC7, 0x0000C0D1, 0x0000C1D8, 0x0000C2DE, 0x0000C3E2, 0x0000C4E4,
    0x0000C5E4, 0x0000C6E2, 0x0000C7DE, 0x0000C8D9, 0x0000C9D1, 0x0000CAC7, 0x0000CBBC, 0x0000CCAE,
    0x0000CD9F, 0x0000CE8E, 0x0000CF7A, 0x0000D065, 0x0000D14D, 0x0000D234, 0x0000D318, 0x0000D3FB,
    0x0000D4DB, 0x0000D5BA, 0x0000D696, 0x0000D770, 0x0000D848, 0x0000D91E, 0x0000D9F2, 0x0000DAC4,
    0x0000DB94, 0x0000DC62, 0x0000DD2D, 0x0000DDF7, 0x0000DEBE, 0x0000DF83, 0x0000E046, 0x0000E107,
    0x0000E1C6, 0x0000E282, 0x0000E33C, 0x0000E3F4, 0x0000E4AA, 0x0000E55E, 0x0000E610, 0x0000E6BF,
    0x0000E76C, 0x0000E817, 0x0000E8BF, 0x0000E966, 0x0000EA0A, 0x0000EAAB, 0x0000EB4B, 0x0000EBE8,
    0x0000EC83, 0x0000ED1C, 0x0000EDB3, 0x0000EE47, 0x0000EED9, 0x0000EF68, 0x0000EFF5, 0x0000F080,
    0x0000F109, 0x0000F18F, 0x0000F213, 0x0000F295, 0x0000F314, 0x0000F391, 0x0000F40C, 0x0000F484,
    0x0000F4FA, 0x0000F56E, 0x0000F5DF, 0x0000F64E, 0x0000F6BA, 0x0000F724, 0x0000F78C, 0x0000F7F1,
    0x0000F854, 0x0000F8B4, 0x0000F913, 0x0000F96E, 0x0000F9C8, 0x0000FA1F, 0x0000FA73, 0x0000FAC5,
    0x0000FB15, 0x0000FB62, 0x0000FBAD, 0x0000FBF5, 0x0000FC3B, 0x0000FC7F, 0x0000FCC0, 0x0000FCFE,
    0x0000FD3B, 0x0000FD74, 0x0000FDAC, 0x0000FDE1, 0x0000FE13, 0x0000FE43, 0x0000FE71, 0x0000FE9C,
    0x0000FEC4, 0x0000FEEB, 0x0000FF0E, 0x0000FF30, 0x0000FF4E, 0x0000FF6B, 0x0000FF85, 0x0000FF9C,
    0x0000FFB1, 0x0000FFC4, 0x0000FFD4, 0x0000FFE1, 0x0000FFEC, 0x0000FFF5, 0x0000FFFB, 0x0000FFFF,
    0x00010000,
];

/// RCP endpoint: `1/m` at `m = 2`, i.e. index 127's `next`. The 128-entry table
/// does not store it, so the datapath supplies the exact constant instead of
/// reading out of bounds.
const RCP_ENDPOINT: i32 = 0x8000;

/// RSQRT endpoint: `1/sqrt(m)` at `m = 4`, i.e. index 255's `next`. The
/// 256-entry table covers `m = 1 + 3i/256` up to `3.98828125`, so the exact
/// `0.5` is supplied as the final interpolation endpoint.
const RSQRT_ENDPOINT: i32 = 0x8000;

/// `round(2/pi * 2^16)`, the leading Q16.16 term of the two-term SINCOS range
/// reduction (revised freeze, design section 9.4).
const TWO_OVER_PI_C0: i32 = 41_722;

/// `round((2/pi - C0 * 2^-16) * 2^32)`, the residual term. It is negative
/// because `C0` rounded up; `|C1| < 2^16` keeps every partial product 16x16
/// class. The represented constant `C0*2^-16 + C1*2^-32` differs from `2/pi` by
/// ~7.1e-11, i.e. `< 0.16` LSB of the reduced argument `t` over the full i32
/// input range.
const TWO_OVER_PI_C1: i32 = -31_890;

/// Linear interpolation shared by RCP (128 entries, 9-bit residue, `>> 9`) and
/// RSQRT (256 entries, 8-bit residue, `>> 8`). The stored table stops one entry
/// short of the interval's right edge, so the final entry's `next` comes from
/// `endpoint` instead of reading out of bounds.
#[inline]
fn interpolate(lut: &[u32], index: usize, residue: i32, shift: u32, endpoint: i32) -> i32 {
    let current = lut[index] as i32;
    let next = if index + 1 < lut.len() {
        lut[index + 1] as i32
    } else {
        endpoint
    };
    let delta = next - current;
    current + ((delta * residue) >> shift)
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
    let interpolated = interpolate(&RCP_LUT, index, residue, 9, RCP_ENDPOINT);
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

/// RSQRT approximation of `x` in Q16.16 (revised freeze, design section 9.4):
/// `x <= 0 -> 0`, otherwise normalize `x = m * 2^(2k)` with `m in [1,4)`
/// (CLZ, even exponent), look up the 256-entry table uniform in `m` (step
/// `3/256`), linearly interpolate with an 8-bit residue, and rescale by `2^-k`.
/// There is **no** Newton step: the finer table already meets the 2 ulp target.
///
/// The table is indexed by `u = (m - 1) / 3` in Q16.16. The top 8 bits of `u`
/// select the entry, the low 8 bits are the interpolation residue, and
/// `delta * residue >> 8` is the arithmetic-shifted correction. Index 255's
/// `next` is the exact endpoint `1/sqrt(4) = 0.5`.
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
    // `normalized = m * 2^30`; the table is uniform over `m = 1 + 3i/256`, so
    // `u = (m - 1) / 3` in Q16.16 supplies the 8-bit index and 8-bit residue.
    let offset = normalized - (1u32 << 30);
    let fraction = (u64::from(offset) << 16) / (3u64 << 30);
    let index = ((fraction >> 8) & 0xFF) as usize;
    let residue = (fraction & 0xFF) as i32;
    let interpolated = interpolate(&RSQRT_LUT, index, residue, 8, RSQRT_ENDPOINT);
    if scale_shift >= 0 {
        interpolated >> scale_shift
    } else {
        interpolated << (-scale_shift)
    }
}

/// Single sample of `sin(pi/2 * u)` for `u` in Q16.16 over `[0,1]`, from the
/// 257-entry quarter-wave sine table. `u == 1.0` is the exact endpoint entry,
/// which the `0x1_0000 - f` quadrant reflection needs when `f == 0`.
#[inline]
fn sine_quarter(u: i32) -> i32 {
    if u >= 0x1_0000 {
        return SINCOS_LUT[256] as i32;
    }
    let index = ((u >> 8) & 0xFF) as usize;
    let residue = u & 0xFF;
    let current = SINCOS_LUT[index] as i32;
    let next = SINCOS_LUT[index + 1] as i32;
    let delta = next - current;
    current + ((delta * residue) >> 8)
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

/// SINCOS of `a` (radians, Q16.16), returning `(sin, cos)`.
///
/// There is no input clamp: any `i32` is evaluated at its actual angle. Range
/// reduction evaluates `t = floor(a * (2/pi))` in Q16.16 with the two-term
/// constant `2/pi ~= C0*2^-16 + C1*2^-32` ([`TWO_OVER_PI_C0`],
/// [`TWO_OVER_PI_C1`]). Splitting `a = a_hi*2^16 + a_lo` (`a_hi = a >> 16`
/// arithmetic in `[-32768, 32767]`, `a_lo` the unsigned low 16 bits in
/// `[0, 65535]`) keeps the whole computation inside `i64` for the full range:
///
/// ```text
/// p0 = a_lo * C0            (unsigned, |p0| <= 2.73e9)
/// p1 = a_lo * C1            (signed,   |p1| <= 2.09e9)
/// h0 = a_hi * C0            (signed,   |h0| <= 1.37e9)
/// h1 = a_hi * C1            (signed,   |h1| <= 1.05e9)
/// t  = h0 + (((p0 + h1) << 16) + p1) >> 32
/// ```
///
/// `|p0 + h1| <= 3.78e9`, so the shift peaks near `2^47.8`; the reduced value
/// itself satisfies `|t| <= ceil(2^31 * 2/pi) + 1 < 2^30.4 < i32::MAX`, which
/// the `debug_assert!` below confirms on every debug/test build (notably for
/// `a = i32::MIN`/`i32::MAX`).
///
/// The numerator `(a*C0 << 16) + a*C1` expands to
/// `(h0 << 32) + ((p0 + h1) << 16) + p1`, and the final `>> 32` is an
/// arithmetic shift, so the formula is `floor(a * represented_constant)`: the
/// only truncation is the final shift (< 1 LSB) plus the constant error
/// (< 0.16 LSB over the full range). Negative `a` works because `a_hi`, `h0`,
/// `h1` and `p1` are signed, `p0` is unsigned, and the arithmetic shift floors.
///
/// The 4 LSB accuracy guarantee holds for `|a| <= 45` rad; outside it the
/// result is still the actual `sin`/`cos` (the argument never wraps or
/// saturates), but the error grows slowly with `|a|` and is only bounded by the
/// measured full-range test.
///
/// `q = (t >> 16) & 3`, `f = t & 0xFFFF`; `sin` uses the quarter-wave table at
/// `(q, f)` and `cos` at `(q + 1, f)` ([`quadrant_sine`]).
pub(crate) fn sincos_q16(a: i32) -> (i32, i32) {
    let a_hi = i64::from(a >> 16);
    let a_lo = i64::from(a & 0xFFFF);
    let c0 = i64::from(TWO_OVER_PI_C0);
    let c1 = i64::from(TWO_OVER_PI_C1);
    let h0 = a_hi * c0;
    let h1 = a_hi * c1;
    let p0 = a_lo * c0;
    let p1 = a_lo * c1;
    let t_wide = h0 + ((((p0 + h1) << 16) + p1) >> 32);
    debug_assert!(
        t_wide >= i64::from(i32::MIN) && t_wide <= i64::from(i32::MAX),
        "SINCOS range reduction t={t_wide} escaped i32 for a={a}"
    );
    let t = t_wide as i32;
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
        for (i, &entry) in RSQRT_LUT.iter().enumerate() {
            let rsqrt = ((1.0 / (1.0 + 3.0 * i as f64 / 256.0).sqrt()) * 65536.0).round() as u32;
            assert_eq!(entry, rsqrt, "RSQRT_LUT[{i}]");
        }
        for (i, &entry) in SINCOS_LUT.iter().enumerate() {
            let sine =
                ((std::f64::consts::PI / 2.0 * i as f64 / 256.0).sin() * 65536.0).round() as u32;
            assert_eq!(entry, sine, "SINCOS_LUT[{i}]");
        }
        assert_eq!(RCP_LUT[0], 0x1_0000);
        assert_eq!(RSQRT_LUT[0], 0x1_0000);
        assert_eq!(SINCOS_LUT[0], 0);
        assert_eq!(SINCOS_LUT[256], 0x1_0000);
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
        for i in 0..255 {
            rsqrt_max = rsqrt_max.max((RSQRT_LUT[i + 1] as i32 - RSQRT_LUT[i] as i32).abs());
        }
        rsqrt_max = rsqrt_max.max((RSQRT_ENDPOINT - RSQRT_LUT[255] as i32).abs());
        let mut sincos_max = 0i32;
        for i in 0..256 {
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
        // Accuracy target domain: |a| <= 45 rad (Q16.16, dense).
        let limit = 45 << 16;
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
        for a in -limit..=limit {
            check(a);
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

    /// Inner-range (|a| <= 2 pi) SINCOS error bound (Q16.16 LSB), the range
    /// that matters for rotation use. Measured value pinned by the test print.
    const SINCOS_INNER_RANGE_ULP: i32 = 4;

    #[test]
    fn sincos_inner_range_accuracy() {
        let limit = (2.0 * std::f64::consts::PI * 65536.0).round() as i32;
        let mut max_error = 0i32;
        let mut worst = (0i32, 0i32, 0i32);
        for a in -limit..=limit {
            let (error, sin, cos) = sincos_abs_error(a);
            if error > max_error {
                max_error = error;
                worst = (a, sin, cos);
            }
        }
        eprintln!(
            "SINCOS inner range (|a|<=2pi): max {max_error} Q16.16 LSB at a={} (sin={}, cos={})",
            worst.0, worst.1, worst.2
        );
        assert!(max_error <= SINCOS_INNER_RANGE_ULP);
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
            dense_worst.0,
            dense_worst.1,
            dense_worst.2,
            range_worst.0,
            range_worst.1,
            range_worst.2
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
}
