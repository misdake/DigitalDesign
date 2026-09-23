//! Depth-mode experiments: how to get a more linear depth than screen-linear
//! `z/w` interpolation, and what it costs. All experiments use the realistic
//! scene scale (near = 0.5 m, far in {150 m, 200 m}, map ~50..100 m across)
//! and integer millimetre arithmetic only.
//!
//! Theory (confirmed): screen-linear interpolation can only carry
//! linear-fractional depth functions. The standard path to truly linear
//! depth is: store `1/w` per vertex (interpolates screen-linearly, exact for
//! planar surfaces), per pixel compute `w = rcp(1/w)`, then
//! `z_lin = (w - near)/(far - near)` and quantize. The experiments:
//!
//! 1. baseline: reconstruction error of the current reverse-Z `z/w` path;
//! 2. linear depth via per-pixel rcp, with the error broken down by source
//!    (U0.16 quantization, `1/w` format quantization, rcp unit);
//! 3. counter-example: screen-linear interpolation of `w` itself as depth,
//!    evaluated per pixel on a big slanted ground triangle;
//! 4. compromise: one rcp per quad (four pixels share the anchor's `w`),
//!    error amplification relative to per-pixel rcp.

use crate::hardware::gpu::rastersim::fixed::rcp_u32;
use crate::hardware::gpu::rastersim::scenes::depth_code;

/// Error statistics over a sampled grid, in millimetres.
#[derive(Clone, Copy, Debug)]
pub struct ErrorStats {
    pub worst_mm: i64,
    pub average_mm_x1000: u64,
    pub samples: u64,
}

fn stats(errors: impl Iterator<Item = i64>) -> ErrorStats {
    let mut worst = 0i64;
    let mut total = 0u64;
    let mut samples = 0u64;
    for e in errors {
        worst = worst.max(e.abs());
        total += e.unsigned_abs();
        samples += 1;
    }
    ErrorStats {
        worst_mm: worst,
        average_mm_x1000: total * 1000 / samples.max(1),
        samples,
    }
}

/// Geometric distance grid from `near_mm` to `far_mm`, step `1 + 2^-8`.
fn grid(near_mm: u64, far_mm: u64) -> Vec<u64> {
    let mut out = Vec::new();
    let mut z = near_mm;
    while z < far_mm && out.len() < 100_000 {
        out.push(z);
        z += (z >> 8).max(1);
    }
    assert!(out.len() < 100_000, "grid exceeded its step limit");
    out
}

// ---------------------------------------------------------------------------
// Experiment 1: baseline reverse-Z (and regular-Z) reconstruction error.
// For each grid distance, quantize to U0.16, invert the code exactly, and
// measure |z_hat - z|.

/// Inverts a U0.16 depth code to the distance it represents (mm), exactly.
fn code_to_distance(code: u16, near_mm: u64, far_mm: u64, reverse: bool) -> u64 {
    let (n, f, c) = (near_mm as u128, far_mm as u128, u128::from(code));
    let span = f - n;
    let z = if reverse {
        // c/65536 = n*(f-z) / (span*z)  =>  z = 65536*n*f / (c*span + 65536*n)
        (65536 * n * f) / (c * span + 65536 * n)
    } else {
        // c/65536 = f*(z-n) / (span*z)  =>  z = 65536*f*n / (65536*f - c*span)
        (65536 * f * n) / (65536 * f - c * span)
    };
    z as u64
}

/// Experiment 1: reconstruction error of the current z/w depth path.
pub fn baseline_reconstruction_error(reverse: bool, far_mm: u64) -> ErrorStats {
    let near_mm = 500;
    stats(grid(near_mm, far_mm).into_iter().map(|z| {
        let code = depth_code(z, near_mm, far_mm, reverse);
        code_to_distance(code, near_mm, far_mm, reverse) as i64 - z as i64
    }))
}

// ---------------------------------------------------------------------------
// Experiment 2: linear depth via per-pixel rcp.
//
// Vertex format choice: `1/w` in **U4.28** (32-bit, metres^-1).
// w in [0.5, 200] m puts 1/w in [0.005, 2], so two integer bits are needed;
// 28 fraction bits keep the relative quantization error at w*2^-28, which at
// 200 m is 7.5e-7 (0.15 mm of w) — negligible against the 2^-16 output
// quantum. An 18-bit alternative (U2.16) would quantize 1/w at 200 m to
// raw 328, a 0.3% relative error = 0.6 m of w: completely unusable. The
// interpolation datapath is 32-bit either way, so U4.28 is free.

/// How `w` is reconstructed from the interpolated `1/w`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RcpMode {
    /// Exact `w` (isolates the U0.16 output quantization).
    Exact,
    /// U4.28 quantization of `1/w`, exact division back.
    QuantizedInvW,
    /// U4.28 plus the 18-bit rcp unit (LUT + two Newton steps).
    RcpUnit,
}

/// Reconstructs `w` (mm) from a true distance (mm) under `mode`.
fn reconstruct_w(z_mm: u64, mode: RcpMode) -> u64 {
    const SCALE: u128 = 1 << 28;
    let inv_w_raw = (SCALE * 1000 + u128::from(z_mm) / 2) / u128::from(z_mm);
    match mode {
        RcpMode::Exact => z_mm,
        RcpMode::QuantizedInvW => ((SCALE * 1000 + inv_w_raw / 2) / inv_w_raw) as u64,
        RcpMode::RcpUnit => {
            let (mag, shift) = rcp_u32(inv_w_raw as u32);
            // w_mm = 2^28 * 1000 * mag * 2^-shift.
            let num = u128::from(mag) * 1000;
            if shift >= 28 {
                (num >> (shift - 28)) as u64
            } else {
                (num << (28 - shift)) as u64
            }
        }
    }
}

/// Experiment 2: reconstruction error of linear depth for each rcp mode.
pub fn linear_depth_error(mode: RcpMode, far_mm: u64) -> ErrorStats {
    let near_mm = 500;
    let span = (far_mm - near_mm) as u128;
    stats(grid(near_mm, far_mm).into_iter().map(|z| {
        let w = reconstruct_w(z, mode);
        // z_lin = (w - near) / span, quantized to U0.16 with rounding.
        let code = ((u128::from(w.saturating_sub(near_mm)) * 65536 + span / 2) / span).min(65535);
        let z_hat = near_mm as u128 + (code * span + 32768) / 65536;
        z_hat as i64 - z as i64
    }))
}

// ---------------------------------------------------------------------------
// Experiments 3 and 4 share one big slanted ground triangle in view space
// (millimetres), spanning 50..150 m of depth across the screen.

/// View-space triangle vertices `(x, y, z)` in mm; `w_clip = z`, focal = 1.
const GROUND_TRI: [[i64; 3]; 3] = [
    [0, -10000, 50000],
    [100000, 10000, 150000],
    [-60000, 20000, 140000],
];

/// Per-pixel experiment data for one covered pixel of the ground triangle.
struct GroundPixel {
    x: i64,
    y: i64,
    /// True view distance along the pixel ray (mm).
    w_true: i64,
    /// Screen-linear interpolated `w` (the counter-example depth), mm.
    w_interp: i64,
    /// True `1/w` in U4.28 raw units (interpolates exactly for planes).
    inv_w_raw: u64,
}

/// Enumerates the covered pixels of the ground triangle at pixel centers,
/// with exact rational arithmetic. Screen mapping matches the rasterizer:
/// `X = ndc_x*200 + 200`, `Y = 120 - ndc_y*120` with `ndc = (x/z, y/z)`.
fn ground_pixels() -> Vec<GroundPixel> {
    // Screen positions at 2^20 fraction (i64): X = (x*200 << 20)/z + 200<<20.
    let s = |coord: i64, z: i64, center: i64, half: i64| -> i64 {
        center * (1 << 20) + (coord * half * (1 << 20)) / z
    };
    let px: Vec<i64> = GROUND_TRI.iter().map(|v| s(v[0], v[2], 200, 200)).collect();
    let py: Vec<i64> = GROUND_TRI
        .iter()
        .map(|v| s(-v[1], v[2], 120, 120))
        .collect();
    // Plane through the view-space points: N . P = d (i128 cross product).
    let [a, b, c] = GROUND_TRI;
    let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let n = [
        ab[1] as i128 * ac[2] as i128 - ab[2] as i128 * ac[1] as i128,
        ab[2] as i128 * ac[0] as i128 - ab[0] as i128 * ac[2] as i128,
        ab[0] as i128 * ac[1] as i128 - ab[1] as i128 * ac[0] as i128,
    ];
    let d = n[0] * a[0] as i128 + n[1] * a[1] as i128 + n[2] * a[2] as i128;
    let mut out = Vec::new();
    let x0 = (*px.iter().min().unwrap() >> 20) as i32;
    let x1 = (*px.iter().max().unwrap() >> 20) as i32;
    let y0 = (*py.iter().min().unwrap() >> 20) as i32;
    let y1 = (*py.iter().max().unwrap() >> 20) as i32;
    for y in y0..=y1 {
        for x in x0..=x1 {
            // Pixel center at 2^20 fraction.
            let sx = (i64::from(x) << 20) + (1 << 19);
            let sy = (i64::from(y) << 20) + (1 << 19);
            // Barycentric weights via edge cross products (i128); the edge
            // function of edge v_i -> v_{i+1} is the weight of the opposite
            // vertex. Sign follows the triangle winding.
            let cross = |ax: i64, ay: i64, bx: i64, by: i64| -> i128 {
                (bx - ax) as i128 * (sy - ay) as i128 - (by - ay) as i128 * (sx - ax) as i128
            };
            let e0 = cross(px[0], py[0], px[1], py[1]);
            let e1 = cross(px[1], py[1], px[2], py[2]);
            let e2 = cross(px[2], py[2], px[0], py[0]);
            let area = e0 + e1 + e2;
            let inside = if area > 0 {
                e0 >= 0 && e1 >= 0 && e2 >= 0
            } else {
                e0 <= 0 && e1 <= 0 && e2 <= 0
            };
            if area == 0 || !inside {
                continue;
            }
            // Ray direction (ndc_x, ndc_y, 1) scaled by 600*2^20 for exact
            // integer arithmetic: dir = (3*(X-200), 5*(120-Y), 600)*2^20.
            let dir_x = (sx - (200 << 20)) as i128 * 3;
            let dir_y = ((120 << 20) - sy) as i128 * 5;
            let dir_z = 600i128 << 20;
            // True distance: w = d * scale / (N . dir).
            let denom = n[0] * dir_x + n[1] * dir_y + n[2] * dir_z;
            assert!(denom > 0, "ray points away from the plane");
            let w_true = ((2 * d * dir_z + denom) / (2 * denom)) as i64;
            // Screen-linear interpolated w (the wrong depth).
            let w_interp = ((e0 * GROUND_TRI[2][2] as i128
                + e1 * GROUND_TRI[0][2] as i128
                + e2 * GROUND_TRI[1][2] as i128)
                / area) as i64;
            // Exact 1/w in U4.28 (valid for any point of the plane):
            // 1/w = (N . dir) / (d * scale).
            let inv_w_raw = (((1i128 << 28) * 1000 * denom) / (d * dir_z)) as u64;
            out.push(GroundPixel {
                x: i64::from(x),
                y: i64::from(y),
                w_true,
                w_interp,
                inv_w_raw,
            });
        }
    }
    out
}

/// Result of the counter-example experiment, errors in mm.
#[derive(Clone, Debug)]
pub struct GroundError {
    pub pixels: u64,
    pub worst_abs_mm: i64,
    /// (bucket low true-w in metres, worst |error|, average |error| x1000).
    pub buckets: Vec<(u64, i64, u64)>,
}

/// Experiment 3: screen-linear `w` interpolation used directly as depth.
pub fn interp_w_error() -> GroundError {
    let pixels = ground_pixels();
    assert!(!pixels.is_empty());
    let mut buckets: Vec<(u64, i64, u64, u64)> = Vec::new(); // low_m, worst, total, count
    let mut worst = 0i64;
    for p in &pixels {
        let err = p.w_interp - p.w_true;
        worst = worst.max(err.abs());
        let low_m = ((p.w_true / 10_000) * 10) as u64;
        let bucket = match buckets.iter_mut().find(|b| b.0 == low_m) {
            Some(b) => b,
            None => {
                buckets.push((low_m, 0, 0, 0));
                buckets.last_mut().unwrap()
            }
        };
        bucket.1 = bucket.1.max(err.abs());
        bucket.2 += err.unsigned_abs();
        bucket.3 += 1;
    }
    buckets.sort_by_key(|b| b.0);
    GroundError {
        pixels: pixels.len() as u64,
        worst_abs_mm: worst,
        buckets: buckets
            .iter()
            .map(|&(low, w, t, c)| (low, w, t * 1000 / c))
            .collect(),
    }
}

/// Experiment 4: one rcp per quad (anchor pixel `w` shared by 2x2 pixels)
/// versus per-pixel rcp. Errors are |w_est - w_true| in mm.
pub fn quad_shared_rcp_error() -> (ErrorStats, ErrorStats) {
    let from_raw = |inv_w_raw: u64| -> u64 {
        let (mag, shift) = rcp_u32(inv_w_raw as u32);
        let num = u128::from(mag) * 1000;
        if shift >= 28 {
            (num >> (shift - 28)) as u64
        } else {
            (num << (28 - shift)) as u64
        }
    };
    // Anchor `1/w` per even-aligned quad, from the anchor pixel's own value.
    let mut anchors: std::collections::HashMap<(i64, i64), u64> = std::collections::HashMap::new();
    let pixels = ground_pixels();
    for p in &pixels {
        anchors.entry((p.x & !1, p.y & !1)).or_insert(p.inv_w_raw);
    }
    let per_pixel = stats(
        pixels
            .iter()
            .map(|p| from_raw(p.inv_w_raw) as i64 - p.w_true),
    );
    let per_quad = stats(pixels.iter().map(|p| {
        let anchor = anchors[&(p.x & !1, p.y & !1)];
        from_raw(anchor) as i64 - p.w_true
    }));
    (per_pixel, per_quad)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn experiment_1_baseline_reverse_z() {
        for far_m in [150u64, 200] {
            let reverse = baseline_reconstruction_error(true, far_m * 1000);
            let regular = baseline_reconstruction_error(false, far_m * 1000);
            println!(
                "exp1 far={far_m}m reverse-Z: worst {} mm avg {}.{:03} mm ({} samples)",
                reverse.worst_mm,
                reverse.average_mm_x1000 / 1000,
                reverse.average_mm_x1000 % 1000,
                reverse.samples
            );
            println!(
                "exp1 far={far_m}m regular-Z: worst {} mm avg {}.{:03} mm ({} samples)",
                regular.worst_mm,
                regular.average_mm_x1000 / 1000,
                regular.average_mm_x1000 % 1000,
                regular.samples
            );
            assert!(reverse.worst_mm < regular.worst_mm);
        }
    }

    #[test]
    fn experiment_2_linear_depth_via_rcp() {
        for far_m in [150u64, 200] {
            let quantum = far_m * 1000 - 500;
            let exact = linear_depth_error(RcpMode::Exact, far_m * 1000);
            let quant = linear_depth_error(RcpMode::QuantizedInvW, far_m * 1000);
            let rcp = linear_depth_error(RcpMode::RcpUnit, far_m * 1000);
            println!(
                "exp2 far={far_m}m: exact worst {} avg {}.{:03}; inv_w worst {} avg {}.{:03}; rcp worst {} avg {}.{:03} mm",
                exact.worst_mm,
                exact.average_mm_x1000 / 1000,
                exact.average_mm_x1000 % 1000,
                quant.worst_mm,
                quant.average_mm_x1000 / 1000,
                quant.average_mm_x1000 % 1000,
                rcp.worst_mm,
                rcp.average_mm_x1000 / 1000,
                rcp.average_mm_x1000 % 1000,
            );
            // Exact mode is pure U0.16 quantization: near quantum/2.
            assert!(exact.worst_mm * 65536 <= (quantum / 2) as i64 + 65536);
            // U4.28 inv_w adds less than 1 mm.
            assert!((quant.worst_mm - exact.worst_mm).abs() <= 1);
            // The rcp unit keeps the total within a few mm of the ideal.
            assert!(rcp.worst_mm <= exact.worst_mm + far_m as i64 * 1000 / 50);
        }
    }

    #[test]
    fn experiment_3_interp_w_is_unusable() {
        let result = interp_w_error();
        println!(
            "exp3 interp-w: {} pixels, worst |error| {} mm",
            result.pixels, result.worst_abs_mm
        );
        for (low_m, worst, avg_x1000) in &result.buckets {
            println!(
                "exp3   true w {:3}..{:3} m: worst {:6} mm avg {:6}.{:03} mm",
                low_m,
                low_m + 10,
                worst,
                avg_x1000 / 1000,
                avg_x1000 % 1000
            );
        }
        // Metre-scale systematic error on a 100 m slanted plane: unusable.
        assert!(result.worst_abs_mm > 1000);
    }

    #[test]
    fn experiment_4_quad_shared_rcp() {
        let (per_pixel, per_quad) = quad_shared_rcp_error();
        let amp_worst = per_quad.worst_mm * 1000 / per_pixel.worst_mm.max(1);
        let amp_avg = per_quad.average_mm_x1000 * 1000 / per_pixel.average_mm_x1000.max(1);
        println!(
            "exp4: per-pixel rcp worst {} avg {}.{:03} mm; per-quad rcp worst {} avg {}.{:03} mm; amplification worst {}.{:03}x avg {}.{:03}x",
            per_pixel.worst_mm,
            per_pixel.average_mm_x1000 / 1000,
            per_pixel.average_mm_x1000 % 1000,
            per_quad.worst_mm,
            per_quad.average_mm_x1000 / 1000,
            per_quad.average_mm_x1000 % 1000,
            amp_worst / 1000,
            amp_worst % 1000,
            amp_avg / 1000,
            amp_avg % 1000,
        );
        assert!(per_quad.worst_mm >= per_pixel.worst_mm);
    }
}
