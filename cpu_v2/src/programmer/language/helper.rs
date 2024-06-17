use crate::programmer::language::push_op;
use crate::{compose_variable_operations, Cond, CondOp, UpdateOp, Variable, VariableOperation1};
use std::ops::Range;

pub fn v(v: u16) -> Variable {
    let r = Variable::new();
    r.set_imm(v);
    r
}

pub fn halt_with_signal(variable: Variable) {
    push_op(VariableOperation1::Update(UpdateOp::Halt(variable)));
}

impl Variable {
    pub fn larger_than(self, v: Variable) -> CondOp<Variable> {
        CondOp::Cmp(self, v, Cond::Greater)
    }
}

pub fn if_then(cond: CondOp<Variable>, then_block: impl FnOnce()) {
    let then_block = compose_variable_operations(then_block);
    push_op(VariableOperation1::If(cond, Box::new(then_block), None));
}
pub fn if_then_else(cond: CondOp<Variable>, then_block: impl FnOnce(), else_block: impl FnOnce()) {
    let then_block = compose_variable_operations(then_block);
    let else_block = compose_variable_operations(else_block);
    push_op(VariableOperation1::If(
        cond,
        Box::new(then_block),
        Some(Box::new(else_block)),
    ));
}

pub fn while_loop(cond: CondOp<Variable>, loop_block: impl FnOnce()) {
    let loop_block = compose_variable_operations(loop_block);
    push_op(VariableOperation1::Loop(cond, Box::new(loop_block)));
}
pub fn for_loop_u4(range: Range<u8>, loop_block: impl FnOnce(Variable)) {
    assert!(range.start < 16);
    assert!(range.end <= 16);
    let mut i = v(range.start as u16);
    let loop_block = compose_variable_operations(|| {
        loop_block(i);
        i += 1;
    });
    push_op(VariableOperation1::Loop(
        CondOp::CmpI(i, range.end, Cond::Less),
        Box::new(loop_block),
    ));
}
pub fn for_loop_reg_up(start: u16, end: Variable, loop_block: impl FnOnce(Variable)) {
    let mut i = v(start);
    let loop_block = compose_variable_operations(|| {
        loop_block(i);
        i += 1;
    });
    push_op(VariableOperation1::Loop(
        CondOp::Cmp(i, end, Cond::Less),
        Box::new(loop_block),
    ));
}
