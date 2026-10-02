//! Growable u16 vector on top of the rcc heap.
//! A vector handle points to a stable 3-word header: `[buf, len, cap]`.

use crate::dsl_rt::*;
use crate::rcc_std::heap::*;

static VEC_INIT_CAP: u16 = 0;

const VEC_BOUNDS_ERROR: u16 = 0xffe3;
const VEC_CAPACITY_ERROR: u16 = 0xffe4;
const VEC_MAX_CAPACITY: u16 = 0x7ffd;

/// Called automatically by the compiler when the program reaches `vec_*`.
pub fn init_vec(cap: u16) {
    if cap > VEC_MAX_CAPACITY {
        halt(VEC_CAPACITY_ERROR);
    }
    unsafe { addr_of(&VEC_INIT_CAP).write(0, cap) };
}

pub fn vec_with_capacity(cap: u16) -> Ptr {
    if cap > VEC_MAX_CAPACITY {
        halt(VEC_CAPACITY_ERROR);
    }
    let header_ptr = malloc(3);
    let mut header = unsafe { header_ptr.as_u16_array() };
    header[0u16] = if cap == 0 { 0 } else { malloc(cap).addr() };
    header[1u16] = 0;
    header[2u16] = cap;
    header_ptr
}

pub fn vec_new() -> Ptr {
    vec_with_capacity(VEC_INIT_CAP)
}

pub fn vec_free(v: Ptr) {
    let header = unsafe { v.as_u16_array() };
    let buf = header[0u16];
    if buf != 0 {
        free(Ptr::from_addr(buf));
    }
    free(v);
}

pub fn vec_len(v: Ptr) -> u16 {
    (unsafe { v.as_u16_array() })[1u16]
}

pub fn vec_cap(v: Ptr) -> u16 {
    (unsafe { v.as_u16_array() })[2u16]
}

pub fn vec_is_empty(v: Ptr) -> bool {
    vec_len(v) == 0
}

pub fn vec_get(v: Ptr, i: u16) -> u16 {
    let header = unsafe { v.as_u16_array() };
    if i >= header[1u16] {
        halt(VEC_BOUNDS_ERROR);
    }
    unsafe { Ptr::from_addr(header[0u16]).read(i as i16) }
}

pub fn vec_set(v: Ptr, i: u16, x: u16) {
    let header = unsafe { v.as_u16_array() };
    if i >= header[1u16] {
        halt(VEC_BOUNDS_ERROR);
    }
    unsafe { Ptr::from_addr(header[0u16]).write(i as i16, x) };
}

fn vec_grow(v: Ptr, need: u16) {
    if need > VEC_MAX_CAPACITY {
        halt(VEC_CAPACITY_ERROR);
    }
    let mut header = unsafe { v.as_u16_array() };
    let cap = header[2u16];
    if need <= cap {
        return;
    }

    let mut new_cap = cap;
    if new_cap == 0 {
        new_cap = VEC_INIT_CAP;
        if new_cap == 0 {
            new_cap = 1;
        }
    }
    while new_cap < need {
        let mut increment = new_cap >> 1;
        if increment == 0 {
            increment = 1;
        }
        if increment > VEC_MAX_CAPACITY - new_cap {
            new_cap = need;
        } else {
            new_cap += increment;
        }
    }

    let old_buf = header[0u16];
    let new_buf = if old_buf == 0 {
        malloc(new_cap)
    } else {
        heap_realloc(Ptr::from_addr(old_buf), new_cap)
    };
    header[0u16] = new_buf.addr();
    header[2u16] = new_cap;
}

pub fn vec_reserve(v: Ptr, additional: u16) {
    let header = unsafe { v.as_u16_array() };
    let len = header[1u16];
    if additional > VEC_MAX_CAPACITY - len {
        halt(VEC_CAPACITY_ERROR);
    }
    let need = len + additional;
    vec_grow(v, need);
}

pub fn vec_clear(v: Ptr) {
    let mut header = unsafe { v.as_u16_array() };
    header[1u16] = 0;
}

pub fn vec_shrink_to_fit(v: Ptr) {
    let mut header = unsafe { v.as_u16_array() };
    let buf = header[0u16];
    let len = header[1u16];
    let cap = header[2u16];
    if len == cap {
        return;
    }
    if len == 0 {
        if buf != 0 {
            free(Ptr::from_addr(buf));
        }
        header[0u16] = 0;
    } else {
        header[0u16] = heap_realloc(Ptr::from_addr(buf), len).addr();
    }
    header[2u16] = len;
}

pub fn vec_push(v: Ptr, x: u16) {
    let mut header = unsafe { v.as_u16_array() };
    let len = header[1u16];
    if len == VEC_MAX_CAPACITY {
        halt(VEC_CAPACITY_ERROR);
    }
    if len >= header[2u16] {
        vec_grow(v, len + 1);
    }
    let buf = header[0u16];
    unsafe { Ptr::from_addr(buf).write(len as i16, x) };
    header[1u16] = len + 1;
}

pub fn vec_pop(v: Ptr) -> u16 {
    let mut header = unsafe { v.as_u16_array() };
    let len = header[1u16];
    if len == 0 {
        halt(VEC_BOUNDS_ERROR);
    }
    let new_len = len - 1;
    let value = unsafe { Ptr::from_addr(header[0u16]).read(new_len as i16) };
    header[1u16] = new_len;
    value
}
