// bench-max-cycles: 10000
// bench-expected-halt: 1
use crate::dsl_rt::*;
static DATA: [u16; 48] = [0; 48];
fn main() {
    let mut d = DATA.as_array();
    let mut i: u16 = 0;
    let mut sum: u16 = 0;
    while i < 48 { d[i] = ((i << 3) + i) ^ 0x55aa; i = i + 1; }
    i = 0;
    while i < 48 { sum = sum + d[i]; i = i + 1; }
    if sum != 0 { halt(1); } else { halt(0); }
}

