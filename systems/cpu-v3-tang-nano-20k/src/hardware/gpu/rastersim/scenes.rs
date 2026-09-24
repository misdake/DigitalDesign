//! Scene registry for the standalone rasterizer sim: the input source for
//! later emu/RTL co-simulation. All scene data is constructed from integers
//! and rationals — no floating point anywhere.
//!
//! Also hosts the performance scene (~20k triangles, CS-class scale) and the
//! reverse-Z depth precision experiment.

use crate::hardware::gpu::rastersim::clip::ClipVertex;
use crate::hardware::gpu::rastersim::fixed::Q16;

/// One named scene: a list of clip-space triangles.
pub struct Scene {
    pub name: &'static str,
    pub triangles: Vec<[ClipVertex; 3]>,
}

/// Clip-space vertex from rational coordinates `num/den` (Q16.16 raw).
fn cv(
    x_num: i64,
    x_den: i64,
    y_num: i64,
    y_den: i64,
    z_num: i64,
    z_den: i64,
    w: i64,
) -> ClipVertex {
    let q = |n: i64, d: i64| Q16::from_raw_const((n << 16) / d);
    ClipVertex::new(q(x_num, x_den), q(y_num, y_den), q(z_num, z_den), q(w, 1))
}

/// Screen-space triangle constructor. `p16` holds the three vertices in
/// 1/16-pixel units; `z_num/z_den` is the NDC depth. `w` is 48000 so all
/// subpixel positions stay exact integers in Q16.16.
fn screen_tri(p16: [[i64; 2]; 3], z_num: i64, z_den: i64) -> [ClipVertex; 3] {
    let w = 48000i64;
    let z = z_num * w / z_den;
    p16.map(|[x16, y16]| {
        // ndc_x = (X - 200)/200 with X = x16/16 -> x = ndc * w.
        let x = (x16 - 3200) * 15;
        let y = (1920 - y16) * 25;
        ClipVertex::new(
            Q16::from_raw_const(x),
            Q16::from_raw_const(y),
            Q16::from_raw_const(z),
            Q16::from_raw_const(w),
        )
    })
}

/// The full scene registry.
pub fn scenes() -> Vec<Scene> {
    let mut scenes = Vec::new();
    let w2 = |x: i64, y: i64| cv(x, 2, y, 2, 1, 4, 2);
    let px = |v: i64| v * 16; // whole pixels -> 1/16 px units

    // Degenerate: zero area, collinear, snap-degenerate.
    scenes.push(Scene {
        name: "degenerate-zero-area",
        triangles: vec![[w2(-1, 1), w2(-1, 1), w2(-1, 1)]],
    });
    scenes.push(Scene {
        name: "degenerate-collinear",
        triangles: vec![[w2(-1, 0), w2(0, 0), w2(1, 0)]],
    });
    scenes.push(Scene {
        name: "degenerate-snap-collapse",
        // All three vertices within one subpixel: collapses at snap.
        triangles: vec![[
            cv(0, 1024, 0, 1024, 1, 2, 1),
            cv(1, 1024, 0, 1024, 1, 2, 1),
            cv(0, 1024, 1, 1024, 1, 2, 1),
        ]],
    });

    // Slivers: one-subpixel-wide triangles in several orientations.
    scenes.push(Scene {
        name: "sliver-horizontal",
        triangles: vec![screen_tri(
            [
                [px(100), px(100)],
                [px(164), px(100)],
                [px(132), px(100) + 1],
            ],
            1,
            2,
        )],
    });
    scenes.push(Scene {
        name: "sliver-vertical",
        triangles: vec![screen_tri(
            [[px(100), px(40)], [px(100) + 1, px(72)], [px(100), px(104)]],
            1,
            2,
        )],
    });
    // 45-degree sliver: base on the exact 45-degree line, apex one subpixel
    // perpendicular off it.
    scenes.push(Scene {
        name: "sliver-diag45",
        triangles: vec![screen_tri(
            [
                [px(60), px(120)],
                [px(124), px(184)],
                [px(124) - 1, px(184) + 1],
            ],
            1,
            2,
        )],
    });
    scenes.push(Scene {
        name: "sliver-diag45-minus-subpixel",
        triangles: vec![screen_tri(
            [
                [px(60), px(120)],
                [px(124), px(184)],
                [px(124), px(184) + 1],
            ],
            1,
            2,
        )],
    });
    scenes.push(Scene {
        name: "sliver-diag45-plus-subpixel",
        triangles: vec![screen_tri(
            [
                [px(60), px(120)],
                [px(124) + 1, px(184)],
                [px(124), px(184)],
            ],
            1,
            2,
        )],
    });

    // Diagonal edges: 45, 26.57 (1:2), 18.43 (1:3) degrees, steep/shallow,
    // crossing tile boundaries and pixel centers. All visually clockwise.
    let diag = |name: &'static str, tri: [[i64; 2]; 3]| Scene {
        name,
        triangles: vec![screen_tri(tri, 1, 2)],
    };
    scenes.push(diag(
        "diag-45",
        [[px(32), px(48)], [px(96), px(48)], [px(96), px(112)]],
    ));
    scenes.push(diag(
        "diag-26.57",
        [[px(32), px(96)], [px(160), px(96)], [px(160), px(160)]],
    ));
    scenes.push(diag(
        "diag-18.43",
        [[px(32), px(80)], [px(224), px(80)], [px(224), px(144)]],
    ));
    scenes.push(diag(
        "diag-steep",
        [[px(48), px(16)], [px(80), px(224)], [px(48), px(224)]],
    ));
    // 45-degree hypotenuse exactly through pixel centers (top-left rule).
    scenes.push(diag(
        "diag-45-pixel-centers",
        [
            [px(64) + 8, px(96) + 8],
            [px(96) + 8, px(64) + 8],
            [px(96) + 8, px(96) + 8],
        ],
    ));
    // Hypotenuse crossing several tile boundaries (tile size 16 px).
    scenes.push(diag(
        "diag-crossing-tiles",
        [[px(15), px(225)], [px(225), px(15)], [px(225), px(225)]],
    ));

    // Winding: front (visually clockwise) and back (counter-clockwise).
    scenes.push(Scene {
        name: "winding-front",
        triangles: vec![[w2(-1, 1), w2(1, 1), w2(0, -1)]],
    });
    scenes.push(Scene {
        name: "winding-back",
        triangles: vec![[w2(-1, -1), w2(1, -1), w2(0, 1)]],
    });

    // Shared-edge watertight pair over a quad.
    scenes.push(Scene {
        name: "shared-edge-quad",
        triangles: vec![
            [w2(-1, 1), w2(1, 1), w2(1, -1)],
            [w2(-1, 1), w2(1, -1), w2(-1, -1)],
        ],
    });

    // Screen borders: a two-triangle rectangle covering the exact screen
    // (ndc corners at +/-1 -> screen pixels (0,0)..(400,240)).
    let border = [
        [-48000, 48000],
        [48000, 48000],
        [48000, -48000],
        [-48000, -48000],
    ]
    .map(|[x, y]| {
        ClipVertex::new(
            Q16::from_raw_const(x),
            Q16::from_raw_const(y),
            Q16::from_raw_const(24000),
            Q16::from_raw_const(48000),
        )
    });
    scenes.push(Scene {
        name: "screen-borders",
        triangles: vec![
            [border[0], border[1], border[2]],
            [border[0], border[2], border[3]],
        ],
    });

    // Fully off-screen (trivial reject) and beyond the guard band.
    scenes.push(Scene {
        name: "fully-offscreen",
        triangles: vec![screen_tri(
            [[px(-400), px(0)], [px(-350), px(0)], [px(-350), px(50)]],
            1,
            2,
        )],
    });
    scenes.push(Scene {
        name: "beyond-guard-band",
        triangles: vec![[
            cv(12, 1, 0, 1, 1, 2, 1),
            cv(13, 1, 1, 1, 1, 2, 1),
            cv(12, 1, 1, 1, 1, 2, 1),
        ]],
    });
    // Inside the viewport on the left, but extending past the right guard
    // band: guard-band clip must engage (x up to 12*w). Visually clockwise.
    scenes.push(Scene {
        name: "clip-guard-band-only",
        triangles: vec![[
            cv(0, 1, 0, 1, 1, 2, 1),
            cv(0, 1, 1, 1, 1, 2, 1),
            cv(12, 1, 1, 1, 1, 2, 1),
        ]],
    });

    // Near clip: one vertex behind the near plane -> clipped polygon.
    // Ordered visually clockwise (front face) after clipping.
    scenes.push(Scene {
        name: "clip-near-polygon",
        triangles: vec![[
            cv(0, 1, 1, 1, -1, 2, 1),
            cv(1, 1, -1, 2, 1, 2, 1),
            cv(-1, 1, -1, 2, 1, 2, 1),
        ]],
    });
    // Two vertices behind near -> quad fan of two triangles.
    scenes.push(Scene {
        name: "clip-near-two-out",
        triangles: vec![[
            cv(0, 1, 3, 2, 1, 1, 2),
            cv(2, 1, -2, 1, -1, 2, 1),
            cv(-2, 1, -2, 1, -1, 2, 1),
        ]],
    });
    // Far clip: z > w on one vertex.
    scenes.push(Scene {
        name: "clip-far",
        triangles: vec![[
            cv(0, 1, 0, 1, 1, 2, 1),
            cv(1, 1, -1, 2, 1, 2, 1),
            cv(-1, 1, -1, 2, 3, 1, 1),
        ]],
    });
    // w < 0 on one vertex (behind the camera): removed via 0 <= z <= w.
    scenes.push(Scene {
        name: "clip-w-negative",
        triangles: vec![[
            cv(0, 1, 1, 2, 1, 2, 1),
            cv(1, 1, -1, 2, 1, 2, 1),
            cv(-1, 1, -1, 2, 1, 2, -1),
        ]],
    });
    // Thin long triangle crossing the near plane.
    scenes.push(Scene {
        name: "clip-near-sliver",
        triangles: vec![[
            cv(-1, 16, 0, 1, 1, 2, 1),
            cv(1, 16, 0, 1, 1, 2, 1),
            cv(0, 1, -1, 4, -3, 4, 1),
        ]],
    });
    // Consecutive clipping: two vertices out across near plus past the
    // guard band on both sides.
    scenes.push(Scene {
        name: "clip-multi-consecutive",
        triangles: vec![[
            cv(0, 1, 1, 1, 1, 2, 1),
            cv(20, 1, -1, 1, -1, 1, 1),
            cv(-20, 1, -1, 1, -1, 1, 1),
        ]],
    });

    scenes
}

// ---------------------------------------------------------------------------
// Performance scene: ~20k triangles, mostly 5..40 px, plus a few large
// wall/floor triangles. Generated by a fixed-seed LCG, fully deterministic.

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// Reorders `tri` (1/16 px units) so it is visually clockwise in y-down
/// screen space.
fn orient_clockwise(tri: &mut [[i64; 2]; 3]) {
    let [a, b, c] = *tri;
    let area2 = (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]);
    if area2 < 0 {
        tri.swap(1, 2);
    }
}

/// Builds the deterministic performance scene.
pub fn performance_scene() -> Scene {
    let mut rng = Lcg(0xdead_beef_cafe_f00d);
    let mut triangles = Vec::with_capacity(20_000);
    // A handful of large wall/floor triangles.
    for _ in 0..40 {
        let x0 = (rng.below(800) as i64 - 200) * 16;
        let y0 = (rng.below(480) as i64 - 120) * 16;
        let x1 = x0 + (rng.below(300) as i64 + 100) * 16;
        let y1 = y0 + (rng.below(200) as i64 - 100) * 16;
        let x2 = x0 + (rng.below(300) as i64 - 50) * 16;
        let y2 = y0 + (rng.below(200) as i64 + 50) * 16;
        let mut tri = [[x0, y0], [x1, y1], [x2, y2]];
        orient_clockwise(&mut tri);
        triangles.push(screen_tri(tri, 1, 16));
    }
    // Small triangles, 5..40 px extent.
    for _ in 0..19_960 {
        let x0 = (rng.below(400) as i64 - 100) * 16;
        let y0 = (rng.below(240) as i64 - 60) * 16;
        let s = rng.below(35) as i64 + 5;
        let x1 = x0 + (rng.below(s as u64) as i64) * 16 + 32;
        let y1 = y0 + (rng.below(s as u64) as i64) * 16;
        let x2 = x0 + (rng.below(s as u64) as i64) * 16;
        let y2 = y0 + (rng.below(s as u64) as i64) * 16 + 32;
        let mut tri = [[x0, y0], [x1, y1], [x2, y2]];
        orient_clockwise(&mut tri);
        let z = rng.below(15) as i64 + 1;
        triangles.push(screen_tri(tri, z, 16));
    }
    Scene {
        name: "performance-20k",
        triangles,
    }
}

// ---------------------------------------------------------------------------
// Depth precision experiment: reverse-Z (near = 1, far = 0) against regular
// Z, both carried at U0.18 and quantized to U0.16. Realistic scene scale:
// near = 0.5 m, far in {150 m, 200 m} (the map is ~50..100 m across),
// distances in millimetres on a geometric grid.

/// Worst/average resolvable depth gap over the grid, in millimetres.
#[derive(Clone, Copy, Debug)]
pub struct DepthPrecision {
    pub far_mm: u64,
    pub worst_gap_mm: u64,
    pub average_gap_mm_x1000: u64,
    pub samples: u64,
}

/// Maps a view distance (mm) to the quantized U0.16 depth code.
/// `reverse`: near maps to 1, far to 0; otherwise near to 0, far to 1.
pub(crate) fn depth_code(z_mm: u64, near_mm: u64, far_mm: u64, reverse: bool) -> u16 {
    let span = (far_mm - near_mm) as u128;
    let z = z_mm as u128;
    // z_ndc = num / (span * z) in both conventions.
    let num = if reverse {
        near_mm as u128 * (far_mm as u128 - z)
    } else {
        far_mm as u128 * (z - near_mm as u128)
    };
    let den = span * z;
    // U0.18 with round-to-nearest, then quantized to U0.16, mirroring the
    // rasterizer's interpolate-then-quantize path. (Analysis-only code: the
    // clamp/rounding here is the exact oracle, not the device path.)
    let depth18 = ((num << 18) + den / 2) / den;
    let depth18 = depth18.min(0x3ffff) as i64;
    crate::hardware::gpu::rastersim::fixed::depth18_to_16(
        crate::hardware::gpu::rastersim::fixed::U0_18::from_raw_const(depth18),
    )
    .to_bits()
}

/// Smallest gap in mm that maps to a different depth code, via binary search
/// (the code is monotone in z, so equal codes form an interval).
fn resolvable_gap(z_mm: u64, near_mm: u64, far_mm: u64, reverse: bool) -> u64 {
    let code = depth_code(z_mm, near_mm, far_mm, reverse);
    if depth_code(far_mm, near_mm, far_mm, reverse) == code {
        return far_mm - z_mm; // never distinguishable again
    }
    let mut lo = 1u64;
    let mut hi = far_mm - z_mm;
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if depth_code(z_mm + mid, near_mm, far_mm, reverse) != code {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    lo
}

/// Runs the experiment on a geometric grid `d *= 1 + 1/256` from near to far.
pub fn depth_precision(reverse: bool, far_mm: u64) -> DepthPrecision {
    let near_mm = 500u64;
    let mut worst = 0u64;
    let mut total = 0u64;
    let mut samples = 0u64;
    let mut z = near_mm;
    while z < far_mm && samples < 100_000 {
        let gap = resolvable_gap(z, near_mm, far_mm, reverse);
        worst = worst.max(gap);
        total += gap;
        samples += 1;
        z += (z >> 8).max(1);
    }
    assert!(samples < 100_000, "depth grid exceeded its step limit");
    DepthPrecision {
        far_mm,
        worst_gap_mm: worst,
        average_gap_mm_x1000: total * 1000 / samples,
        samples,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::gpu::rastersim::clip::clip_triangle;
    use crate::hardware::gpu::rastersim::setup::SetupUnit;

    #[test]
    fn all_scenes_clip_and_setup() {
        let mut unit = SetupUnit::default();
        for scene in scenes() {
            let mut emitted = 0u64;
            for tri in &scene.triangles {
                for clipped in clip_triangle(tri) {
                    if unit.setup_triangle(&clipped).is_some() {
                        emitted += 1;
                    }
                }
            }
            println!("scene {:40} emitted {emitted}", scene.name);
        }
    }

    #[test]
    fn reverse_z_is_strictly_better() {
        for far_m in [150u64, 200] {
            let reverse = depth_precision(true, far_m * 1000);
            let regular = depth_precision(false, far_m * 1000);
            println!(
                "far={far_m}m reverse-Z: worst {} mm, avg {}.{:03} mm over {} samples",
                reverse.worst_gap_mm,
                reverse.average_gap_mm_x1000 / 1000,
                reverse.average_gap_mm_x1000 % 1000,
                reverse.samples
            );
            println!(
                "far={far_m}m regular-Z: worst {} mm, avg {}.{:03} mm over {} samples",
                regular.worst_gap_mm,
                regular.average_gap_mm_x1000 / 1000,
                regular.average_gap_mm_x1000 % 1000,
                regular.samples
            );
            assert!(
                reverse.worst_gap_mm < regular.worst_gap_mm,
                "far={far_m}m: reverse-Z worst gap not strictly better"
            );
            assert!(
                reverse.average_gap_mm_x1000 < regular.average_gap_mm_x1000,
                "far={far_m}m: reverse-Z average gap not strictly better"
            );
        }
    }

    #[test]
    fn performance_scene_size() {
        let scene = performance_scene();
        assert_eq!(scene.triangles.len(), 20_000);
    }
}
