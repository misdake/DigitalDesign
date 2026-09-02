// bench-max-cycles: 2000000
// bench-expected-halt: 0
// bench-tier: frame
use crate::dsl_rt::*;

// fix16 particle physics, 10 frames: pos += vel * dt with edge reflection.
// Scalar fix16 per component, stored as words in statics.
const N: u16 = 128;
const FRAMES: u16 = 10;
static X: [u16; 128] = [0; 128];
static Y: [u16; 128] = [0; 128];
static VX: [u16; 128] = [0; 128];
static VY: [u16; 128] = [0; 128];

fn main() {
    let mut x = X.as_array();
    let mut y = Y.as_array();
    let mut vx = VX.as_array();
    let mut vy = VY.as_array();
    let limit = fix16::from_int(120);
    let dt = fix16::from_bits(64); // 0.25
    let mut i: u16 = 0;
    while i < N {
        x[i] = fix16::from_bits((i & 15) << 3).to_bits();
        y[i] = fix16::from_bits((i & 31) << 2).to_bits();
        vx[i] = fix16::from_bits((((i & 3) + 1) << 4) as u16).to_bits();
        vy[i] = fix16::from_bits((((i & 7) + 1) << 2) as u16).to_bits();
        i = i + 1;
    }
    let mut frame: u16 = 0;
    while frame < FRAMES {
        i = 0;
        while i < N {
            let mut px = fix16::from_bits(x[i]);
            let mut py = fix16::from_bits(y[i]);
            let mut sx = fix16::from_bits(vx[i]);
            let mut sy = fix16::from_bits(vy[i]);
            px = px + sx * dt;
            py = py + sy * dt;
            if px < fix16::zero() {
                px = -px;
                sx = -sx;
            }
            if px > limit {
                px = limit + limit - px;
                sx = -sx;
            }
            if py < fix16::zero() {
                py = -py;
                sy = -sy;
            }
            if py > limit {
                py = limit + limit - py;
                sy = -sy;
            }
            x[i] = px.to_bits();
            y[i] = py.to_bits();
            vx[i] = sx.to_bits();
            vy[i] = sy.to_bits();
            i = i + 1;
        }
        frame = frame + 1;
    }
    let mut cs: u16 = 0;
    i = 0;
    while i < N {
        cs = cs ^ x[i] ^ y[i];
        cs = (cs << 1) | (cs >> 15);
        i = i + 1;
    }
    halt(cs);
}
