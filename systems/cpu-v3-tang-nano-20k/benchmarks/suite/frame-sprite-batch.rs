// bench-max-cycles: 8000000
// bench-expected-halt: 1
// bench-tier: frame
use crate::dsl_rt::*;

static FRAMEBUFFER: [u16; 4096] = [0; 4096];

fn main() {
    let mut pixels = FRAMEBUFFER.as_array();
    let mut frame: u16 = 0;
    let mut checksum: u16 = 0;
    while frame < 10 {
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
    let mut framebuffer_sum: u16 = 0;
    let mut framebuffer_xor: u16 = 0;
    let mut address: u16 = 0;
    while address < 4096 {
        framebuffer_sum = framebuffer_sum + pixels[address];
        framebuffer_xor = framebuffer_xor ^ pixels[address];
        address = address + 1;
    }
    if checksum == 45056 && framebuffer_sum == 12980 && framebuffer_xor == 4074 {
        halt(1);
    } else {
        halt(0);
    }
}
