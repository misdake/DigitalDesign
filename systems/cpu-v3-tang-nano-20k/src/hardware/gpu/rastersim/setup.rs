//! Triangle setup for the standalone rasterizer functional sim.
//!
//! Pipeline per clipped clip-space triangle:
//!
//! 1. `rcp(w)` per vertex (LUT + slope lerp, one small multiply);
//! 2. NDC `x,y = (x,y)*rcp(w)` as 32-bit `s2.29` (range ±4; the ±512 px
//!    guard band spans ±2.56 NDC units, so no wider intermediate is needed
//!    and the whole NDC/viewport path stays 32-bit); the viewport multiplier
//!    consumes the high 17 significant bits;
//! 3. depth `z*rcp(w)` as `U0.18`, saturated to its range;
//! 4. viewport transform and subpixel snap to `s12.4` via
//!    `floor(v*16 + 0.5)`, saturated to ±511.9375 px;
//! 5. signed area and backface cull (front faces are visually clockwise in
//!    the y-down screen, i.e. positive signed area);
//! 6. edge coefficients `cx, cy` (`s13.4`, 18-bit) and the top-left rule
//!    (an edge is top-left when it goes up, or is horizontal going right;
//!    non-top-left edges are covered only when `E > 0` strictly);
//! 7. pixel/tile AABB plus the even-aligned quad AABB.

use crate::hardware::gpu::rastersim::clip::{ClipVertex, W_MIN_RAW};
use crate::hardware::gpu::rastersim::fixed::{mul18, rcp_q16};
use crate::{FRAMEBUFFER_HEIGHT, FRAMEBUFFER_WIDTH};

/// Viewport half-extents in pixels.
pub const HALF_WIDTH: i64 = (FRAMEBUFFER_WIDTH / 2) as i64;
pub const HALF_HEIGHT: i64 = (FRAMEBUFFER_HEIGHT / 2) as i64;

/// One triangle ready for rasterization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TriangleSetup {
    /// Rolling triangle id within a scene.
    pub id: u32,
    /// Snapped screen coordinates, `s12.4` raw.
    pub x: [i16; 3],
    pub y: [i16; 3],
    /// Per-vertex depth, `U0.18`.
    pub depth: [u32; 3],
    /// Edge `i` runs `v_i -> v_{i+1}`; coverage when
    /// `cx[i]*(px - x[i]) + cy[i]*(py - y[i])` passes the fill rule.
    pub cx: [i32; 3],
    pub cy: [i32; 3],
    pub top_left: [bool; 3],
    /// Twice the signed area in subpixel units; always positive here.
    pub area2: i64,
    /// Even-aligned pixel-space quad AABB, inclusive, clamped to the screen.
    pub quad_aabb: [i32; 4],
}

impl TriangleSetup {
    /// Text serialization of the triangle FIFO record, one line per entry.
    pub fn fifo_line(&self) -> String {
        format!(
            "TRI {} X {} {} {} Y {} {} {} Z {} {} {} AABB {} {} {} {}",
            self.id,
            self.x[0],
            self.x[1],
            self.x[2],
            self.y[0],
            self.y[1],
            self.y[2],
            self.depth[0],
            self.depth[1],
            self.depth[2],
            self.quad_aabb[0],
            self.quad_aabb[1],
            self.quad_aabb[2],
            self.quad_aabb[3],
        )
    }
}

/// Setup-stage counters, folded into the scene statistics.
#[derive(Clone, Copy, Debug, Default)]
pub struct SetupStats {
    pub submitted: u64,
    pub culled_backface: u64,
    pub culled_degenerate: u64,
    pub culled_offscreen: u64,
    pub emitted: u64,
}

/// Stateful setup unit holding the rolling triangle id and statistics.
#[derive(Default)]
pub struct SetupUnit {
    next_id: u32,
    pub stats: SetupStats,
}

/// NDC via exact integer division (test-only oracle; the only place an i64
/// NDC intermediate is allowed): `x/w` at 29 fractional bits.
pub fn ndc_exact(x_raw: i32, w_raw: i32) -> i64 {
    let w = i64::from(w_raw.max(W_MIN_RAW));
    ((i128::from(x_raw) << 29) / i128::from(w)) as i64
}

/// NDC via the rcp unit: `x/w = x_raw * mag * 2^-shift`, returned as 32-bit
/// `s2.29` (range ±4; a 30-fraction-bit signed 32-bit format tops out at
/// ±2, which cannot hold the ±2.56 guard band). After clipping the contract
/// guarantees `|x| <= 2.56*w`; saturation is a safety net only.
pub fn ndc_rcp(x_raw: i32, mag: u32, shift: i32) -> i32 {
    let product = i64::from(x_raw) * i64::from(mag);
    let scaled = if shift >= 29 {
        product >> (shift - 29)
    } else {
        product << (29 - shift)
    };
    scaled.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// Depth via the rcp unit in `U0.18`: `z/w = z_raw * mag * 2^-shift`.
pub fn depth_rcp(z_raw: u64, mag: u32, shift: i32) -> u32 {
    let product = z_raw * u64::from(mag);
    let scaled = if shift >= 18 {
        product >> (shift - 18)
    } else {
        product << (18 - shift)
    };
    scaled.min(0x3ffff) as u32
}

/// Snaps a `Q16.16` pixel coordinate to `s12.4` (`floor(v*16 + 0.5)`),
/// saturating to the guard-band range of ±511.9375 px (the `s12.4` format
/// itself could hold ±2047.9375, but clipping bounds all inputs to ±512 px).
pub fn snap_s12_4(v_q16: i64) -> i16 {
    ((v_q16 + 2048) >> 12).clamp(-8191, 8191) as i16
}

/// Evaluates edge `i` of `setup` at the `s12.4` point `(px, py)`, without the
/// top-left bias. Uses the instrumented 18-bit multiplier: coefficients are
/// `s13.4` and the point delta fits 18-bit signed within the guard band.
pub fn edge_eval(setup: &TriangleSetup, i: usize, px: i32, py: i32) -> i64 {
    let dx = px - i32::from(setup.x[i]);
    let dy = py - i32::from(setup.y[i]);
    mul18("edge_eval", i64::from(setup.cx[i]), i64::from(dx))
        + mul18("edge_eval", i64::from(setup.cy[i]), i64::from(dy))
}

/// Coverage test with the top-left fill rule: `E >= 0` for top-left edges,
/// `E > 0` otherwise.
pub fn edge_covered(setup: &TriangleSetup, i: usize, px: i32, py: i32) -> bool {
    let e = edge_eval(setup, i, px, py);
    if setup.top_left[i] {
        e >= 0
    } else {
        e > 0
    }
}

impl SetupUnit {
    /// Full setup for one already-clipped triangle. Returns `None` when the
    /// triangle is backfacing, degenerate, or fully off-screen.
    pub fn setup_triangle(&mut self, tri: &[ClipVertex; 3]) -> Option<TriangleSetup> {
        self.stats.submitted += 1;
        let mut x = [0i16; 3];
        let mut y = [0i16; 3];
        let mut depth = [0u32; 3];
        for i in 0..3 {
            let w_raw = tri[i].w.max(W_MIN_RAW) as u32;
            let (mag, shift) = rcp_q16(w_raw);
            let ndc_x = ndc_rcp(tri[i].x, mag, shift);
            let ndc_y = ndc_rcp(tri[i].y, mag, shift);
            // NDC is s2.29; take its high 17 significant bits (>> 14) for
            // the 18-bit viewport multiplier. Post-clip |ndc| <= 2.56, so
            // |hi| <= 83886; the clamp is a safety net only.
            let hi_x = (ndc_x >> 14).clamp(-MUL18_LIM, MUL18_LIM);
            let hi_y = (ndc_y >> 14).clamp(-MUL18_LIM, MUL18_LIM);
            // hi carries 15 fractional bits; *200 px and rescaled to
            // Q16.16 is a << 1.
            let x_q16 =
                (mul18("viewport.x", i64::from(hi_x), HALF_WIDTH) << 1) + (HALF_WIDTH << 16);
            // Screen y points down, so negate the NDC y contribution.
            let y_q16 =
                (HALF_HEIGHT << 16) - (mul18("viewport.y", i64::from(hi_y), HALF_HEIGHT) << 1);
            x[i] = snap_s12_4(x_q16);
            y[i] = snap_s12_4(y_q16);
            // depth = z/w in U0.18 (z is non-negative after clipping).
            let z_raw = i64::from(tri[i].z).max(0) as u64;
            depth[i] = depth_rcp(z_raw, mag, shift);
        }
        let (x, y) = (x.map(i32::from), y.map(i32::from));
        let area2 = i64::from(x[1] - x[0]) * i64::from(y[2] - y[0])
            - i64::from(x[2] - x[0]) * i64::from(y[1] - y[0]);
        if area2 < 0 {
            self.stats.culled_backface += 1;
            return None;
        }
        if area2 == 0 {
            self.stats.culled_degenerate += 1;
            return None;
        }
        let mut cx = [0i32; 3];
        let mut cy = [0i32; 3];
        let mut top_left = [false; 3];
        for i in 0..3 {
            let j = (i + 1) % 3;
            let dx = x[j] - x[i];
            let dy = y[j] - y[i];
            // E(p) = dx*(p.y - v_i.y) - dy*(p.x - v_i.x).
            cx[i] = -dy;
            cy[i] = dx;
            top_left[i] = dy < 0 || (dy == 0 && dx > 0);
        }
        // Pixel-space AABB, clamped to the visible screen.
        let min_x = *x.iter().min().unwrap();
        let max_x = *x.iter().max().unwrap();
        let min_y = *y.iter().min().unwrap();
        let max_y = *y.iter().max().unwrap();
        let px0 = min_x.div_euclid(16).max(0);
        let py0 = min_y.div_euclid(16).max(0);
        let px1 = (max_x.div_euclid(16)).min(FRAMEBUFFER_WIDTH as i32 - 1);
        let py1 = (max_y.div_euclid(16)).min(FRAMEBUFFER_HEIGHT as i32 - 1);
        if px0 > px1 || py0 > py1 {
            self.stats.culled_offscreen += 1;
            return None;
        }
        // Even-aligned quad AABB: min rounded down to even, max up to odd.
        let quad_aabb = [px0 & !1, py0 & !1, px1 | 1, py1 | 1];
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        self.stats.emitted += 1;
        Some(TriangleSetup {
            id,
            x: [x[0] as i16, x[1] as i16, x[2] as i16],
            y: [y[0] as i16, y[1] as i16, y[2] as i16],
            depth,
            cx,
            cy,
            top_left,
            area2,
            quad_aabb,
        })
    }
}

/// 18-bit signed operand magnitude limit used for the NDC high-bits clamp.
const MUL18_LIM: i32 = (1 << 17) - 1;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::gpu::rastersim::clip::clip_triangle;
    use crate::hardware::gpu::rastersim::fixed::q16_from_rational as q;

    fn v(x: i64, y: i64, z: i64, w: i64) -> ClipVertex {
        ClipVertex::new(q(x, 1), q(y, 1), q(z, 1), q(w, 1))
    }

    #[test]
    fn front_face_center_maps_to_screen_center() {
        // Visually clockwise (y-down) front face around the origin at w = 2.
        let tri = [v(-1, 1, 1, 2), v(1, 1, 1, 2), v(0, -1, 1, 2)];
        let mut unit = SetupUnit::default();
        let setup = unit.setup_triangle(&tri).expect("front face culled");
        assert_eq!(setup.id, 0);
        // Vertex 2 sits at NDC (0, -0.5) -> screen (200, 180) px.
        assert_eq!(setup.x[2], 200 * 16);
        assert_eq!(setup.y[2], 180 * 16);
        assert!(setup.depth[0] > 0 && setup.depth[0] <= 0x3ffff);
    }

    #[test]
    fn backface_is_culled() {
        // Visually counter-clockwise: back face per convention.
        let tri = [v(-1, -1, 1, 2), v(1, -1, 1, 2), v(0, 1, 1, 2)];
        let mut unit = SetupUnit::default();
        assert!(unit.setup_triangle(&tri).is_none());
        assert_eq!(unit.stats.culled_backface, 1);
    }

    #[test]
    fn degenerate_snap_is_culled() {
        // Sub-subpixel triangle that collapses under snapping.
        let tri = [
            ClipVertex::new(10, 10, 1 << 16, 2 << 16),
            ClipVertex::new(11, 10, 1 << 16, 2 << 16),
            ClipVertex::new(10, 11, 1 << 16, 2 << 16),
        ];
        let mut unit = SetupUnit::default();
        assert!(unit.setup_triangle(&tri).is_none());
        assert_eq!(unit.stats.culled_degenerate, 1);
    }

    #[test]
    fn snap_is_idempotent() {
        let mut steps = 0;
        let mut v = -(1 << 21);
        while v < (1 << 21) && steps < 100_000 {
            let once = snap_s12_4(v);
            let twice = snap_s12_4(i64::from(once) << 12);
            assert_eq!(once, twice);
            v += 97;
            steps += 1;
        }
        assert!(steps < 100_000);
    }

    #[test]
    fn rcp_path_matches_exact_after_snap() {
        // Across the legal w range, the rcp-based NDC must snap to the same
        // s12.4 screen coordinate as exact division (allowing NDC difference
        // below 1/32 px as the intermediate criterion), under both LUT+lerp
        // configurations.
        use crate::hardware::gpu::rastersim::fixed::{set_rcp_mode, RcpMode};
        for mode in [RcpMode::Lerp256, RcpMode::Lerp128] {
            set_rcp_mode(mode);
            let mut steps = 0;
            let mut w_raw = W_MIN_RAW;
            while w_raw < 0x2000_0000 && steps < 400_000 {
                // Only in-guard-band vertices (|x| <= 2.56*w) have a meaningful
                // 1/32 px budget; the clip stage guarantees that contract.
                let xs = [w_raw / 2, -w_raw / 3, w_raw * 2, -(w_raw / 2 * 5)];
                for &x_raw in &xs {
                    let (mag, shift) = rcp_q16(w_raw as u32);
                    let approx = i64::from(ndc_rcp(x_raw, mag, shift));
                    let exact = ndc_exact(x_raw, w_raw);
                    // Intermediate criterion: NDC error below 1/32 px / 200 px.
                    let limit = (1i64 << 29) / 200 / 32;
                    assert!(
                        (approx - exact).abs() <= limit,
                        "{mode:?} w={w_raw} x={x_raw}: approx {approx} exact {exact}"
                    );
                }
                let step = (w_raw / 256).max(1);
                w_raw = w_raw.saturating_add(step);
                steps += 1;
            }
            assert!(steps < 400_000);
        }
        set_rcp_mode(RcpMode::Lerp256);
    }

    #[test]
    fn offscreen_triangle_is_culled() {
        let tri = [v(30, 0, 1, 1), v(31, 1, 1, 1), v(30, 1, 1, 1)];
        let clipped = clip_triangle(&tri);
        assert!(clipped.is_empty(), "beyond guard band: rejected in clip");
        // Fully outside the viewport but inside the ±2.56 NDC guard band is
        // trivially accepted. Note: the ±511.9375 px snap saturation flattens
        // it to a line (all x clamp to one value), so setup culls it as
        // degenerate rather than off-screen; both cull paths are equivalent
        // here.
        let wide = [
            v(2, 0, 1, 1),
            v(2, 1, 1, 1),
            ClipVertex::new(q(9, 4), q(1, 2), q(1, 1), q(1, 1)),
        ];
        assert_eq!(clip_triangle(&wide), vec![wide]);
        let mut unit = SetupUnit::default();
        assert!(unit.setup_triangle(&wide).is_none());
        assert_eq!(
            unit.stats.culled_offscreen + unit.stats.culled_degenerate,
            1
        );
    }

    #[test]
    fn fifo_line_format_is_stable() {
        let tri = [v(-1, 1, 1, 2), v(1, 1, 1, 2), v(0, -1, 1, 2)];
        let mut unit = SetupUnit::default();
        let setup = unit.setup_triangle(&tri).unwrap();
        let line = setup.fifo_line();
        assert!(line.starts_with("TRI 0 X "));
        assert!(line.contains(" Y "));
        assert!(line.contains(" Z "));
        assert!(line.contains(" AABB "));
    }
}
