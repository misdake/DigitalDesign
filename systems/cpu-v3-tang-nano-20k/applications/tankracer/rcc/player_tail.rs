//! Gun revision publication and non-AI horn state after the physical update.
use crate::dsl_rt::*;
use crate::integer;
pub struct State {
    pub gun: Array<i16>,
    pub binding: Array<u16>,
    // Down, latched, signed timer, then the four little-endian revision words.
    pub fields: Array<u16>,
}
pub fn capture(state: &State, mut result: Array<u16>) {
    result[0u16] = 0;
    let mut i = 0u16;
    while i < 4 {
        result[1 + i] = 0;
        if state.binding[0u16] != 0 {
            result[0u16] |= 16;
            let vertex = state.binding[2 + i];
            if vertex < state.binding[1u16] {
                result[0u16] |= 1 << i;
                result[1 + i] = state.gun[vertex * 3 + 2] as u16;
            }
        }
        i += 1;
    }
}
// Flags: visible, primary non-AI Player, keys present, horn down.
// Expiry leaves the timer for the caller to clear after ordered peer delivery.
pub fn finish(state: &State, before: Array<u16>, dt: fix32, flags: u16, success: bool) -> bool {
    if flags & 1 == 0 {
        return false;
    }
    let current: Buf<u16, 5> = Buf::new([0; 5]);
    capture(state, current.as_array());
    let mut changed = false;
    let mut i = 0u16;
    while i < 5 {
        if before[i] != current[i] {
            changed = true;
        }
        i += 1;
    }
    let mut fields = state.fields;
    if changed && state.binding[0u16] != 0 {
        i = 3;
        while i < 7 {
            fields[i] += 1;
            if fields[i] != 0 {
                break;
            }
            i += 1;
        }
    }
    if !success || flags & 2 == 0 {
        return false;
    }
    fields[0u16] = 0;
    if flags & 12 == 12 {
        fields[0u16] = 1;
    }
    if fields[0u16] == 0 {
        fields[1u16] = 0;
    } else if fields[1u16] == 0 {
        fields[1u16] = 1;
        fields[2u16] = 1000;
    }
    let timer = fields[2u16];
    if timer == 0 {
        return false;
    }
    if dt < integer::from_i16(timer as i16) {
        fields[2u16] = timer - dt.lo_bits();
        return false;
    }
    true
}
