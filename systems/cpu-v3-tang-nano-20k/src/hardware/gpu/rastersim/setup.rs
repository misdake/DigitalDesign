//! Triangle setup for the standalone rasterizer functional sim.
//!
//! Pipeline per clipped clip-space triangle:
//!
//! 1. `rcp(w)` per vertex (`RcpUnit`: LUT + slope lerp, one small multiply);
//! 2. NDC `x,y = (x,y)*rcp(w)` as 32-bit `S2_29` (range ±4; the ±512 px
//!    guard band spans ±2.56 NDC units, so no wider intermediate is needed
//!    and the whole NDC/viewport path stays 32-bit); the viewport multiplier
//!    consumes the high 17 significant bits;
//! 3. depth `z*rcp(w)` as `U0_18`, saturated to its range;
//! 4. viewport transform and subpixel snap to `S12_4` via
//!    `floor(v*16 + 0.5)`, saturated to ±511.9375 px;
//! 5. signed area and backface cull (front faces are visually clockwise in
//!    the y-down screen, i.e. positive signed area);
//! 6. edge coefficients `cx, cy` (`S13_4`, 18-bit) and the top-left rule
//!    (an edge is top-left when it goes up, or is horizontal going right;
//!    non-top-left edges are covered only when `E > 0` strictly);
//! 7. pixel/tile AABB plus the even-aligned quad AABB.
//!
//! All arithmetic goes through the devices in `devices.rs` (adders wrap and
//! report, multipliers record operand ranges, saturators count events) or
//! through class-3 checked format conversions that panic on violation.

use crate::hardware::gpu::rastersim::clip::{ClipVertex, W_MIN_RAW};
use crate::hardware::gpu::rastersim::devices::{
    Adder16, Adder32, Adder40, Multiplier18x18, Multiplier36x18, RcpOutput, RcpUnit, Saturator,
};
use crate::hardware::gpu::rastersim::fixed::{E40, Q16, S12_4, S13_4, S2_29, U0_18};
use crate::{FRAMEBUFFER_HEIGHT, FRAMEBUFFER_WIDTH};

/// Viewport half-extents in pixels.
pub const HALF_WIDTH: i64 = (FRAMEBUFFER_WIDTH / 2) as i64;
pub const HALF_HEIGHT: i64 = (FRAMEBUFFER_HEIGHT / 2) as i64;

/// Viewport center offsets as Q16.16 constants.
pub(crate) const VIEWPORT_CENTER_X: Q16 = Q16::from_raw_const(HALF_WIDTH << 16);
pub(crate) const VIEWPORT_CENTER_Y: Q16 = Q16::from_raw_const(HALF_HEIGHT << 16);

/// One triangle ready for rasterization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TriangleSetup {
    /// Rolling triangle id within a scene.
    pub id: u32,
    /// Snapped screen coordinates, `S12_4`.
    pub x: [S12_4; 3],
    pub y: [S12_4; 3],
    /// Per-vertex depth, `U0_18`.
    pub depth: [U0_18; 3],
    /// Edge `i` runs `v_i -> v_{i+1}`; coverage when
    /// `cx[i]*(px - x[i]) + cy[i]*(py - y[i])` passes the fill rule.
    pub cx: [S13_4; 3],
    pub cy: [S13_4; 3],
    pub top_left: [bool; 3],
    /// Twice the signed area in subpixel units; always positive here.
    pub area2: E40,
    /// Even-aligned pixel-space quad AABB, inclusive, clamped to the screen.
    pub quad_aabb: [i32; 4],
}

impl TriangleSetup {
    /// Text serialization of the triangle FIFO record, one line per entry.
    pub fn fifo_line(&self) -> String {
        format!(
            "TRI {} X {} {} {} Y {} {} {} Z {} {} {} AABB {} {} {} {}",
            self.id,
            self.x[0].raw(),
            self.x[1].raw(),
            self.x[2].raw(),
            self.y[0].raw(),
            self.y[1].raw(),
            self.y[2].raw(),
            self.depth[0].raw(),
            self.depth[1].raw(),
            self.depth[2].raw(),
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
    /// Rejected because a clipped vertex violates the legal-projection
    /// contract (`w >= 1/8` and `0 <= z <= w`). Counted, never silently
    /// clamped.
    pub culled_invalid_w: u64,
    pub emitted: u64,
}

/// Stateful setup unit holding the rolling triangle id and statistics.
#[derive(Default)]
pub struct SetupUnit {
    next_id: u32,
    pub stats: SetupStats,
}

/// NDC via the rcp unit: `x/w = x_raw * mag * 2^-shift`, as `S2_29`.
/// The x-magnitude-times-mag product is a 36x18 multiplier site; the
/// `S2_29::from_product` range check is class 3 (post-clip contract).
pub fn ndc_rcp(x: Q16, rcp: RcpOutput) -> S2_29 {
    let product = Multiplier36x18::mul_fx_raw("setup.ndc", x, rcp.mag_raw());
    let scaled = if rcp.shift >= 29 {
        product >> (rcp.shift - 29)
    } else {
        product << (29 - rcp.shift)
    };
    S2_29::from_product(scaled)
}

/// Depth via the rcp unit in `U0_18`: `z/w = z_raw * mag * 2^-shift`.
/// The final clamp is the depth saturator (class 1); the sign clamp on `z`
/// is guaranteed by the clip contract.
pub fn depth_rcp(z: Q16, rcp: RcpOutput) -> U0_18 {
    let product = Multiplier36x18::mul_fx_raw("setup.depth", z.clamp_min_zero(), rcp.mag_raw());
    let scaled = if rcp.shift >= 18 {
        product >> (rcp.shift - 18)
    } else {
        product << (18 - rcp.shift)
    };
    Saturator::depth_u0_18(scaled.max(0) as u64)
}

/// Evaluates edge `i` of `setup` at the `S12_4` pixel-center point `(px, py)`,
/// without the top-left bias. Two 18x18 multiplier sites plus a 40-bit
/// accumulator: coefficients are `S13_4` and the point delta fits 16-bit
/// signed within the guard band.
pub fn edge_eval(setup: &TriangleSetup, i: usize, px: S12_4, py: S12_4) -> E40 {
    let dx = S13_4::from_diff(Adder16::sub_fx("edge.delta", px, setup.x[i]));
    let dy = S13_4::from_diff(Adder16::sub_fx("edge.delta", py, setup.y[i]));
    let term_x = Multiplier18x18::mul_fx("edge_eval", setup.cx[i], dx);
    let term_y = Multiplier18x18::mul_fx("edge_eval", setup.cy[i], dy);
    E40::from_raw(Adder40::add("edge.accum", term_x, term_y))
}

/// The top-left fill rule on an edge value: `E >= 0` for top-left edges,
/// `E > 0` otherwise.
pub fn fill_rule(top_left: bool, e: E40) -> bool {
    if top_left {
        e >= E40::zero()
    } else {
        e > E40::zero()
    }
}

/// Coverage test with the top-left fill rule: `E >= 0` for top-left edges,
/// `E > 0` otherwise.
pub fn edge_covered(setup: &TriangleSetup, i: usize, px: S12_4, py: S12_4) -> bool {
    fill_rule(setup.top_left[i], edge_eval(setup, i, px, py))
}

impl SetupUnit {
    /// Full setup for one already-clipped triangle. Returns `None` when the
    /// triangle is backfacing, degenerate, or fully off-screen. Composes the
    /// leaf functions below; the cycle emu steps through the same leaves.
    pub fn setup_triangle(&mut self, tri: &[ClipVertex; 3]) -> Option<TriangleSetup> {
        self.stats.submitted += 1;
        // Legal-projection validation: reject (never clamp) primitives whose
        // clipped vertices violate `w >= 1/8` or `0 <= z <= w` (with a small
        // epsilon for intersection rounding).
        if tri.iter().any(|v| !clip_vertex_valid(v)) {
            self.stats.culled_invalid_w += 1;
            return None;
        }
        let mut x = [S12_4::from_raw_const(0); 3];
        let mut y = [S12_4::from_raw_const(0); 3];
        let mut depth = [U0_18::from_raw_const(0); 3];
        for i in 0..3 {
            let (sx, sy, d) = setup_vertex(&tri[i]);
            x[i] = sx;
            y[i] = sy;
            depth[i] = d;
        }
        // Twice the signed area: two 18x18 products and a 40-bit subtract.
        let area2 = area2(&x, &y);
        if area2.is_negative() {
            self.stats.culled_backface += 1;
            return None;
        }
        if area2 == E40::zero() {
            self.stats.culled_degenerate += 1;
            return None;
        }
        let (cx, cy, top_left) = edge_coefficients(&x, &y);
        let Some(quad_aabb) = quad_aabb(&x, &y) else {
            self.stats.culled_offscreen += 1;
            return None;
        };
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        self.stats.emitted += 1;
        Some(TriangleSetup {
            id,
            x,
            y,
            depth,
            cx,
            cy,
            top_left,
            area2,
            quad_aabb,
        })
    }
}

/// Legal-projection validation leaf: after clipping, a vertex must satisfy
/// `w >= 1/8` (the documented near limit) and `0 <= z <= w`. Intersection
/// rounding can push z a few raw units past the plane, so the z checks carry
/// an epsilon; the w check is hard.
pub fn clip_vertex_valid(v: &ClipVertex) -> bool {
    const Z_EPSILON: i64 = 1024;
    v.w.raw() >= i64::from(W_MIN_RAW)
        && v.z.raw() >= -Z_EPSILON
        && v.z.raw() - v.w.raw() <= Z_EPSILON
}

/// Per-vertex setup leaf: rcp, NDC, viewport multiply, snap, depth — exactly
/// the steps `setup_triangle` performs per vertex. The caller has already
/// validated `v.w >= 1/8` (`clip_vertex_valid`).
pub fn setup_vertex(v: &ClipVertex) -> (S12_4, S12_4, U0_18) {
    let rcp = RcpUnit::rcp_q16(v.w);
    let ndc_x = ndc_rcp(v.x, rcp);
    let ndc_y = ndc_rcp(v.y, rcp);
    // NDC is s2.29; take its high 17 significant bits for the 18-bit
    // viewport multiplier (class-3 checked in `high17`).
    let hi_x = ndc_x.high17();
    let hi_y = ndc_y.high17();
    // hi carries 15 fractional bits; *200 px and rescaled to
    // Q16.16 is a << 1.
    let prod_x = Q16::from_product(Multiplier18x18::mul("viewport.x", hi_x, HALF_WIDTH) << 1);
    let x_q16 = Q16::from_product(Adder32::add_fx(
        "viewport.offset",
        prod_x,
        VIEWPORT_CENTER_X,
    ));
    // Screen y points down, so negate the NDC y contribution.
    let prod_y = Q16::from_product(Multiplier18x18::mul("viewport.y", hi_y, HALF_HEIGHT) << 1);
    let y_q16 = Q16::from_product(Adder32::sub_fx(
        "viewport.offset",
        VIEWPORT_CENTER_Y,
        prod_y,
    ));
    let sx = Saturator::snap_s12_4(x_q16);
    let sy = Saturator::snap_s12_4(y_q16);
    // depth = z/w in U0.18 (z is non-negative after clipping).
    (sx, sy, depth_rcp(v.z, rcp))
}

/// Twice the signed area of the snapped triangle (E40).
pub fn area2(x: &[S12_4; 3], y: &[S12_4; 3]) -> E40 {
    // Coordinate deltas of saturated s12.4 values fit 16-bit adders.
    let dx10 = S13_4::from_diff(Adder16::sub_fx("setup.delta", x[1], x[0]));
    let dy10 = S13_4::from_diff(Adder16::sub_fx("setup.delta", y[1], y[0]));
    let dx20 = S13_4::from_diff(Adder16::sub_fx("setup.delta", x[2], x[0]));
    let dy20 = S13_4::from_diff(Adder16::sub_fx("setup.delta", y[2], y[0]));
    E40::from_raw(Adder40::sub(
        "setup.area",
        Multiplier18x18::mul_fx("setup.area", dx10, dy20),
        Multiplier18x18::mul_fx("setup.area", dx20, dy10),
    ))
}

/// Edge coefficients and the top-left rule per edge. Edge `i` runs
/// `v_i -> v_{i+1}`; `E(p) = dx*(p.y - v_i.y) - dy*(p.x - v_i.x)` gives
/// `cx = -dy`, `cy = dx`. An edge is top-left when it goes up, or is
/// horizontal going right.
pub fn edge_coefficients(x: &[S12_4; 3], y: &[S12_4; 3]) -> ([S13_4; 3], [S13_4; 3], [bool; 3]) {
    let mut cx = [S13_4::from_raw_const(0); 3];
    let mut cy = [S13_4::from_raw_const(0); 3];
    let mut top_left = [false; 3];
    for i in 0..3 {
        let j = (i + 1) % 3;
        let dx = S13_4::from_diff(Adder16::sub_fx("setup.delta", x[j], x[i]));
        let dy = S13_4::from_diff(Adder16::sub_fx("setup.delta", y[j], y[i]));
        cx[i] = -dy;
        cy[i] = dx;
        top_left[i] = dy.is_negative() || (dy == S13_4::zero() && dx > S13_4::zero());
    }
    (cx, cy, top_left)
}

/// Screen-clamped pixel AABB plus even-aligned quad AABB. `None` when the
/// clamped box is empty (fully off-screen).
pub fn quad_aabb(x: &[S12_4; 3], y: &[S12_4; 3]) -> Option<[i32; 4]> {
    // Min/max and the divide-by-16 (a shift in hardware) carry no overflow
    // risk.
    let min_x = x.iter().min().unwrap();
    let max_x = x.iter().max().unwrap();
    let min_y = y.iter().min().unwrap();
    let max_y = y.iter().max().unwrap();
    let px0 = min_x.pixel_floor().max(0);
    let py0 = min_y.pixel_floor().max(0);
    let px1 = max_x.pixel_floor().min((FRAMEBUFFER_WIDTH - 1) as i32);
    let py1 = max_y.pixel_floor().min((FRAMEBUFFER_HEIGHT - 1) as i32);
    if px0 > px1 || py0 > py1 {
        return None;
    }
    // Even-aligned quad AABB: min rounded down to even, max up to odd.
    Some([px0 & !1, py0 & !1, px1 | 1, py1 | 1])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::gpu::rastersim::clip::{clip_triangle, W_MIN_RAW};
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
        assert_eq!(setup.x[2].raw(), 200 * 16);
        assert_eq!(setup.y[2].raw(), 180 * 16);
        assert!(setup.depth[0].raw() > 0 && setup.depth[0].raw() <= 0x3ffff);
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
            ClipVertex::new(
                Q16::from_raw_const(10),
                Q16::from_raw_const(10),
                Q16::from_raw_const(1 << 16),
                Q16::from_raw_const(2 << 16),
            ),
            ClipVertex::new(
                Q16::from_raw_const(11),
                Q16::from_raw_const(10),
                Q16::from_raw_const(1 << 16),
                Q16::from_raw_const(2 << 16),
            ),
            ClipVertex::new(
                Q16::from_raw_const(10),
                Q16::from_raw_const(11),
                Q16::from_raw_const(1 << 16),
                Q16::from_raw_const(2 << 16),
            ),
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
            let once = Saturator::snap_s12_4(Q16::from_raw_const(v));
            let twice = Saturator::snap_s12_4(Q16::from_raw_const(once.raw() << 12));
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
        use crate::hardware::gpu::rastersim::contract;
        use crate::hardware::gpu::rastersim::fixed::{set_rcp_mode, RcpMode};
        use crate::hardware::gpu::rastersim::oracle::ndc_exact;
        for mode in [RcpMode::Lerp256, RcpMode::Lerp128] {
            set_rcp_mode(mode);
            let mut steps = 0;
            let mut w_raw = W_MIN_RAW as i64;
            while w_raw < 0x2000_0000 && steps < 400_000 {
                // Only in-guard-band vertices (|x| <= 2.56*w) have a meaningful
                // 1/32 px budget; the clip stage guarantees that contract.
                let xs = [w_raw / 2, -w_raw / 3, w_raw * 2, -(w_raw / 2 * 5)];
                for &x_raw in &xs {
                    let rcp = RcpUnit::rcp_q16(Q16::from_raw_const(w_raw));
                    let approx = ndc_rcp(Q16::from_raw_const(x_raw), rcp).raw();
                    let exact = ndc_exact(Q16::from_raw_const(x_raw), w_raw);
                    // Contract bound: NDC error below 1/32 px / 200 px.
                    let limit = contract::ndc::ABS_LIMIT_S2_29;
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
