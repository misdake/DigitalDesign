// bench-max-cycles: 6000000
// bench-expected-halt: 9691
// bench-tier: medium
use crate::dsl_rt::*;

// fix32 escape-time Mandelbrot on a 32x24 grid: per point, z = z^2 + c in
// component scalar fix32 with an FCMP escape branch; exact total of the
// escape iteration counts.
const W: u16 = 32;
const H: u16 = 24;
const MAX_ITER: u16 = 32;

fn main() {
    let four = fix32::from_int(4);
    let mut total: u16 = 0;
    let mut py: u16 = 0;
    while py < H {
        // cy in [-1.0, 1.0): step 2/24 = 1/12 -> 21.33/256; use 21/256
        let cy_raw = (py << 4) + (py << 2) + py;
        let cy = fix32::from_words(cy_raw << 8, cy_raw >> 8) - fix32::from_int(1);
        let mut px: u16 = 0;
        while px < W {
            // cx in [-2.0, 1.0): step 3/32 = 24/256
            let cx_raw = (px << 4) + (px << 3);
            let cx = fix32::from_words(cx_raw << 8, cx_raw >> 8) - fix32::from_int(2);
            let mut x = fix32::zero();
            let mut y = fix32::zero();
            let mut iter: u16 = 0;
            while iter < MAX_ITER {
                let xx = x * x;
                let yy = y * y;
                if xx + yy > four {
                    break;
                }
                let nx = xx - yy + cx;
                y = x * y + x * y + cy;
                x = nx;
                iter = iter + 1;
            }
            total = total + iter;
            px = px + 1;
        }
        py = py + 1;
    }
    halt(total);
}
