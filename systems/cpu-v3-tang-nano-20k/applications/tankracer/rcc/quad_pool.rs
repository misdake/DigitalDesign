//! Shared quad records, including the original line/quad exhaustion aliases.
use crate::dsl_rt::*;
use crate::{attitude, effect_pool, integer};

pub struct Pool {
    // Quads 0..158 and the first ten i32 values of quad 159: 5108 words.
    // Its remaining six values live in lines.exhausted, followed by two colors.
    pub records: Array<u16>,
    pub exhausted: Array<u16>,
    pub count: fix32,
}
fn n(v: i16) -> fix32 {
    integer::from_i16(v)
}
pub fn released() -> fix32 {
    fix32::from_words(589, 65204)
}
fn valid(handle: fix32) -> bool {
    (handle >= n(0) && handle <= n(160)) || handle == n(-1)
}
pub fn get(pool: &Pool, lines: &effect_pool::Pool, handle: fix32, index: u16) -> fix32 {
    if handle == n(-1) {
        return attitude::get(pool.exhausted, index);
    }
    if handle == n(160) {
        if index < 2 {
            return attitude::get(lines.exhausted, index + 6);
        }
        return effect_pool::get(lines, index - 2);
    }
    if handle == n(159) && index >= 10 {
        return attitude::get(lines.exhausted, index - 10);
    }
    attitude::get(pool.records, handle.lo_bits() * 16 + index)
}
pub fn put(pool: &Pool, lines: &effect_pool::Pool, handle: fix32, index: u16, value: fix32) {
    if handle == n(-1) {
        attitude::put(pool.exhausted, index, value);
        return;
    }
    if handle == n(160) {
        if index < 2 {
            attitude::put(lines.exhausted, index + 6, value);
            return;
        }
        effect_pool::put(lines, index - 2, value);
        return;
    }
    if handle == n(159) && index >= 10 {
        attitude::put(lines.exhausted, index - 10, value);
        return;
    }
    attitude::put(pool.records, handle.lo_bits() * 16 + index, value);
}
pub fn reset(pool: &mut Pool, lines: &effect_pool::Pool) {
    let mut i = 0u16;
    while i < 160 {
        put(pool, lines, fix32::from_words(i, 0), 1, effect_pool::free());
        i += 1;
    }
    pool.count = n(0);
}
pub fn replace(pool: &Pool, lines: &effect_pool::Pool, handle: fix32, quad: Array<u16>) -> u16 {
    if !valid(handle) {
        return 1;
    }
    let mut i = 0u16;
    while i < 16 {
        let mut value = attitude::get(quad, i);
        if i < 12 {
            value = integer::sar(value, 3);
        }
        put(pool, lines, handle, i, value);
        i += 1;
    }
    0
}
pub fn allocate(pool: &mut Pool, lines: &effect_pool::Pool, quad: Array<u16>) -> fix32 {
    if pool.count == n(160) {
        return n(-1);
    }
    let mut i = 0u16;
    while i < 160 {
        let handle = fix32::from_words(i, 0);
        if get(pool, lines, handle, 1) == effect_pool::free() {
            pool.count = pool.count + n(1);
            replace(pool, lines, handle, quad);
            return handle;
        }
        i += 1;
    }
    n(160)
}
pub fn release(pool: &mut Pool, lines: &effect_pool::Pool, handle: fix32) -> u16 {
    if !valid(handle) {
        return 1;
    }
    if get(pool, lines, handle, 1) != effect_pool::free() {
        put(pool, lines, handle, 1, effect_pool::free());
        pool.count = pool.count - n(1);
    }
    0
}
