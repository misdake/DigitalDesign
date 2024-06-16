use crate::programmer::language::push_op;
use crate::{ResultOp, Variable, VariableOperation1};

struct Field {
    name: String,
    offset: u16,
}

struct Struct {
    fields: Vec<Field>,
}

#[derive(Copy, Clone)]
pub struct DslPtr {
    ptr: Variable,
    offset: u16,
}
impl DslPtr {
    pub fn new(ptr: Variable) -> Self {
        Self { ptr, offset: 0 }
    }
    pub fn resolve(&mut self) {
        self.ptr += self.offset;
        self.offset = 0;
    }
    pub fn offset(self, offset: u16) -> DslPtr {
        Self {
            ptr: self.ptr,
            offset: self.offset + offset,
        }
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
            push_op(VariableOperation1::Result(ResultOp::LoadMem(r, 0), r));
            r
        }
    }
}

struct DslArray<const STRIDE: usize> {
    base: DslPtr,
}
impl<const STRIDE: usize> DslArray<STRIDE> {
    pub fn new(base: DslPtr) -> Self {
        Self { base }
    }
    pub fn index_imm(&self, index: usize) -> DslPtr {
        let offset = (STRIDE * index) as u16;
        self.base.offset(offset)
    }
    pub fn index_reg(&self, index: Variable) -> DslPtr {
        match STRIDE {
            1 => DslPtr {
                ptr: self.base.ptr + index,
                offset: 0,
            },
            2 => DslPtr {
                ptr: self.base.ptr + index.lsl(1),
                offset: 0,
            },
            3 => DslPtr {
                ptr: self.base.ptr + (index.lsl(1) + index),
                offset: 0,
            },
            4 => DslPtr {
                ptr: self.base.ptr + index.lsl(2),
                offset: 0,
            },
            6 => DslPtr {
                ptr: self.base.ptr + (index.lsl(1) + index).lsl(1),
                offset: 0,
            },
            8 => DslPtr {
                ptr: self.base.ptr + index.lsl(3),
                offset: 0,
            },

            _ => unimplemented!(),
        }
    }
}
