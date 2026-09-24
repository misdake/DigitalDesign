//! Exact mathematical oracles. Every function here is:
//!
//! **exact oracle, test-only, not a hardware path.**
//!
//! These compute ground truth with wide integer arithmetic (i64/i128) and
//! exact division. They are the reference for the contract tolerances in
//! `contract.rs`; nothing on the hardware path may call them.

use crate::hardware::gpu::rastersim::clip::{ClipVertex, W_MIN_RAW};
use crate::hardware::gpu::rastersim::fixed::{rcp_q16, ClipDist, Q16};

/// Exact NDC `x/w` at 29 fractional bits via 128-bit integer division.
pub fn ndc_exact(x: Q16, w_raw: i64) -> i64 {
    let w = w_raw.max(i64::from(W_MIN_RAW));
    ((i128::from(x.raw()) << 29) / i128::from(w)) as i64
}

/// Exact check pair for the rcp unit: `1/w = mag * 2^(16-shift)` with
/// `w_true = w_raw * 2^-16` implies `mag * w_raw == 2^shift`; returns
/// `(mag * w_raw, 2^shift)` computed exactly.
pub fn rcp_check(w_raw: u32) -> (u128, u128) {
    let w_raw = w_raw.max(8192);
    let (mag, shift) = rcp_q16(w_raw);
    let product = u128::from(mag) * u128::from(w_raw);
    (product, 1u128 << shift)
}

/// Exact clip intersection of the segment `out -> inside` with a plane, given
/// signed plane distances (`d_out < 0 <= d_in`), via rational arithmetic:
/// `t = -d_out / (d_in - d_out)`, `p = out + t*(inside - out)`.
pub fn clip_intersection_exact(
    out: &ClipVertex,
    d_out: ClipDist,
    inside: &ClipVertex,
    d_in: ClipDist,
) -> ClipVertex {
    let t_num = -i128::from(d_out.raw());
    let t_den = i128::from(d_in.raw()) - i128::from(d_out.raw());
    let exact = |a: Q16, b: Q16| {
        Q16::from_raw_const(
            (i128::from(a.raw()) + (i128::from(b.raw()) - i128::from(a.raw())) * t_num / t_den)
                as i64,
        )
    };
    ClipVertex {
        x: exact(out.x, inside.x),
        y: exact(out.y, inside.y),
        z: exact(out.z, inside.z),
        w: exact(out.w, inside.w),
    }
}
