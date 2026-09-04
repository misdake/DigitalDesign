// bench-max-cycles: 100000
// bench-expected-halt: 835
// bench-tier: short
use crate::dsl_rt::*;

// FSINCOS angle sweep (radians in Q8.8) with a sin/cos accumulation checksum.
fn main() {
    let mut sin_acc = fix16::zero();
    let mut cos_acc = fix16::zero();
    let mut i: u16 = 0;
    while i < 16 {
        let angle = fix16::from_bits(i << 7); // i * 0.5 rad
        let sc = fsincos(angle);
        sin_acc = sin_acc + sc.x();
        cos_acc = cos_acc + sc.y();
        i = i + 1;
    }
    let cs = sin_acc.to_bits() ^ cos_acc.to_bits();
    halt(cs);
}
