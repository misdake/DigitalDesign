//! boundary-tag heap: block = header(1) + content(size) + footer(1);
//! header == footer, bit 15: free flag, bits 14-0: block size (incl. tags)

use crate::programmer::compiler::Compiler;
use crate::programmer::dsl::*;
use once_cell::sync::Lazy;

pub const HEAP_BEGIN: u16 = 0x1000;
pub const HEAP_SIZE: u16 = 20;
pub const HEAP_END: u16 = HEAP_BEGIN + HEAP_SIZE;
pub const FREE_BIT: u16 = 1 << 15;

pub static INIT_HEAP: Lazy<DslFunction<0, 0>> = Lazy::new(|| DslFunction::new("init_heap", [], []));
pub static MALLOC: Lazy<DslFunction<1, 1>> =
    Lazy::new(|| DslFunction::new("malloc", ["size"], ["ptr"]));
pub static FREE: Lazy<DslFunction<1, 0>> = Lazy::new(|| DslFunction::new("free", ["ptr"], []));

pub fn define_heap(compiler: &mut Compiler) {
    if compiler.has_func("init_heap") {
        return;
    }
    INIT_HEAP.compile(compiler, |b, [], ret| {
        let flag = b.v(HEAP_SIZE | FREE_BIT);
        b.v(HEAP_BEGIN).ptr().write(&flag);
        b.v(HEAP_END - 1).ptr().write(&flag);
        ret(b, []);
    });

    MALLOC.compile(compiler, |b, [size], ret| {
        let size = (&size + 2).clone_value(); // include header + footer
        let free_bit = b.v(FREE_BIT);
        let end = b.v(HEAP_END);
        let ptr_var = b.v(HEAP_BEGIN);
        let ptr = ptr_var.ptr();

        // while not end of heap
        b.while_loop(
            |_| ptr_var.lt(&end),
            |b| {
                let header = ptr.read();
                let is_free_bit = &header & &free_bit;
                let block_size = &header ^ &is_free_bit;
                let free_mask = is_free_bit.asr(15); // free ? 0xffff : 0

                // if is_free && block_size >= size
                let masked_size = &block_size & &free_mask;
                b.if_then(masked_size.ge(&size), |b| {
                    // found it; check whether a next block fits
                    let next_block_size = &block_size - &size;
                    b.if_else(
                        next_block_size.gt_imm(2),
                        |_b| {
                            // split: write the next (free) block's tags
                            let next_block_ptr = &ptr + &size;
                            let flag = &next_block_size | &free_bit;
                            next_block_ptr.write(&flag);
                            let footer_off = &next_block_size - 1;
                            (&next_block_ptr + &footer_off).write(&flag);
                        },
                        |_b| {
                            // no room to split: take the whole block
                            size.assign_from(&block_size);
                        },
                    );
                    // write this block's tags
                    ptr.write(&size);
                    let footer_off = &size - 1;
                    (&ptr + &footer_off).write(&size);

                    let r = &ptr_var + 1;
                    ret(b, [r]);
                });

                ptr.add_var(&block_size);
            },
        );

        // reached end of heap, halt
        b.halt(&ptr_var);
    });

    FREE.compile(compiler, |b, [ptr], ret| {
        let free_bit = b.v(FREE_BIT);
        let ptr = (&ptr - 1).clone_value(); // header
        let local_size = ptr.ptr().read();
        let flag = (&local_size | &free_bit).clone_value();
        let left_footer = (&ptr - 1).clone_value();
        let self_footer = (&left_footer + &local_size).clone_value();

        // merge with free blocks on the left
        let left_limit = b.v(HEAP_BEGIN - 1);
        let right_limit = b.v(HEAP_END - 1);
        b.while_loop(
            |_| left_footer.gt(&left_limit),
            |b| {
                let left_flag = left_footer.ptr().read();
                b.if_else(
                    left_flag.gt(&free_bit),
                    |_b| {
                        // left block is free
                        let left_size = &left_flag - &free_bit;
                        left_footer.assign_from(&(&left_footer - &left_size));
                        flag.assign_from(&(&flag + &left_size));
                    },
                    |_b| {
                        left_limit.assign_from(&right_limit); // break
                    },
                );
            },
        );

        // merge with free blocks on the right
        let left_limit = b.v(HEAP_BEGIN - 1);
        b.while_loop(
            |_| self_footer.lt(&right_limit),
            |b| {
                let right_flag = (&self_footer.ptr() + 1).read();
                b.if_else(
                    right_flag.gt(&free_bit),
                    |_b| {
                        // right block is free
                        let right_size = &right_flag - &free_bit;
                        self_footer.assign_from(&(&self_footer + &right_size));
                        flag.assign_from(&(&flag + &right_size));
                    },
                    |_b| {
                        right_limit.assign_from(&left_limit); // break
                    },
                );
            },
        );

        let self_header = &left_footer.ptr() + 1;
        self_header.write(&flag);
        self_footer.ptr().write(&flag);

        ret(b, []);
    });
}

pub fn heap_init(b: &B) {
    INIT_HEAP.call(b, []);
}
pub fn heap_malloc(b: &B, size: &Variable) -> Variable {
    let [r] = MALLOC.call(b, [size]);
    r
}
pub fn heap_free(b: &B, ptr: &Variable) {
    FREE.call(b, [ptr]);
}

// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::programmer::builtin::mem::*;
    use crate::simulate;

    pub(crate) struct HeapStat {
        pub alloc_count: usize,
        pub alloc_size: usize,
    }

    pub(crate) fn print_heap(mem: &[u16]) -> HeapStat {
        println!(
            "heap mem: {:?}",
            &mem[HEAP_BEGIN as usize..HEAP_END as usize]
        );

        let mut alloc_count = 0;
        let mut alloc_size = 0;
        let mut sum = 0;
        let mut ptr = HEAP_BEGIN as usize;
        while ptr < HEAP_END as usize {
            let flag = mem[ptr];
            if flag > FREE_BIT {
                let size = flag - FREE_BIT;
                sum += size;
                assert_eq!(flag, mem[ptr + size as usize - 1]);
                ptr += size as usize;
            } else {
                let size = flag;
                sum += size;
                assert_eq!(flag, mem[ptr + size as usize - 1]);
                ptr += size as usize;
                alloc_count += 1;
                alloc_size += (size - 2) as usize;
            }
        }
        assert_eq!(sum, HEAP_SIZE); // check corruption
        HeapStat {
            alloc_count,
            alloc_size,
        }
    }

    #[test]
    fn test_malloc() {
        let mut compiler = Compiler::new();
        define_heap(&mut compiler);
        define_mem(&mut compiler);

        let test_malloc = DslFunction::new("test_malloc", [], []);
        test_malloc.compile(&mut compiler, |b, [], _ret| {
            heap_init(b);
            let ptr1 = heap_malloc(b, &b.v(1));
            let ptr2 = heap_malloc(b, &b.v(2));
            let _ptr3 = heap_malloc(b, &b.v(3));
            heap_free(b, &ptr2);
            heap_free(b, &ptr1);
            let ptr4 = heap_malloc(b, &b.v(2));
            let ptr5 = heap_malloc(b, &b.v(5));
            mem_set(b, &ptr4.ptr(), &b.v(2), &b.v(44)); // 44 44
            mem_set(b, &ptr5.ptr(), &b.v(5), &b.v(55)); // 55 55 55 55 55
            mem_copy(b, &ptr5.ptr(), &ptr4.ptr(), &b.v(2)); // 44 44 55 55 55

            let z = b.v(0);
            b.halt(&z);
        });

        let instructions = compiler.finish("test_malloc");
        let (state, _signal) = simulate(&instructions, 2000);

        let heap_stat = print_heap(state.mem.as_slice());
        assert_eq!(heap_stat.alloc_count, 3);
        assert_eq!(heap_stat.alloc_size, 11);
        assert_eq!(
            &state.mem[HEAP_BEGIN as usize..HEAP_END as usize],
            [4, 44, 44, 4, 32771, 0, 32771, 5, 0, 0, 0, 5, 8, 44, 44, 55, 55, 55, 0, 8]
        );
    }
}
