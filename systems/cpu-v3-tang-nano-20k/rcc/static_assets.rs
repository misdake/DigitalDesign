//! Immutable external bytes: physical addresses use 128-byte sectors.
use crate::dsl_rt::*;
mod gpu_layout;
use gpu_layout::*;

pub const GPU_ASSET_ALIGNMENT: usize = 128;
pub const GPU_TEXTURE_ALIGNMENT: usize =
    (GPU_TEXTURE_ALIGNMENT_SECTORS as usize) * GPU_ASSET_ALIGNMENT;
pub const UI_ATLAS_ALIGNMENT: usize = 64 * 1024;

#[derive(Copy, Clone)]
pub struct AssetBytes {
    pub first_sector: u16,
    pub sector_count: u16,
    pub byte_len_lo: u16,
    pub byte_len_hi: u16,
}
impl AssetBytes {
    /// An #[asset] declaration supplies placement before target compilation.
    pub const fn new(bytes: &[u8]) -> Self {
        Self {
            first_sector: 0,
            sector_count: ((bytes.len() + 127) / 128) as u16,
            byte_len_lo: bytes.len() as u16,
            byte_len_hi: (bytes.len() >> 16) as u16,
        }
    }
}
