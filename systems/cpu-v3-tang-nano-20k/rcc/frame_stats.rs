//! CPU-side statistics only: no device number or GPU command ABI is assumed.
use crate::dsl_rt::*;

// Four words: previous start, previous interval, started, valid interval.
pub fn frame_stats_reset(mut state: Array<u16>) {
    state[0u16] = 0;
    state[1u16] = 0;
    state[2u16] = 0;
    state[3u16] = 0;
}

// Call at a frame-loop boundary. completed means the preceding frame actually
// completed GPU execution and presentation, without error or timeout. The
// caller must guarantee less than 65536 ms between boundaries, or pass false.
pub fn frame_stats_begin(mut state: Array<u16>, now: u16, completed: bool) {
    state[3u16] = 0;
    if completed && state[2u16] != 0 {
        state[1u16] = now - state[0u16];
        state[3u16] = 1;
    }
    state[0u16] = now;
    state[2u16] = 1;
}

// The caller reads and clears the bounded device timer at each frame boundary.
// A saturated interval has unknown duration and must not look like a valid time.
pub fn frame_stats_elapsed_begin(mut state: Array<u16>, elapsed_us: u16, completed: bool) {
    state[3u16] = 0;
    state[1u16] = elapsed_us;
    if completed && state[2u16] != 0 && elapsed_us != 65535 {
        state[3u16] = 1;
    }
    state[2u16] = 1;
}

// Four glyphs: "00.0" .. "65.5" ms, truncating below 100 us; invalid "--.-".
// The UI omits the first zero, preserving the zero before the decimal point.
pub fn frame_stats_tenths_digits(state: Array<u16>, mut digits: Array<u16>) {
    digits[0u16] = 45;
    digits[1u16] = 45;
    digits[2u16] = 46;
    digits[3u16] = 45;
    if state[3u16] == 0 {
        return;
    }
    let mut value = state[1u16];
    let mut digit: u16 = 48;
    while value >= 10000 {
        value -= 10000;
        digit += 1;
    }
    digits[0u16] = digit;
    digit = 48;
    while value >= 1000 {
        value -= 1000;
        digit += 1;
    }
    digits[1u16] = digit;
    digit = 48;
    while value >= 100 {
        value -= 100;
        digit += 1;
    }
    digits[3u16] = digit;
}

// Produces 5 ASCII-compatible glyph IDs. Invalid: "-----". Valid: "00000"
// through "65535". The UI atlas maps these IDs to the corresponding glyphs.
// There is no division, multiplication, float, or variable-width allocation.
pub fn frame_stats_digits(state: Array<u16>, mut digits: Array<u16>) {
    let mut i: u16 = 0;
    while i < 5 {
        digits[i] = 45;
        i += 1;
    }
    if state[3u16] == 0 {
        return;
    }
    let mut value = state[1u16];
    let mut digit: u16 = 48;
    while value >= 10000 {
        value -= 10000;
        digit += 1;
    }
    digits[0u16] = digit;
    digit = 48;
    while value >= 1000 {
        value -= 1000;
        digit += 1;
    }
    digits[1u16] = digit;
    digit = 48;
    while value >= 100 {
        value -= 100;
        digit += 1;
    }
    digits[2u16] = digit;
    digit = 48;
    while value >= 10 {
        value -= 10;
        digit += 1;
    }
    digits[3u16] = digit;
    digits[4u16] = 48 + value;
}
