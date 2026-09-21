// bench-max-cycles: 100000
// bench-expected-halt: 1
// bench-tier: stress
use crate::dsl_rt::*;

// More simultaneously live vectors than allocatable F registers: forces FPU
// spill/reload traffic through the frame. Exact sum-of-squares checksum.
fn main() {
    let v0 = vec4::new(fix32::from_words(0x4000, 0), fix32::zero(), fix32::zero(), fix32::zero());
    let v1 = vec4::new(fix32::from_words(0x8000, 0), fix32::zero(), fix32::zero(), fix32::zero());
    let v2 = vec4::new(fix32::from_words(0xc000, 0), fix32::zero(), fix32::zero(), fix32::zero());
    let v3 = vec4::new(fix32::from_words(0, 1), fix32::zero(), fix32::zero(), fix32::zero());
    let v4 = vec4::new(fix32::from_words(0x4000, 1), fix32::zero(), fix32::zero(), fix32::zero());
    let v5 = vec4::new(fix32::from_words(0x8000, 1), fix32::zero(), fix32::zero(), fix32::zero());
    let v6 = vec4::new(fix32::from_words(0xc000, 1), fix32::zero(), fix32::zero(), fix32::zero());
    let v7 = vec4::new(fix32::from_words(0, 2), fix32::zero(), fix32::zero(), fix32::zero());
    let v8 = vec4::new(fix32::from_words(0x4000, 2), fix32::zero(), fix32::zero(), fix32::zero());
    let v9 = vec4::new(fix32::from_words(0x8000, 2), fix32::zero(), fix32::zero(), fix32::zero());
    let v10 = vec4::new(fix32::from_words(0xc000, 2), fix32::zero(), fix32::zero(), fix32::zero());
    let v11 = vec4::new(fix32::from_words(0, 3), fix32::zero(), fix32::zero(), fix32::zero());
    let total = fdot(v0, v0)
        + fdot(v1, v1)
        + fdot(v2, v2)
        + fdot(v3, v3)
        + fdot(v4, v4)
        + fdot(v5, v5)
        + fdot(v6, v6)
        + fdot(v7, v7)
        + fdot(v8, v8)
        + fdot(v9, v9)
        + fdot(v10, v10)
        + fdot(v11, v11);
    // sum of (0.25k)^2 for k = 1..12 = 40.625 = 0x0028_a000 in Q16.16
    if total.lo_bits() == 0xa000 && total.hi_bits() == 0x0028 {
        halt(1);
    } else {
        halt(0);
    }
}
