//! growable vector on top of the heap (rcc subset).
//! a vec is a 3-word header: [buf, len, cap]. default initial capacity comes
//! from the compiler-driven `init_vec` call (CompilerOptions::vec_init_cap).

use crate::dsl_rt::*;
use crate::rcc_std::heap::*;
use crate::rcc_std::mem::*;

static VEC_INIT_CAP: u16 = 0;

/// called automatically by the compiler when the program uses vec_*
pub fn init_vec(cap: u16) {
    addr_of(&VEC_INIT_CAP).write(0, cap);
}

pub fn vec_new() -> Ptr {
    let cap = VEC_INIT_CAP;
    let h = malloc(3);
    if cap > 0 {
        let buf = malloc(cap);
        h.write(0, buf.addr());
    } else {
        h.write(0, 0);
    }
    h.write(1, 0);
    h.write(2, cap);
    h
}

pub fn vec_free(v: Ptr) {
    let buf = v.read(0);
    if buf > 0 {
        free(Ptr::from_addr(buf));
    }
    free(v);
}

pub fn vec_len(v: Ptr) -> u16 {
    v.read(1)
}
pub fn vec_cap(v: Ptr) -> u16 {
    v.read(2)
}

pub fn vec_get(v: Ptr, i: u16) -> u16 {
    let buf = Ptr::from_addr(v.read(0));
    buf.add(i as i16).read(0)
}
pub fn vec_set(v: Ptr, i: u16, x: u16) {
    let buf = Ptr::from_addr(v.read(0));
    buf.add(i as i16).write(0, x);
}

fn vec_realloc(v: Ptr, new_cap: u16) {
    let prev_buf = v.read(0);
    let len = v.read(1);
    let new_buf = malloc(new_cap);
    v.write(0, new_buf.addr());
    v.write(2, new_cap);
    mem_copy(new_buf, Ptr::from_addr(prev_buf), len);
    if prev_buf > 0 {
        free(Ptr::from_addr(prev_buf));
    }
}

pub fn vec_push(v: Ptr, x: u16) {
    let len = v.read(1);
    let cap = v.read(2);
    if len >= cap {
        let mut new_cap = cap << 1;
        if new_cap == 0 {
            new_cap = VEC_INIT_CAP;
        }
        let need = len + 1;
        while new_cap < need {
            new_cap <<= 1;
        }
        vec_realloc(v, new_cap);
    }
    vec_set(v, len, x);
    v.write(1, len + 1);
}

pub fn vec_pop(v: Ptr) -> u16 {
    let len = v.read(1);
    let x = vec_get(v, len - 1);
    v.write(1, len - 1);
    x
}
