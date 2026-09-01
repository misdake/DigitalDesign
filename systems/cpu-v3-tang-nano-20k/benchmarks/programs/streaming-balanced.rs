// bench-max-cycles: 8000000
// bench-expected-halt: 1
use crate::dsl_rt::*;
const N: u16 = 4096;
const B_OFFSET: u16 = 4112;
const OUT_OFFSET: u16 = 8224;
static DATA: [u16; 12320] = [0; 12320];
fn main() {
    let mut d = DATA.as_array();
    let mut i: u16 = 0;
    while i < N {
        d[i] = i ^ 0x5a5a;
        d[B_OFFSET + i] = (i << 1) + 3;
        i = i + 1;
    }
    i = 0;
    while i < N {
        d[OUT_OFFSET + i] =
            (d[i] + d[B_OFFSET + i]) ^ (d[i] >> 3) ^ (d[B_OFFSET + i] << 2);
        i = i + 1;
    }
    i = 0;
    while i < N {
        let expected =
            (d[i] + d[B_OFFSET + i]) ^ (d[i] >> 3) ^ (d[B_OFFSET + i] << 2);
        if d[OUT_OFFSET + i] != expected { halt(0); }
        i = i + 1;
    }
    halt(1);
}

