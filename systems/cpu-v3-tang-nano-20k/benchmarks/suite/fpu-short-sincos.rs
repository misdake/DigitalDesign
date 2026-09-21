// bench-max-cycles: 100000
// bench-expected-halt: 15622
// bench-tier: short
use crate::dsl_rt::*;

// FSINCOS angle sweep (radians in Q16.16) with a sin/cos accumulation checksum.
fn main() {
    let mut sin_acc = fix16::zero();
    let mut cos_acc = fix16::zero();
    let mut i: u16 = 0;
    while i < 16 {
        let raw = i << 7;
        let angle = fix16::from_words(raw << 8, raw >> 8); // i * 0.5 rad
        let sc = fsincos(angle);
        sin_acc = sin_acc + sc.x();
        cos_acc = cos_acc + sc.y();
        i = i + 1;
    }
    let cs = sin_acc.lo_bits() ^ sin_acc.hi_bits() ^ cos_acc.lo_bits() ^ cos_acc.hi_bits();
    halt(cs);
}
