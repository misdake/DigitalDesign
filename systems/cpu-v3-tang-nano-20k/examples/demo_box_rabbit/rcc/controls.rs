//! Frame-boundary polling with a 20ms stable-level debounce.
use crate::dsl_rt::*;
// candidate, stable level, candidate timestamp, visible.
pub fn demo_buttons_init(mut state: Array<u16>, raw: u16, now: u16) {
    state[0u16] = raw & 1;
    state[1u16] = raw & 1;
    state[2u16] = now;
    state[3u16] = 1;
}
pub fn demo_buttons_update(mut state: Array<u16>, raw: u16, now: u16) -> bool {
    let button = raw & 1;
    if button != state[0u16] {
        state[0u16] = button;
        state[2u16] = now;
    }
    if state[0u16] != state[1u16] && now - state[2u16] >= 20 {
        state[1u16] = state[0u16];
        if state[1u16] != 0 {
            state[3u16] ^= 1;
        }
    }
    state[3u16] != 0
}
