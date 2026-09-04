// bench-max-cycles: 100000
// bench-expected-halt: 7232
// bench-tier: short
use crate::dsl_rt::*;

// Scale a batch of sixteen vec4 values by one scalar; exercises the
// FACCLOAD.X + FACCSTORE 0b1111 splat idiom feeding FMUL.
fn main() {
    let scale = fix16::from_bits(0x0080); // 0.5
    let mut acc = vec4::zero();
    let mut i: u16 = 0;
    while i < 16 {
        let v = vec4::new(
            fix16::from_bits(i << 4),
            fix16::from_bits(i << 5),
            fix16::from_int(1),
            fix16::from_int(2),
        );
        acc = acc + v * scale;
        i = i + 1;
    }
    let cs = acc.x().to_bits() ^ acc.y().to_bits() ^ acc.z().to_bits() ^ acc.w().to_bits();
    halt(cs);
}
