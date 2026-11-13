//! Const-capable special functions, rounded once into signed Q16.16.
//!
//! This is the shared rustc/RCC constant model, deliberately more accurate than
//! the target LUT. Runtime lowering still emits the target's native operations.

const fn quantize(value: f64) -> i32 {
    let scaled = value * 65536.0;
    if scaled >= i32::MAX as f64 {
        i32::MAX
    } else if scaled <= i32::MIN as f64 {
        i32::MIN
    } else if scaled < 0.0 {
        (scaled - 0.5) as i32
    } else {
        (scaled + 0.5) as i32
    }
}

pub const fn reciprocal(raw: i32) -> i32 {
    if raw == 0 {
        i32::MAX
    } else {
        quantize(65536.0 / raw as f64)
    }
}

pub const fn reciprocal_sqrt(raw: i32) -> i32 {
    if raw <= 0 {
        return 0;
    }
    let value = raw as f64 / 65536.0;
    let mut root = if value > 1.0 { value } else { 1.0 };
    let mut i = 0;
    while i < 32 {
        root = (root + value / root) * 0.5;
        i += 1;
    }
    quantize(1.0 / root)
}

pub const fn sincos(raw: i32) -> (i32, i32) {
    let pi = std::f64::consts::PI;
    let tau = 2.0 * pi;
    let angle = raw as f64 / 65536.0;
    let mut x = angle - (angle / tau) as i32 as f64 * tau;
    if x > pi {
        x -= tau;
    }
    if x < -pi {
        x += tau;
    }
    let mut cosine_sign = 1.0;
    if x > pi * 0.5 {
        x = pi - x;
        cosine_sign = -1.0;
    } else if x < -pi * 0.5 {
        x = -pi - x;
        cosine_sign = -1.0;
    }
    let mut sine = x;
    let mut cosine = 1.0;
    let mut sine_term = x;
    let mut cosine_term = 1.0;
    let mut n = 1;
    // On [-pi/2, pi/2], these fixed twelve terms leave error far below
    // one Q16.16 step, including full-range argument reduction.
    while n < 12 {
        sine_term *= -x * x / ((2 * n) * (2 * n + 1)) as f64;
        cosine_term *= -x * x / ((2 * n - 1) * (2 * n)) as f64;
        sine += sine_term;
        cosine += cosine_term;
        n += 1;
    }
    (quantize(sine), quantize(cosine * cosine_sign))
}

pub const fn radians(raw_degrees: i32) -> i32 {
    quantize(raw_degrees as f64 / 65536.0 * (std::f64::consts::PI / 180.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    const QUARTER: (i32, i32) = sincos(102944);
    #[test]
    fn const_special_functions_track_math_across_the_representable_range() {
        assert_eq!(QUARTER, (65536, 0));
        assert_eq!(reciprocal(0), i32::MAX);
        assert_eq!(reciprocal_sqrt(-1), 0);
        assert_eq!(reciprocal_sqrt(0), 0);
        let mut state = 0x6d2b79f5u32;
        for raw in (0..20000).map(|_| {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            state as i32
        }) {
            let a = raw as f64 / 65536.0;
            let (s, c) = sincos(raw);
            assert!(
                (s as i64 - quantize(a.sin()) as i64).abs() <= 1,
                "sin {raw}"
            );
            assert!(
                (c as i64 - quantize(a.cos()) as i64).abs() <= 1,
                "cos {raw}"
            );
            if raw > 0 {
                assert_eq!(reciprocal(raw), quantize(1.0 / a));
                assert_eq!(reciprocal_sqrt(raw), quantize(1.0 / a.sqrt()));
            }
        }
        assert_eq!(radians(90 * 65536), 102944);
        assert_eq!(radians(225 * 65536 / 10), 25736);
    }
}
