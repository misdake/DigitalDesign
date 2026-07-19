//! boundary-tag heap (rcc subset). the heap bounds live in the data section
//! and are set by the compiler-driven `init_heap` call (CompilerOptions).
//! block = header(1) + content(size) + footer(1); header == footer,
//! bit 15: free flag, bits 14-0: block size (including tags)

use crate::dsl_rt::*;

static HEAP_BEGIN: u16 = 0;
static HEAP_END: u16 = 0;

const FREE_BIT: u16 = 1 << 15;

/// called automatically by the compiler when the program uses malloc/free
pub fn init_heap(begin: u16, size: u16) {
    let end = begin + size;
    addr_of(&HEAP_BEGIN).write(0, begin);
    addr_of(&HEAP_END).write(0, end);
    let flag = size | FREE_BIT;
    let h = Ptr::from_addr(begin);
    h.write(0, flag);
    let t = Ptr::from_addr(end - 1);
    t.write(0, flag);
}

pub fn malloc(size: u16) -> Ptr {
    let mut sz = size + 2; // include header + footer
    let end = HEAP_END;
    let mut ptr = HEAP_BEGIN;
    // while not end of heap
    while ptr < end {
        let p = Ptr::from_addr(ptr);
        let header = p.read(0);
        let is_free_bit = header & FREE_BIT;
        let block_size = header ^ is_free_bit;
        let free_mask = ((is_free_bit as i16) >> 15) as u16; // free ? 0xffff : 0

        // if is_free && block_size >= sz
        let masked_size = block_size & free_mask;
        if masked_size >= sz {
            // found it; check whether a next block fits
            let next_block_size = block_size - sz;
            if next_block_size > 2 {
                // split: write the next (free) block's tags
                let next_block_ptr = p.add(sz as i16);
                let flag = next_block_size | FREE_BIT;
                next_block_ptr.write(0, flag);
                next_block_ptr.add((next_block_size - 1) as i16).write(0, flag);
            } else {
                // no room to split: take the whole block
                sz = block_size;
            }
            // write this block's tags
            p.write(0, sz);
            p.add((sz - 1) as i16).write(0, sz);

            return Ptr::from_addr(ptr + 1);
        }
        ptr += block_size;
    }
    // reached end of heap, halt
    halt(ptr)
}

pub fn free(p: Ptr) {
    let begin = HEAP_BEGIN;
    let end = HEAP_END;
    let ptr = p.addr() - 1; // header
    let header_ptr = Ptr::from_addr(ptr);
    let local_size = header_ptr.read(0);
    let mut flag = local_size | FREE_BIT;
    let mut left_footer = ptr - 1;
    let mut self_footer = left_footer + local_size;

    // merge with free blocks on the left
    let mut left_limit = begin - 1;
    let right_limit = end - 1;
    while left_footer > left_limit {
        let left_flag = Ptr::from_addr(left_footer).read(0);
        if left_flag > FREE_BIT {
            // left block is free
            let left_size = left_flag - FREE_BIT;
            left_footer -= left_size; // goes to left'left footer
            flag += left_size;
        } else {
            left_limit = right_limit; // break
        }
    }

    // merge with free blocks on the right
    let left_limit = begin - 1;
    while self_footer < right_limit {
        let right_flag = Ptr::from_addr(self_footer + 1).read(0);
        if right_flag > FREE_BIT {
            // right block is free
            let right_size = right_flag - FREE_BIT;
            self_footer += right_size; // goes to right footer
            flag += right_size;
        } else {
            break;
        }
    }

    let self_header = Ptr::from_addr(left_footer + 1);
    self_header.write(0, flag);
    let self_footer_ptr = Ptr::from_addr(self_footer);
    self_footer_ptr.write(0, flag);
}
