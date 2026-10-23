//! Immutable boot assets. Units are 128-byte sectors and CPU 16-bit words.
use crate::dsl_rt::*;
mod gpu_memory;
use gpu_memory::*;
mod gpu_window;
use gpu_window::*;
mod gpu_layout;
use gpu_layout::*;

// Trusted packer roots, not arbitrary run-time imports. The packer validates
// native metadata/indices/uniform20 and the entire padded allocation once.
pub struct GpuMesh {
    pub directory_sector: u16,
    pub directory_word: u16,
    pub count: u16,
    pub first_sector: u16,
    pub sectors: u16,
}
// Complete immutable 64-KiB native RGBA5551 atlas, including unused slot 255.
pub struct GpuUiAtlas {
    pub sector: u16,
    pub sectors: u16,
}
pub struct GpuTexture {
    pub sector: u16,
    pub sectors: u16,
    pub size_log2: u16,
    pub full_mip: u16,
}
pub fn gpu_asset_extent(first: u16, sectors: u16) -> bool {
    first >= GPU_ASSET_FIRST_SECTOR && gpu_extent_valid(first, sectors)
}
fn gpu_mesh_root_valid(m: Array<GpuMesh>) -> bool {
    if !gpu_asset_extent(m[0u16].first_sector, m[0u16].sectors)
        || m[0u16].directory_sector < m[0u16].first_sector
        || m[0u16].directory_word >= 64
        || m[0u16].count > 5460
    {
        return false;
    }
    let offset = m[0u16].directory_sector - m[0u16].first_sector;
    if offset >= m[0u16].sectors {
        return false;
    }
    if m[0u16].count == 0 {
        return true;
    }
    let last = (m[0u16].directory_word + m[0u16].count * 2 - 1) >> 6;
    last < m[0u16].sectors - offset
}
fn gpu_mesh_batch(m: Array<GpuMesh>, index: u16, mut words: Array<u16>, count: u16) -> bool {
    if count == 0 || count > 16 || index > m[0u16].count || count > m[0u16].count - index {
        return false;
    }
    gpu_window_copy(
        words,
        m[0u16].directory_sector,
        m[0u16].directory_word + index * 2,
        count * 2,
        true,
    )
}
fn gpu_mesh_entries_valid(m: Array<GpuMesh>, words: Array<u16>, count: u16) -> bool {
    let mut i: u16 = 0;
    while i < count {
        let record = words[i * 2];
        let payload = words[i * 2 + 1];
        if record < m[0u16].first_sector || payload == 0 || payload > 8 {
            return false;
        }
        let offset = record - m[0u16].first_sector;
        if offset >= m[0u16].sectors || payload >= m[0u16].sectors - offset {
            return false;
        }
        // A record must not alias the directory, including its readable tail.
        let dir_first = m[0u16].directory_sector;
        let dir_sectors = (m[0u16].directory_word + m[0u16].count * 2 + 63) >> 6;
        if gpu_extents_overlap(record, payload + 1, dir_first, dir_sectors) {
            return false;
        }
        i += 1;
    }
    true
}
