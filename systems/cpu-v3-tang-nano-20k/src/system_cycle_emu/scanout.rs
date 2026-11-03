//! Passive raster collection from registered Display pins, after real scanout.
use crate::hardware::display_cycle::PixelPipeOutput;
#[derive(Clone, Debug)]
pub struct Frame {
    pub number: u64,
    pub rgb: Vec<u8>,
    pub effective_pixels: usize,
}
#[derive(Default)]
pub struct Scanout {
    vsync: bool,
    started: bool,
    pixels: Vec<u8>,
    effective: usize,
    number: u64,
    complete: bool,
}
impl Scanout {
    pub fn sample(&mut self, p: PixelPipeOutput) -> Result<Option<Frame>, &'static str> {
        if p.vsync && !self.vsync {
            if self.started && !self.pixels.is_empty() && !self.complete {
                return Err("incomplete active raster before vertical sync");
            }
            self.started = true;
            self.pixels.clear();
            self.effective = 0;
            self.complete = false;
        }
        self.vsync = p.vsync;
        if !self.started || !p.de {
            return Ok(None);
        }
        if self.complete {
            return Err("more than one active raster between vertical sync pulses");
        }
        if p.precode.channels.iter().any(|c| !c.known || c.collision) {
            return Err("unknown or colliding registered Display pixel");
        }
        self.pixels.extend_from_slice(&p.rgb888);
        self.effective += usize::from(p.effective_source);
        let config = crate::display::ACTIVE_DISPLAY_CONFIG;
        let size = ((config.h_active_end - config.h_active_start)
            * (config.v_active_end - config.v_active_start)
            * 3) as usize;
        if self.pixels.len() == size {
            let frame = Frame {
                number: self.number,
                rgb: self.pixels.clone(),
                effective_pixels: self.effective,
            };
            self.number += 1;
            self.complete = true;
            return Ok(Some(frame));
        }
        Ok(None)
    }
}
