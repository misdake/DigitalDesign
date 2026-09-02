// bench-max-cycles: 100000
// bench-expected-halt: 416
// bench-tier: short
use crate::dsl_rt::*;

// Naive substring search over a fixed 64-word buffer; exact match count and
// position checksum.
static TEXT: [u16; 64] = [0; 64];
static PAT: [u16; 5] = [0; 5];

fn main() {
    let mut t = TEXT.as_array();
    let mut p = PAT.as_array();
    let mut i: u16 = 0;
    while i < 64 {
        t[i] = ((i << 3) ^ (i >> 1) ^ 0x2b) & 63;
        i = i + 1;
    }
    // the pattern is copied out of the text, so at least one match exists
    let mut k: u16 = 0;
    while k < 5 {
        p[k] = t[10 + k];
        k = k + 1;
    }
    let mut count: u16 = 0;
    let mut cs: u16 = 0;
    i = 0;
    while i + 5 <= 64 {
        let mut j: u16 = 0;
        while j < 5 && t[i + j] == p[j] {
            j = j + 1;
        }
        if j == 5 {
            count = count + 1;
            cs = cs ^ (i << 4);
        }
        i = i + 1;
    }
    halt(cs ^ (count << 8));
}
