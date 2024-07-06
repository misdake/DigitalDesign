use crate::dsl::*;
use crate::Cond::*;
use crate::*;
use once_cell::sync::Lazy;

static VEC_INSERT: Lazy<DslFunction<3, 1>> =
    Lazy::new(|| DslFunction::new("vec_remove", ["self", "index", "len"], ["ptr"]));
static VEC_REMOVE: Lazy<DslFunction<3, 0>> =
    Lazy::new(|| DslFunction::new("vec_remove", ["self", "index", "len"], []));

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
    // compiler.func_gen(&VEC_INSERT, box define_vec_insert);
    // compiler.func_gen(&VEC_REMOVE, box define_vec_remove);
    compiler.func_gen(&VEC_SUBALLOC, box define_vec_suballoc);
    compiler.func_gen(&VEC_REALLOC, box define_vec_realloc);
    compiler.func_gen(&VEC_DROP, box define_vec_drop);
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
        let curr_buf = heap_malloc(curr_cap);
        vec.buf.write(curr_buf);
        vec.cap.write(curr_cap);

        // copy buf from prev to curr
        mem_copy(DslPtr::new(curr_buf), DslPtr::new(prev_buf_ptr), prev_cap);

        // if buf != nullptr => free buf
        if_then(CondOp::CmpI(prev_buf_ptr, 0, Greater), || {
            heap_free(prev_buf_ptr);
        });

        ret([]);
    })
}

fn define_vec_drop() -> VariableOperation1 {
    VEC_DROP.define(|[ptr], ret| {
        let vec = Vec::new(DslPtr::new(ptr));
        heap_free(vec.buf.read());
        let z = v(0);
        vec.buf.write(z);
        vec.len.write(z);
        vec.cap.write(z);
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
    pub fn push1(&self, value: Variable) {
        self.push([value]);
    }
    pub fn push2(&self, a: Variable, b: Variable) {
        self.push([a, b]);
    }
    pub fn push3(&self, a: Variable, b: Variable, c: Variable) {
        self.push([a, b, c]);
    }
    pub fn push4(&self, a: Variable, b: Variable, c: Variable, d: Variable) {
        self.push([a, b, c, d]);
    }

    //TODO LEN check?

    pub fn get_struct<T: DslStruct>(&self, index: Variable) -> T {
        let ptr = DslPtr::new(self.buf.read()) + index.mul_imm_simple(T::SIZE as u8);
        T::new(ptr)
    }
    pub fn get(&self, index: Variable, stride: u8) -> DslPtr {
        if stride == 1 {
            DslPtr::new(self.buf.read()) + index
        } else {
            DslPtr::new(self.buf.read()) + index.mul_imm_simple(stride)
        }
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
    pub fn pop<const N: usize>(&self) -> [Variable; N] {
        let len = self.len.read();
        let start_offset = len - (N + 1) as u16;
        let start_ptr = self.get(start_offset, 1);
        let results = core::array::from_fn(|i| (start_ptr + i as u16).read());
        let new_len = len - N as u16;
        self.len.write(new_len);
        results
    }
    pub fn pop1(&self) -> Variable {
        self.pop::<1>()[0]
    }
    pub fn pop2(&self) -> [Variable; 2] {
        self.pop::<2>()
    }
    pub fn pop3(&self) -> [Variable; 3] {
        self.pop::<3>()
    }
    pub fn pop4(&self) -> [Variable; 4] {
        self.pop::<4>()
    }

    //TODO insert (suballoc, mem_copy, return hole ptr)

    //TODO remove (mem_copy, set len)

    //TODO for each (with index and without)

    pub fn drop(&self) {
        VEC_DROP.call([self.base.ptr]);
    }
}
