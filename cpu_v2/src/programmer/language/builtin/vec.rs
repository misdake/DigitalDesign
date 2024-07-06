use crate::dsl::*;
use crate::Cond::*;
use crate::*;
use once_cell::sync::Lazy;

static VEC_NEW: Lazy<DslFunction<1, 0>> = Lazy::new(|| DslFunction::new("vec_new", ["self"], []));
static VEC_REMOVE: Lazy<DslFunction<2, 1>> =
    Lazy::new(|| DslFunction::new("vec_remove", ["self", "index"], ["val"]));
static VEC_POP: Lazy<DslFunction<1, 1>> =
    Lazy::new(|| DslFunction::new("vec_pop", ["self"], ["val"]));
static VEC_DROP: Lazy<DslFunction<1, 0>> = Lazy::new(|| DslFunction::new("vec_drop", ["ptr"], []));

static VEC_SUBALLOC: Lazy<DslFunction<2, 1>> =
    Lazy::new(|| DslFunction::new("vec_realloc", ["self", "size"], ["ptr"]));
static VEC_REALLOC: Lazy<DslFunction<2, 0>> =
    Lazy::new(|| DslFunction::new("vec_realloc", ["self", "cap"], []));

define_struct!(Vec { buf, len, cap });

pub fn define_vec(compiler: &mut Compiler) {
    define_heap(compiler);
    define_mem(compiler);
    //TODO
    compiler.func_gen(&VEC_SUBALLOC, box define_vec_suballoc);
    // compiler.func_gen(&VEC_GET, box define_vec_get);
    // compiler.func_gen(&VEC_REMOVE, box define_vec_remove);
    // compiler.func_gen(&VEC_POP, box define_vec_pop);
    compiler.func_gen(&VEC_REALLOC, box define_vec_realloc);
    // compiler.func_gen(&VEC_DROP, box define_vec_drop);
}

fn define_vec_suballoc() -> VariableOperation1 {
    VEC_SUBALLOC.define(|[ptr, size], ret| {
        let vec = Vec::new(DslPtr::new(ptr));
        let prev_len = vec.len.read();
        let curr_len = prev_len + size;
        let cap = vec.cap.read();

        // if new_len > cap {
        //     set_cap(cap * 2); // if cap == 0 => it will set cap to 10
        // }
        if_then(CondOp::Cmp(curr_len, cap, Greater), || {
            VEC_REALLOC.call([ptr, cap.lsl(1)]);
        });

        // *len = new_len
        vec.len.write(curr_len);

        // return start of suballoc ptr
        let data_ptr = vec.buf.read();
        let val_ptr = data_ptr + prev_len;
        ret([val_ptr]);
    })
}

fn define_vec_realloc() -> VariableOperation1 {
    VEC_REALLOC.define(|[ptr, curr_cap], ret| {
        let vec = Vec::new(DslPtr::new(ptr));
        let prev_buf_ptr = vec.buf.read();
        let prev_cap = vec.cap.read();

        // if cap == 0 { cap = 10 }
        if_then(CondOp::CmpI(curr_cap, 0, Equal), || {
            curr_cap.set_imm(10);
        });

        // *buf = malloc(new_cap), *cap = new_cap
        let curr_buf = malloc(curr_cap);
        vec.buf.write(curr_buf);
        vec.cap.write(curr_cap);

        // copy buf from prev to curr
        mem_copy(DslPtr::new(curr_buf), DslPtr::new(prev_buf_ptr), prev_cap);

        // if buf != nullptr => free buf
        if_then(CondOp::CmpI(prev_buf_ptr, 0, Greater), || {
            free(prev_buf_ptr);
        });

        ret([]);
    })
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

    pub fn push_struct<T: DslStruct>(&self, t: T::ValueType) {
        let ptr = VEC_SUBALLOC.call([self.buf.ptr, v(T::SIZE as u16)])[0];
        let ptr = DslPtr::new(ptr);
        T::new(ptr).write(t);
    }
    pub fn push<const N: usize>(&self, value: [Variable; N]) {
        let ptr = VEC_SUBALLOC.call([self.buf.ptr, v(N as u16)])[0];
        let ptr = DslPtr::new(ptr);
        (0..N).for_each(|i| (ptr + i as u16).write(value[i]));
    }

    //TODO LEN check?

    pub fn get_struct<T: DslStruct>(&self, index: Variable) -> DslPtr {
        DslPtr::new(self.buf.read()) + index.mul_imm_simple(T::SIZE)
    }
    pub fn get1(&self, index: Variable) -> DslPtr {
        DslPtr::new(self.buf.read()) + index
    }
    pub fn get2(&self, index: Variable) -> DslPtr {
        DslPtr::new(self.buf.read()) + index.lsl(1)
    }
    pub fn get3(&self, index: Variable) -> DslPtr {
        DslPtr::new(self.buf.read()) + (index.lsl(1) + index)
    }
    pub fn get4(&self, index: Variable) -> DslPtr {
        DslPtr::new(self.buf.read()) + index.lsl(2)
    }

    //TODO LEN check? Call or inline?

    pub fn pop_struct<T: DslStruct>(&self) -> T::ValueType {
        let len = self.len.read();
        let start_offset = len - (1 + T::SIZE as u16);
        let ptr = self.buf.read() + start_offset;
        let r = T::new(DslPtr::new(ptr)).read();
        let new_len = len - T::SIZE as u16;
        self.len.write(new_len);
        r
    }
    pub fn pop1(&self) -> Variable {
        let len = self.len.read();
        let start_offset = len - 2;
        let start_ptr = self.get1(start_offset);
        let a = start_ptr.read();
        let new_len = len - 1;
        self.len.write(new_len);
        a
    }
    pub fn pop2(&self) -> [Variable; 2] {
        let len = self.len.read();
        let start_offset = len - 2;
        let start_ptr = self.get1(start_offset);
        let a = start_ptr.read();
        let b = (start_ptr + 1).read();
        let new_len = len - 2;
        self.len.write(new_len);
        [a, b]
    }
    pub fn pop3(&self) -> [Variable; 3] {
        let len = self.len.read();
        let start_offset = len - 3;
        let start_ptr = self.get1(start_offset);
        let a = start_ptr.read();
        let b = (start_ptr + 1).read();
        let c = (start_ptr + 2).read();
        let new_len = len - 3;
        self.len.write(new_len);
        [a, b, c]
    }
    pub fn pop4(&self) -> [Variable; 4] {
        let len = self.len.read();
        let start_offset = len - 4;
        let start_ptr = self.get1(start_offset);
        let a = start_ptr.read();
        let b = (start_ptr + 1).read();
        let c = (start_ptr + 2).read();
        let d = (start_ptr + 3).read();
        let new_len = len - 4;
        self.len.write(new_len);
        [a, b, c, d]
    }

    pub fn drop(&self) {
        VEC_DROP.call([self.buf.read()]);
    }
}
