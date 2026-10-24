//! Eight-bit stick calibration and radial processing. +X right, +Y down.
use crate::dsl_rt::*;

pub struct StickConfig {
    pub min_x: u16,
    pub center_x: u16,
    pub max_x: u16,
    pub min_y: u16,
    pub center_y: u16,
    pub max_y: u16,
    pub inner: u16,
    pub outer: u16,
}
// Q8 components:256 means1. Raw wire bytes remain owned by the input driver.
pub struct StickSample {
    pub cal_x: i16,
    pub cal_y: i16,
    pub x: i16,
    pub y: i16,
    pub dir_x: i16,
    pub dir_y: i16,
    pub magnitude: u16,
    pub valid: u16,
}
pub fn stick_default() -> StickConfig {
    StickConfig {
        min_x: 0,
        center_x: 128,
        max_x: 255,
        min_y: 0,
        center_y: 128,
        max_y: 255,
        inner: 24,
        outer: 256,
    }
}
pub fn stick_zero() -> StickSample {
    StickSample {
        cal_x: 0,
        cal_y: 0,
        x: 0,
        y: 0,
        dir_x: 0,
        dir_y: 0,
        magnitude: 0,
        valid: 0,
    }
}
pub fn stick_config_valid(c: Array<StickConfig>) -> bool {
    c[0u16].min_x < c[0u16].center_x
        && c[0u16].center_x < c[0u16].max_x
        && c[0u16].max_x <= 255
        && c[0u16].min_y < c[0u16].center_y
        && c[0u16].center_y < c[0u16].max_y
        && c[0u16].max_y <= 255
        && c[0u16].inner < c[0u16].outer
        && c[0u16].outer <= 256
}
pub fn stick_set_center(mut c: Array<StickConfig>, x: u16, y: u16) -> bool {
    if !stick_config_valid(c)
        || x <= c[0u16].min_x
        || x >= c[0u16].max_x
        || y <= c[0u16].min_y
        || y >= c[0u16].max_y
    {
        return false;
    }
    c[0u16].center_x = x;
    c[0u16].center_y = y;
    return true;
}
pub fn stick_fixed(q8: i16) -> fix32 {
    fix32::from_int(q8) * fix32::from_words(256, 0)
}
fn stick_recip(value: fix32) -> fix32 {
    let seed = frcp(value);
    seed * (fix32::from_int(2) - value * seed)
}
fn stick_axis(raw: u16, minimum: u16, center: u16, maximum: u16) -> fix32 {
    if raw <= minimum {
        return -fix32::from_int(1);
    }
    if raw >= maximum {
        return fix32::from_int(1);
    }
    if raw == center {
        return fix32::zero();
    }
    let mut span = maximum - center;
    if raw < center {
        span = center - minimum;
    }
    fix32::from_int((raw as i16) - (center as i16)) * stick_recip(fix32::from_int(span as i16))
}
fn stick_q8(value: fix32) -> i16 {
    let mut q = (value * fix32::from_int(256)).round().to_int();
    if q < -256 {
        q = -256;
    }
    if q > 256 {
        q = 256;
    }
    q
}
pub fn stick_read(c: Array<StickConfig>, raw_x: u16, raw_y: u16, connected: bool) -> StickSample {
    let mut s = stick_zero();
    if !connected || raw_x > 255 || raw_y > 255 || !stick_config_valid(c) {
        return s;
    }
    let x = stick_axis(raw_x, c[0u16].min_x, c[0u16].center_x, c[0u16].max_x);
    let y = stick_axis(raw_y, c[0u16].min_y, c[0u16].center_y, c[0u16].max_y);
    s.cal_x = stick_q8(x);
    s.cal_y = stick_q8(y);
    s.valid = 1;
    let squared = x * x + y * y;
    let inner = stick_fixed(c[0u16].inner as i16);
    if squared <= inner * inner {
        return s;
    }
    let seed = frsqrt(squared);
    // Multiply squared*seed first: seed*seed may exceed Q16.16 near zero.
    let inverse = seed
        * (fix32::from_words(32768, 1) - fix32::from_words(32768, 0) * (squared * seed) * seed);
    let dx = x * inverse;
    let dy = y * inverse;
    let radius = squared * inverse;
    let outer = stick_fixed(c[0u16].outer as i16);
    let mut magnitude = (radius - inner) * stick_recip(outer - inner);
    if magnitude > fix32::from_int(1) {
        magnitude = fix32::from_int(1);
    }
    if magnitude < fix32::zero() {
        magnitude = fix32::zero();
    }
    s.dir_x = stick_q8(dx);
    s.dir_y = stick_q8(dy);
    s.magnitude = stick_q8(magnitude) as u16;
    s.x = stick_q8(dx * magnitude);
    s.y = stick_q8(dy * magnitude);
    return s;
}
// 0 neutral,1 right,2 down,3 left,4 up. Activation hysteresis, X wins ties.
// Below enter, a previous valid direction is held until magnitude<=release.
pub fn stick_cardinal(s: Array<StickSample>, previous: u16, enter: u16, release: u16) -> u16 {
    if s[0u16].valid == 0
        || enter == 0
        || enter > 256
        || release >= enter
        || s[0u16].magnitude <= release
    {
        return 0;
    }
    if s[0u16].magnitude < enter {
        if previous >= 1 && previous <= 4 {
            return previous;
        }
        return 0;
    }
    let x = s[0u16].dir_x;
    let y = s[0u16].dir_y;
    let mut ax = x;
    let mut ay = y;
    if ax < 0 {
        ax = -ax;
    }
    if ay < 0 {
        ay = -ay;
    }
    if ax >= ay {
        if x >= 0 {
            return 1;
        }
        return 3;
    }
    if y >= 0 {
        return 2;
    }
    return 4;
}
