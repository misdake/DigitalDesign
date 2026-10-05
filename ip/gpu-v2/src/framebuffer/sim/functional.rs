//! Atomic eight-line tile cache for numerical oracle composition. No cycle/port
//! scheduling is executed; addresses, replacement and writeback affect real data.
use super::oracle::{self, Pixel};
use crate::framebuffer::ports::{Context, Fragment};

#[derive(Clone)]
struct Line {
    tag: Option<usize>,
    pixels: [Pixel; 256],
    dirty: bool,
}
pub struct Cache {
    pub width: usize,
    height: usize,
    pitch: usize,
    backing: Vec<Pixel>,
    lines: [Line; 8],
    pub refills: usize,
    pub writebacks: usize,
}
impl Cache {
    pub fn new(width: u16, height: u16, clear: Pixel) -> Result<Self, String> {
        if width == 0 || width > 400 || height == 0 || height > 240 {
            return Err("functional surface size".into());
        }
        let pitch = usize::from(width).div_ceil(16);
        Ok(Self {
            width: width as usize,
            height: height as usize,
            pitch,
            backing: vec![clear; pitch * usize::from(height).div_ceil(16) * 256],
            lines: std::array::from_fn(|_| Line {
                tag: None,
                pixels: [clear; 256],
                dirty: false,
            }),
            refills: 0,
            writebacks: 0,
        })
    }
    fn writeback(&mut self, index: usize) {
        let line = &mut self.lines[index];
        if line.dirty {
            let start = line.tag.expect("dirty line has tag") * 256;
            self.backing[start..start + 256].copy_from_slice(&line.pixels);
            line.dirty = false;
            self.writebacks += 1;
        }
    }
    pub fn apply(
        &mut self,
        x: u16,
        y: u16,
        fragment: Fragment,
        context: Context,
    ) -> Result<bool, String> {
        let (x, y) = (x as usize, y as usize);
        if x >= self.width || y >= self.height {
            return Err("functional ROP address".into());
        }
        let tag = (y / 16) * self.pitch + x / 16;
        let index = tag & 7;
        if self.lines[index].tag != Some(tag) {
            self.writeback(index);
            self.lines[index]
                .pixels
                .copy_from_slice(&self.backing[tag * 256..tag * 256 + 256]);
            self.lines[index].tag = Some(tag);
            self.refills += 1;
        }
        let row = (y & 15) * 16 + (x & 15);
        let result = oracle::pixel(self.lines[index].pixels[row], fragment, true, context);
        self.lines[index].pixels[row] = result.pixel;
        self.lines[index].dirty |= result.color_written || result.depth_written;
        Ok(result.color_written)
    }
    pub fn materialize(&mut self) -> Vec<Pixel> {
        for index in 0..8 {
            self.writeback(index);
        }
        (0..self.width * self.height)
            .map(|i| {
                let (x, y) = (i % self.width, i / self.width);
                self.backing[((y / 16) * self.pitch + x / 16) * 256 + (y & 15) * 16 + (x & 15)]
            })
            .collect()
    }
}
