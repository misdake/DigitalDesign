// bench-max-cycles: 400000
// bench-expected-halt: 63443
// bench-tier: medium
use crate::dsl_rt::*;

// Cubic Bezier evaluation over a control polygon, sweeping t in 64 steps.
// B(t) = (1-t)^3 P0 + 3(1-t)^2 t P1 + 3(1-t) t^2 P2 + t^3 P3 with fix32 vec4
// control points.
fn main() {
    let p0 = vec4::new(fix32::from_int(0), fix32::from_int(0), fix32::zero(), fix32::zero());
    let p1 = vec4::new(fix32::from_int(1), fix32::from_int(2), fix32::zero(), fix32::zero());
    let p2 = vec4::new(fix32::from_int(3), fix32::from_int(1), fix32::zero(), fix32::zero());
    let p3 = vec4::new(fix32::from_int(4), fix32::from_int(0), fix32::zero(), fix32::zero());
    let three = fix32::from_int(3);
    let mut cs: u16 = 0;
    let mut i: u16 = 0;
    while i < 64 {
        let t = fix32::from_words(i << 10, 0); // i/64
        let s = fix32::from_int(1) - t;
        let b = p0 * (s * s * s)
            + p1 * (three * s * s * t)
            + p2 * (three * s * t * t)
            + p3 * (t * t * t);
        cs = cs ^ b.x().lo_bits() ^ b.x().hi_bits() ^ b.y().lo_bits() ^ b.y().hi_bits();
        cs = (cs << 1) | (cs >> 15);
        i = i + 1;
    }
    halt(cs);
}
