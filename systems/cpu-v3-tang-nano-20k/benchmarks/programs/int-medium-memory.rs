// bench-max-cycles: 600000
// bench-expected-halt: 1
use crate::dsl_rt::*;
static DATA: [u16; 1024] = [0; 1024];
fn main() {
    let mut d = DATA.as_array();
    let mut i: u16 = 0;
    let mut sum: u16 = 0;
    while i < 1024 { d[i] = (i << 3) ^ (i >> 2) ^ 0x6d2b; i = i + 1; }
    i = 0;
    while i < 1024 { sum = sum + d[i]; i = i + 1; }
    if sum != 0 { halt(1); } else { halt(0); }
}

