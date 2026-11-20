//! Diagnostic-only control policy; the shared stick module has no button mapping.
use crate::dsl_rt::*;
mod stick;
use stick::*;
static LEFT_CONFIG: StickConfig = StickConfig {
    min_x: 0,
    center_x: 128,
    max_x: 255,
    min_y: 0,
    center_y: 128,
    max_y: 255,
    inner: 24,
    outer: 256,
};
static RIGHT_CONFIG: StickConfig = StickConfig {
    min_x: 0,
    center_x: 128,
    max_x: 255,
    min_y: 0,
    center_y: 128,
    max_y: 255,
    inner: 24,
    outer: 256,
};
static LEFT_READY: StickPrepared = StickPrepared {
    min_x: 0,
    center_x: 0,
    max_x: 0,
    min_y: 0,
    center_y: 0,
    max_y: 0,
    negative_x: fix32::zero(),
    positive_x: fix32::zero(),
    negative_y: fix32::zero(),
    positive_y: fix32::zero(),
    inner: fix32::zero(),
    inverse_range: fix32::zero(),
    valid: 0,
};
static RIGHT_READY: StickPrepared = StickPrepared {
    min_x: 0,
    center_x: 0,
    max_x: 0,
    min_y: 0,
    center_y: 0,
    max_y: 0,
    negative_x: fix32::zero(),
    positive_x: fix32::zero(),
    negative_y: fix32::zero(),
    positive_y: fix32::zero(),
    inner: fix32::zero(),
    inverse_range: fix32::zero(),
    valid: 0,
};
fn dui_prepare() {
    LEFT_READY = stick_prepare(view_of(&mut LEFT_CONFIG));
    RIGHT_READY = stick_prepare(view_of(&mut RIGHT_CONFIG));
}
pub fn dui_controls_reset() {
    let defaults = stick_default();
    let mut l = view_of(&mut LEFT_CONFIG);
    let mut r = view_of(&mut RIGHT_CONFIG);
    l[0u16] = defaults;
    r[0u16] = defaults;
    dui_prepare();
}
pub fn dui_deadzone() -> u16 {
    LEFT_CONFIG.inner
}
pub fn dui_controls(s: Array<u16>) {
    if s[0u16] == 0 {
        return;
    }
    let pressed = s[8u16];
    if (pressed & 89) == 0 {
        return;
    }
    let mut changed = false;
    if (pressed & 8) != 0 {
        dui_controls_reset();
        return;
    }
    let mut left = view_of(&mut LEFT_CONFIG);
    let mut right = view_of(&mut RIGHT_CONFIG);
    if (pressed & 1) != 0
        && s[3u16] >= 96
        && s[3u16] <= 160
        && s[4u16] >= 96
        && s[4u16] <= 160
        && s[5u16] >= 96
        && s[5u16] <= 160
        && s[6u16] >= 96
        && s[6u16] <= 160
    {
        changed = stick_set_center(left, s[5u16], s[6u16]);
        if stick_set_center(right, s[3u16], s[4u16]) {
            changed = true;
        }
    }
    let directions = pressed & 80;
    if directions == 16 && left[0u16].inner < 60 {
        left[0u16].inner += 4;
        changed = true;
    }
    if directions == 64 && left[0u16].inner >= 4 {
        left[0u16].inner -= 4;
        changed = true;
    }
    if changed {
        right[0u16].inner = left[0u16].inner;
        dui_prepare();
    }
}
pub fn dui_left(s: Array<u16>) -> StickSample {
    stick_read(view_of(&mut LEFT_READY), s[5u16], s[6u16], s[0u16] != 0)
}
pub fn dui_right(s: Array<u16>) -> StickSample {
    stick_read(view_of(&mut RIGHT_READY), s[3u16], s[4u16], s[0u16] != 0)
}
