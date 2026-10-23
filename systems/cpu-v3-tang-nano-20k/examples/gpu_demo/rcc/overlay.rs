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
}

// completed requires successful GPU execution AND actual presentation of the
// preceding frame. The caller guarantees less than one 65536 ms counter wrap.
pub fn demo_overlay_begin(cells: Array<u16>, state: Array<u16>, now: u16, completed: bool) {
    frame_stats_begin(state, now, completed);
    let mut digits: Buf<u16, 5> = Buf::new([0; 5]);
    frame_stats_digits(state, digits.as_array());
    let mut i: u16 = 0;
    while i < 5 {
        demo_cell_set(cells, 52 + i, 255);
        i += 1;
    }
    if state[3u16] == 0 {
        return;
    }
    i = 0;
    while i < 4 && digits[i] == 48 {
        i += 1;
    }
    let mut cell: u16 = 52;
    while i < 5 {
        demo_cell_set(cells, cell, digits[i]);
        cell += 1;
        i += 1;
    }
}
