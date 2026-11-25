//! Fixed line pool with source exhaustion aliases supplied by the shared owner.
use crate::dsl_rt::*;
use crate::{attitude, integer};
pub struct Line {
    pub points: Buf<u16, 12>,
    pub colors: Buf<u16, 4>,
}
pub struct Pool {
    pub lines: Array<u16>,
    pub count: fix32,
    pub overflow: Line,
    // Quad 159's final Y/Z, material/style and the two trailing packed colors.
    // This live view preserves the line -1 alias when the pool is exhausted.
    pub exhausted: Array<u16>,
}
fn n(v: i16) -> fix32 {
    integer::from_i16(v)
}
pub fn line_zero() -> Line {
    Line {
        points: Buf::new([0; 12]),
        colors: Buf::new([0; 4]),
    }
}
pub fn released() -> fix32 {
    fix32::from_words(48658, 65116)
}
pub fn free() -> fix32 {
    fix32::from_words(0, 57344)
}
pub fn reset(pool: &mut Pool) {
    let mut i = 0u16;
    while i < 100 {
        let out = record(pool, fix32::from_words(i, 0));
        attitude::put(out, 1, free());
        i += 1;
    }
    pool.count = n(0);
}
fn valid(handle: fix32) -> bool {
    (handle >= n(0) && handle <= n(100)) || handle == n(-1)
}
fn record(pool: &mut Pool, handle: fix32) -> Array<u16> {
    if handle == n(-1) {
        return pool.exhausted;
    }
    if handle == n(100) {
        return unsafe { view_of(&mut pool.overflow).as_ptr().as_u16_array() };
    }
    unsafe {
        pool.lines
            .as_ptr()
            .add((handle.lo_bits() * 16) as i16)
            .as_u16_array()
    }
}
fn write(mut out: Array<u16>, points: Array<u16>, colors: Array<u16>) {
    let mut i = 0u16;
    while i < 6 {
        attitude::put(out, i, integer::sar(attitude::get(points, i), 3));
        i += 1;
    }
    i = 0;
    while i < 4 {
        out[12 + i] = colors[i];
        i += 1;
    }
}
pub fn allocate(pool: &mut Pool, points: Array<u16>, colors: Array<u16>) -> fix32 {
    if pool.count == n(100) {
        return n(-1);
    }
    let mut i = 0u16;
    while i < 100 {
        let out = record(pool, fix32::from_words(i, 0));
        if attitude::get(out, 1) == free() {
            pool.count = pool.count + n(1);
            write(out, points, colors);
            return fix32::from_words(i, 0);
        }
        i += 1;
    }
    n(100)
}
pub fn update(pool: &mut Pool, handle: fix32, points: Array<u16>, colors: Array<u16>) -> u16 {
    if !valid(handle) {
        return 1;
    }
    let out = record(pool, handle);
    write(out, points, colors);
    0
}
pub fn release(pool: &mut Pool, handle: fix32) -> u16 {
    if !valid(handle) {
        return 1;
    }
    let out = record(pool, handle);
    if attitude::get(out, 1) != free() {
        attitude::put(out, 1, free());
        pool.count = pool.count - n(1);
    }
    0
}
