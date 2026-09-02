// bench-max-cycles: 100000
// bench-expected-halt: 56954
// bench-tier: short
use crate::dsl_rt::*;

// Insertion sort over 24 words from a xorshift generator; exact sorted-array
// checksum.
const N: u16 = 24;
static DATA: [u16; 24] = [0; 24];

fn main() {
    let mut d = DATA.as_array();
    let mut x: u16 = 0x1234;
    let mut i: u16 = 0;
    while i < N {
        x = x ^ (x << 7);
        x = x ^ (x >> 9);
        x = x ^ (x << 8);
        d[i] = x;
        i = i + 1;
    }
    i = 1;
    while i < N {
        let v = d[i];
        let mut j = i;
        while j > 0 && d[j - 1] > v {
            d[j] = d[j - 1];
            j = j - 1;
        }
        d[j] = v;
        i = i + 1;
    }
    let mut cs: u16 = 0;
    i = 0;
    while i < N {
        cs = cs ^ d[i];
        cs = (cs << 1) | (cs >> 15);
        i = i + 1;
    }
    halt(cs);
}
