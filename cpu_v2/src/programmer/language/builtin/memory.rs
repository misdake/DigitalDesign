use crate::dsl::*;
use crate::*;
use once_cell::sync::Lazy;

// size is outer size, including 2-word header
define_struct!(Block { size, is_free });

const HEAP_ROOT: u16 = 0x1000;
const HEAP_SIZE: u16 = 0x1000;

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
        let root = v(HEAP_ROOT);
        let root_ptr = Block::new(DslPtr::new(root));
        let root_value = BlockValue {
            size: v(HEAP_SIZE) - Block::SIZE as u16,
            is_free: v(1),
        };
        root_ptr.write(root_value);

        ret([]);
    })
}

fn define_malloc() -> VariableOperation1 {
    MALLOC.define(|[mut size], ret| {
        println!(".");
        size += 2;
        let end = v(HEAP_ROOT + HEAP_SIZE);
        let ptr_var = v(HEAP_ROOT);
        let block_ptr = DslPtr::new(ptr_var);
        let block = Block::new(block_ptr);

        // while not end of heap
        while_loop(CondOp::Cmp(ptr_var, end, Cond::Less), || {
            let block_value = block.read();
            // if block.size >= size
            if_then(
                CondOp::Cmp(block_value.size, size, Cond::GreaterEqual),
                || {
                    // found it
                    // check whether next block exists
                    let next_block_size = block_value.size - size;
                    if_then_else(
                        CondOp::CmpI(next_block_size, 0, Cond::Greater),
                        || {
                            // next block exists, write next block
                            let next_block_ptr = block_ptr + size;
                            let next_block = Block::new(next_block_ptr);
                            next_block.write(BlockValue {
                                size: next_block_size,
                                is_free: v(1),
                            });
                        },
                        || {
                            // next block does not exist, set size = block_value.size
                            size.assign_from(block_value.size);
                        },
                    );

                    // write this block
                    block.is_free.write(v(0));
                    block.size.write(size);

                    ret([ptr_var + 2]);
                },
            );

            block_ptr.add_var(block_value.size);
        });

        // reached end of heap, halt
        halt_with_signal(ptr_var);
    })
}
fn define_free() -> VariableOperation1 {
    todo!()
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
        let _ptr1 = malloc(v(1));
        let _ptr2 = malloc(v(2));
        let _ptr3 = malloc(v(3));
        halt_with_signal(v(0));
    });

    let instructions = compiler.finish("test_malloc");
    let (state, _halt_signal) = simulate(&instructions, 1000);
    let heap_mem_slice = &state.mem[HEAP_ROOT as usize..(HEAP_ROOT as usize + 20)];
    println!("heap mem: {:?}", heap_mem_slice)
}
