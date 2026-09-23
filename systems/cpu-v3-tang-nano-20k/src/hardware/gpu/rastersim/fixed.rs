//! Fixed-point infrastructure for the standalone rasterizer functional sim.
//!
//! All arithmetic is integer; no `f32`/`f64` is used anywhere in this module.
//! Formats (raw integer in parentheses):
//!
//! * `Q16.16` (`i32`): clip-space coordinates x/y/z/w.
//! * NDC (`i32`, `s2.29`, range ±4): clip-space times reciprocal. The guard
//!   band is ±512 px = ±2.56 NDC units, which needs three integer bits
//!   including the sign; 30 fraction bits in 32 bits would cap at ±2.
//! * `s12.4` (`i16`): snapped screen coordinates, saturated to ±511.9375 px.
//! * `U0.18` (`u32`): high-precision depth, carried through interpolation.
//! * `U0.16` (`u16`): final quantized depth for storage/comparison.
//!
//! `mul18`/`mul36` instrument every call site with operand range statistics
//! instead of enforcing operand widths; the report decides where a 36-bit
//! multiplier is really needed.

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

/// Saturating add for `s12.4` raw values.
pub fn sat_add_s12_4(a: i16, b: i16) -> i16 {
    a.saturating_add(b)
}

/// Saturating subtract for `s12.4` raw values.
pub fn sat_sub_s12_4(a: i16, b: i16) -> i16 {
    a.saturating_sub(b)
}

/// Saturating add for `Q16.16` raw values.
pub fn sat_add_q16(a: i32, b: i32) -> i32 {
    a.saturating_add(b)
}

/// Saturating subtract for `Q16.16` raw values.
pub fn sat_sub_q16(a: i32, b: i32) -> i32 {
    a.saturating_sub(b)
}

/// Largest magnitude representable in an 18-bit signed multiplier operand.
pub const MUL18_OPERAND_LIMIT: i64 = 1 << 17;

/// Per-call-site multiplier operand statistics.
#[derive(Clone, Debug, Default)]
pub struct MulStat {
    pub calls: u64,
    pub max_abs_a: u64,
    pub max_abs_b: u64,
    pub max_abs_product: u64,
    /// Calls where either operand exceeded the 18-bit signed range.
    pub overrange_calls: u64,
}

fn mul_stats() -> &'static Mutex<BTreeMap<&'static str, MulStat>> {
    static STATS: OnceLock<Mutex<BTreeMap<&'static str, MulStat>>> = OnceLock::new();
    STATS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn record_mul(label: &'static str, a: i64, b: i64, product: i64) {
    let mut stats = mul_stats().lock().unwrap();
    let entry = stats.entry(label).or_default();
    entry.calls += 1;
    entry.max_abs_a = entry.max_abs_a.max(a.unsigned_abs());
    entry.max_abs_b = entry.max_abs_b.max(b.unsigned_abs());
    entry.max_abs_product = entry.max_abs_product.max(product.unsigned_abs());
    if a.abs() > MUL18_OPERAND_LIMIT || b.abs() > MUL18_OPERAND_LIMIT {
        entry.overrange_calls += 1;
    }
}

/// Instrumented 18x18-bit multiply. Never panics; operands that exceed the
/// 18-bit signed range are counted in the call-site statistics.
pub fn mul18(label: &'static str, a: i64, b: i64) -> i64 {
    let product = a * b;
    record_mul(label, a, b, product);
    product
}

/// Instrumented wide multiply (stands in for 36x18/36x36 hardware). Same
/// statistics as [`mul18`]; use it where 18-bit operands provably do not fit.
pub fn mul36(label: &'static str, a: i64, b: i64) -> i64 {
    let product = a * b;
    record_mul(label, a, b, product);
    product
}

/// Snapshot of every recorded call site, sorted by label.
pub fn mul_stats_snapshot() -> BTreeMap<&'static str, MulStat> {
    mul_stats().lock().unwrap().clone()
}

/// Clears all multiplier statistics.
pub fn mul_stats_reset() {
    mul_stats().lock().unwrap().clear();
}

/// Unsigned 18x18 multiply with round-to-nearest on the dropped 18 low bits.
/// Both operands must fit 18 bits unsigned; the result fits 18 bits when the
/// mathematical product is below 2^36/2^18 = 2^18. Instrumented under the
/// `rcp.newton` label.
fn mul_u18_round(a: u32, b: u32) -> u32 {
    debug_assert!(a < (1 << 18) && b < (1 << 18));
    let product = u64::from(a) * u64::from(b);
    record_mul("rcp.newton", i64::from(a), i64::from(b), product as i64);
    ((product + (1 << 17)) >> 18) as u32
}

/// 64-entry reciprocal seed table for the normalized fraction range
/// `[0.5, 1)`, values in `U1.18` (true reciprocal of the bin midpoint,
/// divided by two, scaled by 2^18). Computed with integer division only.
const RCP_LUT: [u32; 64] = {
    let mut table = [0u32; 64];
    let mut i = 0usize;
    while i < 64 {
        // Bin midpoint f_mid = (129 + 2*i) / 256; entry = round(2^17 / f_mid).
        let denominator = (129 + 2 * i) as u64;
        table[i] = ((((1u64 << 17) * 256) + denominator / 2) / denominator) as u32;
        i += 1;
    }
    table
};

/// Base value of the piecewise-linear reciprocal table with `n` segments over
/// `[0.5, 1)`: `base[i] = round(2^18 * n / (n + i))`, i.e. `rcp(f_i)/2` in
/// `U1.18` at the segment's left endpoint `f_i = 0.5 + i/(2n)`. The slope of
/// segment `i` is `base[i+1] - base[i]` (signed, at most ~12 bits for
/// `n <= 256`); it needs no separate table, and its quantization error is
/// already accounted for by the quantized endpoints.
const fn rcp_lerp_base(n: usize, i: usize) -> u32 {
    let n = n as u64;
    let denominator = n + i as u64;
    ((((1u64 << 18) * n) + denominator / 2) / denominator) as u32
}

const RCP_LERP128_BASE: [u32; 129] = {
    let mut table = [0u32; 129];
    let mut i = 0usize;
    while i <= 128 {
        table[i] = rcp_lerp_base(128, i);
        i += 1;
    }
    table
};

const RCP_LERP256_BASE: [u32; 257] = {
    let mut table = [0u32; 257];
    let mut i = 0usize;
    while i <= 256 {
        table[i] = rcp_lerp_base(256, i);
        i += 1;
    }
    table
};

/// Selects the reciprocal implementation, for side-by-side comparison.
/// Default is the chosen shipping configuration, [`RcpMode::Lerp256`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RcpMode {
    /// 64-entry midpoint LUT + two Newton steps (previous implementation,
    /// two 18x18 multiplies per step, ~15 ppm measured).
    Newton,
    /// 128-segment LUT + slope lerp (~2^-15 = 30 ppm theory).
    Lerp128,
    /// 256-segment LUT + slope lerp (~2^-17 = 8 ppm theory).
    Lerp256,
}

thread_local! {
    static RCP_MODE: std::cell::Cell<RcpMode> = const { std::cell::Cell::new(RcpMode::Lerp256) };
}

/// Switches the rcp implementation for the current thread (test tooling).
pub fn set_rcp_mode(mode: RcpMode) {
    RCP_MODE.with(|m| m.set(mode));
}

/// The rcp implementation active on the current thread.
pub fn rcp_mode() -> RcpMode {
    RCP_MODE.with(|m| m.get())
}

/// Newton-based reciprocal (see [`RcpMode::Newton`]).
fn rcp_newton(x: u32) -> (u32, i32) {
    let lz = x.leading_zeros();
    let normalized = x << lz;
    let index = ((normalized >> 25) & 0x3f) as usize;
    let mut y = RCP_LUT[index];
    // Top 18 bits of f (f < 1 so bit 31 set means f18 in [2^17, 2^18)).
    let f18 = normalized >> 14;
    // Newton step on y = 1/(2f) scale: p = f*y, d = 1 - p, y' = 2*y*d.
    for _ in 0..2 {
        let p = mul_u18_round(f18, y);
        let d = (1u32 << 18) - p.min(1 << 18);
        y = (2 * mul_u18_round(y, d)).min((1 << 18) - 1);
    }
    // 1/x = 2^(lz-32) * rcp(f), rcp(f) = y / 2^17.
    (y, 49 - lz as i32)
}

/// LUT + slope lerp reciprocal with `2^k` segments over `[0.5, 1)`.
/// One small multiply: a signed ~12-bit slope times a 12-bit segment offset.
fn rcp_lerp(x: u32, base: &[u32], k: u32) -> (u32, i32) {
    let lz = x.leading_zeros();
    let normalized = x << lz;
    // f - 0.5 in units of 2^-32 (top bit was the leading one).
    let frac = normalized - 0x8000_0000;
    let index = (frac >> (31 - k)) as usize;
    // Top 12 bits of the in-segment fraction (segment width 2^(31-k)).
    let off12 = ((frac >> (31 - k - 12)) & 0xfff) as i32;
    let base_lo = base[index] as i32;
    let slope = base[index + 1] as i32 - base_lo;
    record_mul(
        "rcp.lerp",
        i64::from(slope),
        i64::from(off12),
        i64::from(slope) * i64::from(off12),
    );
    let y = base_lo + ((slope * off12 + 2048) >> 12);
    (y.clamp(1 << 17, (1 << 18) - 1) as u32, 49 - lz as i32)
}

/// Low-level reciprocal of a nonzero `u32`. Returns `(mag, shift)` such that
/// `1/x ≈ mag * 2^-shift` with `mag` in `[2^17, 2^18]`. Implementation
/// selected by [`rcp_mode`] (thread-local, default LUT+lerp 256).
pub fn rcp_u32(x: u32) -> (u32, i32) {
    assert!(x != 0, "rcp_u32: zero input");
    match rcp_mode() {
        RcpMode::Newton => rcp_newton(x),
        RcpMode::Lerp128 => rcp_lerp(x, &RCP_LERP128_BASE, 7),
        RcpMode::Lerp256 => rcp_lerp(x, &RCP_LERP256_BASE, 8),
    }
}

/// Reciprocal of a positive `Q16.16` value (raw `u32`), returning
/// `(mag, shift)` such that `1/w = mag * 2^(16 - shift)` with `mag` in
/// `[2^17, 2^18]`. Inputs below `2^-6` (raw 1024) saturate to `2^-6` first,
/// matching the documented `w_min` input restriction. Keeping the normalized
/// magnitude plus exponent (instead of a fixed-scale output) preserves full
/// precision across the whole `w` range.
pub fn rcp_q16(w_raw: u32) -> (u32, i32) {
    rcp_u32(w_raw.max(1024))
}

/// Exact `(mag, shift)` oracle check value: `1/w = mag * 2^(16-shift)` with
/// `w_true = w_raw * 2^-16` implies `mag * w_raw == 2^shift`. Test-only.
pub fn rcp_q16_check(w_raw: u32) -> (u128, u128) {
    let w_raw = w_raw.max(1024);
    let (mag, shift) = rcp_q16(w_raw);
    let product = u128::from(mag) * u128::from(w_raw);
    (product, 1u128 << shift)
}

/// Converts a `Q16.16` raw value from a rational `num/den` without floats.
pub fn q16_from_rational(num: i64, den: i64) -> i32 {
    assert!(den != 0, "q16_from_rational: zero denominator");
    ((num << 16) / den) as i32
}

/// Rounds an `U0.18` depth value down to `U0.16`.
pub fn depth18_to_16(depth: u32) -> u16 {
    ((depth.min(0x3ffff) + 2) >> 2).min(0xffff) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rcp error distribution over a dense sweep for every implementation,
    /// comparing the normalized `(mag, shift)` output against exact integer
    /// arithmetic.
    #[test]
    fn rcp_matches_exact_division() {
        for (mode, bound_ppm) in [
            (RcpMode::Newton, 16u64),
            (RcpMode::Lerp128, 40),
            (RcpMode::Lerp256, 16),
        ] {
            set_rcp_mode(mode);
            let mut max_rel_ppm = 0u64;
            let mut total_rel_ppm = 0u64;
            let mut count = 0u64;
            // Sweep raw w from 2^-6 upward, dense near the small end, up to
            // the Q16.16 positive ceiling of 32768 - 2^-16.
            let mut steps = 0u64;
            let mut w_raw = 1024u32;
            while w_raw < 0x7fff_0000 && steps < 2_000_000 {
                let (product, target) = rcp_q16_check(w_raw);
                let error = product.abs_diff(target);
                let rel_ppm = (error * 1_000_000 / target) as u64;
                max_rel_ppm = max_rel_ppm.max(rel_ppm);
                total_rel_ppm += rel_ppm;
                count += 1;
                // Log-ish sweep: small steps near 2^-6, larger steps for big w.
                let step = (w_raw / 512).max(1);
                w_raw = w_raw.saturating_add(step);
                steps += 1;
            }
            assert!(steps < 2_000_000, "sweep exceeded its step limit");
            let avg_rel_ppm = total_rel_ppm / count.max(1);
            println!(
                "rcp sweep {mode:?}: {count} samples, max rel error {max_rel_ppm} ppm, avg {avg_rel_ppm} ppm"
            );
            assert!(
                max_rel_ppm < bound_ppm,
                "{mode:?} relative error {max_rel_ppm} ppm exceeds {bound_ppm} ppm bound"
            );
        }
        set_rcp_mode(RcpMode::Lerp256);
    }

    #[test]
    fn rcp_lerp_tables_are_monotone_decreasing() {
        for i in 1..=128 {
            assert!(RCP_LERP128_BASE[i] < RCP_LERP128_BASE[i - 1]);
        }
        for i in 1..=256 {
            assert!(RCP_LERP256_BASE[i] < RCP_LERP256_BASE[i - 1]);
        }
        // Slope magnitude bound: |base[i+1]-base[i]| must fit a signed
        // 12-bit operand (2048) for the small lerp multiplier.
        for i in 0..256 {
            let slope = RCP_LERP256_BASE[i + 1] as i32 - RCP_LERP256_BASE[i] as i32;
            assert!(slope.abs() <= 2048, "slope {slope} at segment {i}");
        }
        for i in 0..128 {
            let slope = RCP_LERP128_BASE[i + 1] as i32 - RCP_LERP128_BASE[i] as i32;
            assert!(slope.abs() <= 2048, "slope {slope} at segment {i}");
        }
    }

    #[test]
    fn rcp_lut_is_monotone_decreasing() {
        for i in 1..64 {
            assert!(RCP_LUT[i] < RCP_LUT[i - 1], "LUT not monotone at {i}");
        }
        // f in [0.5, 1) maps to rcp/2 in (0.5, 1] scaled by 2^18.
        assert!(RCP_LUT[0] <= 1 << 18);
        assert!(RCP_LUT[63] > 1 << 17);
    }

    #[test]
    fn q16_from_rational_rounds_toward_zero() {
        assert_eq!(q16_from_rational(1, 2), 0x8000);
        assert_eq!(q16_from_rational(-1, 2), -0x8000);
        assert_eq!(q16_from_rational(3, 2), 0x1_8000);
    }

    #[test]
    fn depth_quantization_rounds() {
        assert_eq!(depth18_to_16(0), 0);
        assert_eq!(depth18_to_16(0x3ffff), 0xffff);
        assert_eq!(depth18_to_16(4), 1);
    }

    #[test]
    fn mul_stats_record_ranges() {
        mul_stats_reset();
        let label = "test::mul_stats_record_ranges";
        assert_eq!(mul18(label, 3, 4), 12);
        assert_eq!(mul18(label, -1 << 20, 2), -2 << 20);
        let stats = mul_stats_snapshot();
        let entry = &stats[label];
        assert_eq!(entry.calls, 2);
        assert_eq!(entry.max_abs_a, 1 << 20);
        assert_eq!(entry.overrange_calls, 1);
        mul_stats_reset();
    }
}
