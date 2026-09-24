//! Fixed-point formats for the standalone rasterizer functional sim.
//!
//! Layer 1 of the "device-as-type" discipline (see `devices.rs` for layer 2):
//! every fixed-point format is an instance of the generic [`Fx`] type, and
//! cross-format math requires explicitly named conversions — no implicit
//! `i32`/`i64` mixing on the hardware path.
//!
//! All arithmetic is integer; no `f32`/`f64` is used anywhere in this module.
//!
//! ## Overflow/saturation discipline (mirrors the intended hardware)
//!
//! Every arithmetic site belongs to exactly one of three classes:
//!
//! 1. **Saturating, like the hardware** (`Saturator` in `devices.rs`):
//!    viewport X/Y snap to ±511.9375 px, depth to its format range, `w`
//!    viewport snap to the s12.4 format range, depth to its format range.
//!    Every saturation is counted per call site. (Illegal `w` is rejected
//!    upstream by `clip::clip_vertex_valid`, never clamped.)
//! 2. **Non-saturating devices** (`Adder`/`Multiplier*` in `devices.rs`):
//!    wrap exactly like the hardware, and the overflow is counted per call
//!    site; tests assert the counts stay zero (a nonzero count means the
//!    design overflowed a real device).
//! 3. **Format invariants** (checked conversions like
//!    [`S2_29::from_product`]): values that must be in range by construction
//!    (post-clip contracts). A violation is a design bug and panics loudly.
//!
//! Test-only exact oracles (`ndc_exact`, i64/i128 helpers) are explicitly
//! named and never on the hardware path.

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

// ---------------------------------------------------------------------------
// Format layer: generic fixed-point type.

/// Generic fixed-point value: `INT` integer bits (excluding the sign bit for
/// signed formats), `FRAC` fractional bits, raw value held in `i64`.
///
/// Same-format `Add`/`Sub`/`Neg` panic on range overflow: those are class-3
/// format invariants, values that must stay in range by construction. Actual
/// hardware adders with wrap semantics live in `devices.rs`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Fx<const INT: u32, const FRAC: u32, const SIGNED: bool> {
    raw: i64,
}

impl<const INT: u32, const FRAC: u32, const SIGNED: bool> Fx<INT, FRAC, SIGNED> {
    /// Signed formats hold raw values from `-RAW_LIMIT` through
    /// `RAW_LIMIT - 1` (scaled by 2^FRAC).
    pub const RAW_LIMIT: i64 = 1i64 << (INT + FRAC);

    /// Wraps a raw value after checking the format range (class 3).
    pub fn from_raw(raw: i64) -> Self {
        let in_range = if SIGNED {
            raw >= -Self::RAW_LIMIT && raw < Self::RAW_LIMIT
        } else {
            (0..Self::RAW_LIMIT).contains(&raw)
        };
        assert!(in_range, "Fx<{INT},{FRAC},{SIGNED}> raw {raw} out of range");
        Self { raw }
    }

    /// Unchecked wrap for constants already known in range.
    pub const fn from_raw_const(raw: i64) -> Self {
        Self { raw }
    }

    /// Unwraps the raw value; explicit by design. Legitimate uses: device
    /// port arguments, bit-pattern extraction (sign, CLZ input, LUT index),
    /// and test-only oracles. Business math should use the typed operations.
    pub const fn raw(self) -> i64 {
        self.raw
    }

    /// The zero value.
    pub const fn zero() -> Self {
        Self { raw: 0 }
    }

    /// Left shift (free in hardware — wiring); range-checked (class 3).
    /// Named `shl_bits` to stay distinct from `std::ops::Shl`.
    pub fn shl_bits(self, n: u32) -> Self {
        Self::from_raw(self.raw << n)
    }

    /// Sign test (bit-pattern extraction).
    pub fn is_negative(self) -> bool {
        self.raw < 0
    }

    /// Absolute value (class 3: panics at the format's negative extreme).
    pub fn abs(self) -> Self {
        Self::from_raw(self.raw.abs())
    }

    /// Clamps negative values to zero. Sign clamp guaranteed by the clip
    /// contract (`z >= 0`); absorbs rcp/lerp epsilon only, so it is not a
    /// counted saturator site.
    pub fn clamp_min_zero(self) -> Self {
        if self.is_negative() {
            Self::zero()
        } else {
            self
        }
    }
}

/// Widening conversions and clip-distance helpers.
impl Fx<23, 16, true> {
    /// Widens a clip-space coordinate into a clip-plane distance (same
    /// Q16.16 scale, wider headroom). Exact, never fails.
    pub fn widen_q16(v: Q16) -> Self {
        Self::from_raw_const(v.raw())
    }

    /// Class-3 constructor for clip-distance arithmetic results (Adder40
    /// outputs, lerp steps); the headroom makes the range check unreachable.
    pub fn from_product(raw: i64) -> Self {
        Self::from_raw(raw)
    }
}

impl Q16 {
    /// Narrows a clip-plane distance back to a clip coordinate. Class 3:
    /// interpolated points lie between the endpoints, so the range check
    /// never fires.
    pub fn narrow_from_clip_dist(d: Fx<23, 16, true>) -> Self {
        Self::from_raw(d.raw())
    }

    /// Class-3 constructor for a device product already known to carry the
    /// Q16.16 scale and to be in range by construction (e.g. the viewport
    /// multiplier output).
    pub fn from_product(raw: i64) -> Self {
        Self::from_raw(raw)
    }
}

impl E40 {
    /// Functional-reference shortcut: exact division of a wide numerator by
    /// this (positive, nonzero) doubled area. Hardware folds the reciprocal
    /// into the interpolation instead of dividing per pixel.
    pub fn div_wide(self, numerator: i64) -> i64 {
        numerator / self.raw().max(1)
    }
}

impl S12_4 {
    /// Named conversion: pixel index of the pixel row/column containing this
    /// coordinate (floor divide by 16, a shift in hardware).
    pub fn pixel_floor(self) -> i32 {
        self.raw.div_euclid(16) as i32
    }

    /// The pixel center of on-screen pixel `p` as a coordinate (class 3:
    /// in range for every on-screen pixel).
    pub fn pixel_center(p: i32) -> Self {
        Self::from_raw(i64::from(p) * 16 + 8)
    }
}

impl S13_4 {
    /// Class 3: edge coefficients and coordinate deltas are differences of
    /// saturated s12.4 coordinates, |value| <= 16382; exceeding the s13.4
    /// range is a bug.
    pub fn from_diff(value: i64) -> Self {
        Self::from_raw(value)
    }
}

impl P36 {
    /// Class-3 constructor for one 18x18 multiplier product (2^8 scale).
    pub fn from_product(raw: i64) -> Self {
        Self::from_raw(raw)
    }
}

impl U0_16 {
    /// Port-level output: the raw 16-bit code for the depth buffer.
    pub fn to_bits(self) -> u16 {
        self.raw as u16
    }
}

impl<const INT: u32, const FRAC: u32> std::ops::Add for Fx<INT, FRAC, true> {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self::from_raw(self.raw + other.raw)
    }
}

impl<const INT: u32, const FRAC: u32> std::ops::Sub for Fx<INT, FRAC, true> {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        Self::from_raw(self.raw - other.raw)
    }
}

impl<const INT: u32, const FRAC: u32> std::ops::Neg for Fx<INT, FRAC, true> {
    type Output = Self;
    fn neg(self) -> Self {
        Self::from_raw(-self.raw)
    }
}

impl<const INT: u32, const FRAC: u32, const SIGNED: bool> std::fmt::Display
    for Fx<INT, FRAC, SIGNED>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Fx{INT}.{FRAC}({})", self.raw)
    }
}

/// Clip-space coordinate, Q16.16 (1 sign + 15 int + 16 frac).
pub type Q16 = Fx<15, 16, true>;
/// NDC coordinate, s2.29 (1 sign + 2 int + 29 frac = 32 bits, range ±4).
/// The ±512 px guard band is ±2.56 NDC units, which needs three integer bits
/// including the sign.
pub type S2_29 = Fx<2, 29, true>;
/// Snapped screen coordinate, s12.4 stored in 16 bits (1 + 11 + 4, format
/// range ±2047.9375 px; the saturator clamps to ±511.9375 px).
pub type S12_4 = Fx<11, 4, true>;
/// Edge-function coefficient, s13.4 (1 + 12 + 4 = 17 bits, ±4095.9375).
pub type S13_4 = Fx<12, 4, true>;
/// High-precision depth, U0.18.
pub type U0_18 = Fx<0, 18, false>;
/// Quantized depth, U0.16.
pub type U0_16 = Fx<0, 16, false>;
/// Reciprocal view distance `1/w`, U4.28 (per metre).
pub type U4_28 = Fx<4, 28, false>;
/// Edge-function accumulator, 40-bit signed with 8 fractional bits
/// (s13.4 coefficient times s12.4 coordinate scale). Only its sign relative
/// to zero decides coverage.
pub type E40 = Fx<31, 8, true>;
/// One multiplier product term of an edge function or area computation,
/// 36-bit signed with 8 fractional bits (same scale as [`E40`]).
pub type P36 = Fx<27, 8, true>;
/// Homogeneous clip-plane distance: signed Q16.16 with 40-bit headroom
/// (23 integer bits; `64*w` reaches 2^37 raw). Guard-band constant products
/// use shift-add chains on this type.
pub type ClipDist = Fx<23, 16, true>;

impl S2_29 {
    /// Class 3: post-clip `|ndc| <= 2.56 << 4`, so the product always fits.
    pub fn from_product(scaled: i64) -> Self {
        Self::from_raw(scaled)
    }

    /// Named conversion: high 17 significant bits for the 18-bit viewport
    /// multiplier. Class 3: post-clip values give |hi| <= 83886.
    pub fn high17(self) -> i64 {
        let hi = self.raw >> 14;
        assert!(
            hi.abs() < (1 << 17),
            "S2_29 high17 out of 18-bit range: {hi}"
        );
        hi
    }
}

impl U0_18 {
    /// Quantizes down to U0.16 with round-to-nearest. The top-code clamp is
    /// a saturator site; callers route through `devices::Saturator`.
    pub fn quantize(self) -> U0_16 {
        U0_16::from_raw_const(((self.raw + 2) >> 2).min(0xffff))
    }
}

// ---------------------------------------------------------------------------
// Multiplier operand statistics (recorded by the devices in `devices.rs`).

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

/// Records one multiplier call (used by the `Multiplier*` devices).
pub(crate) fn record_mul(label: &'static str, a: i64, b: i64, product: i64) {
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

/// Snapshot of every recorded call site, sorted by label.
pub fn mul_stats_snapshot() -> BTreeMap<&'static str, MulStat> {
    mul_stats().lock().unwrap().clone()
}

/// Clears all multiplier statistics.
pub fn mul_stats_reset() {
    mul_stats().lock().unwrap().clear();
}

/// Serializes tests that reset and snapshot the multiplier/device statistics
/// against other long-running tests that record into them.
pub fn stats_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

// ---------------------------------------------------------------------------
// Saturation events (class 1) and device overflow reports (class 2).

fn event_registry() -> &'static Mutex<BTreeMap<&'static str, u64>> {
    static EVENTS: OnceLock<Mutex<BTreeMap<&'static str, u64>>> = OnceLock::new();
    EVENTS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Records one saturation event at `label` (class-1 sites only).
pub(crate) fn record_sat(label: &'static str) {
    *event_registry().lock().unwrap().entry(label).or_default() += 1;
}

/// Records one wrap-overflow event at `label` (class-2 devices).
pub(crate) fn record_overflow(label: &'static str) {
    *event_registry().lock().unwrap().entry(label).or_default() += 1;
}

/// Snapshot of saturation/overflow event counts per call site.
pub fn device_events_snapshot() -> BTreeMap<&'static str, u64> {
    event_registry().lock().unwrap().clone()
}

/// Clears all saturation/overflow counters.
pub fn device_events_reset() {
    event_registry().lock().unwrap().clear();
}

// ---------------------------------------------------------------------------
// Reciprocal unit: CLZ normalization + LUT (+ Newton or slope lerp).

/// Unsigned 18x18 multiply with round-to-nearest on the dropped 18 low bits.
/// Used by the Newton reference path; instrumented under `rcp.newton`.
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
/// One small multiply: a signed ~10-bit slope times a 12-bit segment offset.
fn rcp_lerp(x: u32, base: &[u32], k: u32) -> (u32, i32) {
    let lz = x.leading_zeros();
    let normalized = x << lz;
    // f - 0.5 in units of 2^-32 (top bit was the leading one).
    let frac = normalized - 0x8000_0000;
    let index = (frac >> (31 - k)) as usize;
    // Top 12 bits of the in-segment fraction (segment width 2^(31-k)).
    let off12 = ((frac >> (31 - k - 12)) & 0xfff) as i64;
    let base_lo = i64::from(base[index]);
    let slope = i64::from(base[index + 1]) - base_lo;
    record_mul("rcp.lerp", slope, off12, slope * off12);
    let y = base_lo + ((slope * off12 + 2048) >> 12);
    (y.clamp(1 << 17, (1 << 18) - 1) as u32, 49 - lz as i32)
}

/// Low-level reciprocal of a nonzero `u32`. Returns `(mag, shift)` such that
/// `1/x ≈ mag * 2^-shift` with `mag` in `[2^17, 2^18]`. Implementation
/// selected by [`rcp_mode`] (thread-local, default LUT+lerp 256).
///
/// The raw `u32`/`mag` interface is the device's port level; typed wrappers
/// live in `devices::RcpUnit`.
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
/// `[2^17, 2^18]`. Inputs below `2^-3` (raw 8192, the documented near
/// limit of 1/8 m) saturate to `2^-3` as an
/// absolute floor; the legal input contract (`w >= 1/8`, validated in setup)
/// never engages it. Keeping the normalized magnitude plus exponent (instead
/// of a fixed-scale output) preserves full precision across the `w` range.
pub fn rcp_q16(w_raw: u32) -> (u32, i32) {
    rcp_u32(w_raw.max(8192))
}

/// Converts a `Q16.16` value from a rational `num/den` without floats.
pub fn q16_from_rational(num: i64, den: i64) -> Q16 {
    assert!(den != 0, "q16_from_rational: zero denominator");
    Q16::from_raw_const((num << 16) / den)
}

/// Rounds a `U0.18` depth down to `U0.16` (quantizer, see its docs).
pub fn depth18_to_16(depth: U0_18) -> U0_16 {
    depth.quantize()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rcp error distribution over a dense sweep for every implementation,
    /// comparing the normalized `(mag, shift)` output against the exact
    /// oracle; the bound is the contract's `rcp::MAX_REL_PPM`.
    #[test]
    fn rcp_matches_exact_division() {
        use crate::hardware::gpu::rastersim::{contract, oracle};
        for mode in [RcpMode::Newton, RcpMode::Lerp128, RcpMode::Lerp256] {
            set_rcp_mode(mode);
            let mut max_rel_ppm = 0u64;
            let mut total_rel_ppm = 0u64;
            let mut count = 0u64;
            // Sweep raw w from 2^-6 upward, dense near the small end, up to
            // the Q16.16 positive ceiling of 32768 - 2^-16.
            let mut steps = 0u64;
            let mut w_raw = 8192u32;
            while w_raw < 0x7fff_0000 && steps < 2_000_000 {
                let (product, target) = oracle::rcp_check(w_raw);
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
                max_rel_ppm < contract::rcp::MAX_REL_PPM,
                "{mode:?} relative error {max_rel_ppm} ppm exceeds the contract"
            );
        }
        set_rcp_mode(RcpMode::Lerp256);
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
            let slope = RCP_LERP256_BASE[i + 1] as i64 - RCP_LERP256_BASE[i] as i64;
            assert!(slope.abs() <= 2048, "slope {slope} at segment {i}");
        }
        for i in 0..128 {
            let slope = RCP_LERP128_BASE[i + 1] as i64 - RCP_LERP128_BASE[i] as i64;
            assert!(slope.abs() <= 2048, "slope {slope} at segment {i}");
        }
    }

    #[test]
    fn q16_from_rational_rounds_toward_zero() {
        assert_eq!(q16_from_rational(1, 2), Q16::from_raw_const(0x8000));
        assert_eq!(q16_from_rational(-1, 2), Q16::from_raw_const(-0x8000));
        assert_eq!(q16_from_rational(3, 2), Q16::from_raw_const(0x1_8000));
    }

    #[test]
    fn depth_quantization_rounds() {
        assert_eq!(
            depth18_to_16(U0_18::from_raw_const(0)),
            U0_16::from_raw_const(0)
        );
        assert_eq!(
            depth18_to_16(U0_18::from_raw_const(0x3ffff)),
            U0_16::from_raw_const(0xffff)
        );
        assert_eq!(
            depth18_to_16(U0_18::from_raw_const(4)),
            U0_16::from_raw_const(1)
        );
    }

    #[test]
    fn fx_range_check_panics() {
        // A signed 16-bit format includes -32768 but not +32768.
        assert_eq!(S12_4::from_raw(-32768).raw(), -32768);
        assert_eq!(
            Q16::from_raw(i64::from(i32::MIN)).raw(),
            i64::from(i32::MIN)
        );
        let result = std::panic::catch_unwind(|| {
            let _ = S12_4::from_raw(32768);
        });
        assert!(result.is_err());
    }

    #[test]
    fn mul_stats_record_ranges() {
        mul_stats_reset();
        let label = "test::mul_stats_record_ranges";
        record_mul(label, 3, 4, 12);
        record_mul(label, -1 << 20, 2, -2 << 20);
        let stats = mul_stats_snapshot();
        let entry = &stats[label];
        assert_eq!(entry.calls, 2);
        assert_eq!(entry.max_abs_a, 1 << 20);
        assert_eq!(entry.overrange_calls, 1);
        mul_stats_reset();
    }
}
