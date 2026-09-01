// bench-max-cycles: 12000000
// bench-expected-halt: 1
use crate::dsl_rt::*;
const N: u16 = 4096;
static INPUT_A: [u16; 4096] = [0; 4096];
static INPUT_B: [u16; 4096] = [0; 4096];
static OUTPUT: [u16; 4096] = [0; 4096];
fn main() {
    let mut a = INPUT_A.as_array();
    let mut b = INPUT_B.as_array();
    let mut out = OUTPUT.as_array();
    let mut i: u16 = 0;
    while i < N { a[i] = i ^ 0x5a5a; b[i] = (i << 1) + 3; i = i + 1; }
    i = 0;
    while i < N {
        out[i] = (a[i] + b[i]) ^ (a[i] >> 3) ^ (b[i] << 2);
        i = i + 1;
    }
    i = 0;
    while i < N {
        if out[i] != ((a[i] + b[i]) ^ (a[i] >> 3) ^ (b[i] << 2)) { halt(0); }
        i = i + 1;
    }
    halt(1);
}
