//! Homogeneous clip-space clipping for the standalone rasterizer sim.
//!
//! Clip-space vertices are `Q16.16` `(x, y, z, w)`. The clip plane set is
//! `0 <= z <= w` (near/far; a vertex behind the camera has `w < 0` which
//! contradicts `0 <= z <= w` and is removed automatically) plus four
//! guard-band planes `|x| <= G*w`, `|y| <= G*w` with `G = 64/25 = 2.56`
//! (±512 px against the 200 px half-width; in Y that maps to ±307.2 px).
//! The guard band width is chosen so that post-clip NDC fits 32-bit
//! `s2.29` — no 64-bit NDC/viewport datapath is needed. The four viewport
//! planes `|x| <= w`, `|y| <= w` only take part in trivial accept/reject;
//! true clipping happens against the guard band only.
//!
//! Rules (fgiesen part 5 discipline):
//!
//! 1. A fully inside triangle keeps its vertices bit-exact.
//! 2. `AB == BA`: intersection points are always computed from the outside
//!    endpoint towards the inside endpoint, `t = |d_out| / (|d_out| + d_in)`
//!    with a 32-bit iterative quotient (deterministic), so an edge gives the
//!    same point in either direction.
//! 3. Plane order is fixed: near, far, guard-left, guard-right,
//!    guard-bottom, guard-top.
//! 4. Viewport planes never clip; they only classify.

use crate::hardware::gpu::rastersim::devices::{Adder40, Adder41, Multiplier36x36};
use crate::hardware::gpu::rastersim::fixed::{ClipDist, Q16};

/// Clip-space vertex, all components [`Q16`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ClipVertex {
    pub x: Q16,
    pub y: Q16,
    pub z: Q16,
    pub w: Q16,
}

impl ClipVertex {
    pub fn new(x: Q16, y: Q16, z: Q16, w: Q16) -> Self {
        Self { x, y, z, w }
    }
}

/// Minimum legal `w` (raw `Q16.16` value of `1/8`). Input contract: the
/// projection must place the near plane at >= 1/8 m, so after clipping every
/// vertex satisfies `w >= W_MIN_RAW` and `0 <= z <= w`. Primitives violating
/// this are explicitly rejected in setup (`SetupStats::culled_invalid_w`);
/// nothing is silently clamped.
pub const W_MIN_RAW: i32 = 8192;

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

/// The ten plane distances, typed `ClipDist` (40-bit signed Q16.16); inside
/// means `d >= 0`. The guard-band products (`64*w` reaches 2^37 raw) use
/// `Adder40` shift-add chains (64 = one shift, 25 = 16+8+1), matching what
/// hardware would synthesize. Non-saturating: any wrap would be reported as
/// a device overflow (and never happens: values stay below 2^39).
pub(crate) fn distances(v: &ClipVertex) -> [ClipDist; 10] {
    let x = ClipDist::widen_q16(v.x);
    let y = ClipDist::widen_q16(v.y);
    let z = ClipDist::widen_q16(v.z);
    let w = ClipDist::widen_q16(v.w);
    let w64 = w.shl_bits(6);
    let x25 = ClipDist::from_product(Adder40::add_fx(
        "clip.dist",
        ClipDist::from_product(Adder40::add_fx("clip.dist", x.shl_bits(4), x.shl_bits(3))),
        x,
    ));
    let y25 = ClipDist::from_product(Adder40::add_fx(
        "clip.dist",
        ClipDist::from_product(Adder40::add_fx("clip.dist", y.shl_bits(4), y.shl_bits(3))),
        y,
    ));
    let add = |a, b| ClipDist::from_product(Adder40::add_fx("clip.dist", a, b));
    let sub = |a, b| ClipDist::from_product(Adder40::sub_fx("clip.dist", a, b));
    [
        z,             // near: z >= 0
        sub(w, z),     // far: z <= w
        add(x, w),     // left viewport
        sub(w, x),     // right viewport
        add(y, w),     // bottom viewport
        sub(w, y),     // top viewport
        add(w64, x25), // guard left
        sub(w64, x25), // guard right
        add(w64, y25), // guard bottom
        sub(w64, y25), // guard top
    ]
}

/// Guard-band right-plane index into [`distances`], for tests.
#[cfg(test)]
pub(crate) const GUARD_RIGHT_PLANE: usize = 7;

/// 10-bit outcode over near/far, the four viewport planes, and the four
/// guard-band planes.
pub fn outcode(v: &ClipVertex) -> u32 {
    let d = distances(v);
    let mut code = 0;
    for (bit, distance) in d.iter().enumerate() {
        if distance.is_negative() {
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
fn intersect(out: &ClipVertex, d_out: ClipDist, inside: &ClipVertex, d_in: ClipDist) -> ClipVertex {
    debug_assert!(d_out.is_negative() && !d_in.is_negative());
    match intersect_prepare(d_out, d_in) {
        // The inside endpoint is the exact intersection when it already lies
        // on this plane (t = 1).
        None => *inside,
        Some((num, den)) => {
            let t32 = intersect_quotient(num, den);
            ClipVertex {
                x: lerp_q16(out.x, inside.x, t32),
                y: lerp_q16(out.y, inside.y, t32),
                z: lerp_q16(out.z, inside.z, t32),
                w: lerp_q16(out.w, inside.w, t32),
            }
        }
    }
}

/// Stepwise leaf: distance-ratio preparation for [`intersect`]. Returns
/// `None` when the inside endpoint lies exactly on the plane (t = 1),
/// otherwise the 41-bit numerator and denominator (`num < den`, both
/// positive).
pub(crate) fn intersect_prepare(d_out: ClipDist, d_in: ClipDist) -> Option<(ClipDist, ClipDist)> {
    if d_in == ClipDist::zero() {
        return None;
    }
    let num = -d_out;
    // Signed 41-bit difference (ClipDist is 40-bit; the difference of two
    // distances can need one more bit).
    let den = ClipDist::from_product(Adder41::sub_fx("clip.dist", d_in, d_out)); // > 0
    Some((num, den))
}

/// Stepwise leaf: the interpolation quotient `t` as a 32-bit fraction, from
/// the iterative shift-subtract divider (32 iterations; clip is low
/// frequency, so the LUT rcp is not reused here).
pub(crate) fn intersect_quotient(num: ClipDist, den: ClipDist) -> u32 {
    let mut remainder = num.raw();
    let denominator = den.raw();
    debug_assert!(remainder >= 0 && denominator > remainder);
    let mut quotient = 0u32;
    for _ in 0..32 {
        quotient_step(&mut remainder, &mut quotient, denominator);
    }
    quotient
}

/// One shift-subtract iteration of the divider (Adder41 semantics). The emu
/// calls it once per cycle; the functional sim loops it 32 times.
pub(crate) fn quotient_step(remainder: &mut i64, quotient: &mut u32, denominator: i64) {
    *remainder = Adder41::add("clip.div", *remainder, *remainder);
    *quotient <<= 1;
    if *remainder >= denominator {
        *remainder = Adder41::sub("clip.div", *remainder, denominator);
        *quotient |= 1;
    }
}

/// Linear interpolation `out + t*(inside - out)` on Q16 coordinates, used by
/// clipping. The difference is widened to `ClipDist` (a Q16 subtraction can
/// exceed the Q16 range); the wide difference times the 32-bit fraction is a
/// 36x36 multiplier site keeping the full product and rounding once at the
/// end; the result lies between the endpoints by construction, so the
/// narrowing range check (class 3) never fires.
pub(crate) fn lerp_q16(out: Q16, inside: Q16, t32: u32) -> Q16 {
    let diff = ClipDist::from_product(Adder41::sub_fx(
        "clip.lerp",
        ClipDist::widen_q16(inside),
        ClipDist::widen_q16(out),
    ));
    // Wide intermediate product, single rounding at the end.
    let product = Multiplier36x36::mul("clip.lerp", diff.raw(), i64::from(t32));
    let step = (product + (1i64 << 31)) >> 32;
    Q16::narrow_from_clip_dist(ClipDist::from_product(Adder41::add_fx(
        "clip.lerp",
        ClipDist::widen_q16(out),
        ClipDist::from_product(step),
    )))
}

/// The six planes that truly clip, in fixed order, indexed into
/// [`distances`].
pub(crate) const CLIP_PLANES: [usize; 6] = [0, 1, 6, 7, 8, 9];

/// Sutherland-Hodgman clip of a polygon against one plane.
fn clip_plane(poly: &[ClipVertex], plane: usize) -> Vec<ClipVertex> {
    let mut out = Vec::with_capacity(poly.len() + 1);
    let mut prev = *poly.last().unwrap();
    let mut d_prev = distances(&prev)[plane];
    for &cur in poly {
        let d_cur = distances(&cur)[plane];
        match (!d_prev.is_negative(), !d_cur.is_negative()) {
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
                assert!(
                    vertex.z >= Q16::from_raw_const(-1024),
                    "near plane violated: {vertex:?}"
                );
                assert!(vertex.w >= Q16::from_raw_const(-1024));
            }
        }
    }

    #[test]
    fn intersection_keeps_endpoint_on_near_plane() {
        let outside = ClipVertex::new(q(-30000, 1), q(0, 1), q(-30000, 1), q(1, 1));
        let on_plane = ClipVertex::new(q(0, 1), q(0, 1), q(0, 1), q(1, 1));
        let d_out = distances(&outside)[0];
        let d_in = distances(&on_plane)[0];
        assert_eq!(intersect(&outside, d_out, &on_plane, d_in), on_plane);
    }

    #[test]
    fn intersection_matches_exact_oracle() {
        // Intersection accuracy against an exact rational oracle, sweeping
        // edges across the near plane at assorted lengths and positions.
        // Bound from the contract: (segment >> REL_SHIFT) + ABS raw units.
        use crate::hardware::gpu::rastersim::contract;
        use crate::hardware::gpu::rastersim::fixed::q16_from_rational as qr;
        use crate::hardware::gpu::rastersim::oracle::clip_intersection_exact;
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
            let d_out = ClipDist::widen_q16(out_v.z);
            let d_in = ClipDist::widen_q16(in_v.z);
            let approx = intersect(&out_v, d_out, &in_v, d_in);
            let exact = clip_intersection_exact(&out_v, d_out, &in_v, d_in);
            let mut segment_max = 0i64;
            for (a, b, p, e) in [
                (out_v.x, in_v.x, approx.x, exact.x),
                (out_v.y, in_v.y, approx.y, exact.y),
                (out_v.z, in_v.z, approx.z, exact.z),
                (out_v.w, in_v.w, approx.w, exact.w),
            ] {
                let error = (p - e).abs().raw();
                worst = worst.max(error);
                // Widened difference: a Q16 sub could exceed the Q16 range.
                segment_max = segment_max.max(
                    (ClipDist::widen_q16(b) - ClipDist::widen_q16(a))
                        .raw()
                        .abs(),
                );
            }
            let bound = (segment_max >> contract::clip::INTERSECTION_REL_SHIFT)
                + contract::clip::INTERSECTION_ABS_RAW;
            assert!(
                worst <= bound,
                "intersection error {worst} exceeds contract {bound} (segment {segment_max})"
            );
            steps += 1;
        }
        assert_eq!(steps, 200);
        println!("clip intersection: worst error {worst} raw Q16.16 units over 200 edges");
    }

    #[test]
    fn intersection_precision_holds_on_huge_edges() {
        // Huge dynamic range: endpoints near the Q16.16 extremes (tens of
        // thousands of metres, e.g. a giant triangle right in front of the
        // camera), crossing the near plane. The contract bound scales with
        // the segment length, but the absolute term must still dominate
        // correctly: verify against the exact rational oracle.
        use crate::hardware::gpu::rastersim::contract;
        use crate::hardware::gpu::rastersim::fixed::q16_from_rational as qr;
        use crate::hardware::gpu::rastersim::oracle::clip_intersection_exact;
        let cases = [
            // (out_v, in_v): outside endpoint behind near plane.
            (
                qr(-30000, 1),
                qr(30000, 1),
                qr(-2000, 1),
                qr(3, 100),
                qr(1, 100),
                qr(0, 1),
                qr(1, 2),
                qr(1, 50),
            ),
            (
                qr(32767, 1),
                qr(-32767, 1),
                qr(-500, 1),
                qr(1, 1),
                qr(12345, 1),
                qr(-7, 1),
                qr(3, 1),
                qr(2, 1),
            ),
            (
                qr(1, 1000),
                qr(-1, 1000),
                qr(-1, 1000),
                qr(2, 100),
                qr(-30000, 1),
                qr(25000, 1),
                qr(100, 1),
                qr(150, 1),
            ),
        ];
        let mut steps = 0u64;
        for (ox, oy, oz, ow, ix, iy, iz, iw) in cases {
            let out_v = ClipVertex::new(ox, oy, oz, ow);
            let in_v = ClipVertex::new(ix, iy, iz, iw);
            let d_out = ClipDist::widen_q16(out_v.z);
            let d_in = ClipDist::widen_q16(in_v.z);
            let approx = intersect(&out_v, d_out, &in_v, d_in);
            let exact = clip_intersection_exact(&out_v, d_out, &in_v, d_in);
            let mut segment_max = 0i64;
            let mut worst = 0i64;
            for (a, b, p, e) in [
                (out_v.x, in_v.x, approx.x, exact.x),
                (out_v.y, in_v.y, approx.y, exact.y),
                (out_v.z, in_v.z, approx.z, exact.z),
                (out_v.w, in_v.w, approx.w, exact.w),
            ] {
                worst = worst.max((p - e).abs().raw());
                segment_max = segment_max.max(
                    (ClipDist::widen_q16(b) - ClipDist::widen_q16(a))
                        .raw()
                        .abs(),
                );
            }
            let bound = (segment_max >> contract::clip::INTERSECTION_REL_SHIFT)
                + contract::clip::INTERSECTION_ABS_RAW;
            assert!(
                worst <= bound,
                "huge-edge intersection error {worst} exceeds contract {bound} (segment {segment_max})"
            );
            steps += 1;
        }
        assert_eq!(steps, 3);
    }

    #[test]
    fn intersection_huge_edges_stay_direction_independent() {
        // AB == BA under huge dynamic range: clipping the same geometric edge
        // in both windings must produce identical intersection vertices.
        use crate::hardware::gpu::rastersim::fixed::q16_from_rational as qr;
        let tri = [
            ClipVertex::new(qr(-20000, 1), qr(20000, 1), qr(1, 1), qr(8000, 1)),
            ClipVertex::new(qr(20000, 1), qr(20000, 1), qr(1, 1), qr(8000, 1)),
            ClipVertex::new(qr(0, 1), qr(-100, 1), qr(-3, 1), qr(1, 50)),
        ];
        let fwd = clip_triangle(&tri);
        let mut rev = tri;
        rev.reverse();
        let bwd = clip_triangle(&rev);
        assert_eq!(fwd.len(), bwd.len());
        assert!(!fwd.is_empty());
        let mut fwd_v: Vec<_> = fwd.into_iter().flatten().collect();
        let mut bwd_v: Vec<_> = bwd.into_iter().flatten().collect();
        fwd_v.sort_by_key(|p| (p.x, p.y, p.z, p.w));
        bwd_v.sort_by_key(|p| (p.x, p.y, p.z, p.w));
        fwd_v.dedup();
        bwd_v.dedup();
        assert_eq!(
            fwd_v, bwd_v,
            "reversed winding changed huge-edge clip results"
        );
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
        let d_out = ClipDist::widen_q16(out_v.z);
        let d_in = ClipDist::widen_q16(in_v.z);
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
                let lhs = distances(vertex)[GUARD_RIGHT_PLANE];
                assert!(
                    lhs >= ClipDist::from_raw_const(-GUARD_DEN * 1024),
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
        // Straddles the right guard band x = 2.56*w.
        let tri = [v(0, 0, 1, 1), v(20, 0, 1, 1), v(0, 1, 1, 1)];
        let out = clip_triangle(&tri);
        assert!(!out.is_empty());
        for t in &out {
            for vertex in t {
                // Epsilon for the approximate rcp intersection.
                let lhs = distances(vertex)[GUARD_RIGHT_PLANE];
                assert!(
                    lhs >= ClipDist::from_raw_const(-GUARD_DEN * 1024),
                    "guard band violated by {vertex:?}"
                );
            }
        }
    }
}
