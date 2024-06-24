use crate::dsl::*;
use crate::*;
use once_cell::sync::Lazy;

const HEAP_BEGIN: u16 = 0x1000;
const HEAP_SIZE: u16 = 0x1000;
const HEAP_END: u16 = HEAP_BEGIN + HEAP_SIZE;
const FREE_BIT: u16 = 1 << 15;

static INIT_HEAP: Lazy<DslFunction<0, 0>> = Lazy::new(|| DslFunction::new("init_heap", [], []));
static MALLOC: Lazy<DslFunction<1, 1>> =
    Lazy::new(|| DslFunction::new("malloc", ["size"], ["ptr"]));
static FREE: Lazy<DslFunction<1, 0>> = Lazy::new(|| DslFunction::new("free", ["ptr"], []));

pub fn define_memory(compiler: &mut Compiler) {
    compiler.func_gen(&INIT_HEAP, box define_init_heap);
    compiler.func_gen(&MALLOC, box define_malloc);
    compiler.func_gen(&FREE, box define_free);
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
        while_loop(CondOp::Cmp(ptr_var, end, Cond::Less), || {
            let header = ptr.read();
            let is_free_bit = header & free_bit;
            let block_size = header ^ is_free_bit;
            let free_mask = is_free_bit.asr(15);
            // free_mask == is_free ? 0xffff : 0x0000

            // if is_free && block.size >= size
            if_then(
                CondOp::Cmp(block_size & free_mask, size, Cond::GreaterEqual),
                || {
                    // found it
                    // check whether next block exists
                    let next_block_size = block_size - size;
                    if_then_else(
                        CondOp::CmpI(next_block_size, 0, Cond::Greater),
                        || {
                            // next block exists, write next block
                            let next_block_ptr = ptr + size;
                            next_block_ptr.write(next_block_size | free_bit);
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
                },
            );

            ptr.add_var(block_size);
        });

        // reached end of heap, halt
        halt_with_signal(ptr_var);
    })
}
fn define_free() -> VariableOperation1 {
    FREE.define(|[mut ptr], ret| {
        ptr -= 1;
        let ptr = DslPtr::new(ptr);
        let size = ptr.read();
        let flag = size | v(FREE_BIT);
        ptr.write(flag);
        (ptr + (size - 1)).write(flag);

        //TODO try merge left
        //TODO try merge right

        ret([]);
    })
}

pub fn init_heap() {
    INIT_HEAP.call([]);
}
pub fn malloc(size: Variable) -> Variable {
    MALLOC.call([size])[0]
}
pub fn free(ptr: Variable) {
    FREE.call([ptr]);
}

#[test]
fn test_malloc() {
    use crate::programmer::language::dsl::*;

    let mut compiler = Compiler::default();
    define_memory(&mut compiler);

    let test_malloc = DslFunction::new("test_malloc", [], []);
    test_malloc.compile(&mut compiler, |[], _ret| {
        init_heap();
        let ptr1 = malloc(v(1));
        let _ptr2 = malloc(v(2));
        let _ptr3 = malloc(v(3));
        free(ptr1);
        halt_with_signal(v(0));
    });

    let instructions = compiler.finish("test_malloc");
    let (state, _halt_signal) = simulate(&instructions, 1000);
    let heap_mem_slice = &state.mem[HEAP_BEGIN as usize..(HEAP_BEGIN as usize + 20)];
    println!("heap mem: {:?}", heap_mem_slice)
}
