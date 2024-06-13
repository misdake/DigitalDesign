use crate::programmer::language::push_op;
use crate::{u16_to_hi_lo, UpdateOp, Variable, VariableOperation1};

pub fn v(v: u16) -> Variable {
    let r = Variable::new();
    r.set(v);
    r
}

impl Variable {
    pub fn set(self, v: u16) {
        let (hi, lo) = u16_to_hi_lo(v);
        push_op(VariableOperation1::Update(UpdateOp::LoadImmLo(self, lo)));
        if hi > 0 {
            push_op(VariableOperation1::Update(UpdateOp::LoadImmHi(self, hi)));
        }
    }
    pub fn assign_from(self, v: Variable) {
        push_op(VariableOperation1::Update(UpdateOp::Mov(self, v)));
    }
}

pub fn h() {
    push_op(VariableOperation1::Update(UpdateOp::Halt()));
}
