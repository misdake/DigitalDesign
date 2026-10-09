//! mem_set / mem_copy (rcc subset)

use crate::dsl_rt::*;

pub fn mem_set(dst: Ptr, len: u16, value: u16) {
    let mut data = unsafe { dst.as_u16_array() };
    let mut i: u16 = 0;
    while i < len {
        data[i] = value;
        i += 1;
    }
}

pub fn mem_copy(dst: Ptr, src: Ptr, len: u16) {
    let mut output = unsafe { dst.as_u16_array() };
    let input = unsafe { src.as_u16_array() };
    let mut i: u16 = 0;
    while i < len {
        output[i] = input[i];
        i += 1;
    }
}
