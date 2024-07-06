use crate::programmer::language::push_op;
use crate::*;

#[derive(Copy, Clone)]
pub struct DslPtr {
    pub ptr: Variable,
    pub offset: u16,
}
impl DslPtr {
    pub fn new(ptr: Variable) -> Self {
        Self { ptr, offset: 0 }
    }

    pub fn add_u4(self, u4: u8) {
        push_op(VariableOperation1::Update(UpdateOp::AddiAssign(
            self.ptr, u4,
        )));
    }
    pub fn add_var(self, v: Variable) {
        push_op(VariableOperation1::Update(UpdateOp::AddAssign(self.ptr, v)));
    }

    pub fn read(self) -> Variable {
        if self.offset < 16 {
            let r = Variable::new();
            push_op(VariableOperation1::Result(
                ResultOp::LoadMem(self.ptr, self.offset as u8),
                r,
            ));
            r
        } else {
            let r = self.ptr + self.offset;
            push_op(VariableOperation1::Update(UpdateOp::LoadMem(r, 0, r)));
            r
        }
    }
    pub fn read_to(self, v: Variable) {
        if self.offset < 16 {
            push_op(VariableOperation1::Result(
                ResultOp::LoadMem(self.ptr, self.offset as u8),
                v,
            ));
        } else {
            let r = self.ptr + self.offset;
            push_op(VariableOperation1::Result(ResultOp::LoadMem(r, 0), v));
        }
    }
    pub fn write(self, v: Variable) {
        if self.offset < 16 {
            push_op(VariableOperation1::Update(UpdateOp::StoreMem(
                self.ptr,
                self.offset as u8,
                v,
            )));
        } else {
            let r = self.ptr + self.offset;
            push_op(VariableOperation1::Update(UpdateOp::StoreMem(r, 0, v)));
        }
    }
}
impl std::ops::Add<u16> for DslPtr {
    type Output = DslPtr;
    fn add(self, rhs: u16) -> DslPtr {
        Self {
            ptr: self.ptr,
            offset: self.offset + rhs,
        }
    }
}
impl std::ops::AddAssign<u16> for DslPtr {
    fn add_assign(&mut self, rhs: u16) {
        self.offset = self.offset + rhs;
    }
}
impl std::ops::Add<Variable> for DslPtr {
    type Output = DslPtr;
    fn add(self, rhs: Variable) -> DslPtr {
        Self {
            ptr: self.ptr + rhs,
            offset: self.offset,
        }
    }
}

#[derive(Copy, Clone)]
pub struct DslArray<const STRIDE: usize> {
    pub base: DslPtr,
}
impl<const STRIDE: usize> DslArray<STRIDE> {
    pub fn new(base: DslPtr) -> Self {
        Self { base }
    }
    pub fn index_imm(&self, index: usize) -> DslPtr {
        let offset = (STRIDE * index) as u16;
        self.base + offset
    }
    pub fn index_reg(&self, index: Variable) -> DslPtr {
        DslPtr {
            ptr: self.base.ptr + index.mul_imm_simple(STRIDE),
            offset: 0,
        }
    }
}

#[test]
fn test_ptr_array() {
    use crate::programmer::language::dsl::*;
    let func = DslFunction::new("test_ptr_array", [], []);

    let mut compiler = Compiler::default();
    func.compile(&mut compiler, |[], _ret| {
        let c = v(11);
        let d = v(4);
        let e = v(7);
        for_loop_u4(0..8, |i| DslPtr::new(i).write(c));
        let array1 = DslArray::<2>::new(DslPtr::new(v(8)));
        let array2 = DslArray::<2>::new(DslPtr::new(v(9)));
        array1.index_imm(0).write(d);
        array1.index_imm(1).write(d);
        array1.index_imm(2).write(d);
        array1.index_imm(3).write(d);
        for_loop_u4(0..4, |i| array2.index_reg(i).write(e));

        let mut sum = v(0);
        for_loop_u4(0..8, |i| {
            let v = DslPtr::new(i).read();
            sum += v;
        });
        for_loop_u4(0..4, |i| {
            sum += array1.index_reg(i).read();
        });
        sum += array2.index_imm(0).read();
        sum += array2.index_imm(1).read();
        sum += array2.index_imm(2).read();
        sum += array2.index_imm(3).read();

        halt_with_signal(sum);
    });

    let instructions = compiler.finish("test_ptr_array");
    let (_state, halt_signal) = simulate(&instructions, 1000);
    assert_eq!(halt_signal, Some(11 * 12));
}
