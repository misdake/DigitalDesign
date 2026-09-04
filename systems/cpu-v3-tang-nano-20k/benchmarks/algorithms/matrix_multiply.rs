use crate::dsl_rt::*;
use crate::rcc_std::*;

const N: u16 = 16;
static A: [u16; 256] = [0; 256];
static B: [u16; 256] = [0; 256];
static C: [u16; 256] = [0; 256];

fn main() {
    let mut a = A.as_array();
    let mut b = B.as_array();
    let mut c = C.as_array();
    let mut i: u16 = 0;
    while i < 256 {
        a[i] = (i & 15) + 1;
        b[i] = ((i >> 4) & 15) + 1;
        c[i] = 0;
        i = i + 1;
    }
    let mut row: u16 = 0;
    while row < N {
        let mut col: u16 = 0;
        while col < N {
            let mut k: u16 = 0;
            let mut sum: u16 = 0;
            while k < N {
                sum = sum + mul_16x8(a[(row << 4) + k], b[(k << 4) + col]);
                k = k + 1;
            }
            c[(row << 4) + col] = sum;
            col = col + 1;
        }
        row = row + 1;
    }
    i = 0;
    while i < 256 {
        if c[i] != 1496 { halt(0); }
        i = i + 1;
    }
    halt(1);
}
