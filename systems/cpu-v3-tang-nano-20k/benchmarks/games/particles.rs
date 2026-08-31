use crate::dsl_rt::*;

const N: u16 = 256;
static X: [u16; 256] = [0; 256];
static Y: [u16; 256] = [0; 256];
static VX: [u16; 256] = [0; 256];
static VY: [u16; 256] = [0; 256];

fn main() {
    let mut x = X.as_array(); let mut y = Y.as_array();
    let mut vx = VX.as_array(); let mut vy = VY.as_array();
    let mut i: u16 = 0;
    while i < N {
        x[i] = i & 255; y[i] = ((i << 1) + i) & 255;
        vx[i] = (i & 3) + 1; vy[i] = ((i >> 2) & 3) + 1;
        i = i + 1;
    }
    let mut frame: u16 = 0;
    while frame < 240 {
        i = 0;
        while i < N {
            x[i] = x[i] + vx[i]; y[i] = y[i] + vy[i];
            if x[i] >= 320 { x[i] = x[i] - 320; }
            if y[i] >= 240 { y[i] = y[i] - 240; }
            i = i + 1;
        }
        frame = frame + 1;
    }
    if x[17u16] < 320 && y[93u16] < 240 { halt(1); } else { halt(0); }
}
