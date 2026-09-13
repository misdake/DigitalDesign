//! software division and remainder (rcc subset).
//!
//! Neither ISA has a divide, so `/` and `%` lower to these functions: the core
//! is a 16-step shift-subtract loop over `u16`, and the signed entries take
//! absolute values and restore the C sign rules (the quotient truncates toward
//! zero, the remainder follows the dividend).
//!
//! An rcc function has one return value, so the core publishes *both* results
//! through static cells and the four entry points read the one they need. Each
//! entry runs the core once and reads its cell immediately, so nesting
//! (`a % (b % c)`) is safe.
//!
//! A zero divisor is **defined** here so host and target agree: `x / 0` is 0
//! and `x % 0` is `x`. The raw `/` operator still panics on the host, because
//! rustc executes the real operator there; a program that must run both ways
//! with a possibly-zero divisor calls `div_u16`/`rem_u16`/`div_i16`/`rem_i16`
//! directly (they exist in `dsl_rt` with the same semantics).

use crate::dsl_rt::*;

static DIVMOD_QUO: u16 = 0;
static DIVMOD_REM: u16 = 0;

/// shift-subtract divide: `a / b` and `a % b` into the two static cells
fn divmod_core(a: u16, b: u16) {
    if b == 0 {
        addr_of(&DIVMOD_QUO).write(0, 0);
        addr_of(&DIVMOD_REM).write(0, a);
        return;
    }
    let mut x = a;
    let mut rem: u16 = 0;
    let mut quo: u16 = 0;
    let mut i: u16 = 0;
    while i < 16u16 {
        rem = (rem << 1) | (x >> 15);
        x <<= 1;
        quo <<= 1;
        if rem >= b {
            rem -= b;
            quo |= 1;
        }
        i += 1;
    }
    addr_of(&DIVMOD_QUO).write(0, quo);
    addr_of(&DIVMOD_REM).write(0, rem);
}

/// |x| as a u16 (so the core can work on magnitudes)
fn abs_u(x: i16) -> u16 {
    if x < 0i16 {
        0u16 - (x as u16)
    } else {
        x as u16
    }
}

/// 1 when x is negative
fn sign_bit(x: i16) -> u16 {
    ((x >> 15) as u16) & 1u16
}

pub fn div_u16(a: u16, b: u16) -> u16 {
    divmod_core(a, b);
    DIVMOD_QUO
}

pub fn rem_u16(a: u16, b: u16) -> u16 {
    divmod_core(a, b);
    DIVMOD_REM
}

pub fn div_i16(a: i16, b: i16) -> i16 {
    divmod_core(abs_u(a), abs_u(b));
    let quo = DIVMOD_QUO;
    if (sign_bit(a) ^ sign_bit(b)) != 0u16 {
        0i16 - (quo as i16)
    } else {
        quo as i16
    }
}

pub fn rem_i16(a: i16, b: i16) -> i16 {
    divmod_core(abs_u(a), abs_u(b));
    let rem = DIVMOD_REM;
    if sign_bit(a) != 0u16 {
        0i16 - (rem as i16)
    } else {
        rem as i16
    }
}
