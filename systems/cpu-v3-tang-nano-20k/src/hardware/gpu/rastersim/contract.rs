//! Precision contract table: the single home of every tolerance number used
//! to judge "rastersim vs mathematical truth". Tests reference these
//! constants instead of hand-writing tolerances.
//!
//! Each entry documents: the quantity, the bound, its provenance (measured
//! or derived), and what a violation means. rastersim↔emu↔RTL comparisons
//! are bit-exact and live elsewhere; only this boundary uses tolerances.
//!
//! The measured values cited below are produced by the test suite (run with
//! `--nocapture`); the constants here are the *contract*, chosen with margin
//! above the measurements.

/// Contract for the rcp unit, all implementations.
pub mod rcp {
    /// Maximum relative error over the whole legal `w` domain
    /// (2^-6 .. 32768), in ppm. Bound: 2^-15 ≈ 30 ppm (the linear-depth
    /// consumer needs ~2^-15; see `rasterizer.md`'s consumer table).
    /// Measured: 7 ppm (Lerp256), 18 ppm (Lerp128), 15 ppm (Newton).
    /// A violation means the LUT/lerp tables or the Newton path regressed.
    pub const MAX_REL_PPM: u64 = 30;
}

/// Contract for the perspective divide (per-vertex NDC).
pub mod ndc {
    /// Absolute NDC error bound in s2.29 units: 1/32 px at the viewport
    /// half-width (200 px). Derived from "snapped screen coordinate must not
    /// move" plus the guard-band extreme |ndc| = 2.56. Equivalent to a
    /// relative rcp error of ~60 ppm at the guard edge; the rcp contract is
    /// tighter, so this bound is met with margin. Measured: far below.
    pub const ABS_LIMIT_S2_29: i64 = (1 << 29) / 200 / 32;

    /// Snapped-coordinate difference between the rcp path and the exact
    /// oracle, in s12.4 subpixel units, for any vertex inside the guard
    /// band. Contract: at most one subpixel of snapping jitter on exact
    /// ties. Measured: 0 over every scene vertex (both lerp modes).
    pub const SNAP_MATCH_SUBPIXELS: i64 = 1;

    /// Same, for the narrower Lerp128 configuration (kept as a downgrade
    /// option; measured 0, contract allows one extra subpixel of slack).
    pub const SNAP_MATCH_SUBPIXELS_LERP128: i64 = 2;
}

/// Contract for homogeneous clipping.
pub mod clip {
    /// Absolute intersection error versus the exact rational oracle, in
    /// Q16.16 raw units. Measured: 2 over a 200-edge random sweep.
    /// A violation means the interpolation direction or rounding drifted;
    /// AB==BA itself is exact by construction (deterministic inputs).
    pub const INTERSECTION_ABS_RAW: i64 = 8;

    /// Intersection error also scales with segment length via the rcp error:
    /// bound contribution is `segment >> INTERSECTION_REL_SHIFT`.
    pub const INTERSECTION_REL_SHIFT: u32 = 14;
}

/// Contract for depth reconstruction (linear-depth experiment path).
pub mod depth {
    /// Extra error the rcp unit may add on top of the pure U0.16 quantization
    /// error, in millimetres, at far <= 200 m. Measured: ~0.1 mm.
    pub const RCP_CONTRIBUTION_MAX_MM: i64 = 1;
}

/// Contract for image-level comparisons against a mathematically exact
/// renderer (used by future emu/RTL differential tests).
pub mod image {
    /// Differing pixels may only lie on triangle edges (top-left tie-break
    /// rounding) and their count is bounded by the total triangle perimeter:
    /// each of the three edges can flip at most one pixel per row/column it
    /// crosses, so a triangle of perimeter `p` pixels contributes at most
    /// `p` pixels. Derived, not yet exercised against a second renderer.
    pub fn max_diff_pixels(perimeters_px: &[u64]) -> u64 {
        perimeters_px.iter().sum()
    }
}
