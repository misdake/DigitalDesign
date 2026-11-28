//! Canonical live identity words; retired class/component payloads are absent.
use crate::dsl_rt::*;
use crate::{attitude, entities, integer};
pub fn write(state: &entities::State, mut out: Array<u16>) -> u16 {
    let mut i = 0u16;
    while i < 12 {
        out[i] = state.header[i];
        i += 1;
    }
    let count = attitude::get(state.header, 0).lo_bits();
    i = 0;
    while i < count {
        write_row(state, i, unsafe {
            out.as_ptr().add((12 + i * 6) as i16).as_u16_array()
        });
        i += 1;
    }
    12 + count * 6
}

// A row has canonical flags/class and two raw i32 payloads.
pub fn write_row(state: &entities::State, index: u16, mut out: Array<u16>) {
    let id = fix32::from_words(index, 0);
    let kind = entities::handle_kind(state, id);
    let class = entities::class(state, id);
    let mut flags = kind << 2;
    let mut class_word = 0u16;
    if entities::present(state, id) {
        flags |= 1;
    }
    if class != integer::from_i16(-1) {
        flags |= 2;
        class_word = class.lo_bits();
    }
    out[0u16] = flags;
    out[1u16] = class_word;
    attitude::put(out, 1, entities::handle(state, id));
    attitude::put(out, 2, entities::next_free(state, id));
}
