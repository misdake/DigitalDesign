// bench-max-cycles: 2000000
// bench-expected-halt: 0
// bench-tier: medium
use crate::dsl_rt::*;

// mat4 x vec4 over a 128-vertex batch. The matrix is stored column-major in a
// static; each output lane is one dot product written through the ACC. A Q16.16
// value is two little-endian words, so the static storage doubles.
const N: u16 = 128;
static OUT: Buf<u16, 1024> = Buf::new([0; 1024]);
static MAT: Buf<u16, 32> = Buf::new([0; 32]);

fn main() {
    let mut out = OUT.as_array();
    let mut mat = MAT.as_array();
    // column-major rotation-ish matrix; entries are arbitrary deterministic
    // Q8.8 raw values stored as Q16.16 (low word first, two per lane)
    let mut i: u16 = 0;
    while i < 16 {
        let e = (((i << 1) + i) & 511) + 1; // arbitrary deterministic entries
        mat[i << 1] = e << 8;
        mat[(i << 1) + 1] = e >> 8;
        i = i + 1;
    }
    let c0 = vec4::import(mat.as_ptr());
    let c1 = vec4::import(mat.as_ptr().add(8));
    let c2 = vec4::import(mat.as_ptr().add(16));
    let c3 = vec4::import(mat.as_ptr().add(24));
    i = 0;
    while i < N {
        let v = vec4::new(
            fix16::from_words((i & 15) << 12, 0),
            fix16::from_words((i & 31) << 11, 0),
            fix16::from_words((i & 7) << 13, 0),
            fix16::from_int(1),
        );
        let r = vec4::new(fdot(c0, v), fdot(c1, v), fdot(c2, v), fdot(c3, v));
        vec4::export(r, out.as_ptr().add((i << 3) as i16));
        i = i + 1;
    }
    let mut cs: u16 = 0;
    i = 0;
    while i < 1024 {
        cs = cs ^ out[i];
        cs = (cs << 1) | (cs >> 15);
        i = i + 1;
    }
    halt(cs);
}
