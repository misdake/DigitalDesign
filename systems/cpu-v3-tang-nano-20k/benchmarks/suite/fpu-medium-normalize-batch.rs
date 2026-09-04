// bench-max-cycles: 2000000
// bench-expected-halt: 0
// bench-tier: medium
use crate::dsl_rt::*;

// Normalize 256 vec3-as-vec4 vectors: len^2 via FDOT4ACC, 1/len via FRSQRT,
// scale through the ACC splat + FMUL. Exact bit checksum of the x lanes.
const N: u16 = 256;
static OUT: [u16; 256] = [0; 256];

fn main() {
    let mut out = OUT.as_array();
    let mut cs: u16 = 0;
    let mut i: u16 = 0;
    while i < N {
        let v = vec4::new(
            fix16::from_bits((i & 7) + 64),
            fix16::from_bits((i & 15) + 32),
            fix16::from_bits((i & 31) + 16),
            fix16::zero(),
        );
        let inv = frsqrt(fdot(v, v));
        let n = v * inv;
        out[i] = n.x().to_bits() ^ n.y().to_bits() ^ n.z().to_bits();
        i = i + 1;
    }
    i = 0;
    while i < N {
        cs = cs ^ out[i];
        cs = (cs << 1) | (cs >> 15);
        i = i + 1;
    }
    halt(cs);
}
