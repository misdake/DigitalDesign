//! shift-add multiplication (rcc subset): the ISA has no mul instruction


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
