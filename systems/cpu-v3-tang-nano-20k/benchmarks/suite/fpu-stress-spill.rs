// bench-max-cycles: 100000
// bench-expected-halt: 1
// bench-tier: stress
use crate::dsl_rt::*;

// More simultaneously live vectors than allocatable F registers: forces FPU
// spill/reload traffic through the frame. Exact sum-of-squares checksum.
fn main() {
    let v0 = vec4::new(fix16::from_bits(64), fix16::zero(), fix16::zero(), fix16::zero());
    let v1 = vec4::new(fix16::from_bits(128), fix16::zero(), fix16::zero(), fix16::zero());
    let v2 = vec4::new(fix16::from_bits(192), fix16::zero(), fix16::zero(), fix16::zero());
    let v3 = vec4::new(fix16::from_bits(256), fix16::zero(), fix16::zero(), fix16::zero());
    let v4 = vec4::new(fix16::from_bits(320), fix16::zero(), fix16::zero(), fix16::zero());
    let v5 = vec4::new(fix16::from_bits(384), fix16::zero(), fix16::zero(), fix16::zero());
    let v6 = vec4::new(fix16::from_bits(448), fix16::zero(), fix16::zero(), fix16::zero());
    let v7 = vec4::new(fix16::from_bits(512), fix16::zero(), fix16::zero(), fix16::zero());
    let v8 = vec4::new(fix16::from_bits(576), fix16::zero(), fix16::zero(), fix16::zero());
    let v9 = vec4::new(fix16::from_bits(640), fix16::zero(), fix16::zero(), fix16::zero());
    let v10 = vec4::new(fix16::from_bits(704), fix16::zero(), fix16::zero(), fix16::zero());
    let v11 = vec4::new(fix16::from_bits(768), fix16::zero(), fix16::zero(), fix16::zero());
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
    // sum of (0.25k)^2 for k = 1..12 = 650 * 16 = 10400 in Q8.8
    if total.to_bits() == 10400 { halt(1); } else { halt(0); }
}
