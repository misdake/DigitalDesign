//! Hardware devices modeled as types (layer 2 of the device-as-type
//! discipline). Business code in `clip.rs`/`setup.rs`/`raster.rs` composes
//! these devices and never performs unconstrained raw arithmetic on the
//! hardware path.
//!
//! Device semantics:
//!
//! * [`Adder`] — a plain ripple/parallel adder of `BITS` bits. No saturation:
//!   the result **wraps** exactly like hardware. A wrap is almost always a
//!   design bug, so the device reports it: the event is counted per call
//!   site ([`fixed::device_events_snapshot`]) and a dedicated test asserts
//!   the counters stay zero over every scene. Chosen over panicking because
//!   wrapping is the hardware's actual behavior — the sim must produce the
//!   hardware's value, and the counters show whether the design ever relies
//!   on it. Area/latency: O(BITS) LUTs, single-cycle.
//! * `Multiplier18x18` / `Multiplier36x18` / `Multiplier36x36` — DSP blocks
//!   (GW2A-18 sweet spots: 18x18, with 36x18/36x36 costing 2/4 of them).
//!   Operands are raw fixed-point values; the devices only record operand
//!   range statistics per call site ([`fixed::mul_stats_snapshot`]), they
//!   never panic — the report decides where a wide multiplier is needed.
//! * [`Saturator`] — the saturating clamps the hardware performs: viewport
//!   X/Y to ±511.9375 px, depth to format range, `w` up to `w_min`. Every
//!   saturation event is counted per call site; tests assert the rates.
//! * [`RcpUnit`] — the reciprocal unit (CLZ normalization + LUT + slope
//!   lerp, one ~12x10 multiply; see `fixed.rs` for modes and tables).

use crate::hardware::gpu::rastersim::fixed::{self, record_mul, record_overflow, record_sat, Q16};

/// Non-saturating `BITS`-bit adder: wraps like hardware, reports the wrap.
///
/// The `_fx` variants take typed operands; unwrapping them is the device's
/// port-level job, not the caller's.
pub struct Adder<const BITS: u32>;

impl<const BITS: u32> Adder<BITS> {
    /// Adds two raw values and wraps to a signed `BITS`-bit result.
    pub fn add(label: &'static str, a: i64, b: i64) -> i64 {
        let width = (BITS - 1) as i64;
        let sum = a + b;
        let wrapped = (sum + (1 << width)) & ((1 << BITS) - 1);
        let wrapped = wrapped - (1 << width);
        if wrapped != sum {
            record_overflow(label);
        }
        wrapped
    }

    /// `a - b`, same wrap/report semantics.
    pub fn sub(label: &'static str, a: i64, b: i64) -> i64 {
        Self::add(label, a, -b)
    }

    /// Typed-operand add (port-level unwrap inside).
    pub fn add_fx<const I: u32, const F: u32, const S: bool>(
        label: &'static str,
        a: fixed::Fx<I, F, S>,
        b: fixed::Fx<I, F, S>,
    ) -> i64 {
        Self::add(label, a.raw(), b.raw())
    }

    /// Typed-operand subtract (port-level unwrap inside).
    pub fn sub_fx<const I: u32, const F: u32, const S: bool>(
        label: &'static str,
        a: fixed::Fx<I, F, S>,
        b: fixed::Fx<I, F, S>,
    ) -> i64 {
        Self::sub(label, a.raw(), b.raw())
    }

    /// Typed minuend with a raw subtrahend (pixel coordinates and other
    /// port-level values arrive as raw integers).
    pub fn sub_fx_raw<const I: u32, const F: u32, const S: bool>(
        label: &'static str,
        a: i64,
        b: fixed::Fx<I, F, S>,
    ) -> i64 {
        Self::sub(label, a, b.raw())
    }
}

/// 16-bit adder (s12.4 coordinate arithmetic).
pub type Adder16 = Adder<16>;
/// 18-bit adder (coefficient/range arithmetic).
pub type Adder18 = Adder<18>;
/// 32-bit adder (viewport Q16.16 products accumulation).
pub type Adder32 = Adder<32>;
/// 40-bit adder (edge-function accumulation, homogeneous clip distances).
pub type Adder40 = Adder<40>;
/// 56-bit adder (depth interpolation numerator).
pub type Adder56 = Adder<56>;

/// GW2A DSP sweet spot: 18x18 signed multiply, product up to 36 bits.
pub struct Multiplier18x18;

/// Macro-generates `mul` (raw operands) plus `mul_fx` (typed operands; the
/// port-level unwrap lives inside the device) for one multiplier width.
macro_rules! multiplier {
    ($name:ident) => {
        impl $name {
            pub fn mul(label: &'static str, a: i64, b: i64) -> i64 {
                let product = a * b;
                record_mul(label, a, b, product);
                product
            }

            /// Typed operands; unwrapping is the device's port-level job.
            pub fn mul_fx<
                const I1: u32,
                const F1: u32,
                const S1: bool,
                const I2: u32,
                const F2: u32,
                const S2: bool,
            >(
                label: &'static str,
                a: fixed::Fx<I1, F1, S1>,
                b: fixed::Fx<I2, F2, S2>,
            ) -> i64 {
                Self::mul(label, a.raw(), b.raw())
            }

            /// Typed multiplicand with a raw multiplier (rcp magnitudes,
            /// clip fractions, and other port-level values arrive raw).
            pub fn mul_fx_raw<const I: u32, const F: u32, const S: bool>(
                label: &'static str,
                a: fixed::Fx<I, F, S>,
                b: i64,
            ) -> i64 {
                Self::mul(label, a.raw(), b)
            }
        }
    };
}

multiplier!(Multiplier18x18);

/// 36x18 multiply (two 18x18 DSPs).
pub struct Multiplier36x18;

multiplier!(Multiplier36x18);

/// 36x36 multiply (four 18x18 DSPs).
pub struct Multiplier36x36;

multiplier!(Multiplier36x36);

/// The hardware's saturating clamps; every event is counted.
pub struct Saturator;

impl Saturator {
    /// Clamps `w` up to `w_min` (2^-6 in Q16.16), as the rcp input stage
    /// does. Hardware reason: the LUT+lerp input must be positive and
    /// normalized; a smaller `w` would break the range assumptions.
    pub fn w_min(value: Q16) -> Q16 {
        if value.raw() < 1024 {
            record_sat("setup.w_min");
            Q16::from_raw_const(1024)
        } else {
            value
        }
    }

    /// Saturates a viewport Q16.16 coordinate to the s12.4 guard-band range
    /// ±511.9375 px, via `floor(v*16 + 0.5)`. Hardware reason: the snapped
    /// coordinate register is 16-bit and the guard band bounds all inputs;
    /// the clamp only absorbs rcp/lerp epsilon and pre-clip stragglers.
    pub fn snap_s12_4(v_q16: Q16) -> fixed::S12_4 {
        let raw = (v_q16.raw() + 2048) >> 12;
        if !(-8191..=8191).contains(&raw) {
            record_sat("viewport.snap");
        }
        fixed::S12_4::from_raw_const(raw.clamp(-8191, 8191))
    }

    /// Saturates a depth product to the U0.18 range. Hardware reason: the
    /// depth register is 18-bit; post-clip `z <= w` bounds the value to 1,
    /// so this only absorbs rcp epsilon.
    pub fn depth_u0_18(value: u64) -> fixed::U0_18 {
        if value > 0x3ffff {
            record_sat("setup.depth");
        }
        fixed::U0_18::from_raw_const(value.min(0x3ffff) as i64)
    }

    /// Clamps the clip interpolation fraction to the U0.18 range.
    /// Mathematically `t < 1` already; this only absorbs rcp epsilon.
    pub fn frac18(value: u64) -> fixed::U0_18 {
        if value > (1 << 18) - 1 {
            record_sat("clip.t");
        }
        fixed::U0_18::from_raw_const(value.min((1 << 18) - 1) as i64)
    }

    /// Saturates the interpolated depth at a pixel before quantization.
    /// Class 1 for the same reason as `depth_u0_18`; should never fire on
    /// covered pixels (barycentric weights are non-negative there).
    pub fn interp_u0_18(value: i64) -> fixed::U0_18 {
        if !(0..=0x3ffff).contains(&value) {
            record_sat("raster.depth_interp");
        }
        fixed::U0_18::from_raw_const(value.clamp(0, 0x3ffff))
    }
}

/// The reciprocal unit's output: normalized magnitude (U1.18-ish,
/// `[2^17, 2^18]`) plus exponent, so that `1/x ≈ mag * 2^-shift`.
#[derive(Clone, Copy, Debug)]
pub struct RcpOutput {
    mag: u32,
    pub shift: i32,
}

impl RcpOutput {
    /// Port-level accessor: the magnitude as a multiplier operand.
    pub fn mag_raw(self) -> i64 {
        i64::from(self.mag)
    }
}

/// Reciprocal unit device: CLZ normalization + LUT + slope lerp (default
/// 256 segments). Area: one ~1 KB LUT (half a BSRAM-18K) plus one ~12x10
/// multiplier; single-cycle or short pipeline. Wraps `fixed::rcp_u32`/`rcp_q16`.
pub struct RcpUnit;

impl RcpUnit {
    /// `1/w` for a clip-space `w` (Q16.16). The caller is responsible for
    /// the `w_min` saturator (`Saturator::w_min`) upstream.
    pub fn rcp_q16(w: Q16) -> RcpOutput {
        let (mag, shift) = fixed::rcp_q16(w.raw() as u32);
        RcpOutput { mag, shift }
    }

    /// Low-level `1/x` for raw `u32` (clip-distance interpolation path).
    pub fn rcp_u32(x: u32) -> RcpOutput {
        let (mag, shift) = fixed::rcp_u32(x);
        RcpOutput { mag, shift }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::gpu::rastersim::fixed::device_events_snapshot;

    #[test]
    fn adder_wraps_and_reports() {
        // "test.adder" is used only here, so the count delta is exact even
        // with tests running in parallel.
        let before = device_events_snapshot()
            .get("test.adder")
            .copied()
            .unwrap_or(0);
        assert_eq!(Adder16::add("test.adder", 1, 2), 3);
        // 16-bit signed wrap: 32767 + 1 = -32768, reported.
        assert_eq!(Adder16::add("test.adder", 32767, 1), -32768);
        assert_eq!(Adder16::sub("test.adder", -32768, 1), 32767);
        let after = device_events_snapshot()
            .get("test.adder")
            .copied()
            .unwrap_or(0);
        assert_eq!(after - before, 2);
    }

    #[test]
    fn multipliers_record_ranges() {
        fixed::mul_stats_reset();
        assert_eq!(Multiplier18x18::mul("test.mul18", 3, 4), 12);
        assert_eq!(Multiplier36x18::mul("test.mul36x18", 1 << 20, 2), 1 << 21);
        assert_eq!(
            Multiplier36x36::mul("test.mul36x36", 1 << 30, 1 << 5),
            1 << 35
        );
        let stats = fixed::mul_stats_snapshot();
        assert!(stats["test.mul18"].calls >= 1);
        assert!(stats["test.mul36x18"].overrange_calls >= 1);
    }

    #[test]
    fn saturator_counts_events() {
        // Saturator labels are shared with scene tests, so only deltas of at
        // least the local contribution are asserted.
        let before = device_events_snapshot();
        assert_eq!(
            Saturator::w_min(Q16::from_raw_const(1 << 16)).raw(),
            1 << 16
        );
        assert_eq!(Saturator::w_min(Q16::from_raw_const(100)).raw(), 1024);
        assert_eq!(
            Saturator::snap_s12_4(Q16::from_raw_const(100 << 16)).raw(),
            1600
        );
        assert_eq!(
            Saturator::snap_s12_4(Q16::from_raw_const(600 << 16)).raw(),
            8191
        );
        assert_eq!(Saturator::depth_u0_18(1 << 18).raw(), 0x3ffff);
        let after = device_events_snapshot();
        assert!(
            after.get("setup.w_min").copied().unwrap_or(0)
                - before.get("setup.w_min").copied().unwrap_or(0)
                >= 1
        );
        assert!(
            after.get("viewport.snap").copied().unwrap_or(0)
                - before.get("viewport.snap").copied().unwrap_or(0)
                >= 1
        );
        assert!(
            after.get("setup.depth").copied().unwrap_or(0)
                - before.get("setup.depth").copied().unwrap_or(0)
                >= 1
        );
    }
}
