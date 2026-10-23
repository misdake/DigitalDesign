//! Fixed CPU views into caller-owned physical allocations. No mapping changes.
use crate::dsl_rt::*;

pub struct GpuStorage {
    pub cpu: Ptr,
    pub sector: u16,
    pub sectors: u16,
}

// Physical intervals use inclusive last sectors, so an end exactly at 8 MiB
// is representable without wrapping a u16 exclusive end to zero.
pub fn gpu_extent_valid(base: u16, sectors: u16) -> bool {
    sectors != 0 && sectors - 1 <= 65535 - base
}

pub fn gpu_extents_overlap(a: u16, an: u16, b: u16, bn: u16) -> bool {
    a <= b + (bn - 1) && b <= a + (an - 1)
}

pub fn gpu_storage_valid(s: Array<GpuStorage>) -> bool {
    // One CPU view is at most 64 KiB. Its physical mapping is a platform
    // prerequisite; a logical pointer is never reinterpreted as physical.
    s[0u16].sectors <= 512
        && gpu_extent_valid(s[0u16].sector, s[0u16].sectors)
        && s[0u16].cpu.addr() & 63 == 0
        && (s[0u16].sectors << 6) - 1 <= 65535 - s[0u16].cpu.addr()
}

pub fn gpu_storage_init(mut out: Array<GpuStorage>, cpu: Ptr, sector: u16, sectors: u16) -> bool {
    let mut candidate: GpuStorage = GpuStorage {
        cpu: cpu,
        sector: sector,
        sectors: sectors,
    };
    if !gpu_storage_valid(view_of(&mut candidate)) {
        return false;
    }
    out[0u16] = candidate;
    true
}

pub fn gpu_storage_room(s: Array<GpuStorage>, word: u16, count: u16, align: u16) -> bool {
    gpu_storage_valid(s)
        && word & (align - 1) == 0
        && word <= s[0u16].sectors << 6
        && count <= (s[0u16].sectors << 6) - word
}

pub fn gpu_zero(destination: Ptr, words: u16) {
    let mut i: u16 = 0;
    while i < words {
        unsafe {
            destination.write(i as i16, 0);
        }
        i += 1;
    }
}

pub fn gpu_store32(destination: Ptr, low: u16, high: u16) {
    unsafe {
        destination.write(0, low);
        destination.write(1, high);
    }
}
