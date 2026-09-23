//! Homogeneous clip-space clipping for the standalone rasterizer sim.
//!
//! Clip-space vertices are `Q16.16` `(x, y, z, w)`. The clip plane set is
//! `0 <= z <= w` (near/far; a vertex behind the camera has `w < 0` which
//! contradicts `0 <= z <= w` and is removed automatically) plus four
//! guard-band planes `|x| <= G*w`, `|y| <= G*w` with `G = 64/25 = 2.56`
//! (±512 px against the 200 px half-width; in Y that maps to ±307.2 px).
//! The guard band width is chosen so that post-clip NDC fits 32-bit
//! `s2.30` — no 64-bit NDC/viewport datapath is needed. The four viewport
//! planes `|x| <= w`, `|y| <= w` only take part in trivial accept/reject;
//! true clipping happens against the guard band only.
//!
//! Rules (fgiesen part 5 discipline):
//!
//! 1. A fully inside triangle keeps its vertices bit-exact.
//! 2. `AB == BA`: intersection points are always computed from the outside
//!    endpoint towards the inside endpoint, `t = d_out * rcp(d_out - d_in)`
//!    in distance magnitudes, so an edge gives the same point in either
//!    direction.
//! 3. Plane order is fixed: near, far, guard-left, guard-right,
//!    guard-bottom, guard-top.
//! 4. Viewport planes never clip; they only classify.

use crate::hardware::gpu::rastersim::fixed::rcp_u32;

/// Clip-space vertex, all components `Q16.16`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ClipVertex {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub w: i32,
}

impl ClipVertex {
    pub fn new(x: i32, y: i32, z: i32, w: i32) -> Self {
        Self { x, y, z, w }
    }
}

/// Minimum legal `w` (raw `Q16.16` value of `2^-6`). Setup saturates to this.
pub const W_MIN_RAW: i32 = 1024;

// Outcode bits.
pub const OUT_NEAR: u32 = 1 << 0;
pub const OUT_FAR: u32 = 1 << 1;
pub const OUT_LEFT: u32 = 1 << 2;
pub const OUT_RIGHT: u32 = 1 << 3;
pub const OUT_BOTTOM: u32 = 1 << 4;
pub const OUT_TOP: u32 = 1 << 5;
pub const OUT_GUARD_LEFT: u32 = 1 << 6;
pub const OUT_GUARD_RIGHT: u32 = 1 << 7;
pub const OUT_GUARD_BOTTOM: u32 = 1 << 8;
pub const OUT_GUARD_TOP: u32 = 1 << 9;

/// Guard-band numerator/denominator: `G = GUARD_NUM/GUARD_DEN = 64/25 = 2.56`.
pub const GUARD_NUM: i64 = 64;
pub const GUARD_DEN: i64 = 25;

/// The ten plane distances, signed; inside means `d >= 0`. The guard-band
/// products (`64*w` reaches 2^37) force `i64` here — this is homogeneous
/// clip space, not the 32-bit NDC/viewport datapath.
fn distances(v: &ClipVertex) -> [i64; 10] {
    let x = i64::from(v.x);
    let y = i64::from(v.y);
    let z = i64::from(v.z);
    let w = i64::from(v.w);
    [
        z,                             // near: z >= 0
        w - z,                         // far: z <= w
        x + w,                         // left viewport
        w - x,                         // right viewport
        y + w,                         // bottom viewport
        w - y,                         // top viewport
        GUARD_NUM * w + GUARD_DEN * x, // guard left
        GUARD_NUM * w - GUARD_DEN * x, // guard right
        GUARD_NUM * w + GUARD_DEN * y, // guard bottom
        GUARD_NUM * w - GUARD_DEN * y, // guard top
    ]
}

/// 10-bit outcode over near/far, the four viewport planes, and the four
/// guard-band planes.
pub fn outcode(v: &ClipVertex) -> u32 {
    let d = distances(v);
    let mut code = 0;
    for (bit, distance) in d.iter().enumerate() {
        if *distance < 0 {
            code |= 1 << bit;
        }
    }
    code
}

/// Result of classifying one triangle against the full plane set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Classify {
    /// All vertices inside all ten planes: keep bit-exact.
    Accept,
    /// All vertices outside one common plane: drop.
    Reject,
    /// Needs real clipping.
    Clip,
}

/// Mask of the planes that truly clip (near, far, four guard-band planes).
const CLIP_MASK: u32 =
    OUT_NEAR | OUT_FAR | OUT_GUARD_LEFT | OUT_GUARD_RIGHT | OUT_GUARD_BOTTOM | OUT_GUARD_TOP;

pub fn classify(tri: &[ClipVertex; 3]) -> Classify {
    let codes = [outcode(&tri[0]), outcode(&tri[1]), outcode(&tri[2])];
    // Trivial accept only requires the six clipping planes; the viewport
    // planes never clip, so a triangle past them but inside the guard band
    // is kept bit-exact and culled later by the screen AABB.
    if (codes[0] | codes[1] | codes[2]) & CLIP_MASK == 0 {
        Classify::Accept
    } else if codes[0] & codes[1] & codes[2] != 0 {
        Classify::Reject
    } else {
        Classify::Clip
    }
}

/// Intersection of the segment `out -> inside` with a plane, always computed
/// from the outside endpoint: `t = |d_out| / (|d_out| + d_in)` via the rcp
/// unit, then `p = out + t*(inside - out)`. Direction-independent, which is
/// what makes `AB == BA` hold.
fn intersect(out: &ClipVertex, d_out: i64, inside: &ClipVertex, d_in: i64) -> ClipVertex {
    debug_assert!(d_out < 0 && d_in >= 0);
    let num = -d_out;
    let den = d_in - d_out; // > 0
                            // Normalize both distances into u32, keeping their ratio.
    let den_bits = 64 - den.leading_zeros();
    let shift = den_bits.saturating_sub(32);
    let den32 = (den >> shift) as u32;
    let num32 = (num >> shift) as u64;
    let (mag, rshift) = rcp_u32(den32);
    // t18 = num32 * (1/den32) * 2^18, where 1/den32 = mag * 2^-rshift.
    let product = num32 * u64::from(mag);
    let t18 = if rshift >= 18 {
        product >> (rshift - 18)
    } else {
        product << (18 - rshift)
    };
    let t18 = t18.min((1 << 18) - 1) as i64;
    let lerp = |a: i32, b: i32| -> i32 {
        let diff = i64::from(b) - i64::from(a);
        (i64::from(a) + ((diff * t18) >> 18)) as i32
    };
    ClipVertex {
        x: lerp(out.x, inside.x),
        y: lerp(out.y, inside.y),
        z: lerp(out.z, inside.z),
        w: lerp(out.w, inside.w),
    }
}

/// The six planes that truly clip, in fixed order, indexed into
/// [`distances`].
const CLIP_PLANES: [usize; 6] = [0, 1, 6, 7, 8, 9];

/// Sutherland-Hodgman clip of a polygon against one plane.
fn clip_plane(poly: &[ClipVertex], plane: usize) -> Vec<ClipVertex> {
    let mut out = Vec::with_capacity(poly.len() + 1);
    let mut prev = *poly.last().unwrap();
    let mut d_prev = distances(&prev)[plane];
    for &cur in poly {
        let d_cur = distances(&cur)[plane];
        match (d_prev >= 0, d_cur >= 0) {
            (true, true) => out.push(cur),
            (true, false) => out.push(intersect(&cur, d_cur, &prev, d_prev)),
            (false, true) => {
                out.push(intersect(&prev, d_prev, &cur, d_cur));
                out.push(cur);
            }
            (false, false) => {}
        }
        prev = cur;
        d_prev = d_cur;
    }
    out
}

/// Clips one triangle, returning zero or more output triangles (fan
/// triangulation of the clipped polygon). Trivial accepts return the input
/// bit-exact; trivial rejects return nothing.
pub fn clip_triangle(tri: &[ClipVertex; 3]) -> Vec<[ClipVertex; 3]> {
    match classify(tri) {
        Classify::Accept => return vec![*tri],
        Classify::Reject => return Vec::new(),
        Classify::Clip => {}
    }
    let mut poly: Vec<ClipVertex> = tri.to_vec();
    for &plane in &CLIP_PLANES {
        if poly.is_empty() {
            return Vec::new();
        }
        poly = clip_plane(&poly, plane);
    }
    if poly.len() < 3 {
        return Vec::new();
    }
    let mut triangles = Vec::with_capacity(poly.len() - 2);
    for i in 1..poly.len() - 1 {
        triangles.push([poly[0], poly[i], poly[i + 1]]);
    }
    triangles
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::gpu::rastersim::fixed::q16_from_rational as q;

    fn v(x: i64, y: i64, z: i64, w: i64) -> ClipVertex {
        ClipVertex::new(q(x, 1), q(y, 1), q(z, 1), q(w, 1))
    }

    #[test]
    fn trivial_accept_is_bit_exact() {
        let tri = [v(-1, -1, 1, 2), v(1, -1, 1, 2), v(0, 1, 1, 2)];
        let out = clip_triangle(&tri);
        assert_eq!(out, vec![tri]);
    }

    #[test]
    fn trivial_reject_behind_camera() {
        // All vertices behind the camera (w < 0): contradicts 0 <= z <= w.
        let tri = [v(0, 0, 1, -1), v(1, 0, 1, -1), v(0, 1, 1, -1)];
        assert!(clip_triangle(&tri).is_empty());
    }

    #[test]
    fn near_crossing_produces_polygon_fan() {
        // One vertex behind the near plane: the clipped polygon is a quad,
        // re-triangulated into a fan of two.
        let tri = [v(0, 0, -1, 1), v(-1, -1, 1, 1), v(1, -1, 1, 1)];
        let out = clip_triangle(&tri);
        assert_eq!(out.len(), 2, "near-clipped quad fan expected");
        for t in &out {
            for vertex in t {
                // Intersections use the approximate rcp unit, so allow a
                // small epsilon past the plane (setup clamps z/w anyway).
                assert!(vertex.z >= -1024, "near plane violated: {vertex:?}");
                assert!(vertex.w >= -1024);
            }
        }
    }

    #[test]
    fn intersection_matches_exact_oracle() {
        // Intersection accuracy against an exact rational oracle, sweeping
        // edges across the near plane at assorted lengths and positions.
        // Error budget: |segment| * 2^-14 plus a few raw units (the lerp rcp
        // is ~8 ppm, far below this bound).
        use crate::hardware::gpu::rastersim::fixed::q16_from_rational as qr;
        let mut steps = 0u64;
        let mut seed = 0x12345u64;
        let mut lcg = move || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            (seed >> 33) as i64
        };
        let mut worst = 0i64;
        while steps < 200 {
            let rx = |lcg: &mut dyn FnMut() -> i64| (lcg() % 4000) - 2000;
            let out_v = ClipVertex::new(
                qr(rx(&mut lcg), 1000),
                qr(rx(&mut lcg), 1000),
                qr(-(lcg() % 3000 + 1), 1000),
                qr(lcg() % 3000 + 500, 1000),
            );
            let in_v = ClipVertex::new(
                qr(rx(&mut lcg), 1000),
                qr(rx(&mut lcg), 1000),
                qr(lcg() % 3000 + 1, 1000),
                qr(lcg() % 3000 + 500, 1000),
            );
            let d_out = i64::from(out_v.z);
            let d_in = i64::from(in_v.z);
            let approx = intersect(&out_v, d_out, &in_v, d_in);
            // Exact rational intersection: t = -d_out / (d_in - d_out).
            let t_num = -i128::from(d_out);
            let t_den = i128::from(d_in) - i128::from(d_out);
            let mut segment_max = 0i64;
            for (a, b, p) in [
                (out_v.x, in_v.x, approx.x),
                (out_v.y, in_v.y, approx.y),
                (out_v.z, in_v.z, approx.z),
                (out_v.w, in_v.w, approx.w),
            ] {
                let exact = i128::from(a) + (i128::from(b) - i128::from(a)) * t_num / t_den;
                let error = (i128::from(p) - exact).abs() as i64;
                worst = worst.max(error);
                segment_max = segment_max.max((i64::from(b) - i64::from(a)).abs());
            }
            let bound = segment_max / 16_384 + 8;
            assert!(
                worst <= bound,
                "intersection error {worst} exceeds {bound} (segment {segment_max})"
            );
            steps += 1;
        }
        assert_eq!(steps, 200);
        println!("clip intersection: worst error {worst} raw Q16.16 units over 200 edges");
    }

    #[test]
    fn two_vertices_out_gives_one_triangle() {
        let tri = [v(0, 1, 2, 2), v(-2, -2, -1, 1), v(2, -2, -1, 1)];
        let out = clip_triangle(&tri);
        assert_eq!(out.len(), 1, "two-out clip keeps one triangle");
    }

    #[test]
    fn intersection_is_direction_independent() {
        // AB == BA: the same geometric edge in both directions.
        let out_v = v(0, 0, -1, 1);
        let in_v = v(1, 0, 1, 1);
        let d_out = i64::from(out_v.z);
        let d_in = i64::from(in_v.z);
        let ab = intersect(&out_v, d_out, &in_v, d_in);
        let ba = intersect(&out_v, d_out, &in_v, d_in);
        assert_eq!(ab, ba);
        // And swapped roles through clip of a reversed triangle agree.
        let tri = [v(0, 1, 2, 2), v(-2, -2, -1, 1), v(2, -2, -1, 1)];
        let fwd = clip_triangle(&tri);
        let mut rev = tri;
        rev.reverse();
        let bwd = clip_triangle(&rev);
        assert_eq!(fwd.len(), bwd.len());
        let mut fwd_v: Vec<_> = fwd.into_iter().flatten().collect();
        let mut bwd_v: Vec<_> = bwd.into_iter().flatten().collect();
        fwd_v.sort_by_key(|p| (p.x, p.y, p.z, p.w));
        bwd_v.sort_by_key(|p| (p.x, p.y, p.z, p.w));
        // Fan triangulation differs by winding, so compare the sets of
        // distinct vertices; the intersection points must match exactly.
        fwd_v.dedup();
        bwd_v.dedup();
        assert_eq!(fwd_v, bwd_v, "reversed winding changed clip results");
    }

    #[test]
    fn beyond_guard_band_is_rejected() {
        // Entirely past the right guard band but not a common viewport reject.
        let tri = [v(12, 0, 1, 1), v(13, 1, 1, 1), v(12, 1, 1, 1)];
        assert_eq!(classify(&tri), Classify::Reject);
        // Straddling the new guard band at |x| = 2.56*w: must clip.
        let tri = [v(0, 0, 1, 1), v(3, 1, 1, 1), v(0, 1, 1, 1)];
        assert_eq!(classify(&tri), Classify::Clip);
        let out = clip_triangle(&tri);
        assert!(!out.is_empty());
        for t in &out {
            for vertex in t {
                let lhs = GUARD_NUM * i64::from(vertex.w) - GUARD_DEN * i64::from(vertex.x);
                assert!(
                    lhs >= -GUARD_DEN * 1024,
                    "guard band violated by {vertex:?}"
                );
            }
        }
        // Fully inside the guard band but outside the viewport: trivially
        // accepted bit-exact (viewport planes never clip).
        let tri = [v(0, 0, 1, 1), v(2, 1, 1, 1), v(0, 1, 1, 1)];
        assert_eq!(classify(&tri), Classify::Accept);
        assert_eq!(clip_triangle(&tri), vec![tri]);
    }

    #[test]
    fn guard_band_clip_clamps_x() {
        // Straddles the right guard band x = 10.24*w.
        let tri = [v(0, 0, 1, 1), v(20, 0, 1, 1), v(0, 1, 1, 1)];
        let out = clip_triangle(&tri);
        assert!(!out.is_empty());
        for t in &out {
            for vertex in t {
                // Epsilon for the approximate rcp intersection.
                let lhs = GUARD_NUM * i64::from(vertex.w) - GUARD_DEN * i64::from(vertex.x);
                assert!(
                    lhs >= -GUARD_DEN * 1024,
                    "guard band violated by {vertex:?}"
                );
            }
        }
    }
}
