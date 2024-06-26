use crate::dsl::*;
use crate::{Compiler, DslFunction, Variable, VariableOperation1};
use once_cell::sync::Lazy;

static MEM_SET: Lazy<DslFunction<3, 0>> =
    Lazy::new(|| DslFunction::new("mem_clear", ["dst", "len", "value"], []));
static MEM_COPY: Lazy<DslFunction<3, 0>> =
    Lazy::new(|| DslFunction::new("mem_copy", ["dst", "src", "len"], []));

pub fn mem_copy(dst: DslPtr, src: DslPtr, len: Variable) {
    MEM_COPY.call([dst.ptr, src.ptr, len]);
}
pub fn mem_set(ptr: DslPtr, len: Variable, v: Variable) {
    MEM_SET.call([ptr.ptr, len, v]);
}

pub fn define_mem(compiler: &mut Compiler) {
    compiler.func_gen(&MEM_SET, box define_mem_set);
    compiler.func_gen(&MEM_COPY, box define_mem_copy);
}

fn define_mem_set() -> VariableOperation1 {
    MEM_SET.define(|[ptr, len, v], ret| {
        let end = ptr + len;
        for_loop_reg_up(ptr, end, |ptr| {
            DslPtr::new(ptr).write(v);
        });
        ret([]);
    })
}

fn define_mem_copy() -> VariableOperation1 {
    MEM_COPY.define(|[dst, mut src, len], ret| {
        let end = dst + len;
        for_loop_reg_up(dst, end, |ptr| {
            let value = DslPtr::new(src).read();
            DslPtr::new(ptr).write(value);
            src += 1;
        });
        ret([]);
    })
}
