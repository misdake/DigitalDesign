// bench-max-cycles: 100000
// bench-expected-halt: 64504
// bench-tier: stress
use crate::dsl_rt::*;

// Alternating integer and FPU dependency chains to stress the FPU
// acceptance/execute barrier in both directions.
fn main() {
    let mut a: u16 = 1;
    let mut f = fix16::from_bits(0x0040); // 0.25
    let step = fix16::from_bits(0x0010); // 1/16
    let mut i: u16 = 0;
    while i < 64 {
        a = a + (f.to_bits() & 7);
        f = f + step;
        a = a ^ (a << 3);
        f = f * step + step;
        i = i + 1;
    }
    let cs = a ^ f.to_bits();
    halt(cs);
}
