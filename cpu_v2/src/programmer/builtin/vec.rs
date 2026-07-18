//! growable vector on top of the heap (buf/len/cap triple)

use crate::define_struct;
use crate::programmer::builtin::heap::*;
use crate::programmer::builtin::mem::*;
use crate::programmer::compiler::Compiler;
use crate::programmer::dsl::*;
use once_cell::sync::Lazy;

const VEC_LEN_INIT: u16 = 4;

define_struct!(Vec { buf, len, cap });

/// alloc `size` words at the tail of the vec's buffer, returns the start addr
pub static VEC_SUBALLOC: Lazy<DslFunction<2, 1>> =
    Lazy::new(|| DslFunction::new("vec_suballoc", ["self", "size"], ["ptr"]));
/// alloc a new buffer of `cap` words, copy contents, free the old one
pub static VEC_REALLOC: Lazy<DslFunction<2, 1>> =
    Lazy::new(|| DslFunction::new("vec_realloc", ["self", "cap"], ["new_buf"]));
/// free the buffer and zero the header
pub static VEC_CLEAR: Lazy<DslFunction<1, 0>> =
    Lazy::new(|| DslFunction::new("vec_drop", ["self"], []));

pub fn define_vec(compiler: &mut Compiler) {
    define_heap(compiler);
    define_mem(compiler);
    if compiler.has_func("vec_suballoc") {
        return;
    }

    VEC_SUBALLOC.compile(compiler, |b, [ptr, size], ret| {
        let vec = Vec::new(ptr.ptr());
        let prev_len = vec.len.read();
        let curr_len = &prev_len + &size;
        let prev_cap = vec.cap.read();
        let curr_buf = vec.buf.read();

        b.if_then(curr_len.gt(&prev_cap), |b| {
            // new_cap = max(prev_cap * 2, VEC_LEN_INIT), doubled until it fits
            let new_cap = prev_cap.lsl(1).clone_value();
            b.if_then(new_cap.eq_imm(0), |b| {
                new_cap.assign_from(&b.v(VEC_LEN_INIT));
            });
            b.while_loop(
                |_| new_cap.lt(&curr_len),
                |_b| {
                    let doubled = new_cap.lsl(1);
                    new_cap.assign_from(&doubled);
                },
            );

            let [new_buf] = VEC_REALLOC.call(b, [&ptr, &new_cap]);
            curr_buf.assign_from(&new_buf);
        });

        // *len = curr_len
        vec.len.write(&curr_len);

        // return start of the suballoc region
        let val_ptr = &curr_buf + &prev_len;
        ret(b, [val_ptr]);
    });

    VEC_REALLOC.compile(compiler, |b, [ptr, curr_cap], ret| {
        let vec = Vec::new(ptr.ptr());
        let prev_buf_ptr = vec.buf.read();
        let prev_len = vec.len.read();

        // *buf = malloc(curr_cap), *cap = curr_cap
        let curr_buf = heap_malloc(b, &curr_cap);
        vec.buf.write(&curr_buf);
        vec.cap.write(&curr_cap);

        // copy contents from prev to curr
        mem_copy(b, &curr_buf.ptr(), &prev_buf_ptr.ptr(), &prev_len);

        // free the old buffer if any
        b.if_then(prev_buf_ptr.gt_imm(0), |b| {
            heap_free(b, &prev_buf_ptr);
        });

        ret(b, [curr_buf]);
    });

    VEC_CLEAR.compile(compiler, |b, [ptr], ret| {
        let vec = Vec::new(ptr.ptr());
        let buf_ptr = vec.buf.read();
        b.if_then(buf_ptr.gt_imm(0), |b| {
            heap_free(b, &buf_ptr);
            let z = b.v(0);
            vec.buf.write(&z);
            vec.len.write(&z);
            vec.cap.write(&z);
        });
        ret(b, []);
    });
}

impl Vec {
    /// heap-allocate a vec header (+ initial buffer if init_size > 0)
    pub fn alloc(b: &B, init_size: u16) -> Self {
        let addr = heap_malloc(b, &b.v(3));
        Self::new_at_addr(b, addr.ptr(), init_size)
    }
    pub fn free(&self, b: &B) {
        self.clear(b);
        heap_free(b, &self.base().ptr);
    }

    pub fn new_at_addr(b: &B, addr: DslPtr, init_size: u16) -> Self {
        let vec = Vec::new(addr);
        let zero = b.v(0);
        if init_size > 0 {
            let init_size = b.v(init_size);
            let buf = heap_malloc(b, &init_size);
            vec.buf.write(&buf);
            vec.len.write(&zero);
            vec.cap.write(&init_size);
        } else {
            vec.buf.write(&zero);
            vec.len.write(&zero);
            vec.cap.write(&zero);
        }
        vec
    }

    pub fn len(&self) -> Variable {
        self.len.read()
    }
    pub fn cap(&self) -> Variable {
        self.cap.read()
    }

    pub fn push_struct<T: DslStruct>(&self, b: &B, t: T::ValueType) {
        let [ptr] = VEC_SUBALLOC.call(b, [&self.buf.ptr, &b.v(T::SIZE as u16)]);
        T::new(ptr.ptr()).write(t);
    }
    pub fn push<const N: usize>(&self, b: &B, value: [&Variable; N]) {
        let [ptr] = VEC_SUBALLOC.call(b, [&self.buf.ptr, &b.v(N as u16)]);
        let ptr = ptr.ptr();
        for (i, v) in value.iter().enumerate() {
            (&ptr + i as u16).write(v);
        }
    }
    pub fn push1(&self, b: &B, value: &Variable) {
        self.push(b, [value]);
    }
    pub fn push2(&self, b: &B, a: &Variable, c: &Variable) {
        self.push(b, [a, c]);
    }
    pub fn push3(&self, b: &B, a: &Variable, c: &Variable, d: &Variable) {
        self.push(b, [a, c, d]);
    }
    pub fn push4(&self, b: &B, a: &Variable, c: &Variable, d: &Variable, e: &Variable) {
        self.push(b, [a, c, d, e]);
    }

    pub fn get_struct<T: DslStruct>(&self, index: &Variable) -> T {
        let ptr = self.buf.read().ptr() + &index.mul_imm_simple(T::SIZE as u8);
        T::new(ptr)
    }
    pub fn get_ptr(&self, index: &Variable, stride: u8) -> DslPtr {
        if stride == 1 {
            self.buf.read().ptr() + index
        } else {
            self.buf.read().ptr() + &index.mul_imm_simple(stride)
        }
    }
    pub fn get1(&self, index: &Variable) -> Variable {
        self.get_ptr(index, 1).read()
    }
    pub fn get2(&self, index: &Variable) -> [Variable; 2] {
        let ptr = self.get_ptr(index, 2);
        [ptr.read(), (ptr + 1).read()]
    }

    pub fn pop_struct<T: DslStruct>(&self) -> T::ValueType {
        let len = self.len.read();
        let start_offset = &len - T::SIZE as u16;
        let ptr = &self.buf.read() + &start_offset;
        let r = T::new(ptr.ptr()).read();
        self.len.write(&start_offset);
        r
    }
    pub fn pop<const N: usize>(&self) -> [Variable; N] {
        let len = self.len.read();
        let start_offset = &len - N as u16;
        let start_ptr = self.get_ptr(&start_offset, 1);
        let results = core::array::from_fn(|i| (&start_ptr + i as u16).read());
        self.len.write(&start_offset);
        results
    }
    pub fn pop1(&self) -> Variable {
        let [r] = self.pop::<1>();
        r
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

    pub fn clear(&self, b: &B) {
        VEC_CLEAR.call(b, [&self.base().ptr]);
    }
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::programmer::builtin::heap::tests::print_heap;
    use crate::simulate;

    #[test]
    fn test_vec_basic() {
        let mut compiler = Compiler::new();
        define_vec(&mut compiler);

        let test_vec_basic = DslFunction::new("test_vec_basic", [], []);
        test_vec_basic.compile(&mut compiler, |b, [], _ret| {
            heap_init(b);
            let vec = Vec::new_at_addr(b, b.v(1).ptr(), 0);
            b.assert(vec.len().eq_imm(0), 10);
            b.assert(vec.cap().eq_imm(0), 11);

            vec.push1(b, &b.v(12));
            b.assert(vec.len().eq_imm(1), 20);
            b.assert(vec.get_ptr(&b.v(0), 1).read().eq_imm(12), 21);

            vec.push2(b, &b.v(34), &b.v(56));
            b.assert(vec.len().eq_imm(3), 30);
            b.assert(vec.get1(&b.v(0)).eq_imm(12), 31);
            b.assert(vec.get1(&b.v(1)).eq_imm(34), 32);
            b.assert(vec.get1(&b.v(2)).eq_imm(56), 33);

            let p1 = vec.pop1();
            b.assert(p1.eq_imm(56), 40);
            b.assert(vec.len().eq_imm(2), 41);
            b.assert(vec.get1(&b.v(0)).eq_imm(12), 42);
            b.assert(vec.get1(&b.v(1)).eq_imm(34), 43);

            vec.push4(b, &b.v(1), &b.v(2), &b.v(3), &b.v(4));
            b.assert(vec.len().eq_imm(6), 50);
            b.assert(vec.get1(&b.v(0)).eq_imm(12), 51);
            b.assert(vec.get1(&b.v(1)).eq_imm(34), 52);
            b.assert(vec.get1(&b.v(2)).eq_imm(1), 53);
            b.assert(vec.get1(&b.v(3)).eq_imm(2), 54);
            b.assert(vec.get1(&b.v(4)).eq_imm(3), 55);
            b.assert(vec.get1(&b.v(5)).eq_imm(4), 56);

            let [p3, p4] = vec.pop2();
            b.assert(vec.len().eq_imm(4), 60);
            b.assert(vec.get1(&b.v(0)).eq_imm(12), 61);
            b.assert(vec.get1(&b.v(1)).eq_imm(34), 62);
            b.assert(vec.get1(&b.v(2)).eq_imm(1), 63);
            b.assert(vec.get1(&b.v(3)).eq_imm(2), 64);
            b.assert(p3.eq_imm(3), 65);
            b.assert(p4.eq_imm(4), 66);

            let vec2 = Vec::new_at_addr(b, b.v(4).ptr(), 4); // init size avoids malloc round-up
            b.assert(vec2.len().eq_imm(0), 70);
            b.assert(vec2.cap().eq_imm(4), 71);

            vec2.push1(b, &b.v(123));
            b.assert(vec2.len().eq_imm(1), 80);
            b.assert(vec2.get1(&b.v(0)).eq_imm(123), 81);

            let z = b.v(0);
            b.halt(&z);
        });

        let instructions = compiler.finish("test_vec_basic");
        let (state, signal) = simulate(&instructions, 4000);
        println!("vec {:?}", &state.mem[1..4]);
        let heap_stat = print_heap(state.mem.as_slice());
        assert_eq!(heap_stat.alloc_count, 2);
        assert_eq!(heap_stat.alloc_size, 12);
        assert_eq!(signal, Some(0));
    }
}
