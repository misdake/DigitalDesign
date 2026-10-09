//! Segregated free-list heap for the rcc subset.
//!
//! Blocks keep boundary tags so `free` and `heap_realloc` can coalesce adjacent
//! space. Free blocks also use their first two payload words as intrusive
//! prev/next links. Four coarse size classes avoid scanning allocated blocks
//! without turning this small runtime into a full TLSF implementation.

use crate::dsl_rt::*;
use crate::rcc_std::mem::*;

static HEAP_BEGIN: u16 = 0;
static HEAP_END: u16 = 0;
static FREE_HEADS: Buf<u16, 4> = Buf::new([0; 4]);

const FREE_BIT: u16 = 1 << 15;
const SIZE_MASK: u16 = FREE_BIT - 1;
const MIN_BLOCK_SIZE: u16 = 4;
const HEAP_CONFIG_ERROR: u16 = 0xffe0;
const HEAP_OUT_OF_MEMORY: u16 = 0xffe1;
const HEAP_INVALID_FREE: u16 = 0xffe2;

fn heap_size_of(flag: u16) -> u16 {
    flag & SIZE_MASK
}

fn heap_free_bit(flag: u16) -> u16 {
    flag & FREE_BIT
}

fn heap_bin(size: u16) -> u16 {
    if size <= 8 {
        return 0;
    }
    if size <= 32 {
        return 1;
    }
    if size <= 128 {
        return 2;
    }
    3
}

fn heap_head(bin: u16) -> u16 {
    FREE_HEADS[bin]
}

fn heap_set_head(bin: u16, node: u16) {
    let mut heads = FREE_HEADS.as_array();
    heads[bin] = node;
}

fn heap_write_tags(header: u16, size: u16, free_bit: u16) {
    let flag = if free_bit != 0 { size | FREE_BIT } else { size };
    let p = Ptr::from_addr(header);
    unsafe { p.write(0, flag) };
    unsafe { p.add((size - 1) as i16).write(0, flag) };
}

// Links store `header + 1`, keeping zero as the null value even for a heap at 0.
fn heap_insert_free(header: u16, size: u16) {
    let bin = heap_bin(size);
    let node = header + 1;
    let old_head = heap_head(bin);
    let links = Ptr::from_addr(node);
    unsafe { links.write(0, 0) };
    unsafe { links.write(1, old_head) };
    if old_head != 0 {
        unsafe { Ptr::from_addr(old_head).write(0, node) };
    }
    heap_set_head(bin, node);
}

fn heap_remove_free(header: u16, size: u16) {
    let bin = heap_bin(size);
    let node = header + 1;
    let links = Ptr::from_addr(node);
    let prev = unsafe { links.read(0) };
    let next = unsafe { links.read(1) };
    if prev != 0 {
        unsafe { Ptr::from_addr(prev).write(1, next) };
    } else {
        heap_set_head(bin, next);
    }
    if next != 0 {
        unsafe { Ptr::from_addr(next).write(0, prev) };
    }
}

fn heap_find_fit(need: u16) -> u16 {
    let mut bin = heap_bin(need);
    while bin < 4 {
        let mut node = heap_head(bin);
        let mut best: u16 = 0;
        let mut best_size: u16 = 0xffff;
        while node != 0 {
            let header = node - 1;
            let size = heap_size_of(unsafe { Ptr::from_addr(header).read(0) });
            if size >= need && size < best_size {
                best = node;
                best_size = size;
                if size == need {
                    return best;
                }
            }
            node = unsafe { Ptr::from_addr(node).read(1) };
        }
        if best != 0 {
            return best;
        }
        bin += 1;
    }
    0
}

fn heap_total_size(payload_size: u16) -> u16 {
    if payload_size > SIZE_MASK - 2 {
        halt(HEAP_OUT_OF_MEMORY);
    }
    let total = payload_size + 2;
    if total < MIN_BLOCK_SIZE {
        MIN_BLOCK_SIZE
    } else {
        total
    }
}

fn heap_allocated_size(payload: u16) -> u16 {
    let begin = HEAP_BEGIN;
    let end = HEAP_END;
    if payload <= begin || payload >= end {
        halt(HEAP_INVALID_FREE);
    }
    let header = payload - 1;
    let flag = unsafe { Ptr::from_addr(header).read(0) };
    let size = heap_size_of(flag);
    if heap_free_bit(flag) != 0
        || size < MIN_BLOCK_SIZE
        || size > end - header
        || unsafe { Ptr::from_addr(header + size - 1).read(0) } != flag
    {
        halt(HEAP_INVALID_FREE);
    }
    size
}

fn heap_release_block(block_header: u16, block_size: u16) {
    let mut header = block_header;
    let mut size = block_size;
    let begin = HEAP_BEGIN;
    let end = HEAP_END;

    if header > begin {
        let left_flag = unsafe { Ptr::from_addr(header - 1).read(0) };
        if heap_free_bit(left_flag) != 0 {
            let left_size = heap_size_of(left_flag);
            let left_header = header - left_size;
            heap_remove_free(left_header, left_size);
            header = left_header;
            size += left_size;
        }
    }

    let right_header = header + size;
    if right_header < end {
        let right_flag = unsafe { Ptr::from_addr(right_header).read(0) };
        if heap_free_bit(right_flag) != 0 {
            let right_size = heap_size_of(right_flag);
            heap_remove_free(right_header, right_size);
            size += right_size;
        }
    }

    heap_write_tags(header, size, FREE_BIT);
    heap_insert_free(header, size);
}

/// Called automatically by the compiler when the program uses the heap.
pub fn init_heap(begin: u16, size: u16) {
    if size < MIN_BLOCK_SIZE {
        halt(HEAP_CONFIG_ERROR);
    }
    if size > SIZE_MASK {
        halt(HEAP_CONFIG_ERROR);
    }
    if begin > 0xffff - size {
        halt(HEAP_CONFIG_ERROR);
    }
    let end = begin + size;
    unsafe { addr_of(&HEAP_BEGIN).write(0, begin) };
    unsafe { addr_of(&HEAP_END).write(0, end) };
    let mut heads = FREE_HEADS.as_array();
    let mut i: u16 = 0;
    while i < 4 {
        heads[i] = 0;
        i += 1;
    }
    heap_write_tags(begin, size, FREE_BIT);
    heap_insert_free(begin, size);
}

pub fn malloc(size: u16) -> Ptr {
    let need = heap_total_size(size);
    let node = heap_find_fit(need);
    if node == 0 {
        halt(HEAP_OUT_OF_MEMORY);
    }

    let header = node - 1;
    let block_size = heap_size_of(unsafe { Ptr::from_addr(header).read(0) });
    heap_remove_free(header, block_size);
    let remainder = block_size - need;
    let mut used_size = block_size;
    if remainder >= MIN_BLOCK_SIZE {
        let free_header = header + need;
        heap_write_tags(free_header, remainder, FREE_BIT);
        heap_insert_free(free_header, remainder);
        used_size = need;
    }
    heap_write_tags(header, used_size, 0);
    Ptr::from_addr(header + 1)
}

pub fn free(p: Ptr) {
    let payload = p.addr();
    let header = payload - 1;
    let size = heap_allocated_size(payload);
    heap_release_block(header, size);
}

/// Resize an allocation, preserving `min(old_size, new_size)` payload words.
/// A zero size frees the block and returns the null `Ptr` value.
pub fn heap_realloc(p: Ptr, new_size: u16) -> Ptr {
    if p.addr() == 0 {
        return malloc(new_size);
    }
    if new_size == 0 {
        free(p);
        return Ptr::from_addr(0);
    }

    let header = p.addr() - 1;
    let old_size = heap_allocated_size(p.addr());
    let need = heap_total_size(new_size);

    if need <= old_size {
        let remainder = old_size - need;
        if remainder >= MIN_BLOCK_SIZE {
            heap_write_tags(header, need, 0);
            heap_release_block(header + need, remainder);
        }
        return p;
    }

    let end = HEAP_END;
    let right_header = header + old_size;
    if right_header < end {
        let right_flag = unsafe { Ptr::from_addr(right_header).read(0) };
        if heap_free_bit(right_flag) != 0 {
            let right_size = heap_size_of(right_flag);
            let combined = old_size + right_size;
            if combined >= need {
                heap_remove_free(right_header, right_size);
                let remainder = combined - need;
                if remainder >= MIN_BLOCK_SIZE {
                    heap_write_tags(header, need, 0);
                    let free_header = header + need;
                    heap_write_tags(free_header, remainder, FREE_BIT);
                    heap_insert_free(free_header, remainder);
                } else {
                    heap_write_tags(header, combined, 0);
                }
                return p;
            }
        }
    }

    let old_payload = old_size - 2;
    let copy_len = if old_payload < new_size {
        old_payload
    } else {
        new_size
    };
    let new_ptr = malloc(new_size);
    mem_copy(new_ptr, p, copy_len);
    free(p);
    new_ptr
}
