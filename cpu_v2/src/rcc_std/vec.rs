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
    let mut h = malloc(3).as_u16_array();
    if cap > 0 {
        let buf = malloc(cap);
        h[0u16] = buf.addr();
    } else {
        h[0u16] = 0;
    }
    h[1u16] = 0;
    h[2u16] = cap;
    h.as_ptr()
}

pub fn vec_free(v: Ptr) {
    let header = v.as_u16_array();
    let buf = header[0u16];
    if buf > 0 {
        free(Ptr::from_addr(buf));
    }
    free(v);
}

pub fn vec_len(v: Ptr) -> u16 {
    v.as_u16_array()[1u16]
}
pub fn vec_cap(v: Ptr) -> u16 {
    v.as_u16_array()[2u16]
}

pub fn vec_get(v: Ptr, i: u16) -> u16 {
    let header = v.as_u16_array();
    Ptr::from_addr(header[0u16]).as_u16_array()[i]
}
pub fn vec_set(v: Ptr, i: u16, x: u16) {
    let header = v.as_u16_array();
    let mut data = Ptr::from_addr(header[0u16]).as_u16_array();
    data[i] = x;
}

fn vec_realloc(v: Ptr, new_cap: u16) {
    let mut header = v.as_u16_array();
    let prev_buf = header[0u16];
    let len = header[1u16];
    let new_buf = malloc(new_cap);
    header[0u16] = new_buf.addr();
    header[2u16] = new_cap;
    mem_copy(new_buf, Ptr::from_addr(prev_buf), len);
    if prev_buf > 0 {
        free(Ptr::from_addr(prev_buf));
    }
}

pub fn vec_push(v: Ptr, x: u16) {
    let mut header = v.as_u16_array();
    let len = header[1u16];
    let cap = header[2u16];
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
    header[1u16] = len + 1;
}

pub fn vec_pop(v: Ptr) -> u16 {
    let mut header = v.as_u16_array();
    let len = header[1u16];
    let x = vec_get(v, len - 1);
    header[1u16] = len - 1;
    x
}
