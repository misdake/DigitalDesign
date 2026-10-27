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
// Rebuild only when calibration or dead-zone settings change.
pub struct StickPrepared {
    pub min_x: u16,
    pub center_x: u16,
    pub max_x: u16,
    pub min_y: u16,
    pub center_y: u16,
    pub max_y: u16,
    pub negative_x: fix32,
    pub positive_x: fix32,
    pub negative_y: fix32,
    pub positive_y: fix32,
    pub inner: fix32,
    pub inverse_range: fix32,
    pub valid: u16,
}
// Native FPU Q16.16 components. Raw wire bytes remain owned by the input driver.
pub struct StickSample {
    pub cal_x: fix32,
    pub cal_y: fix32,
    pub x: fix32,
    pub y: fix32,
    pub dir_x: fix32,
    pub dir_y: fix32,
    pub magnitude: fix32,
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
        cal_x: fix32::zero(),
        cal_y: fix32::zero(),
        x: fix32::zero(),
        y: fix32::zero(),
        dir_x: fix32::zero(),
        dir_y: fix32::zero(),
        magnitude: fix32::zero(),
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
// Reciprocal refinement belongs to configuration changes, never the hot path.
fn stick_inverse(value: fix32) -> fix32 {
    let seed = frcp(value);
    seed * (fix32::from_int(2) - value * seed)
}
pub fn stick_prepare(c: Array<StickConfig>) -> StickPrepared {
    if !stick_config_valid(c) {
        return StickPrepared {
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
    }
    let inner = c[0u16].inner;
    let range = c[0u16].outer - inner;
    return StickPrepared {
        min_x: c[0u16].min_x,
        center_x: c[0u16].center_x,
        max_x: c[0u16].max_x,
        min_y: c[0u16].min_y,
        center_y: c[0u16].center_y,
        max_y: c[0u16].max_y,
        negative_x: stick_inverse(fix32::from_int((c[0u16].center_x - c[0u16].min_x) as i16)),
        positive_x: stick_inverse(fix32::from_int((c[0u16].max_x - c[0u16].center_x) as i16)),
        negative_y: stick_inverse(fix32::from_int((c[0u16].center_y - c[0u16].min_y) as i16)),
        positive_y: stick_inverse(fix32::from_int((c[0u16].max_y - c[0u16].center_y) as i16)),
        inner: fix32::from_words(inner << 8, 0),
        inverse_range: stick_inverse(fix32::from_words((range & 255) << 8, range >> 8)),
        valid: 1,
    };
}
// Return directly into the caller's result, with no intermediate aggregate copy.
pub fn stick_read(p: Array<StickPrepared>, raw_x: u16, raw_y: u16, connected: bool) -> StickSample {
    if !connected || raw_x > 255 || raw_y > 255 || p[0u16].valid == 0 {
        return stick_zero();
    }
    let zero = fix32::zero();
    let one = fix32::from_int(1);
    let mut x = zero;
    if raw_x <= p[0u16].min_x {
        x = -one;
    } else if raw_x >= p[0u16].max_x {
        x = one;
    } else {
        let center = p[0u16].center_x;
        if raw_x != center {
            let mut inverse = zero;
            if raw_x < center {
                inverse = p[0u16].negative_x;
            } else {
                inverse = p[0u16].positive_x;
            }
            x = fix32::from_int((raw_x as i16) - (center as i16)) * inverse;
        }
    }
    let mut y = zero;
    if raw_y <= p[0u16].min_y {
        y = -one;
    } else if raw_y >= p[0u16].max_y {
        y = one;
    } else {
        let center = p[0u16].center_y;
        if raw_y != center {
            let mut inverse = zero;
            if raw_y < center {
                inverse = p[0u16].negative_y;
            } else {
                inverse = p[0u16].positive_y;
            }
            y = fix32::from_int((raw_y as i16) - (center as i16)) * inverse;
        }
    }
    let ax = x.abs();
    let ay = y.abs();
    let mut travel = ax;
    if ay > ax {
        travel = ay;
    }
    let inner = p[0u16].inner;
    if travel <= inner {
        return StickSample {
            cal_x: x,
            cal_y: y,
            x: zero,
            y: zero,
            dir_x: zero,
            dir_y: zero,
            magnitude: zero,
            valid: 1,
        };
    }
    // Scale before DOTSTORE to preserve short travel in Q16.16 (length2<=8192).
    let v = vec2::new(x, y);
    let scaled = v * fix32::from_int(64);
    let direction = scaled * frsqrt(fdot(scaled, scaled));
    let mut strength = (travel - inner) * p[0u16].inverse_range;
    if strength > one {
        strength = one;
    }
    if strength < zero {
        strength = zero;
    }
    let motion = direction * strength;
    return StickSample {
        cal_x: v.x(),
        cal_y: v.y(),
        x: motion.x(),
        y: motion.y(),
        dir_x: direction.x(),
        dir_y: direction.y(),
        magnitude: strength,
        valid: 1,
    };
}
// 0 neutral,1 right,2 down,3 left,4 up. Activation hysteresis, X wins ties.
// Below enter, a previous valid direction is held until magnitude<=release.
pub fn stick_cardinal(s: Array<StickSample>, previous: u16, enter: fix32, release: fix32) -> u16 {
    let zero = fix32::zero();
    if s[0u16].valid == 0
        || enter <= zero
        || enter > fix32::from_int(1)
        || release < zero
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
    if ax < zero {
        ax = -ax;
    }
    if ay < zero {
        ay = -ay;
    }
    if ax >= ay {
        if x >= zero {
            return 1;
        }
        return 3;
    }
    if y >= zero {
        return 2;
    }
    return 4;
}
