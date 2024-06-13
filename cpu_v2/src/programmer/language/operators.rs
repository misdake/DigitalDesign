use crate::programmer::language::push_op;
use crate::{ResultOp, UpdateOp, Variable, VariableOperation1};
use std::ops::*;

impl Add<Variable> for Variable {
    type Output = Variable;
    fn add(self, rhs: Variable) -> Self::Output {
        let r = Variable::new();
        push_op(VariableOperation1::Result(ResultOp::Add(self, rhs), r));
        r
    }
}

impl AddAssign<Variable> for Variable {
    fn add_assign(&mut self, rhs: Variable) {
        push_op(VariableOperation1::Update(UpdateOp::AddAssign(*self, rhs)));
    }
}

impl AddAssign<u16> for Variable {
    fn add_assign(&mut self, rhs: u16) {
        if rhs < 16 {
            push_op(VariableOperation1::Update(UpdateOp::AddiAssign(
                *self,
                (rhs & 0b1111) as u8,
            )));
        } else {
            todo!() // new variable and add
        }
    }
}
impl SubAssign<u16> for Variable {
    fn sub_assign(&mut self, rhs: u16) {
        if rhs < 16 {
            push_op(VariableOperation1::Update(UpdateOp::SubiAssign(
                *self,
                (rhs & 0b1111) as u8,
            )));
        } else {
            todo!() // new variable and sub
        }
    }
}
