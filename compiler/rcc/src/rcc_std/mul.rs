//! shift-add multiplication (rcc subset): the CpuV2 library multiply.
//!
//! CpuV2's v2.6 ISA has no hardware multiply, so the CpuV2 backend rewrites
//! integer products into calls to `mul_16x16`. CpuV3 lowers integer `*` to the
//! hardware `MUL0`/`MULI` instead (and `MUL8`/`MUL16` through intrinsics) and
//! does not use this module.

fn mul_bits(a: u16, b: u16, bits: u16) -> u16 {
    let mut x = a;
    let mut y = b;
    let mut sum = 0;
    let mut i = 0;
    while i < bits {
        let bit = y & 1;
        let mask = 0u16 - bit; // bit ? 0xffff : 0
        sum += mask & x;
        y >>= 1;
        x <<= 1;
        i += 1;
    }
    sum
}

pub fn mul_16x4(a: u16, b4: u16) -> u16 {
    mul_bits(a, b4, 4)
}
pub fn mul_16x8(a: u16, b8: u16) -> u16 {
    mul_bits(a, b8, 8)
}
pub fn mul_16x16(a: u16, b16: u16) -> u16 {
    mul_bits(a, b16, 16)
}
