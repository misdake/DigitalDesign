//! mem_set / mem_copy (rcc subset)

use crate::dsl_rt::*;

pub fn mem_set(dst: Ptr, len: u16, value: u16) {
    let end = dst.addr() + len;
    let mut p = dst.addr();
    while p < end {
        Ptr::from_addr(p).write(0, value);
        p += 1;
    }
}

pub fn mem_copy(dst: Ptr, src: Ptr, len: u16) {
    let end = dst.addr() + len;
    let mut d = dst.addr();
    let mut s = src.addr();
    while d < end {
        let v = Ptr::from_addr(s).read(0);
        Ptr::from_addr(d).write(0, v);
        d += 1;
        s += 1;
    }
}
