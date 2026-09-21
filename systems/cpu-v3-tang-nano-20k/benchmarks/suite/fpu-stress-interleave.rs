// bench-max-cycles: 100000
// bench-expected-halt: 34708
// bench-tier: stress
use crate::dsl_rt::*;

// Alternating integer and FPU dependency chains to stress the FPU
// acceptance/execute barrier in both directions.
fn main() {
    let mut a: u16 = 1;
    let mut f = fix32::from_words(0x4000, 0); // 0.25
    let step = fix32::from_words(0x1000, 0); // 1/16
    let mut i: u16 = 0;
    while i < 64 {
        a = a + ((f.lo_bits() ^ f.hi_bits()) & 7);
        f = f + step;
        a = a ^ (a << 3);
        f = f * step + step;
        i = i + 1;
    }
    let cs = a ^ f.lo_bits() ^ f.hi_bits();
    halt(cs);
}
