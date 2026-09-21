// bench-max-cycles: 100000
// bench-expected-halt: 16412
// bench-tier: short
use crate::dsl_rt::*;

// Scale a batch of sixteen vec4 values by one scalar; exercises the
// FACCLOAD.X + FACCSTORE 0b1111 splat idiom feeding FMUL.
fn main() {
    let scale = fix16::from_words(0x8000, 0); // 0.5
    let mut acc = vec4::zero();
    let mut i: u16 = 0;
    while i < 16 {
        let v = vec4::new(
            fix16::from_words(i << 12, 0),
            fix16::from_words(i << 13, i >> 3),
            fix16::from_int(1),
            fix16::from_int(2),
        );
        acc = acc + v * scale;
        i = i + 1;
    }
    let cs = acc.x().lo_bits()
        ^ acc.x().hi_bits()
        ^ acc.y().lo_bits()
        ^ acc.y().hi_bits()
        ^ acc.z().lo_bits()
        ^ acc.z().hi_bits()
        ^ acc.w().lo_bits()
        ^ acc.w().hi_bits();
    halt(cs);
}
