//! mem_set / mem_copy / mem_copy_rev

use crate::Compiler;
use crate::compiler::dsl::*;
use once_cell::sync::Lazy;

pub static MEM_SET: Lazy<DslFunction<3, 0>> =
    Lazy::new(|| DslFunction::new("mem_set", ["dst", "len", "value"], []));
pub static MEM_COPY: Lazy<DslFunction<3, 0>> =
    Lazy::new(|| DslFunction::new("mem_copy", ["dst", "src", "len"], []));
pub static MEM_COPY_REV: Lazy<DslFunction<3, 0>> =
    Lazy::new(|| DslFunction::new("mem_copy_rev", ["dst", "src", "len"], []));

pub fn define_mem(compiler: &mut Compiler) {
    if compiler.has_func("mem_set") {
        return;
    }
    MEM_SET.compile(compiler, |b, [ptr, len, value], ret| {
        let end = &ptr + &len;
        b.for_loop(&ptr, &end, 1, |_b, p| {
            p.ptr().write(&value);
        });
        ret(b, []);
    });
    MEM_COPY.compile(compiler, |b, [dst, src, len], ret| {
        let end = &dst + &len;
        let src = src.clone_value();
        b.for_loop(&dst, &end, 1, |_b, p| {
            let v = src.ptr().read();
            p.ptr().write(&v);
            let next = &src + 1;
            src.assign_from(&next);
        });
        ret(b, []);
    });
    MEM_COPY_REV.compile(compiler, |b, [dst, src, len], ret| {
        let src = (&src + &len).clone_value();
        let end = &dst + &len;
        b.for_loop_rev(&dst, &end, 1, |_b, p| {
            let prev = &src - 1;
            src.assign_from(&prev);
            let v = src.ptr().read();
            p.ptr().write(&v);
        });
        ret(b, []);
    });
}

pub fn mem_set(b: &B, ptr: &DslPtr, len: &Variable, value: &Variable) {
    MEM_SET.call(b, [&ptr.ptr, len, value]);
}
pub fn mem_copy(b: &B, dst: &DslPtr, src: &DslPtr, len: &Variable) {
    MEM_COPY.call(b, [&dst.ptr, &src.ptr, len]);
}
pub fn mem_copy_rev(b: &B, dst: &DslPtr, src: &DslPtr, len: &Variable) {
    MEM_COPY_REV.call(b, [&dst.ptr, &src.ptr, len]);
}
