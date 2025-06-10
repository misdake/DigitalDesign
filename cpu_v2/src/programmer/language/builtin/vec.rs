use crate::dsl::*;
use crate::Cond::*;
use crate::*;
use once_cell::sync::Lazy;

const VEC_LEN_INIT: usize = 4;

/// remove all items, free buffer
static VEC_CLEAR: Lazy<DslFunction<1, 0>> =
    Lazy::new(|| DslFunction::new("vec_drop", ["self"], []));
/// alloc block of size "size" at tail, return pointer of block start addr
static VEC_SUBALLOC: Lazy<DslFunction<2, 1>> =
    Lazy::new(|| DslFunction::new("vec_suballoc", ["self", "size"], ["ptr"]));
/// alloc new buffer, copy buffer and free previous buffer. cap must be > 0.
static VEC_REALLOC: Lazy<DslFunction<2, 1>> =
    Lazy::new(|| DslFunction::new("vec_realloc", ["self", "cap"], ["new_buf"]));

define_struct!(Vec { buf, len, cap });

pub fn define_vec(compiler: &mut Compiler) {
    define_heap(compiler);
    define_mem(compiler);

    compiler.func_gen(&VEC_SUBALLOC, Box::new(define_vec_suballoc));
    compiler.func_gen(&VEC_REALLOC, Box::new(define_vec_realloc));
    compiler.func_gen(&VEC_CLEAR, Box::new(define_vec_clear));
}

fn define_vec_suballoc() -> VariableOperation1 {
    VEC_SUBALLOC.define(|[ptr, size], ret| {
        let vec = Vec::new(DslPtr::new(ptr));
        let prev_len = vec.len.read();
        let curr_len = prev_len + size;
        let prev_cap = vec.cap.read();
        let curr_buf = vec.buf.read();

        if_then(CondOp::Cmp(curr_len, prev_cap, Greater), || {
            // new_cap = prev_cap * 2;
            // if new_cap == 0 { cap = VEC_LEN_INIT }
            // while new_cap < curr_len { curr_cap *= 2 }
            let new_cap = prev_cap.lsl(1);
            if_then(CondOp::CmpI(new_cap, 0, Equal), || {
                new_cap.set_imm(VEC_LEN_INIT as u16);
            });
            while_loop(CondOp::Cmp(new_cap, curr_len, Less), || {
                new_cap.lsl_assign(1);
            });

            let [new_buf] = VEC_REALLOC.call([ptr, new_cap]); // writes vec.buf/cap memory
            curr_buf.assign_from(new_buf);
        });

        // *len = curr_len
        vec.len.write(curr_len);

        // return start of suballoc ptr
        let val_ptr = curr_buf + prev_len;
        ret([val_ptr]);
    })
}

fn define_vec_realloc() -> VariableOperation1 {
    VEC_REALLOC.define(|[ptr, curr_cap], ret| {
        let vec = Vec::new(DslPtr::new(ptr));
        let prev_buf_ptr = vec.buf.read();
        let prev_len = vec.len.read();

        // *buf = malloc(curr_cap), *cap = curr_cap
        let curr_buf = heap_malloc(curr_cap);
        vec.buf.write(curr_buf);
        vec.cap.write(curr_cap);

        // copy buf from prev to curr
        mem_copy(DslPtr::new(curr_buf), DslPtr::new(prev_buf_ptr), prev_len);

        // if buf != nullptr => free buf
        if_then(CondOp::CmpI(prev_buf_ptr, 0, Greater), || {
            heap_free(prev_buf_ptr);
        });

        ret([curr_buf]);
    })
}

fn define_vec_clear() -> VariableOperation1 {
    VEC_CLEAR.define(|[ptr], ret| {
        let vec = Vec::new(DslPtr::new(ptr));
        let buf_ptr = vec.buf.read();
        if_then(CondOp::CmpI(buf_ptr, 0, Greater), || {
            heap_free(buf_ptr);
            let z = v(0);
            vec.buf.write(z);
            vec.len.write(z);
            vec.cap.write(z);
        });
        ret([]);
    })
}

impl Vec {
    pub fn alloc(init_size: u16) -> Self {
        let addr = heap_malloc(v(3));
        Self::new_at_addr(DslPtr::new(addr), init_size)
    }
    pub fn free(self) {
        self.clear();
        heap_free(self.base.ptr);
    }

    pub fn new_at_addr(addr: DslPtr, init_size: u16) -> Self {
        let vec = Vec::new(addr);
        let zero = v(0);

        if init_size > 0 {
            let init_size = v(init_size);
            vec.buf.write(heap_malloc(init_size));
            vec.len.write(zero);
            vec.cap.write(init_size);
        } else {
            vec.buf.write(zero);
            vec.len.write(zero);
            vec.cap.write(zero);
        }

        vec
    }

    pub fn len(&self) -> Variable {
        self.len.read()
    }
    pub fn cap(&self) -> Variable {
        self.cap.read()
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

    pub fn get_struct<T: DslStruct>(&self, index: Variable) -> T {
        let ptr = DslPtr::new(self.buf.read()) + index.mul_imm_simple(T::SIZE as u8);
        T::new(ptr)
    }
    pub fn get_ptr(&self, index: Variable, stride: u8) -> DslPtr {
        if stride == 1 {
            DslPtr::new(self.buf.read()) + index
        } else {
            DslPtr::new(self.buf.read()) + index.mul_imm_simple(stride)
        }
    }
    pub fn get1(&self, index: Variable) -> Variable {
        self.get_ptr(index, 1).read()
    }
    pub fn get2(&self, index: Variable) -> [Variable; 2] {
        let ptr = self.get_ptr(index, 2);
        [ptr.read(), (ptr + 1).read()]
    }

    //TODO LEN check? Call or inline?

    pub fn pop_struct<T: DslStruct>(&self) -> T::ValueType {
        let len = self.len.read();
        let start_offset = len - T::SIZE as u16;
        let ptr = self.buf.read() + start_offset;
        let r = T::new(DslPtr::new(ptr)).read();
        let new_len = len - T::SIZE as u16;
        self.len.write(new_len);
        r
    }
    pub fn pop<const N: usize>(&self) -> [Variable; N] {
        let len = self.len.read();
        let start_offset = len - N as u16;
        let start_ptr = self.get_ptr(start_offset, 1);
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

    //TODO insert

    //TODO remove (mem_copy, set len)

    //TODO iter?

    //TODO for each (with index and without)

    pub fn clear(&self) {
        VEC_CLEAR.call([self.base.ptr]);
    }
}

#[test]
fn test_vec_basic() {
    use crate::programmer::language::dsl::*;

    let mut compiler = Compiler::default();
    define_vec(&mut compiler);

    let test_vec_basic = DslFunction::new("test_vec_basic", [], []);
    test_vec_basic.compile(&mut compiler, |[], _ret| {
        heap_init();
        let vec = Vec::new_at_addr(DslPtr::new(v(1)), 0);
        assert_with_signal(CondOp::CmpI(vec.len(), 0, Equal), 10);
        assert_with_signal(CondOp::CmpI(vec.cap(), 0, Equal), 11);

        vec.push1(v(12));
        assert_with_signal(CondOp::CmpI(vec.len(), 1, Equal), 20);
        assert_with_signal(CondOp::CmpI(vec.get_ptr(v(0), 1).read(), 12, Equal), 21);

        vec.push2(v(34), v(56));
        assert_with_signal(CondOp::CmpI(vec.len(), 3, Equal), 30);
        assert_with_signal(CondOp::CmpI(vec.get1(v(0)), 12, Equal), 31);
        assert_with_signal(CondOp::CmpI(vec.get1(v(1)), 34, Equal), 32);
        assert_with_signal(CondOp::CmpI(vec.get1(v(2)), 56, Equal), 33);

        let p1 = vec.pop1();
        assert_with_signal(CondOp::Cmp(p1, v(56), Equal), 40);
        assert_with_signal(CondOp::CmpI(vec.len(), 2, Equal), 41);
        assert_with_signal(CondOp::CmpI(vec.get1(v(0)), 12, Equal), 42);
        assert_with_signal(CondOp::CmpI(vec.get1(v(1)), 34, Equal), 43);

        vec.push4(v(1), v(2), v(3), v(4));
        assert_with_signal(CondOp::CmpI(vec.len(), 6, Equal), 50);
        assert_with_signal(CondOp::CmpI(vec.get1(v(0)), 12, Equal), 51);
        assert_with_signal(CondOp::CmpI(vec.get1(v(1)), 34, Equal), 52);
        assert_with_signal(CondOp::CmpI(vec.get1(v(2)), 1, Equal), 53);
        assert_with_signal(CondOp::CmpI(vec.get1(v(3)), 2, Equal), 54);
        assert_with_signal(CondOp::CmpI(vec.get1(v(4)), 3, Equal), 55);
        assert_with_signal(CondOp::CmpI(vec.get1(v(5)), 4, Equal), 56);

        let [p3, p4] = vec.pop2();
        assert_with_signal(CondOp::CmpI(vec.len(), 4, Equal), 60);
        assert_with_signal(CondOp::CmpI(vec.get1(v(0)), 12, Equal), 61);
        assert_with_signal(CondOp::CmpI(vec.get1(v(1)), 34, Equal), 62);
        assert_with_signal(CondOp::CmpI(vec.get1(v(2)), 1, Equal), 63);
        assert_with_signal(CondOp::CmpI(vec.get1(v(3)), 2, Equal), 64);
        assert_with_signal(CondOp::CmpI(p3, 3, Equal), 65);
        assert_with_signal(CondOp::CmpI(p4, 4, Equal), 66);

        let vec2 = Vec::new_at_addr(DslPtr::new(v(4)), 4); //  init size = 4 to avoid malloc round-up
        assert_with_signal(CondOp::CmpI(vec2.len(), 0, Equal), 70);
        assert_with_signal(CondOp::CmpI(vec2.cap(), 4, Equal), 71);

        vec2.push1(v(123));
        assert_with_signal(CondOp::CmpI(vec2.len(), 1, Equal), 80);
        assert_with_signal(CondOp::CmpI(vec2.get1(v(0)), 123, Equal), 81);

        halt_with_signal(v(0));
    });

    let instructions = compiler.finish("test_vec_basic");
    let (state, halt_signal) = simulate(&instructions, 1000);
    println!("vec {:?}", &state.mem[1..4]);
    let heap_stat = print_heap(state.mem.as_slice());
    assert_eq!(heap_stat.alloc_count, 2);
    assert_eq!(heap_stat.alloc_size, 12);
    assert_eq!(halt_signal, Some(0));
}
