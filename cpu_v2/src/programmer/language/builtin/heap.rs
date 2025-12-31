use crate::dsl::*;
use crate::*;
use once_cell::sync::Lazy;

const HEAP_BEGIN: u16 = 0x1000;
const HEAP_SIZE: u16 = 20;
const HEAP_END: u16 = HEAP_BEGIN + HEAP_SIZE;
const FREE_BIT: u16 = 1 << 15;

static INIT_HEAP: Lazy<DslFunction<0, 0>> = Lazy::new(|| DslFunction::new("init_heap", [], []));
static MALLOC: Lazy<DslFunction<1, 1>> =
    Lazy::new(|| DslFunction::new("malloc", ["size"], ["ptr"]));
static FREE: Lazy<DslFunction<1, 0>> = Lazy::new(|| DslFunction::new("free", ["ptr"], []));

pub fn define_heap(compiler: &mut Compiler) {
    compiler.func_gen(&INIT_HEAP, Box::new(define_init_heap));
    compiler.func_gen(&MALLOC, Box::new(define_malloc));
    compiler.func_gen(&FREE, Box::new(define_free));
}

fn define_init_heap() -> VariableOperation1 {
    INIT_HEAP.define(|[], ret| {
        let flag = v(HEAP_SIZE | FREE_BIT);
        DslPtr::new(v(HEAP_BEGIN)).write(flag);
        DslPtr::new(v(HEAP_END - 1)).write(flag);

        ret([]);
    })
}

/// each block is header(1 word), content("size" words), footer(1 word)
/// header = footer, bit 15: in-use flag, bit 14-0: size
fn define_malloc() -> VariableOperation1 {
    MALLOC.define(|[mut size], ret| {
        size += 2; // include header footer
        let free_bit = v(FREE_BIT);
        let end = v(HEAP_END);
        let ptr_var = v(HEAP_BEGIN);
        let ptr = DslPtr::new(ptr_var);

        // while not end of heap
        while_loop(cmp!(ptr_var < end), || {
            let header = ptr.read();
            let is_free_bit = header & free_bit;
            let block_size = header ^ is_free_bit;
            let free_mask = is_free_bit.asr(15);
            // free_mask == is_free ? 0xffff : 0x0000

            // if is_free && block.size >= size
            let masked_size = block_size & free_mask;
            if_then(cmp!(masked_size >= size), || {
                // found it
                // check whether next block exists
                let next_block_size = block_size - size;
                if_then_else(
                    cmp!(next_block_size > 2),
                    || {
                        // next block exists, write next block
                        let next_block_ptr = ptr + size;
                        let flag = next_block_size | free_bit;
                        next_block_ptr.write(flag);
                        (next_block_ptr + (next_block_size - 1)).write(flag);
                    },
                    || {
                        // next block does not exist, set size = block_size
                        size.assign_from(block_size);
                    },
                );
                // write this block
                ptr.write(size);
                (ptr + (size - 1)).write(size);

                ret([ptr_var + 1]);
            });

            ptr.add_var(block_size);
        });

        // reached end of heap, halt
        halt_with_signal(ptr_var);
    })
}
fn define_free() -> VariableOperation1 {
    FREE.define(|[mut ptr], ret| {
        let free_bit = v(FREE_BIT);
        ptr -= 1; // header
        let self_header = DslPtr::new(ptr);
        let local_size = self_header.read();
        let mut flag = local_size | free_bit;
        let mut left_footer = self_header.ptr - 1;
        let mut self_footer = left_footer + local_size;

        // merge left block
        let left_limit = v(HEAP_BEGIN - 1);
        let right_limit = v(HEAP_END - 1);
        while_loop(cmp!(left_footer > left_limit), || {
            let left_flag = DslPtr::new(left_footer).read();
            if_then_else(
                cmp!(left_flag > free_bit),
                || {
                    //left block is free
                    let left_size = left_flag - free_bit;
                    left_footer -= left_size; // goes to left'left footer
                    flag += left_size;
                },
                || {
                    left_limit.assign_from(right_limit); // break
                },
            );
        });

        // merge right block
        let left_limit = v(HEAP_BEGIN - 1);
        while_loop(cmp!(self_footer < right_limit), || {
            let right_flag = (DslPtr::new(self_footer) + 1).read();
            if_then_else(
                cmp!(right_flag > free_bit),
                || {
                    //riht block is free
                    let right_size = right_flag - free_bit;
                    self_footer += right_size; // goes to right footer
                    flag += right_size;
                },
                || {
                    right_limit.assign_from(left_limit); // break
                },
            );
        });

        let self_header = DslPtr::new(left_footer) + 1;
        self_header.write(flag);
        let self_footer = DslPtr::new(self_footer);
        self_footer.write(flag);

        ret([]);
    })
}

pub fn heap_init() {
    INIT_HEAP.call([]);
}
pub fn heap_malloc(size: Variable) -> Variable {
    MALLOC.call([size])[0]
}
pub fn heap_free(ptr: Variable) {
    FREE.call([ptr]);
}

#[test]
fn test_malloc() {
    use crate::programmer::language::dsl::*;

    let mut compiler = Compiler::default();
    define_heap(&mut compiler);
    define_mem(&mut compiler);

    let test_malloc = DslFunction::new("test_malloc", [], []);
    test_malloc.compile(&mut compiler, |[], _ret| {
        heap_init();
        let ptr1 = heap_malloc(v(1));
        let ptr2 = heap_malloc(v(2));
        let _ptr3 = heap_malloc(v(3));
        heap_free(ptr2);
        heap_free(ptr1);
        let ptr4 = heap_malloc(v(2));
        let ptr5 = heap_malloc(v(5));
        mem_set(DslPtr::new(ptr4), v(2), v(44)); // 44 44
        mem_set(DslPtr::new(ptr5), v(5), v(55)); // 55 55 55 55 55
        mem_copy(DslPtr::new(ptr5), DslPtr::new(ptr4), v(2)); // 44 44 55 55 55

        halt_with_signal(v(0));
    });

    let instructions = compiler.finish("test_malloc");
    let (state, _halt_signal) = simulate(&instructions, 1000);

    let heap_stat = print_heap(state.mem.as_slice());
    assert_eq!(heap_stat.alloc_count, 3);
    assert_eq!(heap_stat.alloc_size, 10);
    assert_eq!(
        &state.mem[HEAP_BEGIN as usize..HEAP_END as usize],
        [4, 44, 44, 4, 32771, 0, 32771, 5, 0, 0, 0, 5, 8, 44, 44, 55, 55, 55, 0, 8]
    );
}

pub(crate) fn print_heap(mem: &[u16]) -> HeapStat {
    println!(
        "heap mem: {:?}",
        &mem[HEAP_BEGIN as usize..HEAP_END as usize]
    );

    let mut sum_alloc_count: usize = 0;
    let mut sum_alloc_size: usize = 0;

    let mut sum = 0;
    println!("heap dump:");
    let mut ptr = HEAP_BEGIN as usize;
    while ptr < HEAP_END as usize {
        let flag = mem[ptr];
        if flag > FREE_BIT {
            let size = flag - FREE_BIT;
            sum += size;
            println!(
                "  free {} at 0x{:x}({})",
                size - 2,
                ptr,
                ptr - HEAP_BEGIN as usize
            );
            assert_eq!(flag, mem[ptr + size as usize - 1]);
            ptr += size as usize;
        } else {
            let size = flag;
            sum += size;
            let alloc_size = size - 2;
            println!(
                "  used {} at 0x{:x}({})",
                alloc_size,
                ptr,
                ptr - HEAP_BEGIN as usize
            );
            assert_eq!(flag, mem[ptr + size as usize - 1]);
            ptr += size as usize;
            sum_alloc_count += 1;
            sum_alloc_size += alloc_size as usize;
        }
    }
    assert_eq!(sum, HEAP_SIZE); // check corruption

    HeapStat {
        alloc_count: sum_alloc_count,
        alloc_size: sum_alloc_size,
    }
}

#[derive(Copy, Clone, Eq, PartialEq)]
pub(crate) struct HeapStat {
    pub alloc_count: usize,
    pub alloc_size: usize,
}
