use crate::dsl::*;
use crate::*;
use once_cell::sync::Lazy;

define_struct!(Block { size, next });

const HEAP_ROOT: u16 = 0x1000;
const HEAP_SIZE: u16 = 0x1000;

pub static INIT_HEAP: Lazy<DslFunction<0, 0>> = Lazy::new(|| DslFunction::new("init_heap", [], []));

pub fn define_memory(compiler: &mut Compiler) {
    compiler.func_gen(&INIT_HEAP, box || init_heap());
}

fn init_heap() -> VariableOperation1 {
    INIT_HEAP.define(|[], ret| {
        let root = v(HEAP_ROOT);
        let root_ptr = Block::new(DslPtr::new(root));
        let root_value = BlockValue {
            size: v(HEAP_SIZE) - Block::SIZE as u16,
            next: v(0),
        };
        root_ptr.write(root_value);

        ret([]);
    })
}
