#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum DepthFunc {
    Never,
    Less,
    Equal,
    LessEqual,
    Greater,
    NotEqual,
    GreaterEqual,
    Always,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blend {
    Replace,
    SrcOver,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Context {
    pub depth: DepthFunc,
    pub depth_write: bool,
    pub blend: Blend,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fragment {
    pub rgba: [u8; 4],
    pub depth: u16,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub x: u16,
    pub y: u8,
    pub mask: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Quad {
    pub header: Header,
    pub pixels: [Fragment; 4],
}
impl Quad {
    pub fn rows(self) -> [u32; 8] {
        std::array::from_fn(|r| {
            if r & 1 == 0 {
                u32::from_le_bytes(self.pixels[r / 2].rgba)
            } else {
                u32::from(self.pixels[r / 2].depth)
            }
        })
    }
}
/// The only supported content state. Future framemeta belongs outside cache dirty bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MaterializedSurface {
    pub color_base_bytes: u32,
    pub depth_base_bytes: u32,
    pub width: u16,
    pub height: u16,
}
impl MaterializedSurface {
    pub fn validate(self) -> Result<(), String> {
        let length = u64::from(self.width / 16) * u64::from(self.height / 16) * 512;
        let c = u64::from(self.color_base_bytes);
        let d = u64::from(self.depth_base_bytes);
        if self.width == 0
            || self.width > 512
            || self.height == 0
            || self.height > 256
            || !self.width.is_multiple_of(16)
            || !self.height.is_multiple_of(16)
            || c % 512 != 0
            || d % 512 != 0
            || c + length > 1 << 32
            || d + length > 1 << 32
            || !(c + length <= d || d + length <= c)
        {
            return Err("invalid or overlapping materialized planes".into());
        }
        Ok(())
    }
    pub fn tile(self, h: Header) -> u16 {
        (u16::from(h.y) / 16) * (self.width / 16) + h.x / 16
    }
    pub fn address(self, plane: usize, tile: u16, word: usize) -> u64 {
        u64::from(if plane == 0 {
            self.color_base_bytes
        } else {
            self.depth_base_bytes
        }) + u64::from(tile) * 512
            + word as u64 * 2
    }
    pub fn validate_header(self, h: Header) -> Result<(), String> {
        if h.x & 1 != 0
            || h.y & 1 != 0
            || h.x + 1 >= self.width
            || u16::from(h.y) + 1 >= self.height
            || h.mask == 0
            || h.mask > 15
        {
            return Err("quad bounds/alignment/mask".into());
        }
        Ok(())
    }
}
/// A 16-bit word address inside one of four true dual-port banks.
pub fn bank_address(plane: usize, line: usize, x: usize, y: usize) -> (usize, usize) {
    assert!(plane < 2 && line < 8 && x < 16 && y < 16);
    (
        (x & 3) ^ ((y & 1) << 1),
        plane * 512 + line * 64 + y * 4 + (x >> 2),
    )
}
pub use crate::memory::ports::{MemoryPort, Request, Response};
