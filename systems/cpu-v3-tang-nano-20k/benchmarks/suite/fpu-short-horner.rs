// bench-max-cycles: 100000
// bench-expected-halt: 22761
// bench-tier: short
use crate::dsl_rt::*;

// fix16 scalar Horner chain: p(t) = 0.5t^3 - t^2 + 2t + 0.25 evaluated at
// t = 0, 0.25, ..., 2.0; exact bit-pattern checksum.
fn main() {
    let c3 = fix16::from_words(0x8000, 0); // 0.5
    let c2 = fix16::from_int(-1);
    let c1 = fix16::from_int(2);
    let c0 = fix16::from_words(0x4000, 0); // 0.25
    let mut cs: u16 = 0;
    let mut i: u16 = 0;
    while i < 9 {
        let raw = i << 6;
        let t = fix16::from_words(raw << 8, raw >> 8); // i * 0.25
        let p = ((c3 * t + c2) * t + c1) * t + c0;
        cs = cs ^ p.lo_bits() ^ p.hi_bits();
        cs = (cs << 1) | (cs >> 15);
        i = i + 1;
    }
    halt(cs);
}
