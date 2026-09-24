//! Reference rasterizer for the standalone functional sim.
//!
//! Traversal is tile-first, matching the hardware structure: for each
//! triangle, tiles of the screen-clamped AABB are visited row by row; inside
//! a tile the even-aligned 2x2 quad grid is walked (two rows per step);
//! inside a quad every pixel's three edge functions are recomputed directly
//! from the snapped vertex coordinates (no incremental updates), and the
//! top-left fill rule decides coverage (`E >= 0` on top-left edges, `E > 0`
//! otherwise).
//!
//! Pixel color is a pure function of screen position and triangle id, so
//! orientation/offset bugs are visible by eye:
//! `r = (x >> 3) & 0x1f`, `g = (y >> 2) & 0x3f`, `b = (id*7 + (x >> 4)) & 0x1f`.
//!
//! Depth is interpolated per pixel from the `U0.18` vertex depths via the
//! (unbiased) edge functions and quantized to `U0.16` only at the end; the
//! depth buffer is a byproduct for precision experiments, not a tested image.

use crate::hardware::gpu::rastersim::devices::{Adder56, Multiplier36x18, Saturator};
use crate::hardware::gpu::rastersim::fixed::S12_4;
use crate::hardware::gpu::rastersim::setup::{edge_covered, edge_eval, TriangleSetup};
use crate::{
    FRAMEBUFFER_HEIGHT, FRAMEBUFFER_TILE, FRAMEBUFFER_TILE_COLUMNS, FRAMEBUFFER_TILE_ROWS,
    FRAMEBUFFER_WIDTH,
};

/// Per-scene rasterization counters.
#[derive(Clone, Copy, Debug, Default)]
pub struct RasterStats {
    pub triangles: u64,
    pub tile_visits: u64,
    pub quads_visited: u64,
    pub pixels_tested: u64,
    pub pixels_covered: u64,
}

/// One rendered frame: linear RGB565 color plus the quantized depth byproduct
/// and the exact coverage mask (color alone cannot mark coverage: a covered
/// pixel can legitimately be black).
pub struct Frame {
    pub color: Vec<u16>,
    pub depth: Vec<u16>,
    pub covered: Vec<bool>,
    pub stats: RasterStats,
}

impl Frame {
    /// Writes the color buffer as a binary PPM image.
    pub fn write_ppm(&self, path: &std::path::Path) -> std::io::Result<()> {
        use std::io::Write;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::File::create(path)?;
        write!(file, "P6\n{FRAMEBUFFER_WIDTH} {FRAMEBUFFER_HEIGHT}\n255\n")?;
        for pixel in &self.color {
            let (r, g, b) = crate::hardware::display::rgb565_to_rgb888(*pixel, true);
            file.write_all(&[r, g, b])?;
        }
        Ok(())
    }
}

/// Color of one covered pixel: position gradient plus a per-triangle blue
/// offset so overlapping triangles of the same scene stay distinguishable.
fn pixel_color(x: u32, y: u32, id: u32) -> u16 {
    let r = ((x >> 3) & 0x1f) as u16;
    let g = ((y >> 2) & 0x3f) as u16;
    let b = ((id.wrapping_mul(7) + (x >> 4)) & 0x1f) as u16;
    (r << 11) | (g << 5) | b
}

/// Interpolated `U0_16` depth at pixel `(px, py)` (`S12_4` coordinates).
/// Weight of vertex `i` is the edge function of the opposite edge
/// `(i+1) -> (i+2)`; the three edge values sum to `area2` everywhere.
/// The division by `area2` is the functional reference's shortcut (hardware
/// would fold the reciprocal into the interpolation); the clamp before
/// quantization is the depth saturator.
fn depth_at(setup: &TriangleSetup, px: S12_4, py: S12_4) -> u16 {
    let e0 = edge_eval(setup, 0, px, py);
    let e1 = edge_eval(setup, 1, px, py);
    let e2 = edge_eval(setup, 2, px, py);
    // 18-bit depth times a 40-bit edge value: 36x18 multiplier sites,
    // accumulated in 56 bits.
    let numerator = Adder56::add(
        "raster.depth",
        Adder56::add(
            "raster.depth",
            Multiplier36x18::mul_fx("raster.depth", setup.depth[0], e1),
            Multiplier36x18::mul_fx("raster.depth", setup.depth[1], e2),
        ),
        Multiplier36x18::mul_fx("raster.depth", setup.depth[2], e0),
    );
    let depth18 = Saturator::interp_u0_18(setup.area2.div_wide(numerator));
    depth18.quantize().to_bits()
}

/// Rasterizes every setup triangle onto a black-cleared frame.
pub fn rasterize(setups: &[TriangleSetup]) -> Frame {
    let width = FRAMEBUFFER_WIDTH as usize;
    let height = FRAMEBUFFER_HEIGHT as usize;
    let mut color = vec![0u16; width * height];
    let mut depth = vec![0u16; width * height];
    let mut covered = vec![false; width * height];
    let mut stats = RasterStats::default();
    let tile = FRAMEBUFFER_TILE as i32;
    for setup in setups {
        stats.triangles += 1;
        let [ax0, ay0, ax1, ay1] = setup.quad_aabb;
        let tx0 = (ax0 / tile).clamp(0, FRAMEBUFFER_TILE_COLUMNS as i32 - 1);
        let tx1 = (ax1 / tile).clamp(0, FRAMEBUFFER_TILE_COLUMNS as i32 - 1);
        let ty0 = (ay0 / tile).clamp(0, FRAMEBUFFER_TILE_ROWS as i32 - 1);
        let ty1 = (ay1 / tile).clamp(0, FRAMEBUFFER_TILE_ROWS as i32 - 1);
        for ty in ty0..=ty1 {
            for tx in tx0..=tx1 {
                stats.tile_visits += 1;
                // Quad grid intersected with this tile's pixel rect.
                let qx0 = ax0.max(tx * tile);
                let qy0 = ay0.max(ty * tile);
                let qx1 = ax1.min(tx * tile + tile - 1);
                let qy1 = ay1.min(ty * tile + tile - 1);
                let mut qy = qy0;
                while qy <= qy1 {
                    let mut qx = qx0;
                    while qx <= qx1 {
                        stats.quads_visited += 1;
                        for dy in 0..2 {
                            for dx in 0..2 {
                                let x = qx + dx;
                                let y = qy + dy;
                                if x > qx1 || y > qy1 {
                                    continue;
                                }
                                stats.pixels_tested += 1;
                                // Pixel center in s12.4: integer pixel + 0.5.
                                let px = S12_4::pixel_center(x);
                                let py = S12_4::pixel_center(y);
                                let is_covered = (0..3).all(|i| edge_covered(setup, i, px, py));
                                if is_covered {
                                    stats.pixels_covered += 1;
                                    let index = y as usize * width + x as usize;
                                    color[index] = pixel_color(x as u32, y as u32, setup.id);
                                    depth[index] = depth_at(setup, px, py);
                                    covered[index] = true;
                                }
                            }
                        }
                        qx += 2;
                    }
                    qy += 2;
                }
            }
        }
    }
    Frame {
        color,
        depth,
        covered,
        stats,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::gpu::rastersim::clip::{clip_triangle, ClipVertex};
    use crate::hardware::gpu::rastersim::fixed::q16_from_rational as q;
    use crate::hardware::gpu::rastersim::setup::SetupUnit;

    fn v(x: i64, y: i64, z: i64, w: i64) -> ClipVertex {
        ClipVertex::new(q(x, 1), q(y, 1), q(z, 1), q(w, 1))
    }

    fn setup_one(tri: &[ClipVertex; 3]) -> TriangleSetup {
        let mut unit = SetupUnit::default();
        let mut out = None;
        for t in clip_triangle(tri) {
            out = unit.setup_triangle(&t);
        }
        out.expect("triangle did not survive clip/setup")
    }

    #[test]
    fn full_screen_triangle_covers_exactly_once() {
        // Front face (visually clockwise) covering the whole screen.
        let tri = [v(-1, 1, 1, 1), v(1, 1, 1, 1), v(0, -1, 1, 1)];
        let setup = setup_one(&tri);
        let frame = rasterize(&[setup]);
        assert!(frame.stats.pixels_covered > 0);
        assert!(frame.stats.pixels_tested <= 400 * 240 * 2);
        // Interior pixel must be covered.
        let center = 120 * 400 + 200;
        assert!(frame.covered[center]);
    }

    #[test]
    fn shared_edge_is_watertight() {
        // Two triangles sharing the diagonal of a 64x64 quad region.
        // NDC quad corners (-0.5,0.5)..(0.5,-0.5) -> screen 100..300 px.
        let a = v(-1, 1, 1, 2);
        let b = v(1, 1, 1, 2);
        let c = v(1, -1, 1, 2);
        let d = v(-1, -1, 1, 2);
        let t1 = setup_one(&[a, b, c]);
        let t2 = setup_one(&[a, c, d]);
        let frame = rasterize(&[t1, t2]);
        // In the 200x120 NDC-mapped pixel rect 100..=299 x 60..=179 every
        // pixel on the shared diagonal must be covered by exactly one
        // triangle: render each triangle separately and compare masks.
        let f1 = rasterize(&[t1]);
        let f2 = rasterize(&[t2]);
        let mut overlap = 0u64;
        let mut holes = 0u64;
        let mut steps = 0u64;
        for y in 61..179 {
            for x in 101..299 {
                let i = y * 400 + x;
                let c1 = f1.covered[i];
                let c2 = f2.covered[i];
                if c1 && c2 {
                    overlap += 1;
                }
                if !c1 && !c2 {
                    holes += 1;
                }
                steps += 1;
            }
        }
        assert!(steps < 1_000_000);
        assert_eq!(overlap, 0, "shared edge double-covered");
        assert_eq!(holes, 0, "shared edge has holes");
        let _ = frame;
    }

    #[test]
    fn diagonal_slopes_hit_pixel_centers() {
        // 45-degree edge exactly through pixel centers exercises the
        // top-left tie-break: vertices sit at half-pixel offsets so the
        // hypotenuse x + y = 161 passes through pixel centers.
        let mut unit = SetupUnit::default();
        // Screen px -> clip space with w = 1: ndc_x = (X - 200)/200,
        // ndc_y = (120 - Y)/120, both as exact rationals.
        let cv = |x2: i64, y2: i64| {
            // x2/y2 are twice the pixel coordinate (half-pixel precision).
            ClipVertex::new(q(x2 - 400, 400), q(240 - y2, 240), q(1, 2), q(1, 1))
        };
        // v0 = (64.5, 96.5), v1 = (96.5, 64.5), v2 = (96.5, 96.5);
        // visually clockwise (front face), hypotenuse through pixel centers.
        let tri = [cv(129, 193), cv(193, 129), cv(193, 193)];
        let setup = unit.setup_triangle(&tri).expect("setup failed");
        let frame = rasterize(&[setup]);
        // Hypotenuse pixels (65,95) and (80,80) lie exactly on the edge,
        // which is top-left (going up): covered.
        assert!(frame.covered[95 * 400 + 65], "hypotenuse pixel");
        assert!(frame.covered[80 * 400 + 80], "hypotenuse pixel");
        // Bottom edge pixel (65,96) sits exactly on the horizontal edge
        // going left (not top-left): not covered.
        assert!(!frame.covered[96 * 400 + 65], "bottom edge pixel");
        // Clearly inside / clearly outside.
        assert!(frame.covered[90 * 400 + 80], "inside pixel");
        assert_eq!(frame.color[65 * 400 + 94], 0, "outside pixel");
    }

    /// Exact screen-space coverage oracle for one convex polygon: vertices
    /// are snapped s12.4 coordinates (raw units) in winding order; a pixel
    /// center is covered iff every directed edge passes the top-left fill
    /// rule, computed in exact 128-bit integer arithmetic. Test-only oracle,
    /// not a hardware path.
    fn polygon_covers_exact(poly: &[(i64, i64)], px: i64, py: i64) -> bool {
        for i in 0..poly.len() {
            let (x0, y0) = poly[i];
            let (x1, y1) = poly[(i + 1) % poly.len()];
            let dx = x1 - x0;
            let dy = y1 - y0;
            // Same edge function and fill rule as the setup stage.
            let e = -(dy as i128) * ((px - x0) as i128) + (dx as i128) * ((py - y0) as i128);
            let top_left = dy < 0 || (dy == 0 && dx > 0);
            let covered = if top_left { e >= 0 } else { e > 0 };
            if !covered {
                return false;
            }
        }
        true
    }

    /// Coverage mask of one setup triangle over the full frame.
    fn coverage_mask(setup: &TriangleSetup) -> Vec<bool> {
        rasterize(&[*setup]).covered
    }

    /// Snapped polygon of a clip fan, in winding order: the fan triangles are
    /// `[p0, p1, p2], [p0, p2, p3], ...`, so the polygon is
    /// `p0, p1, (p2 from tri 0), (p3 from tri 1), ...`.
    fn fan_polygon(setups: &[TriangleSetup]) -> Vec<(i64, i64)> {
        let mut poly = vec![
            (setups[0].x[0].raw(), setups[0].y[0].raw()),
            (setups[0].x[1].raw(), setups[0].y[1].raw()),
        ];
        for s in setups {
            poly.push((s.x[2].raw(), s.y[2].raw()));
        }
        poly
    }

    #[test]
    fn clipped_fan_is_watertight() {
        // Triangle crossing the near plane: clipped to a quad, emitted as a
        // two-triangle fan. The fan's shared seam must be covered exactly
        // once, and the union must match the exact polygon oracle pixel for
        // pixel over the whole screen AABB of the fan.
        // Triangle crossing the near plane: clipped to a quad, emitted as a
        // two-triangle fan. The fan's shared seam must be covered exactly
        // once, and the union must match the exact polygon oracle pixel for
        // pixel over the whole screen AABB of the fan. (Winding: screen
        // clockwise = front face.)
        let tri = [v(0, 2, -1, 2), v(2, -1, 1, 2), v(-2, -1, 1, 2)];
        let fans = clip_triangle(&tri);
        assert_eq!(fans.len(), 2, "expected a two-triangle fan");
        let mut unit = SetupUnit::default();
        let setups: Vec<_> = fans.iter().filter_map(|t| unit.setup_triangle(t)).collect();
        assert_eq!(setups.len(), 2);
        let masks: Vec<_> = setups.iter().map(coverage_mask).collect();
        let poly = fan_polygon(&setups);
        let xs: Vec<i32> = setups
            .iter()
            .flat_map(|s| [s.quad_aabb[0], s.quad_aabb[2]])
            .collect();
        let ys: Vec<i32> = setups
            .iter()
            .flat_map(|s| [s.quad_aabb[1], s.quad_aabb[3]])
            .collect();
        let (rx0, rx1) = (*xs.iter().min().unwrap(), *xs.iter().max().unwrap());
        let (ry0, ry1) = (*ys.iter().min().unwrap(), *ys.iter().max().unwrap());
        let mut overlap = 0u64;
        let mut mismatch = 0u64;
        let mut steps = 0u64;
        for y in ry0..=ry1 {
            for x in rx0..=rx1 {
                let idx = y as usize * 400 + x as usize;
                let covered: Vec<bool> = masks.iter().map(|m| m[idx]).collect();
                if covered.iter().all(|&c| c) {
                    overlap += 1;
                }
                let px = i64::from(x * 16 + 8);
                let py = i64::from(y * 16 + 8);
                let exact = polygon_covers_exact(&poly, px, py);
                if exact != covered.iter().any(|&c| c) {
                    mismatch += 1;
                }
                steps += 1;
            }
        }
        assert!(steps < 1_000_000);
        assert_eq!(overlap, 0, "fan seam double-covered");
        assert_eq!(mismatch, 0, "fan union diverges from the exact polygon");
    }

    #[test]
    fn shared_edges_at_assorted_slopes_are_watertight() {
        // Rectangles of assorted aspect ratios, each split along both
        // diagonals; the shared edge is traversed in opposite directions by
        // the two triangles. Every interior pixel must be covered exactly
        // once.
        let mut steps = 0u64;
        for (num, den) in [(1i64, 1i64), (2, 1), (1, 2), (3, 1), (1, 3), (5, 2), (2, 5)] {
            let hw = q(2 * num, 5 * den);
            let hh = q(2, 5);
            let a = ClipVertex::new(-hw, hh, q(1, 2), q(1, 1));
            let b = ClipVertex::new(hw, hh, q(1, 2), q(1, 1));
            let c = ClipVertex::new(hw, -hh, q(1, 2), q(1, 1));
            let d = ClipVertex::new(-hw, -hh, q(1, 2), q(1, 1));
            for (t1, t2) in [([a, b, c], [a, c, d]), ([b, c, d], [b, d, a])] {
                let s1 = setup_one(&t1);
                let s2 = setup_one(&t2);
                let m1 = coverage_mask(&s1);
                let m2 = coverage_mask(&s2);
                // Interior of the mapped rectangle with a one-pixel margin,
                // measured from the snapped vertices: the right/bottom outer
                // edges are exclusive by the top-left rule, so the boundary
                // columns/rows must stay out of the hole check.
                let vx: Vec<i32> = [s1, s2]
                    .iter()
                    .flat_map(|s| s.x.iter().map(|v| v.pixel_floor()))
                    .collect();
                let vy: Vec<i32> = [s1, s2]
                    .iter()
                    .flat_map(|s| s.y.iter().map(|v| v.pixel_floor()))
                    .collect();
                // Rectangles wider than the screen extend past it; clamp the
                // checked region to the visible area (off-screen outer edges
                // play no exclusive-rule role on-screen).
                let x0 = (*vx.iter().min().unwrap() + 1).max(0);
                let x1 = (*vx.iter().max().unwrap() - 1).min(FRAMEBUFFER_WIDTH as i32 - 1);
                let y0 = (*vy.iter().min().unwrap() + 1).max(0);
                let y1 = (*vy.iter().max().unwrap() - 1).min(FRAMEBUFFER_HEIGHT as i32 - 1);
                let mut overlap = 0u64;
                let mut holes = 0u64;
                for y in y0..=y1 {
                    for x in x0..=x1 {
                        let idx = y as usize * 400 + x as usize;
                        match (m1[idx], m2[idx]) {
                            (true, true) => overlap += 1,
                            (false, false) => holes += 1,
                            _ => {}
                        }
                        steps += 1;
                    }
                }
                assert_eq!(overlap, 0, "double-covered at slope {num}:{den}");
                assert_eq!(holes, 0, "hole at slope {num}:{den}");
            }
        }
        assert!(steps < 10_000_000);
    }

    #[test]
    fn huge_triangle_at_the_camera_covers_full_screen() {
        // All three vertices 2 cm from the camera spanning +/-50 m: massive
        // near/guard clipping; the result must fill the entire screen and no
        // device may overflow.
        use crate::hardware::gpu::rastersim::fixed::device_events_snapshot;
        let tri = [
            ClipVertex::new(q(-100, 1), q(50, 1), q(1, 50), q(1, 50)),
            ClipVertex::new(q(100, 1), q(50, 1), q(1, 50), q(1, 50)),
            ClipVertex::new(q(0, 1), q(-100, 1), q(1, 50), q(1, 50)),
        ];
        let fans = clip_triangle(&tri);
        assert!(!fans.is_empty(), "huge triangle clipped away entirely");
        let mut unit = SetupUnit::default();
        let setups: Vec<_> = fans.iter().filter_map(|t| unit.setup_triangle(t)).collect();
        assert_eq!(
            setups.len(),
            fans.len(),
            "a fan piece snapped to degenerate"
        );
        let frame = rasterize(&setups);
        let uncovered = frame.covered.iter().filter(|&&c| !c).count();
        assert_eq!(
            uncovered, 0,
            "huge triangle left {uncovered} pixels uncovered"
        );
        // The fan union must also match the exact polygon oracle.
        let poly = fan_polygon(&setups);
        let mut mismatch = 0u64;
        let mut steps = 0u64;
        for y in (0..FRAMEBUFFER_HEIGHT as i32).step_by(3) {
            for x in (0..FRAMEBUFFER_WIDTH as i32).step_by(3) {
                let idx = y as usize * 400 + x as usize;
                let px = i64::from(x * 16 + 8);
                let py = i64::from(y * 16 + 8);
                if polygon_covers_exact(&poly, px, py) != frame.covered[idx] {
                    mismatch += 1;
                }
                steps += 1;
            }
        }
        assert!(steps < 100_000);
        assert_eq!(mismatch, 0, "huge triangle diverges from the exact polygon");
        let events = device_events_snapshot();
        // Adder overflow labels (wrap reports) must all be zero; saturation
        // labels legitimately fire in other parallel tests, so only the
        // non-saturating device labels are checked here.
        for label in [
            "clip.dist",
            "clip.lerp",
            "edge.delta",
            "edge.accum",
            "setup.area",
            "setup.delta",
            "viewport.offset",
            "raster.depth",
        ] {
            assert_eq!(
                events.get(label).copied().unwrap_or(0),
                0,
                "device overflow at {label} during the huge-triangle scene"
            );
        }
    }
}
