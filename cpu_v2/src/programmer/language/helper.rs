use crate::programmer::language::push_op;
use crate::{UpdateOp, Variable, VariableOperation1};

pub fn v(v: u16) -> Variable {
    let r = Variable::new();
    r.set_imm(v);
    r
}

pub fn h() {
    push_op(VariableOperation1::Update(UpdateOp::Halt()));
}
