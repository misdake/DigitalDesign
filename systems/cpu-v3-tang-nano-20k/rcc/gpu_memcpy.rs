//! One native MEMCPY descriptor encoder shared by raw and explicit recording.
use crate::dsl_rt::*;

pub fn gpu_memcpy_native_valid(source: u16, destination: u16, sectors: u16) -> bool {
    if source & 3 != 0
        || destination & 3 != 0
        || sectors < 4
        || sectors > 8192
        || sectors & 3 != 0
        || sectors - 1 > 65535 - source
        || sectors - 1 > 65535 - destination
    {
        return false;
    }
    source + sectors - 1 < destination || destination + sectors - 1 < source
}

// The caller owns a writable 16-word span; reject arguments before touching it.
pub fn gpu_memcpy_native(mut out: Array<u16>, source: u16, destination: u16, sectors: u16) -> bool {
    if !gpu_memcpy_native_valid(source, destination, sectors) {
        return false;
    }
    let mut i: u16 = 0;
    while i < 16 {
        out[i] = 0;
        i += 1;
    }
    let src = source >> 2;
    let dst = destination >> 2;
    let count = (sectors >> 2) - 1;
    out[0u16] = src | (dst << 14);
    out[1u16] = (dst >> 2) | (count << 12);
    out[2u16] = count >> 4;
    true
}
