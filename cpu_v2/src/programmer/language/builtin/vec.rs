use crate::dsl::*;
use crate::*;
use once_cell::sync::Lazy;

static VEC_NEW: Lazy<DslFunction<1, 0>> = Lazy::new(|| DslFunction::new("vec_new", ["ptr"], []));
static VEC_PUSH: Lazy<DslFunction<2, 0>> =
    Lazy::new(|| DslFunction::new("vec_push", ["ptr", "val"], []));
static VEC_GET: Lazy<DslFunction<2, 1>> =
    Lazy::new(|| DslFunction::new("vec_get", ["ptr", "index"], ["val"]));
static VEC_REMOVE: Lazy<DslFunction<2, 1>> =
    Lazy::new(|| DslFunction::new("vec_remove", ["ptr", "index"], ["val"]));
static VEC_POP: Lazy<DslFunction<1, 1>> =
    Lazy::new(|| DslFunction::new("vec_pop", ["ptr"], ["val"]));
static VEC_DROP: Lazy<DslFunction<1, 0>> = Lazy::new(|| DslFunction::new("vec_drop", ["ptr"], []));

define_struct!(Vec { buf, len, cap });

pub fn define_vec(_compiler: &mut Compiler) {
    //TODO
    // compiler.func_gen(&VEC_NEW, box define_vec_new);
    // compiler.func_gen(&VEC_PUSH, box define_vec_push);
    // compiler.func_gen(&VEC_GET, box define_vec_get);
    // compiler.func_gen(&VEC_REMOVE, box define_vec_remove);
    // compiler.func_gen(&VEC_POP, box define_vec_pop);
    // compiler.func_gen(&VEC_DROP, box define_vec_drop);
}

impl Vec {
    pub fn new_at_addr(addr: DslPtr) -> Self {
        let vec = Vec::new(addr);
        let zero = v(0);
        vec.buf.write(zero);
        vec.len.write(zero);
        vec.cap.write(zero);
        vec
    }
    pub fn len(&self) -> Variable {
        self.len.read()
    }
    pub fn cap(&self) -> Variable {
        self.len.read()
    }

    pub fn drop(&self) {
        VEC_DROP.call([self.buf.read()]);
    }
}
