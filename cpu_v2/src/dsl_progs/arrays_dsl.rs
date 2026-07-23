//! rcc subset sample: constants, globals, arrays, addr_of.
//! compiled by `compiler::frontend` tests; also valid host Rust.

use crate::dsl_rt::*;

const WIDTH: u16 = 8;

static SCORE: u16 = 0;
static TILE: [u16; 4] = [0x3c66, 0xc3ff, 0xffc3, 0x663c];

fn add_row(mut buf: Array<u16>, row: u16, value: u16) {
    // row offset = row * WIDTH, WIDTH is 8
    buf[row << 3] = value;
}

fn main() {
    // local array on the stack
    let grid: [u16; 64] = [0; 64];
    let grid_view = grid.as_array();
    let tile = TILE.as_array();
    add_row(grid_view, 2, tile[1u16]);

    // take the address of a local (sp + slot at run time)
    let total: u16 = 0;
    let mut total_view = addr_of(&total).as_u16_array();
    total_view[0u16] = grid_view[16u16];

    // take the address of a global (compile-time constant)
    let mut score = addr_of(&SCORE).as_u16_array();
    score[0u16] = total;
    halt(SCORE);
}
