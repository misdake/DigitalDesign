//! Packed 50x15 character cells: the earlier cell is the low byte.
use crate::dsl_rt::*;
mod frame_stats;
use frame_stats::*;

// The caller owns exactly 375 words. FF means preserve the rendered background.
pub fn demo_cell_set(mut cells: Array<u16>, cell: u16, glyph: u16) -> bool {
    if cell >= 750 || glyph > 255 {
        return false;
    }
    let word = cell >> 1;
    if cell & 1 == 0 {
        cells[word] = (cells[word] & 65280) | glyph;
    } else {
        cells[word] = (cells[word] & 255) | (glyph << 8);
    }
    true
}

pub fn demo_overlay_init(mut cells: Array<u16>, state: Array<u16>) {
    let mut word: u16 = 0;
    while word < 375 {
        cells[word] = 65535;
        word += 1;
    }
    frame_stats_reset(state);
    // Row 1, column 2: FRAME ----- MS. Spaces use the transparent glyph.
    demo_cell_set(cells, 52, 70);
    demo_cell_set(cells, 53, 82);
    demo_cell_set(cells, 54, 65);
    demo_cell_set(cells, 55, 77);
    demo_cell_set(cells, 56, 69);
    demo_cell_set(cells, 57, 32);
    let mut cell: u16 = 58;
    while cell < 63 {
        demo_cell_set(cells, cell, 45);
        cell += 1;
    }
    demo_cell_set(cells, 63, 32);
    demo_cell_set(cells, 64, 77);
    demo_cell_set(cells, 65, 83);
}

// completed requires successful GPU execution AND actual presentation of the
// preceding frame. The caller guarantees less than one 65536 ms counter wrap.
pub fn demo_overlay_begin(cells: Array<u16>, state: Array<u16>, now: u16, completed: bool) {
    frame_stats_begin(state, now, completed);
    let mut digits: Buf<u16, 5> = Buf::new([0; 5]);
    frame_stats_digits(state, digits.as_array());
    let mut i: u16 = 0;
    while i < 5 {
        demo_cell_set(cells, 58 + i, digits[i]);
        i += 1;
    }
}
