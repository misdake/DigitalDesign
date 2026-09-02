// bench-max-cycles: 2000000
// bench-expected-halt: 0
// bench-tier: medium
use crate::dsl_rt::*;

// mat4 x vec4 over a 128-vertex batch. The matrix is stored column-major in a
// static; each output lane is one dot product written through the ACC.
const N: u16 = 128;
static OUT: [u16; 512] = [0; 512];
static MAT: [u16; 16] = [0; 16];

fn main() {
    let mut out = OUT.as_array();
    let mut mat = MAT.as_array();
    // column-major rotation-ish matrix with fix16 constants
    let mut i: u16 = 0;
    while i < 16 {
        mat[i] = (((i << 1) + i) & 511) + 1; // arbitrary deterministic entries
        i = i + 1;
    }
    let c0 = vec4::import(mat.as_ptr());
    let c1 = vec4::import(mat.as_ptr().add(4));
    let c2 = vec4::import(mat.as_ptr().add(8));
    let c3 = vec4::import(mat.as_ptr().add(12));
    i = 0;
    while i < N {
        let v = vec4::new(
            fix16::from_bits((i & 15) << 4),
            fix16::from_bits((i & 31) << 3),
            fix16::from_bits((i & 7) << 5),
            fix16::from_int(1),
        );
        let r = vec4::new(fdot(c0, v), fdot(c1, v), fdot(c2, v), fdot(c3, v));
        vec4::export(r, out.as_ptr().add((i << 2) as i16));
        i = i + 1;
    }
    let mut cs: u16 = 0;
    i = 0;
    while i < 512 {
        cs = cs ^ out[i];
        cs = (cs << 1) | (cs >> 15);
        i = i + 1;
    }
    halt(cs);
}
