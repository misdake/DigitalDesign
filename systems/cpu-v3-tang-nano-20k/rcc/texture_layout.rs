//! Native square-texture layout values. No allocation or device work.
use crate::dsl_rt::*;
mod gpu_scene_objects;
use gpu_scene_objects::*;

#[repr(u16)]
#[derive(Copy, Clone)]
pub enum Mipmaps {
    BaseOnly,
    Full,
}

#[derive(Copy, Clone)]
pub struct TextureLayout {
    pub size_log2: u16,
    pub full_mip: bool,
    pub format: TextureFormat,
}
const fn texture_size_log2(size: u16) -> u16 {
    if size <= 1 {
        0
    } else {
        1 + texture_size_log2(size >> 1)
    }
}
const fn texture_level_sectors(level: u16) -> u16 {
    if level <= 4 {
        4
    } else {
        4 << ((level - 4) * 2)
    }
}
const fn texture_mip_sectors(level: u16) -> u16 {
    if level == 0 {
        4
    } else {
        texture_mip_sectors(level - 1) + texture_level_sectors(level)
    }
}
impl TextureLayout {
    /// Pixel size, format and mip choice are the only independent inputs.
    pub const fn square(size: u16, format: TextureFormat, mipmaps: Mipmaps) -> Self {
        debug_assert!(size != 0 && size <= 1024 && size & (size - 1) == 0);
        Self {
            size_log2: texture_size_log2(size),
            full_mip: mipmaps as u16 != 0,
            format,
        }
    }
    pub const fn required_sectors(self) -> u16 {
        debug_assert!(self.size_log2 <= 10);
        if self.full_mip {
            texture_mip_sectors(self.size_log2)
        } else {
            texture_level_sectors(self.size_log2)
        }
    }
}
