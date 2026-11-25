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
        let id = fix32::from_words(i, 0);
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
        out[12 + i * 6] = flags;
        out[13 + i * 6] = class_word;
        attitude::put(out, 7 + i * 3, entities::handle(state, id));
        attitude::put(out, 8 + i * 3, entities::next_free(state, id));
        i += 1;
    }
    12 + count * 6
}
