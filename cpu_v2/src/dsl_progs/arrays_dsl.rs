//! rcc subset sample: constants, globals, arrays, addr_of.
//! compiled by `compiler::frontend` tests; also valid host Rust.

use crate::dsl_rt::*;

const WIDTH: u16 = 8;

static SCORE: u16 = 0;
static TILE: [u16; 4] = [0x3c66, 0xc3ff, 0xffc3, 0x663c];

fn add_row(buf: Ptr, row: u16, value: u16) {
    // row offset = row * WIDTH, WIDTH is 8
    let base = buf.add((row << 3) as i16);
    base.write(0, value);
}

fn main() {
    // local array on the stack
    let grid: [u16; 64] = [0; 64];
    add_row(grid.as_ptr(), 2, TILE.read(1));

    // take the address of a local (sp + slot at run time)
    let total: u16 = 0;
    let p = addr_of(&total);
    p.write(0, grid.read(16));

    // take the address of a global (compile-time constant)
    let s = addr_of(&SCORE);
    s.write(0, total);
    halt(SCORE);
}
