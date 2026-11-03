//! Physical-far validation and the native pre-scaled depth limit. No ownership state.
use crate::dsl_rt::*;

pub fn gpu_depth_fact_flags_valid(flags: u16) -> bool {
    flags & 3 != 0
        && flags & 32640 == 0
        && (flags & 12 == 0 || flags & 16 != 0)
        && (flags & 32 == 0 || flags & 16 != 0)
}

pub struct GpuDepthEncoding {
    pub low: u16,
    pub high: u16,
}

pub fn gpu_depth_valid(low: u16, high: u16, shift: u16) -> bool {
    if shift > 3 || high >= 200 || (low == 0 && high == 0) {
        return false;
    }
    let last_high: u16 = 255 >> shift;
    let quantum: u16 = 1 << (8 - shift);
    high < last_high || (high == last_high && low < 65535 - (quantum - 1))
}

// The caller validates first. Word0 bits23:0 carry far*2^shift in Q16.16;
// bits25:24 carry shift, allowing CP to install both from the same first row.
pub fn gpu_depth_encode(low: u16, high: u16, shift: u16) -> GpuDepthEncoding {
    let mut carry: u16 = 0;
    if shift != 0 {
        carry = low >> (16 - shift);
    }
    GpuDepthEncoding {
        low: low << shift,
        high: (high << shift) | carry | (shift << 8),
    }
}
pub fn gpu_depth_native_valid(low: u16, high: u16) -> bool {
    let shift = (high >> 8) & 3;
    let mapped_high = high & 255;
    if high & 64512 != 0 || low & ((1 << shift) - 1) != 0 {
        return false;
    }
    let mut physical_low = low >> shift;
    if shift != 0 {
        physical_low |= (mapped_high & ((1 << shift) - 1)) << (16 - shift);
    }
    gpu_depth_valid(physical_low, mapped_high >> shift, shift)
}
