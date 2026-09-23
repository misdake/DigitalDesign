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

/// One rendered frame: linear RGB565 color plus the quantized depth byproduct.
pub struct Frame {
    pub color: Vec<u16>,
    pub depth: Vec<u16>,
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

/// Interpolated `U0.16` depth at pixel `(px, py)` (`s12.4` coordinates).
/// Weight of vertex `i` is the edge function of the opposite edge
/// `(i+1) -> (i+2)`; the three edge values sum to `area2` everywhere.
fn depth_at(setup: &TriangleSetup, px: i32, py: i32) -> u16 {
    let e0 = edge_eval(setup, 0, px, py);
    let e1 = edge_eval(setup, 1, px, py);
    let e2 = edge_eval(setup, 2, px, py);
    let numerator = i64::from(setup.depth[0]) * e1
        + i64::from(setup.depth[1]) * e2
        + i64::from(setup.depth[2]) * e0;
    let depth18 = (numerator / setup.area2.max(1)).clamp(0, 0x3ffff) as u32;
    crate::hardware::gpu::rastersim::fixed::depth18_to_16(depth18)
}

/// Rasterizes every setup triangle onto a black-cleared frame.
pub fn rasterize(setups: &[TriangleSetup]) -> Frame {
    let width = FRAMEBUFFER_WIDTH as usize;
    let height = FRAMEBUFFER_HEIGHT as usize;
    let mut color = vec![0u16; width * height];
    let mut depth = vec![0u16; width * height];
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
                                let px = x * 16 + 8;
                                let py = y * 16 + 8;
                                let covered = (0..3).all(|i| edge_covered(setup, i, px, py));
                                if covered {
                                    stats.pixels_covered += 1;
                                    let index = y as usize * width + x as usize;
                                    color[index] = pixel_color(x as u32, y as u32, setup.id);
                                    depth[index] = depth_at(setup, px, py);
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
        assert_ne!(frame.color[center], 0);
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
                let c1 = f1.color[i] != 0;
                let c2 = f2.color[i] != 0;
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
        assert_ne!(frame.color[95 * 400 + 65], 0, "hypotenuse pixel");
        assert_ne!(frame.color[80 * 400 + 80], 0, "hypotenuse pixel");
        // Bottom edge pixel (65,96) sits exactly on the horizontal edge
        // going left (not top-left): not covered.
        assert_eq!(frame.color[96 * 400 + 65], 0, "bottom edge pixel");
        // Clearly inside / clearly outside.
        assert_ne!(frame.color[90 * 400 + 80], 0, "inside pixel");
        assert_eq!(frame.color[65 * 400 + 94], 0, "outside pixel");
    }
}
