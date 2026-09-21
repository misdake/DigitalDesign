// bench-max-cycles: 2000000
// bench-expected-halt: 0
// bench-tier: frame
use crate::dsl_rt::*;

// fix16 particle physics, 10 frames: pos += vel * dt with edge reflection.
// Scalar Q16.16 per component, stored as a low/high word pair in statics: a
// component's low word sits at `i` and its high word at `i + N`.
const N: u16 = 128;
const FRAMES: u16 = 10;
static X: Buf<u16, 256> = Buf::new([0; 256]);
static Y: Buf<u16, 256> = Buf::new([0; 256]);
static VX: Buf<u16, 256> = Buf::new([0; 256]);
static VY: Buf<u16, 256> = Buf::new([0; 256]);

fn main() {
    let mut x = X.as_array();
    let mut y = Y.as_array();
    let mut vx = VX.as_array();
    let mut vy = VY.as_array();
    let limit = fix16::from_int(120);
    let dt = fix16::from_words(0x4000, 0); // 0.25
    let mut i: u16 = 0;
    while i < N {
        let p = fix16::from_words((i & 15) << 11, 0);
        x[i] = p.lo_bits();
        x[i + N] = p.hi_bits();
        let q = fix16::from_words((i & 31) << 10, 0);
        y[i] = q.lo_bits();
        y[i + N] = q.hi_bits();
        let u = fix16::from_words(((i & 3) + 1) << 12, 0);
        vx[i] = u.lo_bits();
        vx[i + N] = u.hi_bits();
        let w = fix16::from_words(((i & 7) + 1) << 10, 0);
        vy[i] = w.lo_bits();
        vy[i + N] = w.hi_bits();
        i = i + 1;
    }
    let mut frame: u16 = 0;
    while frame < FRAMES {
        i = 0;
        while i < N {
            let mut px = fix16::from_words(x[i], x[i + N]);
            let mut py = fix16::from_words(y[i], y[i + N]);
            let mut sx = fix16::from_words(vx[i], vx[i + N]);
            let mut sy = fix16::from_words(vy[i], vy[i + N]);
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
            x[i] = px.lo_bits();
            x[i + N] = px.hi_bits();
            y[i] = py.lo_bits();
            y[i + N] = py.hi_bits();
            vx[i] = sx.lo_bits();
            vx[i + N] = sx.hi_bits();
            vy[i] = sy.lo_bits();
            vy[i + N] = sy.hi_bits();
            i = i + 1;
        }
        frame = frame + 1;
    }
    let mut cs: u16 = 0;
    i = 0;
    while i < N {
        cs = cs ^ x[i] ^ x[i + N] ^ y[i] ^ y[i + N];
        cs = (cs << 1) | (cs >> 15);
        i = i + 1;
    }
    halt(cs);
}
