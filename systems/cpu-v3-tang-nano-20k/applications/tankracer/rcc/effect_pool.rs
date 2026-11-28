//! Fixed line pool with source exhaustion aliases supplied by the shared owner.
use crate::dsl_rt::*;
use crate::{attitude, integer};
pub struct Line {
    pub points: Buf<u16, 12>,
    pub colors: Buf<u16, 4>,
}
pub struct Pool {
    pub lines: Array<u16>,
    // 65535 retains the incoming mapping. Other values select DSEG2 only for
    // the line records; the descriptor and exhaustion aliases stay resident.
    pub page: u16,
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
pub fn select(pool: &Pool) -> u16 {
    let saved = read_dseg_page(2);
    if pool.page != 65535 && pool.page != saved {
        unsafe {
            write_dseg_page(2, pool.page);
        }
    }
    saved
}
pub fn restore(page: u16) {
    if read_dseg_page(2) != page {
        unsafe {
            write_dseg_page(2, page);
        }
    }
}
// Raw i32 access preserves quad 160's alias into the leading line records.
pub fn get(pool: &Pool, index: u16) -> fix32 {
    let saved = select(pool);
    let value = attitude::get(pool.lines, index);
    restore(saved);
    value
}
pub fn put(pool: &Pool, index: u16, value: fix32) {
    let saved = select(pool);
    attitude::put(pool.lines, index, value);
    restore(saved);
}
pub fn reset(pool: &mut Pool) {
    let saved = select(pool);
    let mut i = 0u16;
    while i < 100 {
        let out = record(pool, fix32::from_words(i, 0));
        attitude::put(out, 1, free());
        i += 1;
    }
    pool.count = n(0);
    restore(saved);
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
// Only inputs in the window being replaced need staging. CPU and DSEG3
// inputs keep their original aliases and are read directly by write().
fn retain_source(pool: &Pool, source: Array<u16>, count: u16, scratch: Array<u16>) -> Array<u16> {
    if pool.page != 65535 && pool.page != read_dseg_page(2) {
        let address = source.as_ptr().addr();
        if address < 0xc000 && address > 0x8000 - count {
            let mut destination = scratch;
            let mut i = 0u16;
            while i < count {
                destination[i] = source[i];
                i += 1;
            }
            return scratch;
        }
    }
    source
}
pub fn allocate(pool: &mut Pool, points: Array<u16>, colors: Array<u16>) -> fix32 {
    if pool.count == n(100) {
        return n(-1);
    }
    let coordinates: Buf<u16, 12> = Buf::new([0; 12]);
    let shades: Buf<u16, 4> = Buf::new([0; 4]);
    let points = retain_source(pool, points, 12, coordinates.as_array());
    let colors = retain_source(pool, colors, 4, shades.as_array());
    let saved = select(pool);
    let mut i = 0u16;
    while i < 100 {
        let out = record(pool, fix32::from_words(i, 0));
        if attitude::get(out, 1) == free() {
            pool.count = pool.count + n(1);
            write(out, points, colors);
            restore(saved);
            return fix32::from_words(i, 0);
        }
        i += 1;
    }
    restore(saved);
    n(100)
}
pub fn update(pool: &mut Pool, handle: fix32, points: Array<u16>, colors: Array<u16>) -> u16 {
    if !valid(handle) {
        return 1;
    }
    let coordinates: Buf<u16, 12> = Buf::new([0; 12]);
    let shades: Buf<u16, 4> = Buf::new([0; 4]);
    let points = retain_source(pool, points, 12, coordinates.as_array());
    let colors = retain_source(pool, colors, 4, shades.as_array());
    let saved = select(pool);
    let out = record(pool, handle);
    write(out, points, colors);
    restore(saved);
    0
}
pub fn release(pool: &mut Pool, handle: fix32) -> u16 {
    if !valid(handle) {
        return 1;
    }
    let saved = select(pool);
    let out = record(pool, handle);
    if attitude::get(out, 1) != free() {
        attitude::put(out, 1, free());
        pool.count = pool.count - n(1);
    }
    restore(saved);
    0
}
