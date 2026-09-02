// bench-max-cycles: 400000
// bench-expected-halt: 52243
// bench-tier: medium
use crate::dsl_rt::*;

// Cubic Bezier evaluation over a control polygon, sweeping t in 64 steps.
// B(t) = (1-t)^3 P0 + 3(1-t)^2 t P1 + 3(1-t) t^2 P2 + t^3 P3 with fix16 vec4
// control points.
fn main() {
    let p0 = vec4::new(fix16::from_int(0), fix16::from_int(0), fix16::zero(), fix16::zero());
    let p1 = vec4::new(fix16::from_int(1), fix16::from_int(2), fix16::zero(), fix16::zero());
    let p2 = vec4::new(fix16::from_int(3), fix16::from_int(1), fix16::zero(), fix16::zero());
    let p3 = vec4::new(fix16::from_int(4), fix16::from_int(0), fix16::zero(), fix16::zero());
    let three = fix16::from_int(3);
    let mut cs: u16 = 0;
    let mut i: u16 = 0;
    while i < 64 {
        let t = fix16::from_bits(i << 2); // i/64
        let s = fix16::from_int(1) - t;
        let b = p0 * (s * s * s)
            + p1 * (three * s * s * t)
            + p2 * (three * s * t * t)
            + p3 * (t * t * t);
        cs = cs ^ b.x().to_bits() ^ b.y().to_bits();
        cs = (cs << 1) | (cs >> 15);
        i = i + 1;
    }
    halt(cs);
}
