use crate::dsl_rt::*;

static FRAMEBUFFER: [u16; 4096] = [0; 4096];

fn main() {
    let mut pixels = FRAMEBUFFER.as_array();
    let mut frame: u16 = 0;
    let mut checksum: u16 = 0;
    while frame < 60 {
        let mut sprite: u16 = 0;
        while sprite < 64 {
            let base = ((sprite << 6) - (sprite << 2) - sprite
                + (frame << 4) + frame) & 0x0fff;
            let mut pixel: u16 = 0;
            while pixel < 64 {
                let address = (base + pixel) & 0x0fff;
                let color = (sprite << 6) ^ pixel ^ frame;
                pixels[address] = color;
                checksum = checksum + color;
                pixel = pixel + 1;
            }
            sprite = sprite + 1;
        }
        frame = frame + 1;
    }
    if checksum != 0 { halt(1); } else { halt(0); }
}
